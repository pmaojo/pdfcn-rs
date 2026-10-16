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
//!
//! **Vector content has its own oracle.** `render_to_svg` only materialises
//! `XObject::Image`, so the Form XObjects that SVG placeholders become (see
//! docs/spikes/004) would render as nothing and pass trivially. With the
//! `vector` feature each snapshot's first page therefore ends with one
//! `<!-- vector ... -->` line per Form: its resource name, `/BBox` and a
//! digest of its drawing operators. And no example may leave a vector
//! placeholder (`%Vector`, charts, barcodes, icons, QR codes) as a raster
//! image: [`vector_placeholders_never_land_as_raster_images`] fails if one does.
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

use pdfcn_core::{render_files, RenderOptions};
use printpdf::{parse_pdf_from_bytes, render_to_svg, PdfParseOptions, PdfToSvgOptions};

/// Every example that ships in `examples/`. These are the documents the
/// README tells a new user to run first, so a regression in one of them is a
/// regression in the project's own front door.
const EXAMPLES: &[&str] = &[
    "invoice",
    "catalog",
    "showcase",
    "dashboard",
    "pricing",
    "certificate",
    "ticket",
    "codes",
    "links",
    "verifactu",
    "utilities",
    "icons",
    "brochure",
    // The same pipeline fed from other front-ends (see `pdfcn-formats`).
    "format-html",
    "format-markdown",
    "format-blocks",
    "format-typst",
    "format-latex",
];

/// Template extensions an example may use, in lookup order.
const TEMPLATE_EXTENSIONS: &[&str] = &["haml", "html", "md", "yaml", "typ", "tex"];

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
    let examples = root.join("examples");
    let template = TEMPLATE_EXTENSIONS
        .iter()
        .map(|ext| examples.join(format!("{example}.{ext}")))
        .find(|p| p.exists())
        .unwrap_or_else(|| panic!("no template found for example {example}"));
    // The format-* examples share one data file.
    let own_data = examples.join(format!("{example}.json"));
    let data = if own_data.exists() {
        own_data
    } else {
        examples.join("formats.json")
    };

    let bytes = match render_files(&template, &data, &RenderOptions::default()) {
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
    // Only the `vector` oracle below appends to the pages.
    #[cfg_attr(not(feature = "vector"), allow(unused_mut))]
    let mut pages: Vec<String> = doc
        .pages
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
        .collect();
    #[cfg(feature = "vector")]
    if let Some(first) = pages.first_mut() {
        for xobject in xobjects(example, &bytes) {
            if xobject.form {
                first.push_str(&format!(
                    "\n<!-- vector {} bbox={} ops={} -->",
                    xobject.name, xobject.bbox, xobject.ops
                ));
            }
        }
    }
    pages
}

/// One `HtmlImg_*` XObject of a rendered example, as the vector oracle sees it.
#[cfg(feature = "vector")]
struct PlacedXObject {
    name: String,
    form: bool,
    bbox: String,
    ops: String,
}

/// Every image/form XObject printpdf placed for an `<img>`, sorted by name.
#[cfg(feature = "vector")]
fn xobjects(example: &str, bytes: &[u8]) -> Vec<PlacedXObject> {
    use lopdf::{Document, Object};
    let doc = match Document::load_mem(bytes) {
        Ok(doc) => doc,
        Err(e) => panic!("lopdf could not read the {example} PDF: {e}"),
    };
    let mut found = std::collections::BTreeMap::new();
    for page_id in doc.get_pages().into_values() {
        let resources = doc
            .get_dictionary(page_id)
            .and_then(|page| page.get(b"Resources"))
            .and_then(|r| doc.dereference(r))
            .and_then(|(_, r)| r.as_dict());
        let Ok(resources) = resources else { continue };
        let xobjects = resources
            .get(b"XObject")
            .and_then(|x| doc.dereference(x))
            .and_then(|(_, x)| x.as_dict());
        let Ok(xobjects) = xobjects else { continue };
        for (name, value) in xobjects.iter() {
            let name = String::from_utf8_lossy(name).to_string();
            if !name.starts_with("HtmlImg_") {
                continue;
            }
            let Ok(stream) = value
                .as_reference()
                .and_then(|id| doc.get_object(id))
                .and_then(Object::as_stream)
            else {
                continue;
            };
            let form =
                stream.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(&b"Form"[..]);
            let bbox = stream
                .dict
                .get(b"BBox")
                .and_then(Object::as_array)
                .map(|a| {
                    let n: Vec<String> = a
                        .iter()
                        .map(|v| v.as_float().map(|f| format!("{f:.2}")).unwrap_or_default())
                        .collect();
                    format!("[{}]", n.join(" "))
                })
                .unwrap_or_default();
            let ops = stream
                .decompressed_content()
                .map(|c| digest(&c))
                .unwrap_or_else(|_| digest(&stream.content));
            found.insert(
                name.clone(),
                PlacedXObject {
                    name,
                    form,
                    bbox,
                    ops,
                },
            );
        }
    }
    found.into_values().collect()
}

