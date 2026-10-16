//! Hyperlinks and in-document links (`<a href="https://…">`, `<a href="#id">`).
//!
//! Each link (and each element a `#id` link points at) gets a tracking class
//! and `cursor:pointer`, neither of which changes a single painted pixel. The
//! layout engine then reports, in the same pass that lays out the document,
//! every hit-test area it produced for a tracked element or for anything
//! inside it: the glyph runs of its text, wrapped lines and page breaks
//! included, and -- because `cursor` makes it interactive -- its border box
//! when it is a block (printpdf's `track_class_prefix`, docs/spikes/007). The
//! matching `Link` annotations are added to that same document: one layout,
//! not the two the old marker-colour probe render needed.
//!
//! Supported targets: `http(s):`, `mailto:`, `tel:` (URI actions) and `#id`
//! (jump to the element with that `id`, wherever it ended up). Anything else
//! (`javascript:`, relative paths, unknown ids) is left as plain text.
//!
//! Limits: a link rectangle is taken from layout, untransformed, so a link
//! inside a CSS-transformed element is placed untransformed; the rectangle of
//! an inline link covers its glyphs, not its padding.

use std::collections::{BTreeMap, BTreeSet};

use printpdf::html::{TrackedBox, TrackedBoxes};
use printpdf::{Actions, Destination, LinkAnnotation, Op, PdfDocument, Pt, Rect};

/// The class prefix the layout engine is asked to track. `a` + index marks a
/// link, `t` + index a jump target; no stylesheet rule names either.
pub(crate) const TRACK_PREFIX: &str = "pdfcn-track-";

fn link_class(index: usize) -> String {
    format!("{TRACK_PREFIX}a{index}")
}

fn target_class(index: usize) -> String {
    format!("{TRACK_PREFIX}t{index}")
}

/// What the pre-pass found in the HTML.
pub(crate) struct LinkPlan {
    /// The document with tracking classes injected: render this one.
    pub marked_html: String,
    links: Vec<String>,
    targets: Vec<String>,
}

/// Schemes a link may use; everything else is dropped rather than embedded.
fn allowed_uri(href: &str) -> bool {
    let lower = href.trim().to_ascii_lowercase();
    ["http://", "https://", "mailto:", "tel:"]
        .iter()
        .any(|p| lower.starts_with(p))
}

/// One start tag in the source: its byte span and attribute text.
struct Tag {
    start: usize,
    end: usize,
}

/// Finds every `<name …>` start tag (quote-aware, so `>` inside an attribute
/// value does not end it). `name` is matched case-insensitively.
fn scan_tags(html: &str, mut wanted: impl FnMut(&str) -> bool) -> Vec<Tag> {
    let bytes = html.as_bytes();
    let mut tags = Vec::new();
    let mut i = 0;
    while let Some(rel) = html[i..].find('<') {
        let start = i + rel;
        let rest = &html[start + 1..];
        let name_len = rest
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'-')
            .count();
        if name_len == 0 || rest.starts_with('/') || rest.starts_with('!') {
            i = start + 1;
            continue;
        }
        let name = &rest[..name_len];
        // Walk to the closing '>' honoring quotes.
        let mut j = start + 1 + name_len;
        let mut quote: Option<u8> = None;
        while j < bytes.len() {
            let b = bytes[j];
            match quote {
                Some(q) if b == q => quote = None,
                Some(_) => {}
                None if b == b'"' || b == b'\'' => quote = Some(b),
                None if b == b'>' => break,
                None => {}
            }
            j += 1;
        }
        if j >= bytes.len() {
            break;
        }
        if wanted(name) {
            tags.push(Tag { start, end: j + 1 });
        }
        i = j + 1;
        // Skip raw-text elements whose content may contain '<'.
        if name.eq_ignore_ascii_case("style") || name.eq_ignore_ascii_case("script") {
            let close = format!("</{}", name.to_ascii_lowercase());
            if let Some(pos) = html[i..].to_ascii_lowercase().find(&close) {
                i += pos;
            }
        }
    }
    tags
}

