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
