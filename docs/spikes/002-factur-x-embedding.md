# Spike 002: Factur-X invoice embedding

## Question

Ola 3 targets electronic-invoice support. Two candidate formats surfaced:
Factur-X/ZUGFeRD (a PDF/A-3 file with an EN 16931 CII XML embedded inside
it) and Facturae (Spain's B2G format: a standalone signed XML document,
no PDF involved at all). The user picked "both, Factur-X first" -- this
spike scopes Factur-X, since Facturae doesn't touch the PDF pipeline at
all and is really a separate feature.

## What Factur-X actually requires of the PDF

- The container PDF must conform to **PDF/A-3** (any of MINIMUM, BASIC WL,
  BASIC, EN 16931, EXTENDED profiles map to PDF/A-3B -- none of them need
  the accessibility/tagging machinery of the "a" conformance letter).
- The CII XML must be embedded as `/Type /EmbeddedFile`, named exactly
  `factur-x.xml`, with `/AFRelationship /Data`, referenced from both a
  `/Names /EmbeddedFiles` name tree **and** a document-level `/AF` array
  (both are required by ISO 19005-3, not just one).
- The catalog's XMP metadata must declare `pdfaid:part = 3`,
  `pdfaid:conformance = B`, plus the Factur-X extension schema
  (`fx:` namespace: `DocumentType`, `DocumentFileName`, `Version`,
  `ConformanceLevel`).
- PDF/A also requires an `/OutputIntent` with an embedded ICC profile
  matching the document's actual color space.

## What printpdf 0.12.6 actually gives us (verified against its real source)

- **No embedded-file support whatsoever.** `grep -rn "embedded_file|EmbeddedFile|Attachment"`
  across printpdf's `src/` returns zero matches. There is no API to attach
  a file, named or not.
- **`PdfConformance` is real but partial.** `src/conformance.rs` is a
  genuine enum with `must_have_xmp_metadata()`/`must_have_icc_profile()`,
  and `src/serialize.rs:118-147` really does add an `/OutputIntents` entry
  and an XMP `/Metadata` stream when those return true -- this is not a
  stub. But: `must_have_xmp_metadata()` returns `false` for every `A*`
  variant including `A3_2012_PDF_1_7` (only the `X*` print-conformance
  variants return `true`) -- so setting `PdfConformance::A3_2012_PDF_1_7`
  does **not** make printpdf emit the XMP block PDF/A actually needs. And
  the ICC profile printpdf embeds when `must_have_icc_profile()` is true is
  hardcoded (`src/serialize.rs:121`, `include_bytes!("./res/CoatedFOGRA39.icc")`)
  to a **CMYK** print profile -- wrong color space for an RGB-rendered
  invoice; embedding it here would produce a document whose declared
  OutputIntent doesn't match its own content, which is worse than no
  OutputIntent for PDF/A validation purposes.
- Conclusion: printpdf's conformance plumbing is real but aimed at
  print-industry (PDF/X, CMYK) workflows, not PDF/A-3 hybrid invoices.
  None of it is usable as-is for Factur-X.

## What we build instead: post-process with lopdf

`lopdf` is already a transitive dependency of printpdf (confirmed in
`Cargo.lock`, same 0.44.0), so pinning it as a direct optional dependency
adds no new binary in the dependency tree, only a direct edge to one
already being compiled. Verified against its real source
(`lopdf-0.44.0/src/`):

- `Document::load_mem(&[u8]) -> Result<Document>` parses the bytes printpdf
  already produced (`src/reader.rs:94`).
- `doc.add_object(object) -> ObjectId` (`src/creator.rs:21`) inserts a new
  indirect object (stream or dictionary) and hands back a reference; no
  renumbering happens on save, so references built during construction
  stay valid.
- `doc.catalog_mut()` (`src/document.rs:521`) gives a mutable `&mut
  Dictionary` for the trailer's `/Root` -- exactly what's needed to set
  `/Names`, `/AF`, `/OutputIntents` and `/Metadata` by hand.
- `doc.save_to(&mut Vec<u8>)` (`src/writer.rs:22`) re-serializes.

lopdf has **no** built-in Filespec/EmbeddedFiles/AF helpers (confirmed:
zero matches for those terms anywhere in its source, README, or
examples/) -- every dictionary here is built by hand from `Dictionary`/
`Object`/`Stream`.

## Scope decision: ICC profile is caller-supplied, not fabricated

The project's existing rule for this kind of gap (see `pdfcn-core/src/
barcode.rs`'s DataMatrix/PDF417 exclusion): a subtly wrong artifact is
worse than a documented absence. An sRGB ICC profile is a precise binary
blob; typing out "a close-enough sRGB profile" from memory risks shipping
bytes that silently fail validator checksums or, worse, pass validation
while describing the wrong transfer curve. Rather than guess:

- `embed_invoice()` takes `icc_srgb_profile: Option<&[u8]>`. When `Some`,
  it embeds a real `/OutputIntent` referencing exactly the bytes the
  caller supplied (their responsibility to source a genuine sRGB ICC
  profile once, e.g. from their own asset pipeline). When `None`, no
  `/OutputIntent` is added and the resulting PDF is Factur-X-shaped
  (embedded XML, correct `/AF`, correct XMP conformance claims) but not a
  fully validator-clean PDF/A-3 file -- documented as a known limitation,
  not silently claimed as complete.
- Everything else (the XML attachment, `/Names`/`AF`, and the XMP
  `pdfaid`/`fx` block) is fully implemented, since those are exact,
  well-specified text/structure this session can construct correctly and
  verify by re-parsing the output with `lopdf` -- not something requiring
  an external, hard-to-verify binary asset.

## Not yet done, called out explicitly

No actual Factur-X validator (veraPDF, Mustang, the EU's official test
suite) is available in this sandboxed environment to confirm the produced
file validates end-to-end. The XMP field names/values and the ISO
19005-3 structural requirements above come from the published Factur-X/
ZUGFeRD specification and cross-checked structural rules, not from a
verified passing run through a real validator. This should be validated
against a real tool (e.g. veraPDF) before this is relied on for actual
regulatory e-invoicing submissions.