/// Value of attribute `name` inside a start tag's text.
fn attr_value(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(rel) = lower[from..].find(name) {
        let at = from + rel;
        let before_ok = at > 0 && lower.as_bytes()[at - 1].is_ascii_whitespace();
        let after = &tag[at + name.len()..];
        let after_trim = after.trim_start();
        if before_ok && after_trim.starts_with('=') {
            let value = after_trim[1..].trim_start();
            let (quote, body) = match value.chars().next()? {
                q @ ('"' | '\'') => (q, &value[1..]),
                _ => {
                    return value
                        .split(|c: char| c.is_whitespace() || c == '>')
                        .next()
                        .map(str::to_string)
                }
            };
            return body.find(quote).map(|e| body[..e].to_string());
        }
        from = at + name.len();
    }
    None
}

/// Rewrites a start tag to carry `class` (added to an existing `class`
/// attribute) and `cursor:pointer` (added to an existing `style`): the
/// cursor makes a block interactive, so layout reports its border box.
fn with_tracking(tag: &str, class: &str) -> String {
    let tag = with_attr_value(tag, "class", class, ' ');
    with_attr_value(&tag, "style", "cursor:pointer", ';')
}

/// Appends `value` to attribute `name` (after `sep`), or adds the attribute.
fn with_attr_value(tag: &str, name: &str, value: &str, sep: char) -> String {
    let lower = tag.to_ascii_lowercase();
    let needle = format!("{name}=");
    let mut from = 0;
    while let Some(rel) = lower[from..].find(&needle) {
        let pos = from + rel;
        let after = pos + needle.len();
        let starts_attr = pos > 0 && lower.as_bytes()[pos - 1].is_ascii_whitespace();
        if let Some(q) = tag[after..]
            .chars()
            .next()
            .filter(|c| *c == '"' || *c == '\'')
        {
            if starts_attr {
                if let Some(close) = tag[after + 1..].find(q) {
                    let at = after + 1 + close;
                    let existing = tag[after + 1..at].trim_end();
                    let joined = if existing.is_empty() {
                        value.to_string()
                    } else if existing.ends_with(sep) {
                        format!("{existing}{value}")
                    } else {
                        format!("{existing}{sep}{value}")
                    };
                    return format!("{}{joined}{}", &tag[..after + 1], &tag[at..]);
                }
            }
        }
        from = after;
    }
    let body = tag.trim_end_matches('>').trim_end_matches('/').trim_end();
    let self_closing = tag.trim_end_matches('>').ends_with('/');
    format!(
        "{body} {name}=\"{value}\"{}>",
        if self_closing { " /" } else { "" }
    )
}

/// Scans `html` for links; `None` when there is nothing to link.
pub(crate) fn plan(html: &str) -> Option<LinkPlan> {
    if !html.contains("href") {
        return None;
    }
    let anchors = scan_tags(html, |n| n.eq_ignore_ascii_case("a"));
    let mut links: Vec<String> = Vec::new();
    let mut link_tag: BTreeMap<usize, usize> = BTreeMap::new(); // tag start -> link index
    let mut wanted_ids: BTreeSet<String> = BTreeSet::new();
    for tag in &anchors {
        let text = &html[tag.start..tag.end];
        let Some(href) = attr_value(text, "href") else {
            continue;
        };
        let href = decode_entities(href.trim());
        if let Some(id) = href.strip_prefix('#') {
            if id.is_empty() {
                continue;
            }
            wanted_ids.insert(id.to_string());
        } else if !allowed_uri(&href) {
            continue;
        }
        link_tag.insert(tag.start, links.len());
        links.push(href);
    }
    if links.is_empty() {
        return None;
    }

    // Jump targets: any element whose id some link points at.
    let mut targets: Vec<String> = Vec::new();
    let mut target_tag: BTreeMap<usize, usize> = BTreeMap::new();
    if !wanted_ids.is_empty() {
        for tag in scan_tags(html, |_| true) {
            let text = &html[tag.start..tag.end];
            if let Some(id) = attr_value(text, "id") {
                if wanted_ids.contains(&id) && !targets.contains(&id) {
                    target_tag.insert(tag.start, targets.len());
                    targets.push(id);
                }
            }
        }
    }

    // Rebuild the document with tracking injected, back to front so earlier
    // offsets stay valid. A tag that is both a link and a target is tracked
    // as the link (its position is then the link's own). The text inside a
    // link needs nothing: layout attributes it to its nearest tracked
    // ancestor.
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for tag in scan_tags(html, |_| true) {
        let text = &html[tag.start..tag.end];
        if let Some(&i) = link_tag.get(&tag.start) {
            edits.push((tag.start, tag.end, with_tracking(text, &link_class(i))));
        } else if let Some(&i) = target_tag.get(&tag.start) {
            edits.push((tag.start, tag.end, with_tracking(text, &target_class(i))));
        }
    }
    let mut marked_html = html.to_string();
    for (start, end, replacement) in edits.into_iter().rev() {
        marked_html.replace_range(start..end, &replacement);
    }
    Some(LinkPlan {
        marked_html,
        links,
        targets,
    })
}

fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

/// A box layout gave a tracked element: 1-based page and PDF-space rectangle.
#[derive(Clone, Copy)]
struct Found {
    page: usize,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

fn found(boxes: &[TrackedBox]) -> Vec<Found> {
    boxes
        .iter()
        .map(|b| Found {
            page: b.page + 1,
            x0: b.x,
            y0: b.y,
            x1: b.x + b.width,
            y1: b.y + b.height,
        })
        .collect()
}

/// Merges boxes of one link that overlap (a run's background and its
/// decorations are often painted as several rectangles).
fn merge_boxes(boxes: &[Found]) -> Vec<Found> {
    let mut out: Vec<Found> = Vec::new();
    for b in boxes {
        let mut cur = *b;
        loop {
            let hit = out.iter().position(|o| {
                o.page == cur.page
                    && o.x0 <= cur.x1 + 0.5
                    && cur.x0 <= o.x1 + 0.5
                    && o.y0 <= cur.y1 + 0.5
                    && cur.y0 <= o.y1 + 0.5
            });
            match hit {
                Some(i) => {
                    let o = out.swap_remove(i);
                    cur = Found {
                        page: cur.page,
                        x0: cur.x0.min(o.x0),
                        y0: cur.y0.min(o.y0),
                        x1: cur.x1.max(o.x1),
                        y1: cur.y1.max(o.y1),
                    };
                }
                None => break,
            }
        }
        out.push(cur);
    }
    out
}

/// Adds the link annotations to `doc`, using the boxes its own layout
/// reported. Links that could not be located are skipped.
pub(crate) fn apply(plan: &LinkPlan, boxes: &TrackedBoxes, doc: &mut PdfDocument) {
    for (i, href) in plan.links.iter().enumerate() {
        let Some(rects) = boxes.get(&link_class(i)).map(|b| found(b)) else {
            continue;
        };
        let actions = if let Some(id) = href.strip_prefix('#') {
            let Some(ti) = plan.targets.iter().position(|t| t == id) else {
                continue;
            };
            // The target's first box (document order) is where the jump lands.
            let Some(first) = boxes
                .get(&target_class(ti))
                .and_then(|b| found(b).first().copied())
            else {
                continue;
            };
            Actions::Goto(Destination::Xyz {
                page: first.page,
                left: Some(first.x0),
                top: Some(first.y1),
                zoom: None,
            })
        } else {
            Actions::Uri(href.clone())
        };
        for r in &merge_boxes(&rects) {
            if r.x1 <= r.x0 || r.y1 <= r.y0 {
                continue;
            }
            let Some(page) = doc.pages.get_mut(r.page - 1) else {
                continue;
            };
            let rect = Rect {
                x: Pt(r.x0),
                y: Pt(r.y0),
                width: Pt(r.x1 - r.x0),
                height: Pt(r.y1 - r.y0),
                mode: None,
                winding_order: None,
            };
            let mut link = LinkAnnotation::new(
                rect,
                actions.clone(),
                Some(printpdf::BorderArray::Solid([0.0, 0.0, 0.0])),
                None,
                None,
            );
            link.color = Default::default();
            page.ops.push(Op::LinkAnnotation { link });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_tracks_links_and_referenced_targets_only() {
        let html = r##"<body><a href="https://x.dev/a?b=1&amp;c=2" class="u">go</a>
<a href="#sec">jump</a><h2 id="sec">S</h2><p id="other">o</p>
<a href="javascript:alert(1)">bad</a><a name="n">no href</a></body>"##;
        let plan = plan(html).unwrap();
        assert_eq!(plan.links, vec!["https://x.dev/a?b=1&c=2", "#sec"]);
        assert_eq!(plan.targets, vec!["sec"]);
        assert!(plan
            .marked_html
            .contains(r#"class="u pdfcn-track-a0" style="cursor:pointer">go"#));
        assert!(plan.marked_html.contains(&link_class(1)));
        assert!(plan
            .marked_html
            .contains(r#"<h2 id="sec" class="pdfcn-track-t0" style="cursor:pointer">"#));
        assert!(!plan.marked_html.contains("javascript:alert(1)\" class"));
        // `other` is not referenced, so it stays untouched.
        assert!(plan.marked_html.contains(r#"<p id="other">"#));
    }

    #[test]
    fn existing_class_and_style_are_extended_not_duplicated() {
        let tag = r#"<a href="https://x.dev" class="a b" style="color:red">"#;
        assert_eq!(
            with_tracking(tag, "pdfcn-track-a0"),
            r#"<a href="https://x.dev" class="a b pdfcn-track-a0" style="color:red;cursor:pointer">"#
        );
        assert_eq!(
            with_tracking(r#"<a href="https://x.dev">"#, "pdfcn-track-a0"),
            r#"<a href="https://x.dev" class="pdfcn-track-a0" style="cursor:pointer">"#
        );
        // `data-class=` is not `class=`; a trailing `;` is not doubled.
        assert_eq!(
            with_tracking(r#"<a data-class="x" style="color:red;">"#, "k"),
            r#"<a data-class="x" style="color:red;cursor:pointer" class="k">"#
        );
    }

    #[test]
    fn no_links_means_nothing_to_track() {
        assert!(plan("<p>hello</p>").is_none());
        assert!(plan(r#"<a href="ftp://x">x</a>"#).is_none());
    }

    /// The tracking class and `cursor:pointer` change nothing the layout
    /// engine draws: the marked document's pages are the original's, op for
    /// op, and the boxes come along for free.
    #[test]
    fn tracking_draws_exactly_what_the_document_draws() {
        use printpdf::html::{add_xml_to_document, add_xml_to_document_tracked, XmlRenderOptions};
        let html = r##"<html><body>
<p>See <a href="https://x.dev" style="color:blue">the site</a> or <a href="#s">below</a>.</p>
<a href="https://x.dev/card" style="display:block;padding:12px;background-color:#eee">A card</a>
<h2 id="s">Section</h2></body></html>"##;
        let plan = plan(html).expect("links");
        let mut options = XmlRenderOptions::default();
        let mut plain = PdfDocument::new("plain");
        add_xml_to_document(&mut plain, html, &options).expect("renders");
        options.track_class_prefix = Some(TRACK_PREFIX.to_string());
        let mut marked = PdfDocument::new("marked");
        let boxes =
            add_xml_to_document_tracked(&mut marked, &plan.marked_html, &options).expect("renders");
        assert_eq!(plain.pages.len(), marked.pages.len());
        for (a, b) in plain.pages.iter().zip(&marked.pages) {
            assert_eq!(a.ops, b.ops);
        }
        for class in [link_class(0), link_class(1), link_class(2), target_class(0)] {
            assert!(boxes.contains_key(&class), "{class} not found in {boxes:?}");
        }
    }

    #[test]
    fn attribute_values_may_contain_a_closing_angle_bracket() {
        let html = r#"<a title="a>b" href="https://x.dev">t</a>"#;
        let plan = plan(html).unwrap();
        assert_eq!(plan.links, vec!["https://x.dev"]);
    }
}
