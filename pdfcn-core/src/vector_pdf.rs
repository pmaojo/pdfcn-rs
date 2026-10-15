//! Real vectors for SVG placeholders, behind the `vector` cargo feature.
//!
//! The layout engine places every SVG placeholder (`%Vector`, Charts v2,
//! `%Barcode`, `%Icon`) as a PNG, because printpdf's HTML bridge has no route
//! for vector content (docs/spikes/001). Once the document is saved, this pass
//! swaps each of those image XObjects for a Form XObject drawing the same SVG
//! as PDF vectors (docs/spikes/004):
//!
//! - printpdf names every `<img>` XObject from its src
//!   (`printpdf::html::bridge::image_xobject_id`), so the placeholder's own
//!   src finds its object;
//! - an image paints the unit square of the space its `cm` sets up, and a Form
//!   whose `/Matrix` maps its `/BBox` onto that unit square paints exactly the
//!   same box -- so replacing the object *in place*, same object number, leaves
//!   every resource dictionary and every `Do` untouched.
//!
//! Never makes a document worse: an SVG that fails to convert keeps its PNG,
//! and a document lopdf can't load or save comes back byte-for-byte unchanged.

use std::collections::BTreeMap;

use lopdf::{dictionary, Document, Object, ObjectId, Stream};

/// The XObject name printpdf's bridge gives the `<img src=KEY>` it places.
/// Calls printpdf's own function so the two can never drift apart.
fn xobject_name(src: &str) -> Vec<u8> {
    printpdf::html::bridge::image_xobject_id(src).0.into_bytes()
}

/// Replaces the PNG of every placeholder in `sources` (placeholder src -> SVG
/// text) with vector content. Returns `pdf` unchanged when there is nothing
/// to replace or the document can't be processed.
pub(crate) fn vectorize(pdf: Vec<u8>, sources: &BTreeMap<String, String>) -> Vec<u8> {
    if sources.is_empty() {
        return pdf;
    }
    let Ok(mut doc) = Document::load_mem(&pdf) else {
        return pdf;
    };
    let targets = image_xobjects(&doc);
    let mut replaced = 0usize;
    for (src, svg) in sources {
        let Some(&image_id) = targets.get(&xobject_name(src)) else {
            continue;
        };
        if let Some(form) = svg_form(&mut doc, svg) {
            doc.objects.insert(image_id, Object::Stream(form));
            replaced += 1;
        }
    }
    if replaced == 0 {
        return pdf;
    }
    // svg2pdf's own catalog, page tree and page came along with the imported
    // objects; nothing references them now.
    doc.prune_objects();
    let mut out = Vec::with_capacity(pdf.len());
    match doc.save_to(&mut out) {
        Ok(()) => out,
        Err(_) => pdf,
    }
}

/// Every `/XObject` entry reachable from a page: name -> object id. printpdf
/// shares one dictionary across all pages, but walking each page costs little
/// and does not depend on that.
fn image_xobjects(doc: &Document) -> BTreeMap<Vec<u8>, ObjectId> {
    let mut found = BTreeMap::new();
    for page_id in doc.get_pages().into_values() {
        let Ok(page) = doc.get_dictionary(page_id) else {
            continue;
        };
        let Some(resources) = page
            .get(b"Resources")
            .ok()
            .and_then(|r| doc.dereference(r).ok())
            .and_then(|(_, r)| r.as_dict().ok())
        else {
            continue;
        };
        let Some(xobjects) = resources
            .get(b"XObject")
            .ok()
            .and_then(|x| doc.dereference(x).ok())
            .and_then(|(_, x)| x.as_dict().ok())
        else {
            continue;
        };
        for (name, value) in xobjects.iter() {
            if let Ok(id) = value.as_reference() {
                found.insert(name.clone(), id);
            }
        }
    }
    found
}

/// Converts `svg` with svg2pdf, imports the result's objects into `doc`, and
/// returns a Form XObject drawing it onto the unit square. `None` on any
/// failure, leaving `doc` free of half-imported state that matters (anything
/// imported but unreferenced is pruned before saving).
fn svg_form(doc: &mut Document, svg: &str) -> Option<Stream> {
    let tree = crate::raster::parse_svg(svg)?;
    let options = svg2pdf::ConversionOptions {
        // Text becomes outlines: no per-SVG font subset to embed, and the
        // labels look exactly as the PNG showed them.
        embed_text: false,
        ..Default::default()
    };
    let bytes = svg2pdf::to_pdf(&tree, options, svg2pdf::PageOptions::default()).ok()?;
    let mut svg_doc = Document::load_mem(&bytes).ok()?;
    svg_doc.renumber_objects_with(doc.max_id + 1);
    let page_id = *svg_doc.get_pages().get(&1)?;
    let page = svg_doc.get_dictionary(page_id).ok()?;
    let media_box = page.get(b"MediaBox").ok()?.as_array().ok()?;
    let [x0, y0, x1, y1] = media_box
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
    let content = svg_doc.get_page_content(page_id);

    doc.max_id = doc.max_id.max(svg_doc.max_id);
    doc.objects.append(&mut svg_doc.objects);

    let mut form = Stream::new(
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
    let _ = form.compress();
    Some(form)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_source_that_cannot_be_converted_keeps_its_png_and_the_document() {
        // A PDF whose placeholder is still the PNG (supplied directly, so the
        // render itself has no SVG source to swap in)...
        let png = crate::raster::svg_to_png(LOGO, Some(120.0), Some(60.0)).expect("png");
        let mut images = BTreeMap::new();
        images.insert("pdfcn-vector:logo".to_string(), png);
        let pdf = crate::render_pdf_with_assets(
            r#"<html><body><img src="pdfcn-vector:logo" style="width:120px;height:60px"></body></html>"#,
            &BTreeMap::new(),
            &images,
            &crate::RenderOptions::default(),
        )
        .expect("renders");
        // ...then a source the pass cannot read: nothing changes.
        let mut sources = BTreeMap::new();
        sources.insert("pdfcn-vector:logo".to_string(), "<not svg".to_string());
        let out = vectorize(pdf.clone(), &sources);
        assert_eq!(out, pdf);
        let doc = Document::load_mem(&out).unwrap();
        assert_eq!(
            xobject(&doc)
                .dict
                .get(b"Subtype")
                .unwrap()
                .as_name()
                .unwrap(),
            b"Image"
        );
    }

    #[test]
    fn nothing_to_replace_returns_the_input_untouched() {
        let pdf = b"not even a pdf".to_vec();
        assert_eq!(vectorize(pdf.clone(), &BTreeMap::new()), pdf);
        let mut sources = BTreeMap::new();
        sources.insert("pdfcn-vector:x".to_string(), "<svg/>".to_string());
        assert_eq!(vectorize(pdf.clone(), &sources), pdf);
    }
}
