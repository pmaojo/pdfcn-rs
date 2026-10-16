Feature: Hyperlinks and in-document links

  @covers(pdfcn-core/src/links.rs)
  Scenario Outline: A link becomes a PDF link annotation
    Given a template with the link "<href>"
    When the PDF is rendered
    Then the page carries a link annotation whose action is "<action>"

    Examples:
      | href                       | action                  |
      | https://example.com/a?b=1  | URI https://example.com/a?b=1 |
      | mailto:hi@example.com      | URI mailto:hi@example.com     |
      | #details                   | GoTo the page and position of id "details" |

  @covers(pdfcn-core/src/links.rs)
  Scenario: Unsafe or unresolvable targets stay plain text
    Given a template with the links "javascript:alert(1)", "#missing" and "ftp://host/file"
    When the PDF is rendered
    Then the page carries no link annotations

  @covers(pdfcn-core/src/links.rs)
  Scenario: A link that wraps gets one clickable area per line
    Given a link whose text wraps onto two lines
    When the PDF is rendered
    Then the link has two annotation rectangles

  @covers(pdfcn-core/src/links.rs)
  Scenario: Finding every link in the one layout that draws the document
    Given a template with fifty links and jump targets over several pages
    When the PDF is rendered
    Then the document is laid out once, with each link and target tracked by a class
    And the tracking class and cursor change no drawing operation
    And every link gets its annotations and every jump lands on its target's page

  @covers(pdfcn-core/tests/links.rs)
  Scenario: Fifty links over several pages
    Given twenty-five external links and twenty-five jumps to sections spread over pages
    When the PDF is rendered
    Then there are twenty-five URI annotations and twenty-five jumps in document order
