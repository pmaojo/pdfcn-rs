Feature: pdfcn-rs — Vercel-safe HAML-to-PDF pipeline
  As a developer generating documents (invoices, reports) on Vercel Functions
  I want a HAML-like template compiled with UI components and Tailwind-style
  utility classes to render deterministically to PDF bytes in memory
  So that I can serve PDFs from a serverless route with no Chromium, no
  process spawning, and a small static binary

  # FR-1: parser and template engine
  Scenario: Parsing indentation-based HAML-like syntax into an AST
    Given a template source using "%tag", ".class", "#id" and "(attr=\"val\")" syntax
    When the source is parsed
    Then the parser produces an AST node tree with no closing tags required

  Scenario: Interpolating variables from a JSON data context
    Given a template containing "{{ user.name }}"
    And a JSON payload where "user.name" is "Ada Lovelace"
    When the template is rendered against that payload
    Then the output HTML contains "Ada Lovelace" in place of the interpolation

  Scenario: Iterating over a collection with "- for"
    Given a template with "- for item in items" over a list of 3 items
    When the template is rendered
    Then the loop body is emitted once per item in order

  Scenario: Branching with "- if" / "- else"
    Given a template with "- if active" and an "- else" branch
    When rendered with "active" false
    Then only the else branch's output appears in the result

  Scenario: Including a reusable partial
    Given a template with "- include \"partials/footer.haml\""
    When the template is rendered
    Then the partial's nodes are spliced into the parent document at that point

  # FR-2: component registry
  Scenario: Expanding a first-class UI component tag
    Given a template using "%InvoiceTable(rows={{ items }})"
    When rendered
    Then the component registry expands it into its underlying HTML subtree
    And the expansion carries the component's default Tailwind-style classes

  Scenario: Overriding a component's variant
    Given a "%Badge(variant=\"destructive\")" component instance
    When rendered
    Then the emitted markup uses the destructive variant's utility classes

  # FR-3: styles pipeline
  Scenario: Extracting only the utility classes actually used
    Given rendered HTML referencing "p-4", "flex" and "text-lg"
    When the styles pipeline scans the output
    Then the generated stylesheet contains rules for exactly those classes
    And no unused utility rules are emitted

  Scenario: Injecting print-safe CSS by default
    Given any rendered document
    When the stylesheet is generated
    Then it includes "@page" and "print-color-adjust: exact" rules
    And table rows carry a "break-inside: avoid" rule

  # FR-4: rendering and pagination
  Scenario: Rendering a document to PDF bytes in memory
    Given a fully rendered HTML document with embedded styles
    When it is passed to the render pipeline
    Then the output is a Vec<u8> of valid PDF bytes
    And no external process or dynamic browser dependency is spawned

  Scenario: Honoring page size, orientation and margins
    Given a page configuration of size "A4", orientation "portrait", margin 10mm
    When a document is rendered
    Then every page in the resulting PDF matches those dimensions and margins

  Scenario: Avoiding a table row split across a page boundary
    Given a table taller than one page with break-inside-avoid rows
    When the document paginates
    Then no single row's content is split between two pages

  # FR-6: CLI tooling
  Scenario: Scaffolding a new template project
    When a developer runs "pdfcn new <template>"
    Then a boilerplate ".haml" file and a mock "data.json" are created

  Scenario: Building a PDF from the command line
    Given a template file and a data file
    When a developer runs "pdfcn build <template.haml> -d <data.json> -o <out.pdf>"
    Then a valid PDF file is written to the given output path

  # NFR-4: security
  Scenario: Auto-escaping interpolated values
    Given a JSON payload where a field's value is "<script>alert(1)</script>"
    When that field is interpolated into the template
    Then the rendered HTML contains the escaped entities, not raw markup
