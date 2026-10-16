//! Real vectors for SVG placeholders, behind the `vector` cargo feature.
//!
//! printpdf's HTML bridge has no route for vector content (docs/spikes/001),
//! so every SVG placeholder (`%Vector`, Charts v2, `%Barcode`, `%Icon`, QR
//! codes) is laid out as an image and, once the document is saved, that image
//! XObject is swapped for a Form XObject drawing the SVG as PDF vectors
//! (docs/spikes/004):
//!
//! - printpdf names every `<img>` XObject from its src
//!   (`printpdf::html::bridge::image_xobject_id`), so the placeholder's own
//!   src finds its object;
//! - an image paints the unit square of the space its `cm` sets up, and a Form
//!   whose `/Matrix` maps its `/BBox` onto that unit square paints exactly the
//!   same box -- so replacing the object *in place*, same object number, leaves
//!   every resource dictionary and every `Do` untouched.
//!
//! [`prepare`] runs before layout: it converts each SVG with svg2pdf, and a
//! placeholder that converts is laid out by size alone (printpdf's
//! `image_placeholders`, docs/spikes/006) -- no PNG is rendered, encoded,
//! decoded or written for it. Only an SVG that cannot become a vector is
//! rasterized, so it still lands as the PNG it always was. [`import`] puts a
//! converted Form into the saved document (see `pdf_xobjects::finish`).

use std::collections::BTreeMap;

use lopdf::{dictionary, Document, Object, Stream};

use crate::assets::{Images, VectorSource};

/// An SVG converted by svg2pdf, checked to have everything [`import`] reads.
pub(crate) struct Form {
    doc: Document,
}

/// Converts every vector source before layout. Returns the Forms by
/// placeholder src; each converted placeholder without a PNG of its own is
/// recorded in `placeholders` at the size its PNG would have had, and each one
/// that failed to convert is rasterized into `images` instead.
pub(crate) fn prepare(
    sources: &BTreeMap<String, VectorSource>,
    images: &mut Images<'_>,
    placeholders: &mut BTreeMap<String, (u32, u32)>,
) -> BTreeMap<String, Form> {
    let mut forms = BTreeMap::new();
    for (src, source) in sources {
        let Some(tree) = crate::raster::parse_svg(&source.svg) else {
            continue;
        };
        let form = convert(&tree);
        if let Some((w, h)) = source.raster_box {
            if images.contains_key(src) {
                // Supplied by the caller under the placeholder's own src.
            } else if form.is_some() {
                let Some((_, pw, ph)) = crate::raster::raster_size(&tree, w, h) else {
                    continue;
                };
                placeholders.insert(src.clone(), (pw, ph));
            } else {
                if let Some(png) = crate::raster::tree_to_png(&tree, w, h) {
                    images.insert(src.clone(), png.into());
                }
                continue;
            }
        }
        if let Some(form) = form {
            forms.insert(src.clone(), form);
        }
    }
    forms
}

/// svg2pdf's one-page document for `tree`, if it has a page with a usable
/// media box, resources and content.
fn convert(tree: &resvg::usvg::Tree) -> Option<Form> {
    let options = svg2pdf::ConversionOptions {
        // Text becomes outlines: no per-SVG font subset to embed, and the
        // labels look exactly as the PNG showed them.
        embed_text: false,
        ..Default::default()
    };
    let bytes = svg2pdf::to_pdf(tree, options, svg2pdf::PageOptions::default()).ok()?;
    let doc = Document::load_mem(&bytes).ok()?;
    page_parts(&doc)?;
    Some(Form { doc })
}

/// The first page's media box, resources and content stream.
fn page_parts(doc: &Document) -> Option<([f32; 4], Object, Vec<u8>)> {
    let page_id = *doc.get_pages().get(&1)?;
    let page = doc.get_dictionary(page_id).ok()?;
    let media_box = page.get(b"MediaBox").ok()?.as_array().ok()?;
    let [x0, y0, x1, y1]: [f32; 4] = media_box
        .iter()
        .map(|n| n.as_float().ok())
        .collect::<Option<Vec<f32>>>()?
        .try_into()
        .ok()?;
    let (w, h) = (x1 - x0, y1 - y0);
    if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
        return None;
    }
    let resources = page.get(b"Resources").ok()?.clone();
    Some(([x0, y0, x1, y1], resources, doc.get_page_content(page_id)))
}

