Feature: Zero copies — photos and SVGs are laid out by size, never as pixels
  As a developer rendering image-heavy documents on a memory-capped function
  I want a photo's bytes to travel to the PDF without copies or decoded pixels
  So that a hundred-photo catalog peaks at tens of megabytes, not hundreds

  @covers(pdfcn-core/src/jpeg_passthrough.rs)
  Scenario: Laying out a passed-through JPEG by its size alone
    Given a template with an img whose src is a baseline RGB JPEG
    When the JPEG is taken out of the image map before layout
    Then its width and height are recorded as an image placeholder
    And the bytes it is written from are still the caller's own

  @covers(pdfcn-core/src/vector_pdf.rs)
  Scenario: Converting an SVG placeholder before layout instead of rasterizing it
    Given a "%Vector" placeholder whose SVG svg2pdf converts
    When the vector pass prepares it
    Then no PNG is rendered for it
    And it is laid out at the size its PNG would have had

  @covers(pdfcn-core/src/vector_pdf.rs)
  Scenario: Falling back to the PNG when an SVG cannot become a vector
    Given a placeholder whose SVG cannot be converted
    When the vector pass prepares it
    Then it is rasterized and laid out as the PNG it always was

  @covers(pdfcn-core/src/pdf_xobjects.rs)
  Scenario: Replacing every placeholder in one load and one save
    Given a saved document with vector and JPEG placeholders
    When the post-processing pass runs
    Then the document is loaded once, every placeholder is replaced in place, and it is saved once
    And a document lopdf cannot load comes back byte for byte unchanged

  @covers(pdfcn-core/tests/zero_copy.rs)
  Scenario: A photo costs under three copies of itself and none of its pixels
    Given a render of a large photo and of a tiny one, measured by a counting allocator
    When the two peaks of live memory are compared
    Then the large photo costs less than three times its file size
    And a smooth 3000x3000 photo costs far less than its decoded pixels

  @covers(pdfcn-template/src/lib.rs)
  Scenario: Entering a loop iteration without copying the context
    Given a "- for" over many items inside a context that holds them all
    When the template is evaluated
    Then each iteration binds its item and "loop" in a frame of its own
    And an inner binding shadows the context only inside the loop, as before
    And a "- set" inside an iteration stays in that iteration
