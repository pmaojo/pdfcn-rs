Feature: Image fast path — photos reach the PDF without being re-encoded
  As a developer rendering image-heavy documents (catalogs, brochures)
  I want each photo embedded the way Typst embeds it, as the file it came in
  So that a hundred-photo catalog renders in well under a second instead of
  paying a decode, a resample and two JPEG encodes per photo

  @covers(pdfcn-core/src/jpeg_passthrough.rs)
  Scenario: Embedding a JPEG byte for byte
    Given a template with an img whose src is a baseline RGB JPEG
    When it is rendered with default image options
    Then the PDF holds exactly one image XObject for it
    And that XObject is a DCTDecode stream whose bytes are the original file

  @covers(pdfcn-core/src/jpeg_passthrough.rs)
  Scenario: Keeping a greyscale JPEG's single channel
    Given a template with an img whose src is a one-component JPEG
    When it is rendered
    Then its XObject declares the DeviceGray colour space

  @covers(pdfcn-core/src/jpeg_passthrough.rs)
  Scenario Outline: Leaving files and options the pass cannot vouch for to printpdf
    Given an image that is <case>
    When it is rendered
    Then printpdf decodes and re-encodes it exactly as before

    Examples:
      | case                                                |
      | a PNG                                               |
      | a CMYK, 12-bit or arithmetic-coded JPEG             |
      | a JPEG rendered with a quality, size budget or greyscale option |

  @covers(pdfcn-core/src/jpeg_passthrough.rs)
  Scenario: Reading arbitrary bytes as a JPEG header never panics
    Given empty, truncated or non-JPEG bytes
    When the frame header is sniffed
    Then the result is "not a JPEG this pass handles", never a crash

  @covers(pdfcn-core/src/assets.rs)
  Scenario: Embedding photos at their own resolution unless asked to shrink
    Given a photo far larger than its layout box can print
    When it is rendered with default options
    Then it is embedded at its own resolution, untouched

  @covers(pdfcn-core/src/assets.rs)
  Scenario: Deciding whether to shrink from the header alone
    Given shrinking is requested and a photo less than twice the resolution its box can use
    When the asset pass runs
    Then the photo is neither decoded nor re-encoded
    And a photo at least twice over is shrunk by area averaging to ~300dpi of its box

  @covers(pdfcn-template/src/lib.rs)
  Scenario: Resolving plain variable paths without serializing the context
    Given an expression that is a plain path such as "p.name" or "rows[0].total"
    When it is evaluated against a large data context
    Then it resolves straight against the JSON, giving what minijinja would give
    And any other expression, or a path that does not resolve, is left to minijinja
