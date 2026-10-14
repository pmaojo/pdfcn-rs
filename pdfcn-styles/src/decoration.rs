//! Makes `underline` / `line-through` work on block elements.
//!
//! In CSS, `text-decoration` set on an element decorates all the text inside
//! it, block descendants included. The `azul-layout` engine only draws the
//! line when the decoration sits on an *inline* element wrapping the text:
//! `<p class="underline">` renders plain, `<p><span class="underline">` is
//! underlined (verified against the engine, see
//! `docs/spikes/003-style-coverage.md`). So, as with `gap` (see `gap.rs`),
//! this pass rewrites the HTML: under any element carrying `underline` or
//! `line-through`, every run of text is wrapped in a `<span>` with the same
//! decoration classes. `no-underline` / `no-line-through` on a descendant
//! stops the propagation, matching how the utilities are used in practice.

use crate::gap::{scan_scope, Node};

const DECORATION_CLASSES: &[&str] = &["underline", "line-through"];

fn decorations_of(classes: &[&str], inherited: &[&'static str]) -> Vec<&'static str> {
    if classes.contains(&"no-underline") {
        return Vec::new();
    }
    let mut out: Vec<&'static str> = inherited.to_vec();
    for want in DECORATION_CLASSES {
        if classes.contains(want) && !out.contains(want) {
            out.push(want);
        }
    }
    out
}

fn wrap(text: &str, decorations: &[&'static str]) -> String {
    let mut out = text.to_string();
    for class in decorations.iter().rev() {
        out = format!(r#"<span class="{class}">{out}</span>"#);
    }
    out
}

fn rewrite_scope(s: &str, inherited: &[&'static str], out: &mut String) {
    for node in scan_scope(s) {
        match node {
            Node::Text(range) => {
                let text = &s[range];
                if inherited.is_empty() || text.trim().is_empty() {
                    out.push_str(text);
                    continue;
                }
                let start = text.len() - text.trim_start().len();
                let end = text.trim_end().len();
                out.push_str(&text[..start]);
                out.push_str(&wrap(&text[start..end], inherited));
                out.push_str(&text[end..]);
            }
            Node::Element(e) => match &e.inner {
                Some(inner) => {
                    let decorations = decorations_of(&e.classes, inherited);
                    out.push_str(&s[e.span.start..inner.start]);
                    rewrite_scope(&s[inner.clone()], &decorations, out);
                    out.push_str(&s[inner.end..e.span.end]);
                }
                None => out.push_str(&s[e.span]),
            },
        }
    }
}

/// Propagates `underline` / `line-through` from any element down onto its
/// text as inline spans. Markup using neither utility passes through
/// unchanged.
pub fn rewrite_text_decorations(html: &str) -> String {
    if !html.contains("underline") && !html.contains("line-through") {
        return html.to_string();
    }
    let mut out = String::with_capacity(html.len() + html.len() / 8);
    rewrite_scope(html, &[], &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_block_text_in_a_decorated_span() {
        let out = rewrite_text_decorations(r#"<p class="underline">Hello</p>"#);
        assert_eq!(
            out,
            r#"<p class="underline"><span class="underline">Hello</span></p>"#
        );
    }

    #[test]
    fn reaches_text_in_nested_elements_and_keeps_whitespace() {
        let out = rewrite_text_decorations(
            r#"<div class="line-through"> a <b>b</b> </div><p>untouched</p>"#,
        );
        assert!(
            out.contains(r#" <span class="line-through">a</span> "#),
            "{out}"
        );
        assert!(
            out.contains(r#"<b><span class="line-through">b</span></b>"#),
            "{out}"
        );
        assert!(out.contains("<p>untouched</p>"), "{out}");
    }

    #[test]
    fn both_decorations_nest_two_spans() {
        let out = rewrite_text_decorations(r#"<p class="underline line-through">x</p>"#);
        assert!(
            out.contains(r#"<span class="underline"><span class="line-through">x</span></span>"#),
            "{out}"
        );
    }

    #[test]
    fn no_underline_stops_propagation() {
        let out = rewrite_text_decorations(
            r#"<div class="underline"><a class="no-underline">link</a></div>"#,
        );
        assert!(out.contains(r#"<a class="no-underline">link</a>"#), "{out}");
    }

    #[test]
    fn markup_without_decorations_is_unchanged() {
        let html = r#"<div class="p-4"><p>hi</p></div>"#;
        assert_eq!(rewrite_text_decorations(html), html);
    }
}
