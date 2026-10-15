//! End-to-end behaviour of the non-HAML front-ends through the whole
//! pipeline (`pdfcn-formats` has the parser-level unit tests).

use pdfcn_core::{render_html_as, render_html_with_theme, Format, NoPartials, Theme};
use serde_json::json;

fn html(source: &str, format: Option<Format>, data: serde_json::Value) -> String {
    render_html_as(source, format, &data, &NoPartials, &Theme::light()).expect("render")
}

#[test]
fn html_and_haml_agree_on_the_rendered_body() {
    let data = json!({ "n": "Ada" });
    let a = html("%p.font-bold Hello {{ n }}", None, data.clone());
    let b = html("<p class=\"font-bold\">Hello {{ n }}</p>", None, data);
    assert_eq!(a, b);
}

#[test]
fn format_is_detected_from_the_text() {
    let data = json!({});
    assert!(html("<h2>x</h2>", None, data.clone()).contains("<h2"));
    assert!(html("# x", None, data.clone()).contains("<h1"));
    assert!(html("[{\"type\":\"h3\",\"text\":\"x\"}]", None, data.clone()).contains("<h3"));
    assert!(html(
        "\\documentclass{article}\\begin{document}\\section{x}\\end{document}",
        None,
        data
    )
    .contains("<h2"));
}

#[test]
fn explicit_format_beats_detection() {
    // Valid YAML that no detector could claim.
    let out = html("- type: h4\n  text: hi\n", Some(Format::Yaml), json!({}));
    assert!(out.contains("<h4"), "{out}");
    // Typst `= Heading` would otherwise be plain HAML text.
    let out = html("= Title\n", Some(Format::Typst), json!({}));
    assert!(out.contains("<h1"), "{out}");
}

#[test]
fn markdown_front_matter_is_default_data_and_caller_data_wins() {
    let src = "---\ntitle: From front matter\nauthor: Ann\n---\n# {{ title }} by {{ author }}\n";
    let out = html(src, None, json!({}));
    assert!(out.contains("From front matter by Ann"), "{out}");
    let out = html(src, None, json!({ "title": "From data" }));
    assert!(out.contains("From data by Ann"), "{out}");
}

#[test]
fn author_style_blocks_are_hoisted_into_the_stylesheet() {
    let out = html(
        "<style>.hl{background:#fef08a}</style><p class=\"hl\">x</p>",
        None,
        json!({}),
    );
    // In the head stylesheet, not as a stray element in the body.
    let (head, body) = out.split_once("<body>").unwrap();
    assert!(head.contains(".hl{background:#fef08a}"), "{out}");
    assert!(!body.contains("<style"), "{out}");
}

#[test]
fn author_classes_are_not_reported_as_unresolved() {
    let doc = html(
        "<style>.hl{color:red}</style><p class=\"hl p-4 nonsense\">x</p>",
        None,
        json!({}),
    );
    let unresolved = pdfcn_core::unresolved_classes(&doc, &Theme::light());
    assert!(!unresolved.contains("hl"), "{unresolved:?}");
    assert!(unresolved.contains("nonsense"), "{unresolved:?}");
}

#[test]
fn text_punctuation_hugs_inline_elements() {
    let out = html(
        "<p>See <a href=\"u\">link</a>, then <b>bold</b>.</p>",
        None,
        json!({}),
    );
    assert!(out.contains("</a>, then"), "{out}");
    assert!(out.contains("</b>."), "{out}");
}

#[test]
fn components_inside_markdown_render() {
    let out = html(
        "<Badge label=\"New\"></Badge>\n",
        Some(Format::Markdown),
        json!({}),
    );
    assert!(out.contains("New"), "{out}");
}

#[test]
fn every_format_produces_a_pdf() {
    let cases: [(&str, Format); 5] = [
        ("<h1>x</h1>", Format::Html),
        ("# x\n", Format::Markdown),
        ("- {type: p, text: x}\n", Format::Yaml),
        ("= x\n", Format::Typst),
        ("\\section{x}", Format::Latex),
    ];
    for (src, fmt) in cases {
        let doc = html(src, Some(fmt), json!({}));
        let pdf = pdfcn_core::render_pdf(&doc, &pdfcn_core::RenderOptions::default()).unwrap();
        assert!(pdf.starts_with(b"%PDF"), "{fmt:?}");
    }
    // Keep the auto-detecting entry point exercised too.
    let _ = render_html_with_theme("<p>x</p>", &json!({}), &NoPartials, &Theme::light()).unwrap();
}
