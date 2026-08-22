//! `%QRCode`: a scannable QR code encoding a caller-supplied value.
//!
//! The component itself emits only a placeholder
//! `<img src="pdfcn-qrcode:<hex payload>">`; `pdfcn-core`'s asset pass
//! (`assets::prepare_assets`) decodes the payload, renders the actual PNG
//! and registers it under that exact src just before layout. Keeping byte
//! generation out of this crate preserves the layering — components produce
//! markup, never binary assets — while callers still write the obvious
//! thing: `%QRCode(value="https://pay.example/INV-1042" size="96px")`.

use maud::{html, Markup};
use pdfcn_template::ResolvedAttr;

use crate::attr;

/// Hex-encodes the value for the placeholder src. Lowercase hex keeps the
/// generated attribute boring ASCII, safe inside any attribute position.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn qrcode(attrs: &[ResolvedAttr]) -> Markup {
    // Empty or missing values render as an inert marker instead of an
    // undecodable src, so the mistake is visible in the output rather than
    // silently producing a broken image.
    let Some(value) = attr(attrs, "value").filter(|v| !v.is_empty()) else {
        return html! {
            div class="pdfcn-invalid-component bg-destructive.text-white.text-xs.font-semibold.rounded.px-2.py-1" {
                "QRCode: a non-empty \"value\" is required"
            }
        };
    };
    let size = attr(attrs, "size").unwrap_or("96px");
    let alt = attr(attrs, "alt").unwrap_or("QR code");
    html! {
        img class="qrcode inline-block" src=(format!("pdfcn-qrcode:{}", hex_encode(value.as_bytes()))) style={ "width:" (size) ";height:" (size) ";" } alt=(alt);
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
    fn emits_a_hex_encoded_placeholder_src() {
        let out = qrcode(&[a("value", "INV-1042")]).into_string();
        assert!(out.contains("src=\"pdfcn-qrcode:"), "{out}");
        assert!(!out.contains("INV"), "raw value must be hex-encoded: {out}");
    }

    #[test]
    fn hex_encoding_round_trips_arbitrary_values() {
        let value = "https://pay.example/INV 1042?amt=42";
        let out = qrcode(&[a("value", value)]).into_string();
        let src = out
            .split("src=\"pdfcn-qrcode:")
            .nth(1)
            .and_then(|r| r.split('"').next())
            .unwrap();
        let decoded = (0..src.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&src[i..i + 2], 16).unwrap())
            .collect::<Vec<u8>>();
        assert_eq!(String::from_utf8(decoded).unwrap(), value);
    }

    #[test]
    fn size_and_alt_are_honored() {
        let out = qrcode(&[a("value", "x"), a("size", "48px"), a("alt", "Scan me")]).into_string();
        assert!(out.contains("width:48px;height:48px;"), "{out}");
        assert!(out.contains("alt=\"Scan me\""), "{out}");
    }

    #[test]
    fn empty_or_missing_value_renders_a_visible_marker() {
        assert!(qrcode(&[]).into_string().contains("non-empty"));
        assert!(qrcode(&[a("value", "")])
            .into_string()
            .contains("non-empty"));
    }
}
