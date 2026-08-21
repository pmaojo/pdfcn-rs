//! Rewrites `gap-*` utilities into margins on children, at the HTML level,
//! before the stylesheet is built.
//!
//! The `azul-layout` engine behind `printpdf` ignores the CSS `gap`
//! declaration entirely (see README's known limitations), so `.flex.gap-4`
//! renders its children flush together. Rather than asking every template
//! author to hand-write the `margin-on-the-children` workaround, this pass
//! does it for them: any element carrying a `gap-*` utility **and** a
//! flex/grid display class gets equivalent margin utilities (`mr-*` /
//! `mb-*`) injected into its direct element children — skipping the last
//! child on each axis, exactly like real gap semantics.
//!
//! Why rewrite the HTML instead of emitting a descendant selector like
//! `.gap-4 > *`? The styles pipeline maps one class to one flat declaration
//! on that exact class selector, and margins-on-children is the one spacing
//! mechanism already proven to work in the layout engine (`m-2` on catalog
//! cards). Injecting plain `mr-*`/`mb-*` classes keeps everything on that
//! proven path — `build_stylesheet` resolves them like any other utility.
//!
//! The scanner is hand-rolled for the same reason `pdfcn_core::img_srcs` is:
//! the HTML is our own `maud` output — well-formed, double-quoted attributes,
//! no `<`/`>` inside attribute values — so a tokenizer needs no dependency
//! and no error handling.

use std::ops::Range;

/// Elements that never have a closing tag when emitted by real `maud`
/// component templates. (Template-authored `%img` goes through
/// `html_render::render_node`, which emits a matching `</img>`; the scanner
/// tolerates both shapes.)
const VOID_TAGS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param",
    "source", "track", "wbr",
];

fn is_tag_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == ':'
}

/// One node in a scope (the inside of an element, or the document top).
enum Node<'a> {
    Text(Range<usize>),
    Element(Element<'a>),
}

struct Element<'a> {
    /// Full span of the element (open tag through closing tag) in its scope.
    span: Range<usize>,
    classes: Vec<&'a str>,
    /// Span of the `class="…"` attribute value, if the element has one.
    class_value: Option<Range<usize>>,
    /// Offset just after the tag name — where a `class="…"` attribute can be
    /// inserted when the element has none.
    attrs_start: usize,
    /// Span of the element's content, `None` for void/self-closing elements.
    inner: Option<Range<usize>>,
}

/// Finds the end of an open tag's markup (inclusive of `>`), honoring quoted
/// attribute values.
fn find_tag_end(s: &str, from: usize) -> (usize, bool) {
    let bytes = s.as_bytes();
    let mut i = from;
    let mut quote: Option<u8> = None;
    while i < s.len() {
        let c = bytes[i];
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else if c == b'"' || c == b'\'' {
            quote = Some(c);
        } else if c == b'>' {
            let self_closing = i > from && bytes[i - 1] == b'/';
            return (i + 1, self_closing);
        }
        i += 1;
    }
    (s.len(), false)
}

/// Extracts the `class="…"` value span (offsets relative to `scope_start`)
/// from an open tag's markup. Requires the whitespace before the attribute
/// name so `data-class="…"` style names never match.
fn find_class_value(open_tag: &str, scope_start: usize) -> Option<Range<usize>> {
    let marker = "class=\"";
    let mut from = 0;
    while let Some(ci) = open_tag[from..].find(marker) {
        let abs = from + ci;
        let preceded_by_space = abs > 0
            && open_tag[..abs]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_whitespace() || c == '=');
        if preceded_by_space {
            let value_start = abs + marker.len();
            let value_end = value_start + open_tag[value_start..].find('"')?;
            return Some(scope_start + value_start..scope_start + value_end);
        }
        from = abs + marker.len();
    }
    None
}

fn tag_name_after(s: &str, lt_pos: usize) -> (&str, usize) {
    let name_start = lt_pos + 1;
    let name_end = s[name_start..]
        .find(|c: char| !is_tag_name_char(c))
        .map(|i| name_start + i)
        .unwrap_or(s.len());
    (&s[name_start..name_end], name_end)
}

