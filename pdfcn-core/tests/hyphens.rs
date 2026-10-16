//! CSS `hyphens: auto` behind the `hyphenation` feature (off by default,
//! docs/spikes/009): with or without the dictionaries the text is drawn, and
//! without them it is never an error.

use pdfcn_core::{render_pdf, RenderOptions};

#[test]
fn hyphens_auto_renders_with_or_without_the_feature() {
    let html = r#"<html><body><p lang="en" style="width:70px;hyphens:auto">internationalization characteristically</p></body></html>"#;
    let pdf = render_pdf(html, &RenderOptions::default()).expect("renders");
    assert!(pdf.starts_with(b"%PDF"));
}
