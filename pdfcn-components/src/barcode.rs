//! `%Barcode` (Ola 1.3), behind the crate's `vector` cargo feature: a
//! scannable 1D barcode encoding a caller-supplied value, same placeholder
//! convention as `%QRCode` -- `<img src="pdfcn-barcode:<scheme>:<hex>">`,
//! with pdfcn-core's asset pass doing the actual encoding and rasterizing.
//!
//! Schemes: `code128` (any printable ASCII) and `ean13` (12 or 13 digits;
//! the check digit is computed for 12 and validated for 13). DataMatrix /
//! PDF417 land later as vetted crates, not hand-rolled ECC.

use maud::{html, Markup};
use pdfcn_template::ResolvedAttr;

use crate::{attr, attr_or, hex_encode, invalid_component};

const SCHEMES: [&str; 2] = ["code128", "ean13"];

pub fn barcode(attrs: &[ResolvedAttr]) -> Markup {
    let Some(value) = attr(attrs, "value").filter(|v| !v.is_empty()) else {
        return html! {
            div class=(invalid_component()) { "Barcode: a non-empty \"value\" is required" }
        };
    };
    let scheme = attr_or(attrs, "scheme", "code128");
    if !SCHEMES.contains(&scheme) {
        return html! {
            div class=(invalid_component()) {
                "Barcode: unknown scheme \"" (scheme) "\" (supported: code128, ean13)"
            }
        };
    }
    if scheme == "ean13"
        && (value.len() != 12 && value.len() != 13 || !value.bytes().all(|b| b.is_ascii_digit()))
    {
        return html! {
            div class=(invalid_component()) {
                "Barcode: ean13 needs exactly 12 or 13 digits"
            }
        };
    }
    let w = attr_or(attrs, "w", "240px");
    let h = attr_or(attrs, "h", "60px");
    let src = format!("pdfcn-barcode:{scheme}:{}", hex_encode(value.as_bytes()));
    let alt = attr_or(attrs, "alt", "barcode");
    html! {
        img class="pdfcn-barcode inline-block" src=(src) style={ "width:" (w) ";height:" (h) ";" } alt=(alt);
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

    fn decode_src(out: &str) -> String {
        let marker = r#"src="pdfcn-barcode:"#;
        let rest = out
            .split(marker)
            .nth(1)
            .and_then(|r| r.split('"').next())
            .unwrap();
        let (scheme_hexed, _) = rest.split_once('"').unwrap();
        let hexed = scheme_hexed.split(':').nth(1).unwrap();
        (0..hexed.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hexed[i..i + 2], 16).unwrap())
            .map(|b| b as char)
            .collect()
    }

    #[test]
    fn encodes_the_value_into_a_schemed_placeholder() {
        let out = barcode(&[a("scheme", "code128"), a("value", "INV-2026-0042")]).into_string();
        assert!(out.contains("pdfcn-barcode:code128:"), "{out}");
        assert_eq!(decode_src(&out), "INV-2026-0042");
    }

    #[test]
    fn ean13_is_validated_upfront() {
        assert!(barcode(&[a("scheme", "ean13"), a("value", "400638133393")])
            .into_string()
            .contains("pdfcn-barcode:ean13:"));
        assert!(barcode(&[a("scheme", "ean13"), a("value", "40")])
            .into_string()
            .contains("needs exactly"));
        assert!(barcode(&[a("scheme", "ean13"), a("value", "40063813339a")])
            .into_string()
            .contains("needs exactly"));
    }

    #[test]
    fn unknown_scheme_and_missing_value_are_explicit_markers() {
        assert!(barcode(&[a("scheme", "qr"), a("value", "x")])
            .into_string()
            .contains("unknown scheme"));
        assert!(barcode(&[]).into_string().contains("non-empty"));
    }

    #[test]
    fn size_attributes_flow_through() {
        let out = barcode(&[a("value", "X"), a("w", "300px"), a("h", "80px")]).into_string();
        assert!(out.contains("width:300px;height:80px;"), "{out}");
    }
}