/// Scans one scope into sibling nodes. Malformed input degrades gracefully:
/// anything the scanner can't structure is preserved verbatim as text, so
/// the rewrite is always lossless on markup it doesn't understand.
fn scan_scope(s: &str) -> Vec<Node<'_>> {
    let mut nodes = Vec::new();
    let mut pos = 0usize;
    while pos < s.len() {
        if !s[pos..].starts_with('<') {
            let next = s[pos..].find('<').map_or(s.len(), |i| pos + i);
            nodes.push(Node::Text(pos..next));
            pos = next;
            continue;
        }
        // Comments, doctypes, and stray close tags (e.g. `</img>` from
        // render_node's uniform closing-tag emission) pass through verbatim.
        if s[pos..].starts_with("<!--")
            || s[pos..].starts_with("<!")
            || s[pos..].starts_with("</")
        {
            let end = if s[pos..].starts_with("<!--") {
                s[pos..].find("-->").map_or(s.len(), |i| pos + i + 3)
            } else {
                s[pos..].find('>').map_or(s.len(), |i| pos + i + 1)
            };
            nodes.push(Node::Text(pos..end));
            pos = end;
            continue;
        }

        let (tag, name_end) = tag_name_after(s, pos);
        let (tag_end, self_closing) = find_tag_end(s, name_end);
        let open_tag = &s[pos..tag_end];
        let class_value = find_class_value(open_tag, pos);

        let is_void = VOID_TAGS.contains(&tag) || self_closing;
        let inner = if is_void {
            None
        } else {
            find_inner_span(s, tag_end, tag)
        };

        let span_end = match &inner {
            Some(r) => {
                // Find the closing tag's end for the full span.
                let mut close_end = r.end + 1;
                while close_end < s.len() && !s[close_end..].starts_with('>') {
                    close_end += 1;
                }
                (close_end + 1).min(s.len())
            }
            None => tag_end,
        };

        nodes.push(Node::Element(Element {
            span: pos..span_end,
            classes: match &class_value {
                Some(r) => s[r.clone()].split_whitespace().collect(),
                None => Vec::new(),
            },
            class_value,
            attrs_start: name_end,
            inner,
        }));
        pos = span_end;
    }
    nodes
}

/// Finds the content span of the next matching `</tag>` starting after an
/// open tag that ended at `from`. Depth-aware so nested same-tag elements
/// don't terminate early.
fn find_inner_span(s: &str, from: usize, tag: &str) -> Option<Range<usize>> {
    let mut depth = 1usize;
    let mut pos = from;
    while pos < s.len() {
        let Some(lt) = s[pos..].find('<') else {
            return None;
        };
        let abs = pos + lt;
        if s[abs..].starts_with("</") {
            let (close_tag, name_end) = tag_name_after(s, abs + 1);
            if close_tag == tag {
                depth -= 1;
                if depth == 0 {
                    return Some(from..abs);
                }
            }
            pos = name_end;
            continue;
        }
        if s[abs..].starts_with("<!--") {
            pos = abs + s[abs..].find("-->").map_or(s.len() - abs, |i| i + 3);
            continue;
        }
        if s[abs..].starts_with("<!") {
            pos = abs + 2;
            continue;
        }
        let (open_tag, name_end) = tag_name_after(s, abs);
        if open_tag == tag {
            depth += 1;
        }
        pos = name_end;
    }
    None
}

/// How a container's `gap-*` utilities translate into margins on its direct
/// element children.
#[derive(Default)]
struct ChildSpacing {
    /// Scale key driving `mr-*` injections (horizontal main axis).
    h: Option<String>,
    /// Scale key driving `mb-*` injections (vertical axis).
    v: Option<String>,
    /// Column count for grids (`grid-cols-N`), 1 when unspecified — a bare
    /// `grid` stacks items in a single implicit column.
    cols: usize,
    /// Whether this container is a grid (column-position math applies).
    is_grid: bool,
}

impl ChildSpacing {
    /// Derives the spacing plan from a container's own classes. Containers
    /// without a flex/grid display class keep `gap` doing nothing (as in
    /// real CSS outside flex/grid/multi-column), so no margins are injected.
    fn from_classes(classes: &[&str]) -> Self {
        let mut gap = None;
        let mut gap_x = None;
        let mut gap_y = None;
        let mut is_flex = false;
        let mut is_col = false;
        let mut is_grid = false;
        let mut cols = 1;
        for c in classes {
            if let Some(k) = c.strip_prefix("gap-x-") {
                gap_x = Some(k.to_string());
            } else if let Some(k) = c.strip_prefix("gap-y-") {
                gap_y = Some(k.to_string());
            } else if let Some(k) = c.strip_prefix("gap-") {
                gap = Some(k.to_string());
            }
            match *c {
                "flex" | "inline-flex" => is_flex = true,
                "flex-col" => is_col = true,
                "grid" => is_grid = true,
                _ => {}
            }
            if let Some(k) = c.strip_prefix("grid-cols-") {
                if let Ok(n) = k.parse::<usize>() {
                    cols = n.max(1);
                }
            }
        }
        if !is_flex && !is_grid {
            return Self::default();
        }
        if is_grid {
            Self {
                h: gap_x.or_else(|| gap.clone()),
                v: gap_y.or(gap),
                cols,
                is_grid: true,
            }
        } else if is_col {
            Self {
                h: None,
                v: gap_y.or(gap),
                cols: 1,
                is_grid: false,
            }
        } else {
            Self {
                h: gap_x.or(gap),
                v: None,
                cols: 1,
                is_grid: false,
            }
        }
    }

