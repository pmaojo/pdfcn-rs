//! Golden snapshots of what the layout engine actually draws.
//!
//! Every other PDF assertion in this workspace is some flavour of
//! `assert!(bytes.starts_with(b"%PDF"))`. That proves the pipeline produced
//! *a* document; it says nothing about whether the invoice's table still has
//! its borders, whether the catalog's price badge is still on top of the
//! photo, or whether a `printpdf` bump silently moved every baseline by two
//! points. A layout regression is currently invisible to CI, which is a poor
//! footing from which to start changing the renderer.
//!
//! The oracle here is `printpdf`'s own `render_to_svg`: take the PDF bytes
//! the pipeline produced, parse them back into a document, and re-render each
//! page to SVG. The result is a text description of the drawing operations --
//! glyph positions, fills, paths, image placements -- which diffs
//! meaningfully in a pull request, unlike the compressed binary it came from.
//!
//! Two things are deliberately *not* golden-tested here:
//!
//! * **Binary payloads.** `render_to_svg` inlines every image *and* every
//!   subsetted font program as a base64 `data:` URI, which together are most
//!   of the file and none of it reviewable. Each payload is replaced with a
//!   stable digest, so changed bytes still fail the test while unchanged ones
//!   cost one line.
//! * **Vector content.** `render_to_svg` only materialises `XObject::Image`
//!   (its image map is populated solely from that variant), so a form XObject
//!   renders as nothing at all. If the vector substrate lands, these
//!   snapshots will show its output as blank -- they would pass trivially and
//!   prove nothing. That path needs its own oracle; see the roadmap.
//!
//! Refresh after an intentional change with:
//!
//! ```sh
//! UPDATE_GOLDEN=1 cargo test -p pdfcn-core --test golden
//! ```
//!
//! and read the resulting diff before committing it -- the point of the
//! snapshot is that a human looks at what moved.

use std::path::{Path, PathBuf};

use pdfcn_core::{render_files, PageConfig};
use printpdf::{parse_pdf_from_bytes, render_to_svg, PdfParseOptions, PdfToSvgOptions};

/// Every example that ships in `examples/`. These are the documents the
/// README tells a new user to run first, so a regression in one of them is a
/// regression in the project's own front door.
const EXAMPLES: &[&str] = &["invoice", "catalog", "showcase"];

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is `<repo>/pdfcn-core`.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
}

