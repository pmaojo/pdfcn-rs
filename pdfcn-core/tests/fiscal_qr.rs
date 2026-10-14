//! `%FiscalQR` end to end through the public API: a profile registered from
//! JSON drives the printed code, and a wrong value is a visible error.

use pdfcn_core::{register_fiscal_profile, render_html, render_pdf, NoPartials, RenderOptions};
use serde_json::json;

const PROFILE: &str = r#"{
    "id": "xx-core-test",
    "label": "CHECK",
    "ecc": "Q",
    "fields": {
        "id": { "type": "text", "charset": "digits", "length": 4 },
        "total": { "type": "amount", "max_decimals": 2, "fixed_decimals": 2 }
    },
    "payload": "https://verify.test/?i={id}&t={total}"
}"#;

fn html(template: &str) -> String {
    match render_html(template, &json!({}), &NoPartials) {
        Ok(html) => html,
        Err(e) => panic!("render_html failed: {e:?}"),
    }
}

#[test]
fn a_registered_profile_prints_its_code_and_label() {
    register_fiscal_profile(PROFILE).expect("profile");
    let html = html(
        "%DocumentLayout(size=\"a4\")\n  %FiscalQR(profile=\"xx-core-test\" id=\"1234\" total=\"9.5\")\n",
    );
    assert!(html.contains("CHECK"), "{html}");
    assert!(
        html.contains("pdfcn-qrcode:Q."),
        "level Q must reach the placeholder: {html}"
    );
    // And the whole thing renders to a PDF.
    let pdf = render_pdf(&html, &RenderOptions::default()).expect("pdf");
    assert!(pdf.starts_with(b"%PDF"));
}

#[test]
fn a_wrong_value_is_a_visible_error_and_no_code() {
    register_fiscal_profile(PROFILE).expect("profile");
    let html = html("%DocumentLayout(size=\"a4\")\n  %FiscalQR(profile=\"xx-core-test\" id=\"12\" total=\"9.5\")\n");
    assert!(html.contains("pdfcn-invalid-component"), "{html}");
    assert!(html.contains("exactly 4"), "{html}");
    assert!(!html.contains("pdfcn-qrcode:"), "{html}");
}

#[test]
fn the_built_in_spanish_profile_is_always_registered() {
    assert!(pdfcn_core::fiscal_profile_ids().contains(&"es-verifactu".to_string()));
}

#[test]
fn an_invalid_profile_is_refused_with_the_reason() {
    let err =
        register_fiscal_profile(r#"{"id":"xx-bad","fields":{},"payload":"{nope}"}"#).unwrap_err();
    assert!(err.contains("nope"), "{err}");
}
