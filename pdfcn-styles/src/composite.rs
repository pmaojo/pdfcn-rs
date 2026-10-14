//! Utilities that Tailwind builds from several classes at once -- or that
//! need to act on an element's children -- composed here into inline styles.
//!
//! * `divide-x` / `divide-y` (+ width, color, style): a border between
//!   siblings. Real CSS uses a `> * + *` selector, which the one-class-one-
//!   declaration model can't express, so each non-first child gets its own
//!   border (same approach as `gap.rs`).
//! * `bg-gradient-to-*` + `from-*` / `via-*` / `to-*`: one `linear-gradient`.
//! * `ring` / `ring-N` / `ring-<color>` and `outline` / `outline-N` /
//!   `outline-<color>`: a solid band around the box. The layout engine has no
//!   `outline`, and its `box-shadow` is the one thing that paints outside the
//!   box, so both are emitted as a zero-blur `box-shadow` with spread
//!   (`outline-offset-*` is accepted and ignored: a shadow can't leave a gap).
//!   A ring/outline therefore replaces a `shadow-*` on the same element.
//!
//! The classes consumed here are removed from the element, so they don't
//! show up as unrecognized later; a class that can't be composed (a
//! `from-*` with no gradient direction, a `ring-red-500` with no `ring`
//! width) is left in place and reported by `unresolved_classes`.

use std::ops::Range;

use crate::gap::{scan_scope, Element, Node};
use crate::theme::Theme;
use crate::utilities::color_value;

/// Edge widths Tailwind offers for rings/outlines.
const EDGE_WIDTHS: &[u32] = &[0, 1, 2, 4, 8];

fn edge_width(suffix: &str) -> Option<u32> {
    suffix.parse().ok().filter(|n| EDGE_WIDTHS.contains(n))
}

/// What composing one element's own classes produced.
#[derive(Default)]
struct Composed {
    consumed: Vec<String>,
    style: Vec<String>,
}

fn gradient_direction(class: &str) -> Option<&'static str> {
    Some(match class.strip_prefix("bg-gradient-to-")? {
        "t" => "to top",
        "tr" => "to top right",
        "r" => "to right",
        "br" => "to bottom right",
        "b" => "to bottom",
        "bl" => "to bottom left",
        "l" => "to left",
        "tl" => "to top left",
        _ => return None,
    })
}

fn compose_gradient(classes: &[&str], theme: &Theme, out: &mut Composed) {
    let Some(direction) = classes.iter().find_map(|c| gradient_direction(c)) else {
        return;
    };
    let stop = |prefix: &str| {
        classes.iter().find_map(|c| {
            let suffix = c.strip_prefix(prefix)?;
            color_value(suffix, theme).map(|color| (c.to_string(), suffix.to_string(), color))
        })
    };
    let Some((from_class, from_key, from)) = stop("from-") else {
        return;
    };
    let via = stop("via-");
    let to = stop("to-");
    // Tailwind fades to the same color at zero alpha when `to-*` is absent.
    let to_color = to
        .as_ref()
        .map(|(_, _, c)| c.clone())
        .or_else(|| color_value(&format!("{from_key}/0"), theme))
        .unwrap_or_else(|| "transparent".to_string());
    for c in classes {
        if gradient_direction(c).is_some() {
            out.consumed.push(c.to_string());
        }
    }
    out.consumed.push(from_class);
    let mut stops = vec![from];
    if let Some((class, _, color)) = via {
        out.consumed.push(class);
        stops.push(color);
    }
    if let Some((class, _, _)) = &to {
        out.consumed.push(class.clone());
    }
    stops.push(to_color);
    out.style.push(format!(
        "background:linear-gradient({direction}, {})",
        stops.join(", ")
    ));
}