/// FNV-1a, spelled out rather than reached for via `DefaultHasher`.
///
/// `DefaultHasher`'s output is explicitly not guaranteed stable across Rust
/// releases, and a digest that changes with the toolchain would turn every
/// compiler upgrade into a wall of spurious snapshot failures. FNV-1a is
/// eight lines and stable forever.
fn digest(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Replaces the payload of every inlined `data:` URI with a digest.
///
/// `render_to_svg` inlines both images and subsetted font programs as base64.
/// Left in, they are the overwhelming majority of the snapshot and none of it
/// is reviewable -- a reader cannot tell a meaningful change from noise in a
/// wall of base64. Replacing each payload with a digest keeps the property
/// that matters (different bytes, different snapshot, test fails) and drops
/// the part that does not (which bytes, exactly).
///
/// The MIME prefix is deliberately kept: a PNG silently becoming a JPEG is a
/// real change and should be legible as one in the diff.
fn elide_data_payloads(svg: &str) -> String {
    const MARKER: &str = ";base64,";
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(start) = rest.find(MARKER) {
        let payload_start = start + MARKER.len();
        out.push_str(&rest[..payload_start]);
        let tail = &rest[payload_start..];
        // A data URI payload runs to the end of the attribute value; both
        // `href="..."` and `url("...")` terminate on the same quote.
        let end = tail.find('"').unwrap_or(tail.len());
        out.push_str("<elided:");
        out.push_str(&digest(&tail.as_bytes()[..end]));
        out.push('>');
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

/// Renders one example end-to-end and returns one normalised SVG per page.
fn render_pages(example: &str) -> Vec<String> {
    let root = repo_root();
    let template = root.join("examples").join(format!("{example}.haml"));
    let data = root.join("examples").join(format!("{example}.json"));

    let bytes = match render_files(&template, &data, &PageConfig::default()) {
        Ok(bytes) => bytes,
        Err(e) => panic!("rendering {example} failed: {e:?}"),
    };
    assert!(
        bytes.starts_with(b"%PDF"),
        "{example} did not produce PDF bytes"
    );

    let mut warnings = Vec::new();
    let doc = match parse_pdf_from_bytes(&bytes, &PdfParseOptions::default(), &mut warnings) {
        Ok(doc) => doc,
        Err(e) => panic!("re-parsing the {example} PDF failed: {e}"),
    };
    assert!(
        !doc.pages.is_empty(),
        "{example} rendered to a document with no pages"
    );

    let opts = PdfToSvgOptions::default();
    doc.pages
        .iter()
        .map(|page| {
            let mut page_warnings = Vec::new();
            elide_data_payloads(&render_to_svg(
                page,
                &doc.resources,
                &opts,
                &mut page_warnings,
            ))
        })
        .collect()
}

fn check_example(example: &str) {
    let pages = render_pages(example);
    let dir = golden_dir();
    let updating = std::env::var_os("UPDATE_GOLDEN").is_some();

    if updating {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            panic!("creating {dir:?} failed: {e}");
        }
    }

    for (index, page) in pages.iter().enumerate() {
        let path = dir.join(format!("{example}-page{}.svg", index + 1));
        if updating {
            if let Err(e) = std::fs::write(&path, page) {
                panic!("writing {path:?} failed: {e}");
            }
            continue;
        }
        let expected = match std::fs::read_to_string(&path) {
            Ok(expected) => expected,
            Err(e) => panic!(
                "no golden snapshot at {path:?} ({e}). \
                 Create it with `UPDATE_GOLDEN=1 cargo test -p pdfcn-core --test golden`."
            ),
        };
        assert_eq!(
            page.trim_end(),
            expected.trim_end(),
            "page {} of {example} no longer matches its golden snapshot. \
             If the change is intended, refresh with \
             `UPDATE_GOLDEN=1 cargo test -p pdfcn-core --test golden` and review the diff.",
            index + 1
        );
    }

    // A page count change is a layout regression that per-page comparison
    // alone would miss: dropping a page leaves every surviving page matching.
    if !updating {
        let stale = dir.join(format!("{example}-page{}.svg", pages.len() + 1));
        assert!(
            !stale.exists(),
            "{example} now renders {} page(s) but a snapshot for page {} still exists: \
             the document got shorter.",
            pages.len(),
            pages.len() + 1
        );
    }
}

#[test]
fn examples_match_their_golden_snapshots() {
    for example in EXAMPLES {
        check_example(example);
    }
}

/// Rendering the same source twice must produce byte-identical output.
/// Snapshot tests are only worth having if the thing they snapshot is
/// deterministic; this asserts the premise rather than assuming it.
#[test]
fn rendering_is_deterministic() {
    let root = repo_root();
    let template = root.join("examples").join("invoice.haml");
    let data = root.join("examples").join("invoice.json");
    let page = PageConfig::default();

    let first = render_files(&template, &data, &page);
    let second = render_files(&template, &data, &page);
    match (first, second) {
        (Ok(a), Ok(b)) => assert_eq!(
            a.len(),
            b.len(),
            "two renders of the same invoice differ in length"
        ),
        (a, b) => panic!("rendering the invoice failed: {a:?} / {b:?}"),
    }
}

#[test]
fn data_payloads_are_elided_but_still_compared() {
    let svg =
        r#"<image href="data:image/png;base64,AAAA" /><image href="data:image/png;base64,BBBB" />"#;
    let elided = elide_data_payloads(svg);
    assert!(
        !elided.contains("AAAA") && !elided.contains("BBBB"),
        "payloads survived elision: {elided}"
    );
    // Different bytes must still produce different snapshots, or the elision
    // would hide exactly the regression it is meant to surface.
    let changed = elide_data_payloads(
        r#"<image href="data:image/png;base64,AAAA" /><image href="data:image/png;base64,CCCC" />"#,
    );
    assert_ne!(elided, changed);

    // The MIME type stays visible, so a format change reads as one.
    assert!(
        elided.contains("data:image/png;base64,<elided:"),
        "{elided}"
    );

    // Font programs are inlined the same way and must be elided too --
    // untouched, they are most of the snapshot's bytes.
    let font = elide_data_payloads(r#"src: url("data:font/otf;charset=utf-8;base64,AAEAAAAK")"#);
    assert!(!font.contains("AAEAAAAK"), "font payload survived: {font}");
    assert!(
        font.contains("data:font/otf;charset=utf-8;base64,<elided:"),
        "{font}"
    );
}
