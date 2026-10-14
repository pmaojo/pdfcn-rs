//! `shadow-*` utilities must reach the drawn output. The layout engine emits
//! a box-shadow display item that upstream printpdf's HTML bridge drops; the
//! `pmaojo/printpdf` fork (see `docs/spikes/003-style-coverage.md`) paints it.

use pdfcn_core::{render_html, render_pdf, NoPartials, RenderOptions};
use printpdf::{parse_pdf_from_bytes, PdfParseOptions};
use serde_json::json;

fn page_ops(template: &str) -> String {
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
    doc.pages
        .iter()
        .map(|p| format!("{:?}", p.ops))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn shadow_utilities_change_the_drawn_output() {
    let flat = page_ops("%div(class=\"bg-white w-32 h-16\") x");
    for shadow in [
        "shadow-sm",
        "shadow",
        "shadow-md",
        "shadow-lg",
        "shadow-2xl",
    ] {
        let lifted = page_ops(&format!("%div(class=\"bg-white w-32 h-16 {shadow}\") x"));
        assert_ne!(flat, lifted, "{shadow} had no visible effect");
    }
}

#[test]
fn a_larger_shadow_draws_more_than_a_smaller_one() {
    let sm = page_ops("%div(class=\"bg-white w-32 h-16 shadow-sm\") x");
    let xl = page_ops("%div(class=\"bg-white w-32 h-16 shadow-xl\") x");
    assert_ne!(sm, xl);
}

#[test]
fn opacity_utilities_change_the_drawn_output() {
    let opaque = page_ops("%div(class=\"bg-red-500 w-32 h-16\") x");
    for opacity in ["opacity-50", "opacity-25"] {
        let faded = page_ops(&format!("%div(class=\"bg-red-500 w-32 h-16 {opacity}\") x"));
        assert!(opaque != faded, "{opacity} had no visible effect");
    }
}
