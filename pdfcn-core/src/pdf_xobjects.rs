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