/// `ring` and `outline` share one shape: a width class (`ring`, `ring-2`),
/// an optional color class, and a default color.
fn compose_edge(
    classes: &[&str],
    theme: &Theme,
    name: &str,
    default_width: u32,
    default_color: &str,
    out: &mut Composed,
) {
    let dash = format!("{name}-");
    let mut width = None;
    let mut width_classes = Vec::new();
    let mut color = None;
    let mut color_classes = Vec::new();
    let mut ignorable = Vec::new();
    for c in classes {
        if *c == name {
            width = Some(default_width);
            width_classes.push(c.to_string());
        } else if let Some(suffix) = c.strip_prefix(&dash) {
            if let Some(w) = edge_width(suffix) {
                width = Some(w);
                width_classes.push(c.to_string());
            } else if suffix == "inset" || suffix.starts_with("offset-") {
                ignorable.push(c.to_string());
            } else if let Some(col) = color_value(suffix, theme) {
                color = Some(col);
                color_classes.push(c.to_string());
            }
        }
    }
    let Some(width) = width else { return };
    out.consumed.extend(width_classes);
    out.consumed.extend(color_classes);
    out.consumed.extend(ignorable);
    if width > 0 {
        let color = color.unwrap_or_else(|| default_color.to_string());
        out.style
            .push(format!("box-shadow:0 0 0 {width}px {color}"));
    }
}

fn compose(classes: &[&str], theme: &Theme) -> Composed {
    let mut out = Composed::default();
    compose_gradient(classes, theme, &mut out);
    let mut edge = Composed::default();
    compose_edge(classes, theme, "ring", 3, "rgba(59,130,246,0.5)", &mut edge);
    let ring_applies = !edge.style.is_empty();
    let mut outline = Composed::default();
    compose_edge(classes, theme, "outline", 1, "#000000", &mut outline);
    out.consumed.extend(edge.consumed);
    out.consumed.extend(outline.consumed);
    // One `box-shadow` per element: a ring beats an outline.
    if ring_applies {
        out.style.extend(edge.style);
    } else {
        out.style.extend(outline.style);
    }
    out
}

/// How a container's `divide-*` classes style the borders between its
/// direct element children.
#[derive(Default)]
struct DividePlan {
    x: Option<u32>,
    y: Option<u32>,
    color: String,
    style: &'static str,
    consumed: Vec<String>,
}

impl DividePlan {
    fn from_classes(classes: &[&str], theme: &Theme) -> Self {
        let mut plan = Self {
            color: color_value("border", theme).unwrap_or_else(|| "#e5e7eb".to_string()),
            style: "solid",
            ..Self::default()
        };
        for c in classes {
            let Some(rest) = c.strip_prefix("divide-") else {
                continue;
            };
            let axis = |slot: &mut Option<u32>, suffix: &str| match suffix {
                "" => {
                    *slot = Some(1);
                    true
                }
                n => match n.strip_prefix('-').and_then(edge_width) {
                    Some(w) => {
                        *slot = Some(w);
                        true
                    }
                    None => false,
                },
            };
            let matched = if let Some(r) = rest.strip_prefix('x') {
                axis(&mut plan.x, r)
            } else if let Some(r) = rest.strip_prefix('y') {
                axis(&mut plan.y, r)
            } else if let Some(style) = ["solid", "dashed", "dotted", "double", "none"]
                .iter()
                .find(|s| **s == rest)
            {
                plan.style = style;
                true
            } else if let Some(color) = color_value(rest, theme) {
                plan.color = color;
                true
            } else {
                false
            };
            if matched {
                plan.consumed.push(c.to_string());
            }
        }
        // Style/color alone, with no axis, divides nothing -- and the
        // classes are then left for `unresolved_classes` to report.
        if plan.x.is_none() && plan.y.is_none() {
            plan.consumed.clear();
        }
        plan
    }

