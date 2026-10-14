//! Hyperlinks and in-document links (`<a href="https://…">`, `<a href="#id">`).
//!
//! The layout engine has no notion of link rectangles, so they are found by a
//! cheap two-pass trick that needs nothing from the engine: pass 1 renders a
//! copy of the document in which every link (and every `id` a link points at)
//! gets a unique, otherwise-unused background color; the filled rectangles
//! those colors leave in the page operations are exactly the boxes the links
//! occupy, wrapped lines and page breaks included. Pass 2 is the real
//! document, to which the matching `Link` annotations are added. The two
//! passes lay out identically because only a background color differs.
//!
//! Supported targets: `http(s):`, `mailto:`, `tel:` (URI actions) and `#id`
//! (jump to the element with that `id`, wherever it ended up). Anything else
//! (`javascript:`, relative paths, unknown ids) is left as plain text.
//!
//! Limits: a link rectangle is taken from the box's paint position, so a link
//! inside a CSS-transformed element is placed untransformed; a link whose
//! content is itself a block should use `display:block` (class `block`).

use std::collections::{BTreeMap, BTreeSet};

use printpdf::{
    Actions, Color, Destination, LinkAnnotation, Op, PdfDocument, Pt, Rect,
};

/// Marker colors: red channel 254 for links, 253 for jump targets; the
/// 16-bit index travels in green/blue.
const LINK_RED: u8 = 254;
const TARGET_RED: u8 = 253;

/// What the pre-pass found in the HTML.
pub(crate) struct LinkPlan {
    /// The document with marker backgrounds injected (render this for pass 1).
    pub marked_html: String,
    links: Vec<String>,
    targets: Vec<String>,
}

fn marker(red: u8, index: usize) -> String {
    format!("rgb({red},{},{})", (index >> 8) & 0xff, index & 0xff)
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
                _ => return value.split(|c: char| c.is_whitespace() || c == '>').next().map(str::to_string),
            };
            return body.find(quote).map(|e| body[..e].to_string());
        }
        from = at + name.len();
    }
    None
}

/// Rewrites a start tag so its `style` carries `background-color:<color>`.
fn with_background(tag: &str, color: &str) -> String {
    let decl = format!("background-color:{color}");
    if let Some(pos) = tag.to_ascii_lowercase().find("style=") {
        let after = pos + "style=".len();
        if let Some(q) = tag[after..].chars().next().filter(|c| *c == '"' || *c == '\'') {
            if let Some(close) = tag[after + 1..].find(q) {
                let at = after + 1 + close;
                return format!("{};{decl}{}", &tag[..at], &tag[at..]);
            }
        }
    }
    let body = tag.trim_end_matches('>').trim_end_matches('/').trim_end();
    let self_closing = tag.trim_end_matches('>').ends_with('/');
    format!("{body} style=\"{decl}\"{}>", if self_closing { " /" } else { "" })
}

fn is_inline_tag(name: &str) -> bool {
    const INLINE: [&str; 13] = [
        "span", "b", "strong", "i", "em", "u", "s", "small", "sub", "sup", "code", "mark", "abbr",
    ];
    INLINE.iter().any(|t| name.eq_ignore_ascii_case(t))
}

