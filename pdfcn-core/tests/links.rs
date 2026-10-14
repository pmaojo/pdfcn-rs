//! `<a href>` must become real PDF link annotations: external URIs, `#id`
//! jumps to the right page and position, one area per wrapped line, and no
//! annotation for unsafe or unresolvable targets.

use pdfcn_core::{render_html, render_pdf, NoPartials, RenderOptions};
use printpdf::{parse_pdf_from_bytes, Actions, Op, PdfParseOptions};
use serde_json::json;

/// (page number from 1, annotation) for every link in the rendered PDF.
fn links(template: &str) -> Vec<(usize, printpdf::LinkAnnotation)> {
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
    let mut out = Vec::new();
    for (i, page) in doc.pages.iter().enumerate() {
        for op in &page.ops {
            if let Op::LinkAnnotation { link } = op {
                out.push((i + 1, link.clone()));
            }
        }
    }
    out
}

#[test]
fn external_and_mail_links_become_uri_actions() {
    let found = links(
        "%DocumentLayout(size=\"a4\")\n  %p\n    %a(href=\"https://example.com/a?b=1&c=2\") docs\n    %a(href=\"mailto:hi@example.com\") mail\n",
    );
    let uris: Vec<String> = found
        .iter()
        .filter_map(|(_, l)| match &l.actions {
            Actions::Uri(u) => Some(u.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(uris, vec!["https://example.com/a?b=1&c=2", "mailto:hi@example.com"]);
    for (_, l) in &found {
        assert!(l.rect.width.0 > 5.0 && l.rect.height.0 > 5.0, "{:?}", l.rect);
    }
}

#[test]
fn an_id_jump_lands_on_the_page_of_its_target() {
    let found = links(
        "%DocumentLayout(size=\"a4\")\n  %p\n    %a(href=\"#later\") go\n  .break-before-page\n  %h2#later Later\n",
    );
    assert_eq!(found.len(), 1, "{found:?}");
    match &found[0].1.actions {
        Actions::Goto(printpdf::Destination::Xyz { page, top, .. }) => {
            assert_eq!(*page, 2);
            assert!(top.is_some());
        }
        other => panic!("expected a jump, got {other:?}"),
    }
}

#[test]
fn unsafe_or_unresolvable_targets_are_not_links() {
    let found = links(
        "%DocumentLayout(size=\"a4\")\n  %p\n    %a(href=\"javascript:alert(1)\") a\n    %a(href=\"#nowhere\") b\n    %a(href=\"ftp://host/f\") c\n",
    );
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_wrapped_link_gets_one_area_per_line() {
    let long = "word ".repeat(80);
    let found = links(&format!(
        "%DocumentLayout(size=\"a4\")\n  %p\n    %a(href=\"https://example.com\") {long}\n"
    ));
    assert!(found.len() >= 2, "{} areas", found.len());
}

#[test]
fn a_block_link_covers_its_whole_box() {
    let found = links(
        "%DocumentLayout(size=\"a4\")\n  %a.block.border.p-4(href=\"https://example.com\")\n    %p Card\n",
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].1.rect.width.0 > 300.0, "{:?}", found[0].1.rect);
}
