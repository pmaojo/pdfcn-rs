Feature: Photos and SVGs cost only what they must
  As someone rendering image-heavy documents with pdfcn
  I want each photo's header read once, in place, and each SVG compressed once
  So that a catalog of 100 photos and 100 SVGs spends its time laying out
  the page, not copying and re-compressing bytes it already has

  @covers(pdfcn-core/src/jpeg_header.rs)
  Scenario: A JPEG's size and orientation come from its markers
    Given a JPEG with an EXIF orientation of 1 to 8, or none
    When the asset passes ask for its size and orientation
    Then they get exactly what the image crate's decoder reports
    And they are read from the marker segments in place, with no decoder

  @covers(pdfcn-core/src/jpeg_header.rs)
  Scenario: Anything that is not a JPEG still gets an answer
    Given a PNG, or bytes cut off at any point
    When its size and orientation are asked for
    Then a PNG is read by the image crate as before
    And truncated or arbitrary bytes return nothing, never a panic

  @covers(pdfcn-core/src/vector_pdf.rs)
  Scenario: A converted SVG is compressed once and never inflated
    Given an SVG placeholder converted by svg2pdf
    When it becomes a Form XObject in the saved PDF
    Then svg2pdf writes it uncompressed, its streams are deflated once on the
    SVG's own thread with that thread's one compressor
    And the page content moves into the Form as it is, filter and all
    And the Form draws exactly what it drew before (the golden snapshots
    differ only in a trailing newline lopdf used to append)