/// Imports `form`'s objects into `doc` and returns a Form XObject drawing it
/// onto the unit square. `None` leaves `doc` free of half-imported state that
/// matters (anything imported but unreferenced is pruned before saving).
pub(crate) fn import(doc: &mut Document, form: Form) -> Option<Stream> {
    let mut svg_doc = form.doc;
    svg_doc.renumber_objects_with(doc.max_id + 1);
    let ([x0, y0, x1, y1], resources, content) = page_parts(&svg_doc)?;
    let (w, h) = (x1 - x0, y1 - y0);

    doc.max_id = doc.max_id.max(svg_doc.max_id);
    doc.objects.append(&mut svg_doc.objects);

    let mut stream = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![x0.into(), y0.into(), x1.into(), y1.into()],
            "Matrix" => vec![
                (1.0 / w).into(), 0.into(), 0.into(), (1.0 / h).into(),
                (-x0 / w).into(), (-y0 / h).into(),
            ],
            "Resources" => resources,
        },
        content,
    );
    // Best effort: an uncompressed content stream is still valid.
    let _ = stream.compress();
    Some(stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf_xobjects::{image_xobjects, xobject_name};

    const LOGO: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 120 60"><rect x="4" y="4" width="112" height="52" rx="8" fill="#2563eb"/><text x="60" y="37" font-size="20" fill="#fff" text-anchor="middle">pdfcn</text></svg>"##;

    fn render_with_vector(svg: &str) -> Document {
        let mut options = crate::RenderOptions::default();
        options
            .svg_assets
            .insert("logo".to_string(), svg.to_string());
        let pdf = crate::render(
            r#"%Vector(id="logo" w="120px" h="60px" alt="logo")"#,
            &serde_json::json!({}),
            &crate::NoPartials,
            &options,
        )
        .expect("renders");
        Document::load_mem(&pdf).expect("valid PDF")
    }

    fn xobject(doc: &Document) -> &Stream {
        let id = image_xobjects(doc)[&xobject_name("pdfcn-vector:logo")];
        doc.get_object(id)
            .and_then(Object::as_stream)
            .expect("a stream")
    }

    #[test]
    fn a_vector_placeholder_lands_as_a_form_not_an_image() {
        let doc = render_with_vector(LOGO);
        let form = xobject(&doc);
        assert_eq!(
            form.dict.get(b"Subtype").unwrap().as_name().unwrap(),
            b"Form"
        );
        // Drawn into the unit square the image occupied.
        let matrix = form.dict.get(b"Matrix").unwrap().as_array().unwrap();
        let bbox = form.dict.get(b"BBox").unwrap().as_array().unwrap();
        let width = bbox[2].as_float().unwrap() - bbox[0].as_float().unwrap();
        assert!((matrix[0].as_float().unwrap() * width - 1.0).abs() < 1e-4);
        // Real drawing operators: the rect and the outlined text are fills.
        let content = form.decompressed_content().unwrap();
        let text = String::from_utf8_lossy(&content);
        assert!(
            text.contains(" f") || text.contains("\nf"),
            "no fill operator in {text}"
        );
        // And no raster image is left anywhere in the document.
        let images = doc
            .objects
            .values()
            .filter_map(|o| o.as_stream().ok())
            .filter(|s| {
                s.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(&b"Image"[..])
            })
            .count();
        assert_eq!(images, 0);
    }

    #[test]
    fn a_vector_placeholder_is_laid_out_by_size_and_never_rasterized() {
        let sources = BTreeMap::from([(
            "pdfcn-vector:logo".to_string(),
            VectorSource {
                svg: LOGO.to_string(),
                raster_box: Some((Some(120.0), Some(60.0))),
            },
        )]);
        let mut images = Images::new();
        let mut placeholders = BTreeMap::new();
        let forms = prepare(&sources, &mut images, &mut placeholders);
        assert!(forms.contains_key("pdfcn-vector:logo"));
        assert!(images.is_empty());
        // The size its PNG would have had: the box at 3x.
        assert_eq!(placeholders["pdfcn-vector:logo"], (360, 180));
    }

    #[test]
    fn an_svg_that_cannot_be_converted_falls_back_to_its_png() {
        // svg2pdf rejects nothing usvg parses, so stand in for a failure with
        // a document that parses but has nothing to draw at a usable size.
        let empty = r#"<svg xmlns="http://www.w3.org/2000/svg" width="0" height="0"></svg>"#;
        let sources = BTreeMap::from([(
            "pdfcn-vector:x".to_string(),
            VectorSource {
                svg: empty.to_string(),
                raster_box: Some((Some(10.0), Some(10.0))),
            },
        )]);
        let mut images = Images::new();
        let mut placeholders = BTreeMap::new();
        let forms = prepare(&sources, &mut images, &mut placeholders);
        // Whatever svg2pdf made of it, the placeholder is never left with
        // neither a Form nor an image of some kind to lay out.
        assert!(
            forms.contains_key("pdfcn-vector:x") == placeholders.contains_key("pdfcn-vector:x")
        );
        assert!(!(forms.contains_key("pdfcn-vector:x") && images.contains_key("pdfcn-vector:x")));
    }

    #[test]
    fn a_source_that_does_not_parse_is_left_unresolved() {
        let sources = BTreeMap::from([(
            "pdfcn-vector:x".to_string(),
            VectorSource {
                svg: "<not svg".to_string(),
                raster_box: Some((None, None)),
            },
        )]);
        let mut images = Images::new();
        let mut placeholders = BTreeMap::new();
        assert!(prepare(&sources, &mut images, &mut placeholders).is_empty());
        assert!(images.is_empty() && placeholders.is_empty());
    }

    #[test]
    fn a_caller_supplied_png_under_the_placeholder_src_is_kept() {
        let png = crate::raster::svg_to_png(LOGO, Some(120.0), Some(60.0)).expect("png");
        let mut images = Images::from([("pdfcn-vector:logo".to_string(), png.clone().into())]);
        let sources = BTreeMap::from([(
            "pdfcn-vector:logo".to_string(),
            VectorSource {
                svg: LOGO.to_string(),
                raster_box: Some((Some(120.0), Some(60.0))),
            },
        )]);
        let mut placeholders = BTreeMap::new();
        let forms = prepare(&sources, &mut images, &mut placeholders);
        assert!(forms.contains_key("pdfcn-vector:logo"));
        assert!(placeholders.is_empty());
        assert_eq!(images["pdfcn-vector:logo"], png);
    }
}
