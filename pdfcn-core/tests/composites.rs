//! `divide-*`, gradients, `ring-*`/`outline-*` and grid row utilities must
//! reach the drawn output (see `pdfcn_styles::rewrite_composites`).

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

fn assert_visible(with: &str, without: &str, what: &str) {
    assert_ne!(
        page_ops(with),
        page_ops(without),
        "{what} had no visible effect"
    );
}

#[test]
fn divide_y_draws_lines_between_children() {
    assert_visible(
        "%div(class=\"divide-y\")\n  %p a\n  %p b\n  %p c",
        "%div\n  %p a\n  %p b\n  %p c",
        "divide-y",
    );
}

#[test]
fn divide_x_draws_lines_between_flex_children() {
    assert_visible(
        "%div(class=\"flex divide-x divide-slate-300\")\n  %p(class=\"px-2\") a\n  %p(class=\"px-2\") b",
        "%div(class=\"flex\")\n  %p(class=\"px-2\") a\n  %p(class=\"px-2\") b",
        "divide-x",
    );
}

#[test]
fn gradient_utilities_paint_a_gradient() {
    assert_visible(
        "%div(class=\"bg-gradient-to-r from-blue-500 to-red-500 w-32 h-16\") x",
        "%div(class=\"w-32 h-16\") x",
        "bg-gradient-to-r",
    );
}

#[test]
fn ring_and_outline_paint_a_band_around_the_box() {
    assert_visible(
        "%div(class=\"ring-2 ring-red-500 bg-white w-32 h-16\") x",
        "%div(class=\"bg-white w-32 h-16\") x",
        "ring-2",
    );
    assert_visible(
        "%div(class=\"outline outline-blue-500 bg-white w-32 h-16\") x",
        "%div(class=\"bg-white w-32 h-16\") x",
        "outline",
    );
}

#[test]
fn grid_rows_split_a_fixed_height_container() {
    let cells = "\n  %div(class=\"bg-slate-200\") a\n  %div(class=\"bg-slate-300\") b\n  %div(class=\"bg-slate-400\") c\n  %div(class=\"bg-slate-500\") d";
    assert_visible(
        &format!("%div(class=\"grid grid-cols-2 grid-rows-4 h-40\"){cells}"),
        &format!("%div(class=\"grid grid-cols-2 grid-rows-2 h-40\"){cells}"),
        "grid-rows-4",
    );
}

#[test]
fn row_span_changes_the_layout() {
    let cell = |class: &str| {
        format!("\n  %div(class=\"bg-slate-200 h-8 {class}\") a\n  %div(class=\"bg-slate-300 h-8\") b\n  %div(class=\"bg-slate-400 h-8\") c")
    };
    assert_visible(
        &format!("%div(class=\"grid grid-cols-2\"){}", cell("row-span-2")),
        &format!("%div(class=\"grid grid-cols-2\"){}", cell("")),
        "row-span-2",
    );
}
