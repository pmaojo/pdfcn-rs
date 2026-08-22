//! Ola 3: splices a Factur-X EN 16931 invoice attachment into an
//! already-rendered PDF, behind pdfcn-core's opt-in `factur-x` cargo
//! feature. See docs/spikes/002-factur-x-embedding.md for why this is a
//! post-processing pass over printpdf's own output rather than something
//! printpdf's own conformance API can do: printpdf has no embedded-file
//! support at all, and its PDF/A XMP/ICC wiring (verified against its
//! real source) is aimed at PDF/X print workflows, not PDF/A-3 hybrid
//! invoices -- it never emits the XMP block PDF/A-3 needs even when told
//! to conform to it, and its only bundled ICC profile is a CMYK print
//! profile, wrong for an RGB-rendered invoice.
//!
//! Nothing here panics; a malformed input PDF or an `lopdf` failure comes
//! back as `Err`, never a silent partial write.

use lopdf::{dictionary, Document, Object, ObjectId, Stream};

/// Which Factur-X profile the embedded XML conforms to. Every profile
/// maps to the same PDF/A-3B container; only the XMP
/// `fx:ConformanceLevel` value differs (and, in the caller's own XML,
/// which optional fields it actually carries).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FacturXProfile {
    Minimum,
    BasicWl,
    Basic,
    En16931,
    Extended,
}

impl FacturXProfile {
    fn xmp_conformance_level(self) -> &'static str {
        match self {
            Self::Minimum => "MINIMUM",
            Self::BasicWl => "BASIC WL",
            Self::Basic => "BASIC",
            Self::En16931 => "EN 16931",
            Self::Extended => "EXTENDED",
        }
    }
}

/// The filename Factur-X mandates for the embedded XML -- validators
/// check this exact name, not just that some file is attached.
const FACTUR_X_FILENAME: &str = "factur-x.xml";

/// Errors specific to this module, independent of `pdfcn-core`'s own
/// `CoreError` -- the `factur-x` feature is optional, and nothing outside
/// this module should have to know `lopdf` exists.
#[derive(Debug)]
pub struct FacturXError(String);

impl std::fmt::Display for FacturXError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Factur-X embedding failed: {}", self.0)
    }
}

impl std::error::Error for FacturXError {}

impl From<lopdf::Error> for FacturXError {
    fn from(e: lopdf::Error) -> Self {
        FacturXError(e.to_string())
    }
}

impl From<std::io::Error> for FacturXError {
    fn from(e: std::io::Error) -> Self {
        FacturXError(e.to_string())
    }
}

/// Splices `invoice_xml` into `pdf` as a Factur-X-compliant embedded
/// file: a `/Type /EmbeddedFile` stream named exactly `factur-x.xml`,
/// referenced from both the `/Names /EmbeddedFiles` name tree and the
/// document-level `/AF` array (ISO 19005-3 requires both, not just one),
/// plus XMP metadata declaring `pdfaid:part`/`pdfaid:conformance` and the
/// Factur-X `fx:` extension schema.
///
/// `icc_srgb_profile`, when supplied, is embedded verbatim as the
/// document's `/OutputIntent` -- sourcing a genuine sRGB ICC profile is
/// the caller's responsibility. When `None`, no `/OutputIntent` is added:
/// see docs/spikes/002-factur-x-embedding.md for why fabricating one here
/// would be worse than leaving it out.
pub fn embed_invoice(
    pdf: &[u8],
    invoice_xml: &[u8],
    profile: FacturXProfile,
    icc_srgb_profile: Option<&[u8]>,
) -> Result<Vec<u8>, FacturXError> {
    let mut doc = Document::load_mem(pdf)?;

    let file_id = embed_xml_stream(&mut doc, invoice_xml);
    let filespec_id = build_filespec(&mut doc, file_id);
    link_embedded_file(&mut doc, filespec_id)?;

    if let Some(icc) = icc_srgb_profile {
        add_output_intent(&mut doc, icc)?;
    }

    set_xmp_metadata(&mut doc, profile)?;

    let mut out = Vec::new();
    doc.save_to(&mut out)?;
    Ok(out)
}