    /// Margin utility classes to inject into element child `i` of `n`.
    /// Children that already carry a margin utility on an axis keep their
    /// own (injecting alongside it would double up unpredictably), and
    /// absolutely-positioned children are skipped — a margin shifts an
    /// out-of-flow element's resolved offset, breaking overlays like the
    /// card discount ribbon.
    fn classes_for(&self, i: usize, n: usize, child_classes: &[&str]) -> Vec<String> {
        let mut out = Vec::new();
        if n <= 1
            || child_classes
                .iter()
                .any(|c| *c == "absolute" || *c == "fixed")
        {
            return out;
        }
        let has_h_margin = child_classes
            .iter()
            .any(|c| c.starts_with("m-") || c.starts_with("mx-") || c.starts_with("mr-"));
        let has_v_margin = child_classes
            .iter()
            .any(|c| c.starts_with("m-") || c.starts_with("my-") || c.starts_with("mb-"));
        let cols = self.cols.max(1);
        if let Some(k) = &self.h {
            // Grids skip the margin at end-of-row positions; a flex row only
            // skips the very last child.
            let not_row_end = !self.is_grid || (i + 1) % cols != 0;
            if not_row_end && i + 1 != n && !has_h_margin {
                out.push(format!("mr-{k}"));
            }
        }
        if let Some(k) = &self.v {
            // Everything before the last (partial) row gets bottom margin.
            let last_row_start = n - ((n - 1) % cols + 1);
            if i < last_row_start && !has_v_margin {
                out.push(format!("mb-{k}"));
            }
        }
        out
    }
}

/// Rewrites one scope of siblings, injecting spacing derived from `plan`
/// (the enclosing element's gap classes) into its direct element children
/// and recursing into each child with that child's own derived plan.
fn rewrite_scope(s: &str, plan: &ChildSpacing) -> String {
    let nodes = scan_scope(s);
    let element_count = nodes
        .iter()
        .filter(|n| matches!(n, Node::Element(_)))
        .count();
    let mut out = String::with_capacity(s.len() + s.len() / 8);
    let mut elem_index = 0usize;
    for node in &nodes {
        match node {
            Node::Text(r) => out.push_str(&s[r.clone()]),
            Node::Element(e) => {
                let inject = plan.classes_for(elem_index, element_count, &e.classes);
                elem_index += 1;
                let inner_html = match &e.inner {
                    Some(r) => {
                        let child_plan = ChildSpacing::from_classes(&e.classes);
                        rewrite_scope(&s[r.clone()], &child_plan)
                    }
                    None => String::new(),
                };
                emit_element(s, e, &inject, &inner_html, &mut out);
            }
        }
    }
    out
}

