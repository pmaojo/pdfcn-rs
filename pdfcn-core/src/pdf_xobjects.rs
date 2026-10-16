//! Finding the image XObject printpdf wrote for an `<img src>`, for the
//! passes that replace one in place after saving (`vector_pdf`,
//! `jpeg_passthrough`).

use std::collections::BTreeMap;

use lopdf::{Document, ObjectId};

/// The XObject name printpdf's bridge gives the `<img src=KEY>` it places.
/// Calls printpdf's own function so the two can never drift apart.
pub(crate) fn xobject_name(src: &str) -> Vec<u8> {
    printpdf::html::bridge::image_xobject_id(src).0.into_bytes()
}

/// Every `/XObject` entry reachable from a page: name -> object id. printpdf
/// shares one dictionary across all pages, but walking each page costs little
/// and does not depend on that.
pub(crate) fn image_xobjects(doc: &Document) -> BTreeMap<Vec<u8>, ObjectId> {
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

/// What [`finish`] puts into the saved document, by placeholder `src`.
pub(crate) struct Replacements<'a> {
    /// SVG placeholders that became vectors (see `vector_pdf`).
    #[cfg(feature = "vector")]
    pub(crate) forms: BTreeMap<String, crate::vector_pdf::Form>,
    /// JPEGs held back from the layout engine (see `jpeg_passthrough`).
    #[cfg(feature = "jpeg-passthrough")]
    pub(crate) jpegs: BTreeMap<String, crate::jpeg_passthrough::Jpeg<'a>>,
    /// printpdf was told to write images uncompressed because every one of
    /// them is replaced here: Flate-compress any image stream still without
    /// a filter, so skipping printpdf's compression never leaves a raw image
    /// behind.
    pub(crate) compress_leftovers: bool,
    pub(crate) borrows: std::marker::PhantomData<&'a [u8]>,
}

impl Replacements<'_> {
    fn is_empty(&self) -> bool {
        #[allow(unused_mut)]
        let mut empty = !self.compress_leftovers;
        #[cfg(feature = "vector")]
        {
            empty &= self.forms.is_empty();
        }
        #[cfg(feature = "jpeg-passthrough")]
        {
            empty &= self.jpegs.is_empty();
        }
        empty
    }
}

/// The one pass over printpdf's saved output: load it once, replace each
/// placeholder's XObject in place, prune what nothing references any more
/// and save once. Returns `pdf` unchanged when there is nothing to do or
/// lopdf cannot load or save the document.
pub(crate) fn finish(pdf: Vec<u8>, replacements: Replacements<'_>) -> Vec<u8> {
    if replacements.is_empty() {
        return pdf;
    }
    let Ok(mut doc) = Document::load_mem(&pdf) else {
        return pdf;
    };
    let targets = image_xobjects(&doc);
    // The output carries every passed-through photo: size it for them up
    // front instead of letting it double its way there.
    #[allow(unused_mut)]
    let mut capacity = pdf.len();
    #[cfg(feature = "jpeg-passthrough")]
    {
        capacity += replacements.jpegs.values().map(|j| j.len()).sum::<usize>();
    }
    #[allow(unused_mut)]
    let mut replaced = 0usize;
    #[cfg(feature = "vector")]
    for (src, form) in replacements.forms {
        let Some(&id) = targets.get(&xobject_name(&src)) else {
            continue;
        };
        if let Some(stream) = crate::vector_pdf::import(&mut doc, form) {
            doc.objects.insert(id, lopdf::Object::Stream(stream));
            replaced += 1;
        }
    }
    #[cfg(feature = "jpeg-passthrough")]
    for (src, jpeg) in replacements.jpegs {
        let Some(&id) = targets.get(&xobject_name(&src)) else {
            continue;
        };
        doc.objects.insert(
            id,
            lopdf::Object::Stream(crate::jpeg_passthrough::dct_stream(jpeg)),
        );
        replaced += 1;
    }
    if replacements.compress_leftovers {
        for object in doc.objects.values_mut() {
            if let lopdf::Object::Stream(stream) = object {
                let is_image = stream
                    .dict
                    .get(b"Subtype")
                    .and_then(lopdf::Object::as_name)
                    .ok()
                    == Some(&b"Image"[..]);
                if is_image && !stream.dict.has(b"Filter") {
                    // Best effort: an uncompressed image is still valid.
                    let _ = stream.compress();
                    replaced += 1;
                }
            }
        }
    }
    if replaced == 0 {
        return pdf;
    }
    // The placeholders' own objects went with the ones they were; svg2pdf's
    // catalog, page tree and page came along with each Form; nothing
    // references either now.
    doc.prune_objects();
    let mut out = Vec::with_capacity(capacity);
    match doc.save_to(&mut out) {
        Ok(()) => out,
        Err(_) => pdf,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nothing() -> Replacements<'static> {
        Replacements {
            #[cfg(feature = "vector")]
            forms: BTreeMap::new(),
            #[cfg(feature = "jpeg-passthrough")]
            jpegs: BTreeMap::new(),
            compress_leftovers: false,
            borrows: std::marker::PhantomData,
        }
    }

    #[test]
    fn nothing_to_replace_returns_the_input_untouched() {
        let pdf = b"not even a pdf".to_vec();
        assert_eq!(finish(pdf.clone(), nothing()), pdf);
        // Something to do, but a document lopdf cannot load: still untouched.
        let compress = Replacements {
            compress_leftovers: true,
            ..nothing()
        };
        assert_eq!(finish(pdf.clone(), compress), pdf);
    }
}