fn inside_link(spans: &[(usize, usize)], at: usize) -> bool {
    spans.iter().any(|&(s, e)| at > s && at < e)
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
        if links.len() >= 0xfff0 {
            break;
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

    // Rebuild the document with markers injected, back to front so earlier
    // offsets stay valid. A tag that is both a link and a target keeps the
    // link marker (its position is then the link's own).
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let link_spans: Vec<(usize, usize)> = link_tag
        .keys()
        .map(|&start| {
            let end = html[start..].to_ascii_lowercase().find("</a").map_or(start, |c| start + c);
            (start, end)
        })
        .collect();
    for tag in scan_tags(html, |_| true) {
        let text = &html[tag.start..tag.end];
        if let Some(&i) = link_tag.get(&tag.start) {
            let color = marker(LINK_RED, i + 1);
            edits.push((tag.start, tag.end, with_background(text, &color)));
            // A background is not inherited: text wrapped in an inline child
            // (`<a><span class="underline">…`) paints that child's own box,
            // so the marker goes on those children too.
            let close = html[tag.end..].to_ascii_lowercase().find("</a").unwrap_or(0);
            for child in scan_tags(&html[tag.end..tag.end + close], is_inline_tag) {
                let (cs, ce) = (tag.end + child.start, tag.end + child.end);
                edits.push((cs, ce, with_background(&html[cs..ce], &color)));
            }
        } else if inside_link(&link_spans, tag.start) {
            // handled with its anchor above
        } else if let Some(&i) = target_tag.get(&tag.start) {
            edits.push((tag.start, tag.end, with_background(text, &marker(TARGET_RED, i + 1))));
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

/// A box found in the probe render: 1-based page and PDF-space rectangle.
#[derive(Clone, Copy)]
struct Found {
    page: usize,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

fn decode(col: &Color) -> Option<(u8, usize)> {
    let Color::Rgb(c) = col else { return None };
    let ch = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
    let (r, g, b) = (ch(c.r), ch(c.g), ch(c.b));
    if r != LINK_RED && r != TARGET_RED {
        return None;
    }
    Some((r, ((g as usize) << 8) | b as usize))
}

/// Collects the marker-filled rectangles in `probe`, by marker.
fn collect(probe: &PdfDocument) -> BTreeMap<(u8, usize), Vec<Found>> {
    let mut found: BTreeMap<(u8, usize), Vec<Found>> = BTreeMap::new();
    for (page_index, page) in probe.pages.iter().enumerate() {
        let mut fill: Option<Color> = None;
        let mut stack: Vec<Option<Color>> = Vec::new();
        for op in &page.ops {
            match op {
                Op::SaveGraphicsState => stack.push(fill.clone()),
                Op::RestoreGraphicsState => {
                    if let Some(prev) = stack.pop() {
                        fill = prev;
                    }
                }
                Op::SetFillColor { col } => fill = Some(col.clone()),
                Op::DrawRectangle { rectangle } => {
                    if let Some(key) = fill.as_ref().and_then(decode) {
                        found.entry(key).or_default().push(Found {
                            page: page_index + 1,
                            x0: rectangle.x.0,
                            y0: rectangle.y.0,
                            x1: rectangle.x.0 + rectangle.width.0,
                            y1: rectangle.y.0 + rectangle.height.0,
                        });
                    }
                }
                Op::DrawPolygon { polygon } => {
                    let Some(key) = fill.as_ref().and_then(decode) else {
                        continue;
                    };
                    let mut bb: Option<(f32, f32, f32, f32)> = None;
                    for p in polygon.rings.iter().flat_map(|r| r.points.iter()) {
                        let (x, y) = (p.p.x.0, p.p.y.0);
                        bb = Some(match bb {
                            None => (x, y, x, y),
                            Some((a, b, c, d)) => (a.min(x), b.min(y), c.max(x), d.max(y)),
                        });
                    }
                    if let Some((x0, y0, x1, y1)) = bb {
                        found.entry(key).or_default().push(Found {
                            page: page_index + 1,
                            x0,
                            y0,
                            x1,
                            y1,
                        });
                    }
                }
                _ => {}
            }
        }
    }
    found
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

/// Adds the link annotations to the real document, using the boxes found in
/// the probe render. Links that could not be located are skipped.
pub(crate) fn apply(plan: &LinkPlan, probe: &PdfDocument, doc: &mut PdfDocument) {
    let found = collect(probe);
    for (i, href) in plan.links.iter().enumerate() {
        let Some(rects) = found.get(&(LINK_RED, i + 1)) else {
            continue;
        };
        let actions = if let Some(id) = href.strip_prefix('#') {
            let Some(ti) = plan.targets.iter().position(|t| t == id) else {
                continue;
            };
            // The target's first box (document order) is where the jump lands.
            let Some(first) = found.get(&(TARGET_RED, ti + 1)).and_then(|v| v.first()) else {
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
        for r in &merge_boxes(rects) {
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
    fn plan_marks_links_and_referenced_targets_only() {
        let html = r##"<body><a href="https://x.dev/a?b=1&amp;c=2" class="u">go</a>
<a href="#sec">jump</a><h2 id="sec">S</h2><p id="other">o</p>
<a href="javascript:alert(1)">bad</a><a name="n">no href</a></body>"##;
        let plan = plan(html).unwrap();
        assert_eq!(plan.links, vec!["https://x.dev/a?b=1&c=2", "#sec"]);
        assert_eq!(plan.targets, vec!["sec"]);
        assert!(plan.marked_html.contains(&marker(LINK_RED, 1)));
        assert!(plan.marked_html.contains(&marker(LINK_RED, 2)));
        assert!(plan.marked_html.contains(&marker(TARGET_RED, 1)));
        assert!(!plan.marked_html.contains("javascript:alert(1)\" style"));
        // `other` is not referenced, so it stays untouched.
        assert!(plan.marked_html.contains(r#"<p id="other">"#));
    }

    #[test]
    fn existing_style_is_extended_not_duplicated() {
        let tag = r#"<a href="https://x.dev" style="color:red">"#;
        let out = with_background(tag, "rgb(254,0,1)");
        assert_eq!(out, r#"<a href="https://x.dev" style="color:red;background-color:rgb(254,0,1)">"#);
        let bare = with_background(r#"<a href="https://x.dev">"#, "rgb(254,0,1)");
        assert_eq!(bare, r#"<a href="https://x.dev" style="background-color:rgb(254,0,1)">"#);
    }

    #[test]
    fn no_links_means_no_second_pass() {
        assert!(plan("<p>hello</p>").is_none());
        assert!(plan(r#"<a href="ftp://x">x</a>"#).is_none());
    }

    #[test]
    fn attribute_values_may_contain_a_closing_angle_bracket() {
        let html = r#"<a title="a>b" href="https://x.dev">t</a>"#;
        let plan = plan(html).unwrap();
        assert_eq!(plan.links, vec!["https://x.dev"]);
    }

    #[test]
    fn marker_colors_round_trip() {
        for idx in [1usize, 255, 256, 4097, 0xfff0] {
            let m = marker(LINK_RED, idx);
            let nums: Vec<usize> = m
                .trim_start_matches("rgb(")
                .trim_end_matches(')')
                .split(',')
                .map(|n| n.parse().unwrap())
                .collect();
            assert_eq!(nums[0], 254);
            assert_eq!((nums[1] << 8) | nums[2], idx);
        }
    }
}
