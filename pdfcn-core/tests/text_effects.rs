//! `vertical-align`, `transform` and `text-shadow` must reach the drawn output.
//! They needed fixes in the engine forks (see `docs/spikes/003-style-coverage.md`):
//! the dense text form dropped a shifted run's y, CSS transforms never became
//! reference frames in paged export, and neither transforms nor text shadows
//! were painted by the printpdf bridge.

use pdfcn_core::{render_html, render_pdf, NoPartials, RenderOptions};
use printpdf::{parse_pdf_from_bytes, Op, PdfParseOptions};
use serde_json::json;

fn page_ops(template: &str) -> Vec<Op> {
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
    doc.pages.into_iter().flat_map(|p| p.ops).collect()
}

/// The y of every text run, in order.
fn text_ys(ops: &[Op]) -> Vec<f32> {
    ops.iter()
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

#[test]
fn super_and_sub_move_a_span_off_the_baseline() {
    let doc = |class: &str| format!("%p\n  base\n  %span(class=\"{class}\") mid\n  tail");
    let flat = text_ys(&page_ops(&doc("text-xs")));
    let up = text_ys(&page_ops(&doc("text-xs align-super")));
    let down = text_ys(&page_ops(&doc("text-xs align-sub")));
    assert_eq!(flat.len(), up.len());
    assert!(
        up.iter().zip(&flat).any(|(u, f)| u > f),
        "super never rose: {up:?} vs {flat:?}"
    );
    assert!(
        down.iter().zip(&flat).any(|(d, f)| d < f),
        "sub never dropped: {down:?} vs {flat:?}"
    );
}

#[test]
fn a_transform_wraps_the_element_in_a_non_identity_matrix() {
    let has_cm = |template: &str| {
        page_ops(template).iter().any(|op| match op {
            Op::SetTransformationMatrix { matrix } => {
                let s = format!("{matrix:?}");
                // Image placement also emits a cm; a rotation has a non-zero skew term.
                s.contains("Raw") && !s.contains("[1.0, 0.0, 0.0, 1.0, 0.0, 0.0]")
            }
            _ => false,
        })
    };
    let plain = has_cm("%div(class=\"w-32 h-16 bg-blue-500\") x");
    let rotated = has_cm("%div(class=\"w-32 h-16 bg-blue-500 rotate-12\") x");
    assert!(!plain, "an untransformed box must not emit a matrix");
    assert!(rotated, "rotate-12 emitted no transformation matrix");
}

#[test]
fn transform_variants_all_change_the_output() {
    let base = format!("{:?}", page_ops("%div(class=\"w-32 h-16 bg-blue-500\") x"));
    for class in [
        "rotate-45",
        "scale-75",
        "translate-x-8",
        "-translate-y-2",
        "skew-x-6",
        "scale-110 rotate-6",
    ] {
        let out = format!(
            "{:?}",
            page_ops(&format!("%div(class=\"w-32 h-16 bg-blue-500 {class}\") x"))
        );
        assert!(out != base, "{class} had no visible effect");
    }
}

#[test]
fn text_shadow_draws_the_text_again() {
    let shows = |template: &str| {
        page_ops(template)
            .iter()
            .filter(|op| matches!(op, Op::ShowText { .. }))
            .count()
    };
    let plain = shows("%p Hello");
    let shadowed = shows("%p(class=\"text-shadow\") Hello");
    assert!(plain > 0);
    assert!(
        shadowed > plain,
        "text-shadow drew no shadow copy ({shadowed} vs {plain})"
    );
}
