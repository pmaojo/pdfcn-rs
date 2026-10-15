//! A class with brackets (`bg-[#0f766e]`, `w-[calc(100%-2rem)]`) must not cost
//! other classes their styling. The layout engine's CSS parser used to drop
//! every rule that followed a selector containing `[` or `]`, so a rule for
//! `p-8` that sorted after `bg-[...]` simply vanished.

use pdfcn_core::{render_html, render_pdf, NoPartials, RenderOptions};
use printpdf::{parse_pdf_from_bytes, Op, PdfParseOptions};
use serde_json::json;

/// The y of every text run, in order.
fn text_ys(template: &str) -> Vec<f32> {
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
        .into_iter()
        .flat_map(|p| p.ops)
        .filter_map(|op| match op {
            Op::SetTextMatrix { matrix } => {
                let s = format!("{matrix:?}");
                let inner = s.split('[').nth(1)?.split(']').next()?.to_string();
                inner.split(',').nth(5)?.trim().parse::<f32>().ok()
            }
            _ => None,
        })
        .collect()
}

fn doc(class: &str) -> String {
    format!("%DocumentLayout(size=\"a4\")\n  %p(class=\"{class}\") Hello\n")
}

#[test]
fn a_bracket_class_does_not_cost_later_classes_their_rules() {
    let reference = text_ys(&doc("p-8"));
    assert!(!reference.is_empty());
    for with_brackets in [
        "bg-[#ff0000] p-8",
        "bg-[rgb(255,0,0)] p-8",
        "w-[10px] p-8",
        "w-[calc(100%-2rem)] p-8",
        "grid-cols-[1fr_2fr] p-8",
        "tracking-[0.05em] p-8",
        "font-[600] p-8",
    ] {
        assert_eq!(
            text_ys(&doc(with_brackets)),
            reference,
            "`{with_brackets}` must leave p-8's padding in place"
        );
    }
}

#[test]
fn the_bracket_class_itself_still_applies() {
    // A taller line box than the default proves the arbitrary value took effect.
    let plain = text_ys(&doc("p-2"));
    let padded = text_ys(&doc("p-[2rem]"));
    assert!(padded[0] < plain[0], "{padded:?} vs {plain:?}");
}
