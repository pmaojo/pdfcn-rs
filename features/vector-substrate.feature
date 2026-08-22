Feature: Wave 1 vector substrate — %Vector, Charts v2 and %Barcode over an opt-in SVG-to-PNG pass
  As a developer generating business documents
  I want charts with axes, scannable barcodes, and arbitrary caller-supplied SVG
  rendered through one shared substrate
  So that six media features are one primitive plus cheap generators, the
  default serverless binary stays byte-for-byte unchanged, and every invalid
  input degrades to a visible marker instead of failing the render

  # Ola 1.2 — the substrate itself
  @covers(pdfcn-core/src/raster.rs)
  Scenario: Rendering SVG source text to print-density PNG bytes
    Given a valid SVG document and no layout box
    When it is rasterized by the vector substrate
    Then the output is PNG bytes sized at ~300dpi of the SVG viewport

  @covers(pdfcn-core/src/raster.rs)
  Scenario: Capping a monster SVG at the absolute dimension limit
    Given an SVG whose viewport exceeds the pipeline's absolute pixel cap
    When it is rasterized without a known layout box
    Then no output dimension exceeds that cap

  @covers(pdfcn-core/src/options.rs)
  Scenario: Carrying caller SVG through the side channel
    Given RenderOptions populated with id-to-SVG-source entries
    When a template references "%Vector(id="...")"
    Then each placeholder is filled from its side-channel entry, not from markup

  @covers(pdfcn-core/src/assets.rs)
  Scenario: Degrading an unfillable placeholder like any missing image
    Given a pdfcn-vector placeholder whose id is absent from the side channel
    When the asset pass runs
    Then the placeholder is left unresolved and the render still succeeds

  # Ola 1.3 — the generators
  @covers(pdfcn-components/src/chart_vector.rs)
  Scenario Outline: Emitting a self-describing spec placeholder per chart kind
    Given a "<component>" instance fed a JSON array of numbers via "values"
    When it expands
    Then the output is an img placeholder whose src carries a hex-encoded spec of kind "<kind>"

    Examples:
      | component        | kind  |
      | LineChart        | line  |
      | StackedBarChart  | stack |
      | PieChart         | pie   |
      | Sparkline        | spark |

  @covers(pdfcn-core/src/charts_svg.rs)
  Scenario: Drawing axes, gridlines and escaped labels for a line chart
    Given a line spec with series, x labels and series names
    When the SVG is generated
    Then it contains gridlines, nice-tick axis labels, one polyline per series
    And label text is XML-escaped rather than raw

  @covers(pdfcn-core/src/charts_svg.rs)
  Scenario: Rejecting malformed chart specs without panicking
    Given a chart spec that is empty, unknown-kind, ragged or negative-valued
    When the SVG is requested
    Then generation returns nothing and the caller degrades gracefully

  @covers(pdfcn-core/src/barcode.rs)
  Scenario: Encoding a value as Code 128 with automatic B/C switching
    Given a printable ASCII value containing a run of at least four digits
    When the Code 128 symbols are produced
    Then digit runs are packed two-per-symbol in Code C with a valid mod-103 checksum and stop pattern

  @covers(pdfcn-core/src/barcode.rs)
  Scenario: Encoding EAN-13 with checksum enforcement
    Given twelve digits for an EAN-13 symbol
    When it is encoded
    Then the check digit is computed and the guard bars render taller than the data bars

  @covers(pdfcn-core/src/barcode.rs)
  Scenario: Refusing unencodable payloads explicitly
    Given an EAN-13 value with a wrong check digit or Code 128 input outside printable ASCII
    When encoding is attempted
    Then the result is None and nothing is drawn

  @covers(pdfcn-components/src/barcode.rs)
  Scenario: Validating scheme and shape in the component layer
    Given a "%Barcode" with an unsupported scheme or a non-digit ean13 value
    When it expands
    Then the output is an invalid-component marker naming the mistake

  @covers(pdfcn-components/src/lib.rs)
  Scenario: Naming the missing cargo feature instead of rendering dead placeholders
    Given pdfcn built without the "vector" cargo feature
    When a template uses any Charts v2 component or "%Barcode"
    Then it expands to an explicit marker naming the disabled feature

  @covers(pdfcn-components/src/vector.rs)
  Scenario: Requiring an id to route the side channel
    Given a "%Vector" without a non-empty "id"
    When it expands
    Then the output is an invalid-component marker explaining the side channel

  # Ola 1.4 — utility fidelity riding along this wave
  @covers(pdfcn-styles/src/utilities.rs)
  Scenario: Resolving the new static typography and box utilities
    Given the classes leading-relaxed, tracking-wide, max-w-xl, max-h-16 and opacity-50
    When each is resolved
    Then each returns its Tailwind-equivalent declaration

  @covers(pdfcn-styles/src/utilities.rs)
  Scenario: Resolving directional borders, column spans and aspect ratios
    Given the classes border-b-2, border-r-red-500, col-span-3, col-span-full and aspect-video
    When each is resolved
    Then each returns its directional or structural CSS equivalent