fn embed_xml_stream(doc: &mut Document, invoice_xml: &[u8]) -> ObjectId {
    let mut stream = Stream::new(
        dictionary! {
            "Type" => "EmbeddedFile",
            "Subtype" => "text/xml",
        },
        invoice_xml.to_vec(),
    );
    // An uncompressed embedded XML stream is still perfectly valid, just
    // a little larger -- never worth failing the whole embed over.
    let _ = stream.compress();
    doc.add_object(stream)
}

fn build_filespec(doc: &mut Document, file_id: ObjectId) -> ObjectId {
    let filespec = dictionary! {
        "Type" => "Filespec",
        "F" => Object::string_literal(FACTUR_X_FILENAME.to_string()),
        "UF" => Object::string_literal(FACTUR_X_FILENAME.to_string()),
        "AFRelationship" => "Data",
        "Desc" => Object::string_literal("Factur-X invoice data".to_string()),
        "EF" => dictionary! {
            "F" => Object::Reference(file_id),
            "UF" => Object::Reference(file_id),
        },
    };
    doc.add_object(Object::Dictionary(filespec))
}

fn link_embedded_file(doc: &mut Document, filespec_id: ObjectId) -> Result<(), FacturXError> {
    let catalog = doc.catalog_mut()?;
    catalog.set(
        "Names",
        dictionary! {
            "EmbeddedFiles" => dictionary! {
                "Names" => vec![
                    Object::string_literal(FACTUR_X_FILENAME.to_string()),
                    Object::Reference(filespec_id),
                ],
            },
        },
    );
    catalog.set("AF", vec![Object::Reference(filespec_id)]);
    Ok(())
}

fn add_output_intent(doc: &mut Document, icc_srgb_profile: &[u8]) -> Result<(), FacturXError> {
    let mut icc_stream = Stream::new(
        dictionary! {
            "N" => 3, // 3 color components: RGB
        },
        icc_srgb_profile.to_vec(),
    );
    let _ = icc_stream.compress();
    let icc_id = doc.add_object(icc_stream);

    let output_intent = dictionary! {
        "Type" => "OutputIntent",
        "S" => "GTS_PDFA1",
        "OutputConditionIdentifier" => Object::string_literal("sRGB IEC61966-2.1".to_string()),
        "Info" => Object::string_literal("sRGB IEC61966-2.1".to_string()),
        "DestOutputProfile" => Object::Reference(icc_id),
    };
    doc.catalog_mut()?
        .set("OutputIntents", vec![Object::Dictionary(output_intent)]);
    Ok(())
}