    fn child_style(&self, index: usize, child_classes: &[&str]) -> Option<String> {
        if index == 0
            || child_classes
                .iter()
                .any(|c| *c == "absolute" || *c == "fixed")
        {
            return None;
        }
        let mut decls = Vec::new();
        for (side, width) in [("top", self.y), ("left", self.x)] {
            if let Some(w) = width.filter(|w| *w > 0) {
                decls.push(format!(
                    "border-{side}-width:{w}px;border-{side}-style:{};border-{side}-color:{}",
                    self.style, self.color
                ));
            }
        }
        (!decls.is_empty()).then(|| decls.join(";"))
    }
}

/// One text edit inside an element's open tag.
struct Edit {
    at: usize,
    remove: usize,
    insert: String,
}

fn rewrite_open_tag(
    open_tag: &str,
    base: usize,
    element: &Element,
    consumed: &[String],
    styles: &[String],
) -> String {
    let remaining: Vec<&str> = element
        .classes
        .iter()
        .copied()
        .filter(|c| !consumed.iter().any(|x| x == c))
        .collect();
    let mut edits = Vec::new();
    if let Some(cv) = &element.class_value {
        if remaining.len() != element.classes.len() {
            edits.push(Edit {
                at: cv.start - base,
                remove: cv.len(),
                insert: remaining.join(" "),
            });
        }
    }
    if !styles.is_empty() {
        let joined = styles.join(";");
        match style_value_end(open_tag) {
            Some(end) => edits.push(Edit {
                at: end,
                remove: 0,
                insert: format!(";{joined}"),
            }),
            None => edits.push(Edit {
                at: element.attrs_start - base,
                remove: 0,
                insert: format!(r#" style="{joined}""#),
            }),
        }
    }
    edits.sort_by_key(|e| std::cmp::Reverse(e.at));
    let mut out = open_tag.to_string();
    for e in edits {
        out.replace_range(e.at..e.at + e.remove, &e.insert);
    }
    out
}

/// Offset (within the open tag) of the closing quote of an existing
/// `style="..."` attribute.
fn style_value_end(open_tag: &str) -> Option<usize> {
    let marker = r#" style=""#;
    let start = open_tag.find(marker)? + marker.len();
    Some(start + open_tag[start..].find('"')?)
}

fn rewrite_scope(s: &str, theme: &Theme, plan: &DividePlan, out: &mut String) {
    let nodes = scan_scope(s);
    let mut index = 0usize;
    for node in nodes {
        let Node::Element(e) = node else {
            if let Node::Text(r) = node {
                out.push_str(&s[r]);
            }
            continue;
        };
        let composed = compose(&e.classes, theme);
        let child_plan = DividePlan::from_classes(&e.classes, theme);
        let mut consumed = composed.consumed;
        consumed.extend(child_plan.consumed.iter().cloned());
        let mut styles = composed.style;
        if let Some(divider) = plan.child_style(index, &e.classes) {
            styles.push(divider);
        }
        index += 1;

        let open_end: usize = match &e.inner {
            Some(inner) => inner.start,
            None => e.span.end,
        };
        let open_tag = &s[e.span.start..open_end];
        out.push_str(&rewrite_open_tag(
            open_tag,
            e.span.start,
            &e,
            &consumed,
            &styles,
        ));
        if let Some(inner) = &e.inner {
            rewrite_scope(&s[Range::clone(inner)], theme, &child_plan, out);
            out.push_str(&s[inner.end..e.span.end]);
        }
    }
}

/// Composes gradients, rings/outlines and `divide-*` into inline styles.
/// Markup using none of them passes through unchanged.
pub fn rewrite_composites(html: &str, theme: &Theme) -> String {
    const MARKERS: &[&str] = &["divide-", "bg-gradient-to-", "ring", "outline"];
    if !MARKERS.iter().any(|m| html.contains(m)) {
        return html.to_string();
    }
    let mut out = String::with_capacity(html.len() + html.len() / 8);
    rewrite_scope(html, theme, &DividePlan::default(), &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(html: &str) -> String {
        rewrite_composites(html, &Theme::light())
    }

    #[test]
    fn divide_y_borders_every_child_but_the_first() {
        let out = run(r#"<ul class="divide-y"><li>a</li><li>b</li><li>c</li></ul>"#);
        assert!(out.contains(r#"<ul class="">"#), "{out}");
        assert!(out.contains("<li>a</li>"), "{out}");
        assert_eq!(out.matches("border-top-width:1px").count(), 2, "{out}");
    }

    #[test]
    fn divide_x_with_width_color_and_style() {
        let out = run(
            r#"<div class="flex divide-x-2 divide-dashed divide-red-500"><p>a</p><p>b</p></div>"#,
        );
        assert!(out.contains(r#"<div class="flex">"#), "{out}");
        assert!(out.contains("border-left-width:2px"), "{out}");
        assert!(out.contains("border-left-style:dashed"), "{out}");
        assert!(out.contains("border-left-color:#ef4444"), "{out}");
    }

    #[test]
    fn divide_keeps_an_existing_style_attribute() {
        let out = run(r#"<div class="divide-y"><p>a</p><p style="color:red">b</p></div>"#);
        assert!(
            out.contains(r#"style="color:red;border-top-width:1px"#),
            "{out}"
        );
    }

    #[test]
    fn divide_color_without_an_axis_is_left_alone() {
        let html = r#"<div class="divide-red-500"><p>a</p><p>b</p></div>"#;
        assert_eq!(run(html), html);
    }

    #[test]
    fn gradient_composes_direction_and_stops() {
        let out = run(r#"<div class="bg-gradient-to-r from-blue-500 to-red-500 p-4">x</div>"#);
        assert!(out.contains(r#"class="p-4""#), "{out}");
        assert!(
            out.contains("background:linear-gradient(to right, #3b82f6, #ef4444)"),
            "{out}"
        );
    }

    #[test]
    fn gradient_with_via_and_no_to_fades_to_transparent() {
        let out = run(r#"<div class="bg-gradient-to-b from-black via-blue-500">x</div>"#);
        assert!(
            out.contains("linear-gradient(to bottom, #000000, #3b82f6, rgba(0,0,0,0))"),
            "{out}"
        );
    }

    #[test]
    fn stops_without_a_direction_or_from_are_left_alone() {
        let html = r#"<div class="from-blue-500 to-red-500">x</div>"#;
        assert_eq!(run(html), html);
        let html = r#"<div class="bg-gradient-to-r to-red-500">x</div>"#;
        assert_eq!(run(html), html);
    }

    #[test]
    fn ring_becomes_a_zero_blur_spread_shadow() {
        let out = run(r#"<div class="ring-2 ring-red-500 p-2">x</div>"#);
        assert!(out.contains(r#"class="p-2""#), "{out}");
        assert!(out.contains("box-shadow:0 0 0 2px #ef4444"), "{out}");
        let out = run(r#"<div class="ring">x</div>"#);
        assert!(
            out.contains("box-shadow:0 0 0 3px rgba(59,130,246,0.5)"),
            "{out}"
        );
    }

    #[test]
    fn a_ring_color_without_a_width_does_nothing() {
        let html = r#"<div class="ring-red-500">x</div>"#;
        assert_eq!(run(html), html);
    }

    #[test]
    fn outline_uses_the_same_mechanism_and_ring_wins() {
        let out = run(r#"<div class="outline outline-blue-500">x</div>"#);
        assert!(out.contains("box-shadow:0 0 0 1px #3b82f6"), "{out}");
        let out = run(r#"<div class="outline-2 ring-4">x</div>"#);
        assert!(out.contains("box-shadow:0 0 0 4px"), "{out}");
        assert!(!out.contains("0 0 0 2px"), "{out}");
    }

    #[test]
    fn markup_without_composites_is_unchanged() {
        let html = r#"<div class="p-4"><p>hi</p></div>"#;
        assert_eq!(run(html), html);
    }
}
