//! `%Icon` end to end: a bundled or registered icon ends up as an image in the
//! PDF, and a mistake is a visible error rather than an empty space.

#[cfg(feature = "vector")]
use pdfcn_core::{icon_names, register_icon};
use pdfcn_core::{render_html, render_pdf, NoPartials, RenderOptions};
use printpdf::{parse_pdf_from_bytes, Op, PdfParseOptions};
use serde_json::json;

fn render(template: &str) -> (String, Vec<Op>) {
    let html = match render_html(template, &json!({}), &NoPartials) {
        Ok(html) => html,
        Err(e) => panic!("render_html failed: {e:?}"),
    };
    let bytes = match render_pdf(&html, &RenderOptions::default()) {
        Ok(bytes) => bytes,
        Err(e) => panic!("render_pdf failed: {e:?}"),
    };
    let mut warnings = Vec::new();
    let doc = match parse_pdf_from_bytes(&bytes, &PdfParseOptions::default(), &mut warnings) {
        Ok(doc) => doc,
        Err(e) => panic!("re-parsing failed: {e}"),
    };
    (html, doc.pages.into_iter().flat_map(|p| p.ops).collect())
}

fn images(ops: &[Op]) -> usize {
    ops.iter()
        .filter(|op| matches!(op, Op::UseXobject { .. }))
        .count()
}

// Drawn only with the `vector` feature (see the test below for without).
#[cfg(feature = "vector")]
#[test]
fn a_bundled_icon_is_drawn_as_an_image() {
    let (html, ops) = render(
        "%DocumentLayout(size=\"a4\")\n  %Icon(name=\"receipt\" size=\"32px\" color=\"#0f766e\")\n",
    );
    assert!(html.contains("pdfcn-icon:"), "{html}");
    assert_eq!(images(&ops), 1);
}

// Drawn only with the `vector` feature (see the test below for without).
#[cfg(feature = "vector")]
#[test]
fn the_same_icon_twice_is_one_image_resource_used_twice() {
    let (_, ops) = render(concat!(
        "%DocumentLayout(size=\"a4\")\n",
        "  %Icon(name=\"mail\")\n",
        "  %Icon(name=\"mail\")\n",
        "  %Icon(name=\"phone\")\n",
    ));
    assert_eq!(images(&ops), 3);
}

// Drawn only with the `vector` feature (see the test below for without).
#[cfg(feature = "vector")]
#[test]
fn a_registered_icon_is_available_next_to_the_bundled_ones() {
    register_icon(
        "xx-core-icon",
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="8"/></svg>"#,
    )
    .expect("register");
    assert!(icon_names().contains(&"xx-core-icon".to_string()));
    assert!(icon_names().contains(&"receipt".to_string()));
    let (_, ops) =
        render("%DocumentLayout(size=\"a4\")\n  %Icon(name=\"xx-core-icon\" color=\"red\")\n");
    assert_eq!(images(&ops), 1);
}

#[test]
fn an_unknown_icon_or_a_bad_colour_is_a_visible_error_and_draws_nothing() {
    for body in [
        "%Icon(name=\"definitely-not-an-icon\")",
        "%Icon(name=\"receipt\" color=\"red\\\"><script>\")",
        "%Icon(name=\"receipt\" stroke=\"9\")",
        "%Icon",
    ] {
        let (html, ops) = render(&format!("%DocumentLayout(size=\"a4\")\n  {body}\n"));
        assert!(html.contains("pdfcn-invalid-component"), "{body}: {html}");
        assert!(!html.contains("pdfcn-icon:"), "{body}");
        assert_eq!(images(&ops), 0, "{body}");
    }
}

/// Without `vector` an icon is the explicit "feature disabled" marker, never
/// an empty space or an unfilled placeholder.
#[cfg(not(feature = "vector"))]
#[test]
fn without_the_vector_feature_an_icon_is_a_visible_marker() {
    let (html, ops) = render("%DocumentLayout(size=\"a4\")\n  %Icon(name=\"receipt\")\n");
    assert!(!html.contains("pdfcn-icon:"), "{html}");
    assert!(html.contains("vector"), "{html}");
    assert_eq!(images(&ops), 0);
}