/// The resource-name prefixes of every vector placeholder scheme, as
/// printpdf sanitizes them (`pdfcn-chart:` -> `pdfcn_chart_`).
#[cfg(feature = "vector")]
const VECTOR_PLACEHOLDERS: &[&str] = &[
    "HtmlImg_pdfcn_vector_",
    "HtmlImg_pdfcn_chart_",
    "HtmlImg_pdfcn_barcode_",
    "HtmlImg_pdfcn_icon_",
    "HtmlImg_pdfcn_qrcode_",
];

/// Whether this build draws images the way the default build that wrote the
/// snapshots does. Without `vector` an SVG placeholder lands as a PNG, and
/// without `jpeg-passthrough` printpdf re-encodes every photo, so a snapshot
/// holding either can only match the default build.
const DEFAULT_IMAGE_PATH: bool = cfg!(all(feature = "vector", feature = "jpeg-passthrough"));

/// A snapshot whose pages draw no raster image and no vector Form: what every
/// build renders identically.
fn feature_independent(example: &str) -> bool {
    let dir = golden_dir();
    (1..)
        .map(|page| dir.join(format!("{example}-page{page}.svg")))
        .take_while(|path| path.exists())
        .all(|path| {
            std::fs::read_to_string(&path)
                .map(|svg| !svg.contains("<image") && !svg.contains("<!-- vector"))
                .unwrap_or(false)
        })
}

/// Compares `example` against its snapshots; `false` when this build cannot
/// (see [`DEFAULT_IMAGE_PATH`]) and it was skipped.
fn check_example(example: &str) -> bool {
    let updating = std::env::var_os("UPDATE_GOLDEN").is_some();
    if !DEFAULT_IMAGE_PATH {
        assert!(
            !updating,
            "regenerate golden snapshots with the default features: they are what CI compares"
        );
        if !feature_independent(example) {
            return false;
        }
    }
    let pages = render_pages(example);
    let dir = golden_dir();

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
    true
}

/// Every SVG-backed placeholder must reach the PDF as vectors. A new
/// generator that forgets to register its SVG source would otherwise slip
/// back to a PNG without any snapshot noticing.
#[cfg(feature = "vector")]
#[test]
fn vector_placeholders_never_land_as_raster_images() {
    let root = repo_root();
    let mut checked = 0usize;
    for example in EXAMPLES {
        let examples = root.join("examples");
        let template = TEMPLATE_EXTENSIONS
            .iter()
            .map(|ext| examples.join(format!("{example}.{ext}")))
            .find(|p| p.exists())
            .unwrap_or_else(|| panic!("no template found for example {example}"));
        let own_data = examples.join(format!("{example}.json"));
        let data = if own_data.exists() {
            own_data
        } else {
            examples.join("formats.json")
        };
        let bytes = match render_files(&template, &data, &RenderOptions::default()) {
            Ok(bytes) => bytes,
            Err(e) => panic!("rendering {example} failed: {e:?}"),
        };
        for xobject in xobjects(example, &bytes) {
            if VECTOR_PLACEHOLDERS
                .iter()
                .any(|p| xobject.name.starts_with(p))
            {
                assert!(
                    xobject.form,
                    "{example}: {} is still a raster image, not vectors",
                    xobject.name
                );
                checked += 1;
            }
        }
    }
    // The examples do contain charts, barcodes, icons and QR codes: a check
    // that never found one would prove nothing.
    assert!(checked > 0, "no vector placeholder found in any example");
}

#[test]
fn examples_match_their_golden_snapshots() {
    let skipped: Vec<&str> = EXAMPLES
        .iter()
        .copied()
        .filter(|example| !check_example(example))
        .collect();
    if DEFAULT_IMAGE_PATH {
        assert!(skipped.is_empty());
    } else {
        // A reduced build still compares every example that draws no image.
        assert!(skipped.len() < EXAMPLES.len(), "no example was compared");
        eprintln!("golden: skipped (they draw images or vectors): {skipped:?}");
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
    let options = RenderOptions::default();

    let first = render_files(&template, &data, &options);
    let second = render_files(&template, &data, &options);
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
