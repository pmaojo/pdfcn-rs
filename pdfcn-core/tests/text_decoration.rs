//! `underline` / `line-through` on a block element must reach the drawn
//! output. The layout engine only draws the line for an inline element
//! wrapping the text, so `pdfcn-styles` rewrites the HTML (see
//! `pdfcn_styles::rewrite_text_decorations`); this checks the whole pipeline
//! against the engine, not just the string rewrite.

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
fn underline_on_a_block_draws_a_line() {
    let plain = page_ops("%p Hello world text");
    let underlined = page_ops("%p.underline Hello world text");
    assert_ne!(
        plain, underlined,
        "underline on a <p> had no visible effect"
    );
}

#[test]
fn line_through_on_a_block_draws_a_line() {
    let plain = page_ops("%p Hello world text");
    let struck = page_ops("%p.line-through Hello world text");
    assert_ne!(plain, struck, "line-through on a <p> had no visible effect");
}

#[test]
fn underline_and_line_through_differ_from_each_other() {
    assert_ne!(
        page_ops("%p.underline Hello world text"),
        page_ops("%p.line-through Hello world text")
    );
}
