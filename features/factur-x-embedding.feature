Feature: Wave 3 Factur-X invoice embedding
  As a developer generating invoices with pdfcn
  I want to splice a Factur-X EN 16931 XML into the rendered PDF
  So that the same document is both a human-readable PDF and a
  machine-readable e-invoice, without printpdf needing embedded-file
  support it doesn't have

  @covers(pdfcn-core/src/factur_x.rs)
  Scenario: Embedding the XML under both the Names tree and AF
    Given an already-rendered PDF and an invoice XML payload
    When the invoice is embedded at a given Factur-X profile
    Then the catalog's AF array and its Names/EmbeddedFiles tree both
    reference the same Filespec object
    And that Filespec's AFRelationship is Data and its EF stream holds the
    XML bytes unchanged

  @covers(pdfcn-core/src/factur_x.rs)
  Scenario: XMP metadata names the conformance level and attachment filename
    Given an invoice embedded at the Basic profile
    When the catalog's Metadata stream is inspected
    Then it declares pdfaid:part 3, the profile's fx:ConformanceLevel, and
    fx:DocumentFileName "factur-x.xml"

  @covers(pdfcn-core/src/factur_x.rs)
  Scenario: No OutputIntent is fabricated when the caller supplies no ICC profile
    Given an invoice embedded with no ICC profile argument
    When the catalog is inspected
    Then no OutputIntents entry exists, rather than one built from guessed bytes

  @covers(pdfcn-core/src/factur_x.rs)
  Scenario: A caller-supplied ICC profile is embedded verbatim as the OutputIntent
    Given an invoice embedded with a caller-supplied ICC profile
    When the catalog's OutputIntents entry is inspected
    Then its DestOutputProfile stream holds exactly those bytes

  @covers(pdfcn-core/src/factur_x.rs)
  Scenario: A corrupt input PDF degrades to a clean error, never a panic
    Given bytes that are not a valid PDF
    When embedding is attempted
    Then the result is an explicit error naming the failure