/// Emits an element, splicing injected classes into its class attribute
/// (appended to the existing value, or added as a new attribute when the
/// element had none) and swapping in the rewritten inner HTML.
fn emit_element(s: &str, e: &Element, inject: &[String], inner_html: &str, out: &mut String) {
    if inject.is_empty() {
        match &e.inner {
            Some(r) => {
                out.push_str(&s[e.span.start..r.start]);
                out.push_str(inner_html);
                out.push_str(&s[r.end..e.span.end]);
            }
            None => out.push_str(&s[e.span.clone()]),
        }
        return;
    }
    let injected = inject.join(" ");
    match &e.class_value {
        Some(cv) => {
            out.push_str(&s[e.span.start..cv.start]);
            out.push_str(&s[cv.clone()]);
            out.push(' ');
            out.push_str(&injected);
            let after_value = cv.end;
            match &e.inner {
                Some(r) => {
                    out.push_str(&s[after_value..r.start]);
                    out.push_str(inner_html);
                    out.push_str(&s[r.end..e.span.end]);
                }
                None => out.push_str(&s[after_value..e.span.end]),
            }
        }
        None => {
            out.push_str(&s[e.span.start..e.attrs_start]);
            out.push_str(&format!(r#" class="{injected}""#));
            match &e.inner {
                Some(r) => {
                    out.push_str(&s[e.attrs_start..r.start]);
                    out.push_str(inner_html);
                    out.push_str(&s[r.end..e.span.end]);
                }
                None => out.push_str(&s[e.attrs_start..e.span.end]),
            }
        }
    }
}

/// Rewrites every flex/grid `gap-*` container in rendered HTML so its direct
/// element children carry equivalent `mr-*`/`mb-*` margin utilities — making
/// `gap` work despite the layout engine ignoring the CSS declaration.
/// Markup with no `gap-*` usage passes through byte-for-byte unchanged.
pub fn rewrite_gaps(html: &str) -> String {
    // Fast path: injected classes are always `mr-*`/`mb-*`, so a document
    // with no `gap-` anywhere (the common case) can skip the scan-and-rebuild
    // entirely. A false positive (the literal text "gap-" in prose) just
    // falls through to the normal lossless rewrite.
    if !html.contains("gap-") {
        return html.to_string();
    }
    rewrite_scope(html, &ChildSpacing::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flex_row_gap_injects_mr_on_all_but_last_child() {
        let out = rewrite_gaps(
            r#"<div class="flex gap-2"><p>a</p><p>b</p><p>c</p></div>"#,
        );
        assert!(out.contains(r#"<p class="mr-2">a</p>"#), "{out}");
        assert!(out.contains(r#"<p class="mr-2">b</p>"#), "{out}");
        assert!(out.contains("<p>c</p>"), "{out}");
    }

    #[test]
    fn flex_column_gap_injects_mb_instead() {
        let out = rewrite_gaps(
            r#"<div class="flex flex-col gap-4"><p>a</p><p>b</p></div>"#,
        );
        assert!(out.contains(r#"<p class="mb-4">a</p>"#), "{out}");
        assert!(out.contains("<p>b</p>"), "{out}");
        assert!(!out.contains("mr-4"), "{out}");
    }

    #[test]
    fn grid_gap_follows_row_geometry() {
        let out = rewrite_gaps(
            concat!(
                r#"<div class="grid grid-cols-2 gap-3">"#,
                r#"<p>1</p><p>2</p><p>3</p><p>4</p></div>"#
            ),
        );
        assert!(out.contains(r#"<p class="mr-3 mb-3">1</p>"#), "{out}");
        assert!(out.contains(r#"<p class="mb-3">2</p>"#), "{out}");
        assert!(out.contains(r#"<p class="mr-3">3</p>"#), "{out}");
        assert!(out.contains("<p>4</p>"), "{out}");
    }

    #[test]
    fn partial_last_grid_row_gets_no_trailing_margins() {
        let out = rewrite_gaps(
            concat!(
                r#"<div class="grid grid-cols-2 gap-2">"#,
                r#"<p>1</p><p>2</p><p>3</p></div>"#
            ),
        );
        assert!(out.contains(r#"<p class="mr-2 mb-2">1</p>"#), "{out}");
        assert!(out.contains(r#"<p class="mb-2">2</p>"#), "{out}");
        // Item 3 sits alone in the last row: no mr (row end) and no mb
        // (last row).
        assert!(out.contains("<p>3</p>"), "{out}");
    }

    #[test]
    fn gap_x_and_gap_y_split_the_axes() {
        let out = rewrite_gaps(
            concat!(
                r#"<div class="grid grid-cols-2 gap-x-6 gap-y-1">"#,
                r#"<p>1</p><p>2</p><p>3</p><p>4</p></div>"#
            ),
        );
        assert!(out.contains(r#"<p class="mr-6 mb-1">1</p>"#), "{out}");
        assert!(out.contains(r#"<p class="mb-1">2</p>"#), "{out}");
        assert!(out.contains(r#"<p class="mr-6">3</p>"#), "{out}");
    }

    #[test]
    fn absolute_children_are_skipped_so_overlays_keep_their_offset() {
        let out = rewrite_gaps(
            concat!(
                r#"<div class="flex gap-2">"#,
                r#"<p>a</p>"#,
                r#"<span class="absolute top-2 right-2">ribbon</span>"#,
                r#"</div>"#
            ),
        );
        assert!(out.contains(r#"<p class="mr-2">a</p>"#), "{out}");
        assert!(
            out.contains(r#"<span class="absolute top-2 right-2">"#),
            "{out}"
        );
    }

    #[test]
    fn same_axis_margins_suppress_injection_opposite_axis_coexists() {
        let out = rewrite_gaps(
            concat!(
                r#"<div class="flex gap-2">"#,
                r#"<p class="m-2">a</p><p class="ml-1">b</p><p>c</p>"#,
                r#"</div>"#
            ),
        );
        // All-axis m-2 suppresses the right-margin injection entirely...
        assert!(out.contains(r#"<p class="m-2">a</p>"#), "{out}");
        // ...while a left-only margin coexists: gap still applies on b's
        // right side.
        assert!(out.contains(r#"<p class="ml-1 mr-2">b</p>"#), "{out}");
        assert!(out.contains("<p>c</p>"), "{out}");
    }

    #[test]
    fn gap_without_a_flex_or_grid_display_does_nothing() {
        let html = r#"<div class="gap-4"><p>a</p><p>b</p></div>"#;
        assert_eq!(rewrite_gaps(html), html);
    }

    #[test]
    fn markup_without_gap_passes_through_unchanged() {
        let html = concat!(
            r#"<div class="flex justify-between"><p>x</p></div>"#,
            r#"<table class="table-striped"><tr><td>y</td></tr></table>"#,
        );
        assert_eq!(rewrite_gaps(html), html);
    }

    #[test]
    fn nested_containers_each_get_their_own_plan() {
        let out = rewrite_gaps(
            concat!(
                r#"<div class="flex flex-col gap-4">"#,
                r#"<div class="flex gap-1"><p>a</p><p>b</p></div>"#,
                r#"<p>c</p>"#,
                r#"</div>"#
            ),
        );
        // Inner flex row: a gets mr-1, b untouched.
        assert!(out.contains(r#"<p class="mr-1">a</p>"#), "{out}");
        assert!(out.contains("<p>b</p>"), "{out}");
        // Outer flex column: the (rewritten) inner div gets mb-4, c untouched.
        assert!(out.contains(r#"<div class="flex gap-1 mb-4">"#), "{out}");
        assert!(out.contains("<p>c</p>"), "{out}");
    }

    #[test]
    fn child_without_a_class_attribute_gets_one_added() {
        let out =
            rewrite_gaps(r#"<div class="flex gap-2"><span>a</span><span>b</span></div>"#);
        assert!(
            out.contains(r#"<span class="mr-2">a</span>"#),
            "no-class child must gain a class attribute: {out}"
        );
    }

    #[test]
    fn void_elements_and_text_survive_inside_gap_containers() {
        let out = rewrite_gaps(
            concat!(
                r#"<div class="flex gap-2">"#,
                r#"Hello <img src="x.png"> <span>world</span></div>"#
            ),
        );
        // An <img> is an in-flow flex item just like any other child, so it
        // participates in gap spacing too...
        assert!(out.contains("Hello <img"), "{out}");
        assert!(out.contains(r#"<img class="mr-2" src="x.png">"#), "{out}");
        // ...while the trailing span, as the last child, correctly gets
        // nothing.
        assert!(out.contains(r#"<span>world</span>"#), "{out}");
    }

    #[test]
    fn inline_flex_counts_as_a_flex_container() {
        let out =
            rewrite_gaps(r#"<div class="inline-flex gap-1"><i>a</i><i>b</i></div>"#);
        assert!(out.contains(r#"<i class="mr-1">a</i>"#), "{out}");
    }

    /// End-to-end sanity with the exact markup shape the catalog example
    /// uses: a per-row grid whose cards previously needed hand-written
    /// `m-2` workarounds.
    #[test]
    fn catalog_style_row_grid_is_rewritten() {
        let out = rewrite_gaps(
            concat!(
                r#"<div class="grid grid-cols-2 break-inside-avoid gap-4">"#,
                r#"<div class="card m-2">A</div>"#,
                r#"<div class="card m-2">B</div>"#,
                r#"</div>"#
            ),
        );
        // Cards already carry m-2, so the rewriter must not stack mr-4/mb-4
        // on top of their own margins...
        assert!(out.contains(r#"<div class="card m-2">A</div>"#), "{out}");
        // ...but break-inside-avoid stays put and nothing else changes.
        assert!(out.contains("break-inside-avoid"), "{out}");
    }
}
