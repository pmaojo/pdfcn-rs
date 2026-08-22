//! `%Vector`: renders caller-supplied SVG onto the Wave 1 vector substrate
//! (Ola 1.2) -- the escape hatch that covers math/LaTeX output, musical
//! notation, CAD exports and anything else a client can serialize as SVG,
//! without pdfcn shipping any of those engines.
//!
//! The markup carries only `<img src="pdfcn-vector:{id}">`; the SVG source
//! itself travels through `RenderOptions::svg_assets` (`id -> SVG text`),
//! because an arbitrary-length XML document doesn't belong hex-encoded in an
//! attribute. Callers write the obvious thing:
//!
//! ```haml
//! %Vector(id="org-chart" w="480px" h="280px")
//! ```
//!
//! Requires pdfcn-core built with its `vector` cargo feature; without it the
//! placeholder degrades like any unresolved image.

use maud::{html, Markup};
use pdfcn_template::ResolvedAttr;

use crate::{attr, attr_or, invalid_component};

pub fn vector(attrs: &[ResolvedAttr]) -> Markup {
    // An empty id would look up nothing at render time; surface the mistake
    // in the document instead of silently rendering a broken image.
    let Some(id) = attr(attrs, "id").filter(|v| !v.is_empty()) else {
        return html! {
            div class=(invalid_component()) {
                "Vector: a non-empty \"id\" is required (the SVG source goes in RenderOptions::svg_assets)"
            }
        };
    };
    let w = attr_or(attrs, "w", "320px");
    let h = attr_or(attrs, "h", "200px");
    let alt = attr_or(attrs, "alt", "");
    html! {
        img class="pdfcn-vector inline-block" src=(format!("pdfcn-vector:{id}")) style={ "width:" (w) ";height:" (h) ";" } alt=(alt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(name: &str, value: &str) -> ResolvedAttr {
        ResolvedAttr {
            name: name.into(),
            value: value.into(),
        }
    }

    #[test]
    fn emits_a_side_channel_placeholder_src() {
        let out = vector(&[a("id", "org-chart"), a("w", "480px"), a("h", "280px")]).into_string();
        assert!(out.contains(r#"src="pdfcn-vector:org-chart""#), "{out}");
        assert!(out.contains("width:480px;height:280px;"), "{out}");
        // The SVG source never travels in the markup -- only the id does.
        assert!(!out.contains("<svg"), "{out}");
    }

    #[test]
    fn defaults_are_sensible() {
        let out = vector(&[a("id", "map")]).into_string();
        assert!(out.contains("width:320px;height:200px;"), "{out}");
    }

    #[test]
    fn missing_id_renders_a_visible_marker() {
        assert!(vector(&[]).into_string().contains("non-empty"));
        assert!(vector(&[a("id", "")]).into_string().contains("non-empty"));
    }
}
