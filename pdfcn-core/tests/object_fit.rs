//! `object-fit` must reach the drawn output even when the box's size is only
//! known to the layout engine (a relative width). `pdfcn-core`'s own crop
//! pass can only help when both dimensions are fixed px in the markup; the
//! patched engine (see `docs/spikes/003-style-coverage.md`) handles the rest.

use std::collections::BTreeMap;

use pdfcn_core::{render_pdf_with_assets, RenderOptions};
use printpdf::{parse_pdf_from_bytes, PdfParseOptions};

fn png(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbImage::from_fn(w, h, |x, _| image::Rgb([(x * 3 % 256) as u8, 100, 200]));
    let mut out = Vec::new();
    let encoded = image::DynamicImage::ImageRgb8(img)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png);
    assert!(encoded.is_ok(), "encoding the test image failed");
    out
}

fn page_ops(img_style: &str) -> String {
    let html = format!(
        "<!DOCTYPE html><html><body><div style=\"width:300px\">\
         <img src=\"a.png\" style=\"{img_style}\"></div></body></html>"
    );
    let mut images = BTreeMap::new();
    images.insert("a.png".to_string(), png(80, 40));
    let bytes =
        match render_pdf_with_assets(&html, &BTreeMap::new(), &images, &RenderOptions::default()) {
            Ok(bytes) => bytes,
            Err(e) => panic!("render failed: {e:?}"),
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
fn object_fit_changes_the_drawn_image_for_a_relative_width_box() {
    // Square-ish box (width relative, height fixed) for a 2:1 image.
    let stretched = page_ops("width:100%;height:150px");
    for fit in ["cover", "contain", "none", "scale-down"] {
        let fitted = page_ops(&format!("width:100%;height:150px;object-fit:{fit}"));
        assert!(
            stretched != fitted,
            "object-fit:{fit} had no visible effect"
        );
    }
}

#[test]
fn object_position_moves_a_cropped_image() {
    let centered = page_ops("width:100%;height:150px;object-fit:cover");
    let left = page_ops("width:100%;height:150px;object-fit:cover;object-position:left");
    assert!(centered != left, "object-position had no visible effect");
}
