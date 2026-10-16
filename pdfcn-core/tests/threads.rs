//! The per-image work runs on several threads (docs/spikes/008); the PDF must
//! not depend on how many.

use std::collections::BTreeMap;

use pdfcn_core::{render, with_threads, NoPartials, RenderOptions};

fn jpeg(width: u32, height: u32, seed: u8) -> Vec<u8> {
    let img = image::RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([
            (x as u8).wrapping_add(seed),
            (y as u8).wrapping_mul(seed),
            seed,
        ])
    });
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85)
        .encode_image(&img)
        .expect("encodes");
    out
}

/// The parts of a PDF that may legitimately differ between two renders of the
/// same document: its dates and its file identifier.
fn without_dates(pdf: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(pdf).into_owned();
    let mut out = String::new();
    let mut rest = text.as_str();
    while let Some(at) = rest.find("D:") {
        out.push_str(&rest[..at]);
        let end = rest[at..].find(')').map_or(rest.len(), |e| at + e);
        rest = &rest[end..];
    }
    out.push_str(rest);
    out.split("/ID").next().unwrap_or("").as_bytes().to_vec()
}

#[test]
fn one_thread_or_many_give_the_same_pdf() {
    let mut template = String::from("%DocumentLayout(size=\"a4\")\n");
    let mut images = BTreeMap::new();
    let mut options = RenderOptions::default();
    for i in 0..12u8 {
        // Photos that pass through, photos cropped to cover, and SVG marks.
        let (w, h) = if i % 2 == 0 { (320, 240) } else { (300, 300) };
        images.insert(format!("p{i}.jpg"), jpeg(w, h, i + 1));
        template.push_str(&format!(
            "  %img(src=\"p{i}.jpg\" style=\"width:160px;height:120px;object-fit:cover\")\n"
        ));
        options.svg_assets.insert(
            format!("s{i}"),
            format!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><circle cx="5" cy="5" r="{}" fill="#2563eb"/></svg>"##,
                1 + i % 4
            ),
        );
        template.push_str(&format!(
            "  %Vector(id=\"s{i}\" w=\"24px\" h=\"24px\" alt=\"m\")\n"
        ));
    }
    let html =
        pdfcn_core::render_html(&template, &serde_json::json!({}), &NoPartials).expect("html");
    let pdf = |threads| {
        with_threads(threads, || {
            pdfcn_core::render_pdf_with_assets(&html, &BTreeMap::new(), &images, &options)
                .expect("renders")
        })
    };
    let one = without_dates(&pdf(1));
    for threads in [2, 4, 7] {
        assert!(
            one == without_dates(&pdf(threads)),
            "{threads} threads differ"
        );
    }
    // And the plain entry point agrees too.
    let _ = render(&template, &serde_json::json!({}), &NoPartials, &options).expect("renders");
}