/// ISO 19005-2/3's PDF/A Extension Schema mechanism, populated with the
/// Factur-X namespace. A validator like veraPDF rejects any custom XMP
/// namespace it doesn't already recognize as "Extension schema not
/// defined" / "Omission of extension schema description" unless the
/// producer declares it this way -- this is what makes the `fx:` block
/// below legal PDF/A-3B rather than merely well-formed XML. The
/// `pdfaExtension`/`pdfaSchema`/`pdfaProperty` namespace URIs are the
/// fixed ones ISO 19005 itself defines for this mechanism, not
/// Factur-X-specific.
fn xmp_packet(profile: FacturXProfile) -> String {
    format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n\
  <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
    <rdf:Description rdf:about=\"\" xmlns:pdfaid=\"http://www.aiim.org/pdfa/ns/id/\">\n\
      <pdfaid:part>3</pdfaid:part>\n\
      <pdfaid:conformance>B</pdfaid:conformance>\n\
    </rdf:Description>\n\
    <rdf:Description rdf:about=\"\"\n\
        xmlns:pdfaExtension=\"http://www.aiim.org/pdfa/ns/extension/\"\n\
        xmlns:pdfaSchema=\"http://www.aiim.org/pdfa/ns/schema#\"\n\
        xmlns:pdfaProperty=\"http://www.aiim.org/pdfa/ns/property#\">\n\
      <pdfaExtension:schemas>\n\
        <rdf:Bag>\n\
          <rdf:li rdf:parseType=\"Resource\">\n\
            <pdfaSchema:schema>Factur-X PDFA Extension Schema</pdfaSchema:schema>\n\
            <pdfaSchema:namespaceURI>urn:factur-x:pdfa:CrossIndustryDocument:invoice:1p0#</pdfaSchema:namespaceURI>\n\
            <pdfaSchema:prefix>fx</pdfaSchema:prefix>\n\
            <pdfaSchema:property>\n\
              <rdf:Seq>\n\
                <rdf:li rdf:parseType=\"Resource\">\n\
                  <pdfaProperty:name>DocumentFileName</pdfaProperty:name>\n\
                  <pdfaProperty:valueType>Text</pdfaProperty:valueType>\n\
                  <pdfaProperty:category>external</pdfaProperty:category>\n\
                  <pdfaProperty:description>name of the embedded XML invoice file</pdfaProperty:description>\n\
                </rdf:li>\n\
                <rdf:li rdf:parseType=\"Resource\">\n\
                  <pdfaProperty:name>DocumentType</pdfaProperty:name>\n\
                  <pdfaProperty:valueType>Text</pdfaProperty:valueType>\n\
                  <pdfaProperty:category>external</pdfaProperty:category>\n\
                  <pdfaProperty:description>type of the hybrid document, always INVOICE</pdfaProperty:description>\n\
                </rdf:li>\n\
                <rdf:li rdf:parseType=\"Resource\">\n\
                  <pdfaProperty:name>Version</pdfaProperty:name>\n\
                  <pdfaProperty:valueType>Text</pdfaProperty:valueType>\n\
                  <pdfaProperty:category>external</pdfaProperty:category>\n\
                  <pdfaProperty:description>version of the Factur-X XML schema</pdfaProperty:description>\n\
                </rdf:li>\n\
                <rdf:li rdf:parseType=\"Resource\">\n\
                  <pdfaProperty:name>ConformanceLevel</pdfaProperty:name>\n\
                  <pdfaProperty:valueType>Text</pdfaProperty:valueType>\n\
                  <pdfaProperty:category>external</pdfaProperty:category>\n\
                  <pdfaProperty:description>conformance level of the embedded Factur-X data</pdfaProperty:description>\n\
                </rdf:li>\n\
              </rdf:Seq>\n\
            </pdfaSchema:property>\n\
          </rdf:li>\n\
        </rdf:Bag>\n\
      </pdfaExtension:schemas>\n\
    </rdf:Description>\n\
    <rdf:Description rdf:about=\"\" xmlns:fx=\"urn:factur-x:pdfa:CrossIndustryDocument:invoice:1p0#\">\n\
      <fx:DocumentType>INVOICE</fx:DocumentType>\n\
      <fx:DocumentFileName>{FACTUR_X_FILENAME}</fx:DocumentFileName>\n\
      <fx:Version>1.0</fx:Version>\n\
      <fx:ConformanceLevel>{level}</fx:ConformanceLevel>\n\
    </rdf:Description>\n\
  </rdf:RDF>\n\
</x:xmpmeta>\n\
<?xpacket end=\"w\"?>",
        level = profile.xmp_conformance_level(),
    )
}

