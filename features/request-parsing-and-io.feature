Feature: One place parses page requests and reads files
  As a maintainer of pdfcn's four entry points (CLI, HTTP API, batch API,
  napi bindings)
  I want page size, orientation, margin and image format parsed by one
  function in pdfcn-core, and file IO kept out of the render pipeline
  So that the entry points cannot drift apart and the core reads as
  "prepare, lay out, save"

  @covers(pdfcn-core/src/page.rs)
  Scenario: The HTTP and napi page parsing keeps its lenient behaviour
    Given a request with size "letter", "a4", "210x297", "bogus" or none
    And an orientation of "landscape", "portrait", "sideways" or none
    When it is turned into a PageConfig with PageConfig::from_request
    Then the result equals what the former per-entry-point parser produced
    for every combination, unknown values falling back to A4 portrait

  @covers(pdfcn-core/src/page.rs)
  Scenario: The CLI keeps rejecting an unknown page size
    Given the CLI flag --size "bogus"
    When it is parsed with PageSize::from_name
    Then no size is returned and the CLI reports an error instead of
    silently using A4

  @covers(src/dto.rs)
  Scenario: Both HTTP endpoints share one page DTO
    Given the single-document and the batch endpoint
    When either deserializes a "page" object
    Then both use pdfcn_vercel::dto::PageConfigDto and get the same
    PageConfig for the same JSON

  @covers(pdfcn-core/src/fs.rs)
  Scenario: Image sources are read from disk, fetched, or left alone
    Given a document whose images use a relative path, an http(s) URL and a
    data: URI
    When render_files gathers the image bytes (pdfcn-core/src/fs.rs)
    Then the relative path is read from the template's directory
    And the URL is fetched only when a RemoteImageFetcher is supplied
    And the data: URI is never read or fetched