fn set_xmp_metadata(doc: &mut Document, profile: FacturXProfile) -> Result<(), FacturXError> {
    let xmp = xmp_packet(profile);
    let stream = Stream::new(
        dictionary! {
            "Type" => "Metadata",
            "Subtype" => "XML",
        },
        xmp.into_bytes(),
    )
    // XMP packets are read directly by parsers that don't expect a
    // stream filter; leave this one uncompressed regardless of size.
    .with_compression(false);
    let metadata_id = doc.add_object(stream);
    doc.catalog_mut()?
        .set("Metadata", Object::Reference(metadata_id));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::Object as LoObject;

    fn minimal_pdf() -> Vec<u8> {
        crate::render_pdf(
            "<html><body><p>Invoice</p></body></html>",
            &crate::options::RenderOptions::default(),
        )
        .expect("renders")
    }

    #[test]
    fn embeds_the_xml_under_both_names_and_af() {
        let pdf = minimal_pdf();
        let out = embed_invoice(&pdf, b"<Invoice/>", FacturXProfile::En16931, None).unwrap();
        let doc = Document::load_mem(&out).unwrap();
        let catalog = doc.catalog().unwrap();

        let af = catalog.get(b"AF").unwrap().as_array().unwrap();
        assert_eq!(af.len(), 1, "{af:?}");
        let LoObject::Reference(filespec_id) = af[0] else {
            panic!("AF entry should be a reference")
        };

        let names = catalog.get(b"Names").unwrap().as_dict().unwrap();
        let embedded = names.get(b"EmbeddedFiles").unwrap().as_dict().unwrap();
        let name_array = embedded.get(b"Names").unwrap().as_array().unwrap();
        assert_eq!(name_array.len(), 2, "{name_array:?}");
        assert_eq!(
            name_array[0].as_str().unwrap(),
            FACTUR_X_FILENAME.as_bytes()
        );
        let LoObject::Reference(named_filespec_id) = name_array[1] else {
            panic!("Names entry should be a reference")
        };
        assert_eq!(
            named_filespec_id, filespec_id,
            "AF and Names must point at the same filespec"
        );

        let filespec = doc.get_object(filespec_id).unwrap().as_dict().unwrap();
        assert_eq!(
            filespec.get(b"AFRelationship").unwrap().as_name().unwrap(),
            b"Data"
        );
        let ef = filespec.get(b"EF").unwrap().as_dict().unwrap();
        let LoObject::Reference(file_id) = *ef.get(b"F").unwrap() else {
            panic!("EF/F should be a reference")
        };
        let embedded_file = doc.get_object(file_id).unwrap().as_stream().unwrap();
        let xml = embedded_file.get_plain_content().unwrap();
        assert_eq!(xml, b"<Invoice/>");
    }

    #[test]
    fn xmp_names_the_conformance_level_and_filename() {
        let pdf = minimal_pdf();
        let out = embed_invoice(&pdf, b"<Invoice/>", FacturXProfile::Basic, None).unwrap();
        let doc = Document::load_mem(&out).unwrap();
        let catalog = doc.catalog().unwrap();
        let LoObject::Reference(metadata_id) = *catalog.get(b"Metadata").unwrap() else {
            panic!("Metadata should be a reference")
        };
        let xmp = doc
            .get_object(metadata_id)
            .unwrap()
            .as_stream()
            .unwrap()
            .get_plain_content()
            .unwrap();
        let xmp = String::from_utf8(xmp).unwrap();
        assert!(xmp.contains("<pdfaid:part>3</pdfaid:part>"), "{xmp}");
        assert!(
            xmp.contains("<fx:ConformanceLevel>BASIC</fx:ConformanceLevel>"),
            "{xmp}"
        );
        assert!(
            xmp.contains("<fx:DocumentFileName>factur-x.xml</fx:DocumentFileName>"),
            "{xmp}"
        );
        // The extension schema block declaring the `fx` namespace --
        // without it a PDF/A validator rejects the file for an
        // undeclared custom namespace, even though the fx: block above
        // is well-formed XML.
        assert!(
            xmp.contains("<pdfaSchema:namespaceURI>urn:factur-x:pdfa:CrossIndustryDocument:invoice:1p0#</pdfaSchema:namespaceURI>"),
            "{xmp}"
        );
        assert!(
            xmp.contains("<pdfaSchema:prefix>fx</pdfaSchema:prefix>"),
            "{xmp}"
        );
    }

    #[test]
    fn without_an_icc_profile_no_output_intent_is_added() {
        let pdf = minimal_pdf();
        let out = embed_invoice(&pdf, b"<Invoice/>", FacturXProfile::En16931, None).unwrap();
        let doc = Document::load_mem(&out).unwrap();
        assert!(doc.catalog().unwrap().get(b"OutputIntents").is_err());
    }

    #[test]
    fn a_supplied_icc_profile_is_embedded_as_the_output_intent() {
        let pdf = minimal_pdf();
        let icc = b"fake icc profile bytes";
        let out = embed_invoice(&pdf, b"<Invoice/>", FacturXProfile::En16931, Some(icc)).unwrap();
        let doc = Document::load_mem(&out).unwrap();
        let intents = doc
            .catalog()
            .unwrap()
            .get(b"OutputIntents")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(intents.len(), 1);
        let intent = intents[0].as_dict().unwrap();
        let LoObject::Reference(icc_id) = *intent.get(b"DestOutputProfile").unwrap() else {
            panic!("DestOutputProfile should be a reference")
        };
        let embedded_icc = doc
            .get_object(icc_id)
            .unwrap()
            .as_stream()
            .unwrap()
            .get_plain_content()
            .unwrap();
        assert_eq!(embedded_icc, icc);
    }

    #[test]
    fn a_corrupt_input_pdf_is_a_clean_error_not_a_panic() {
        let err = embed_invoice(b"not a pdf", b"<Invoice/>", FacturXProfile::Basic, None)
            .expect_err("garbage input should fail cleanly");
        assert!(!err.to_string().is_empty());
    }
}
