Feature: The stylesheet minifier, written in-house
  As a developer shipping pdfcn in a size-capped serverless function
  I want the generated stylesheet compacted without a CSS engine dependency
  So that the binary drops lightningcss (a fifth of its code) and colours always reach the layout engine readable

  @covers(pdfcn-styles/src/minify.rs)
  Scenario Outline: Writing every colour as hex
    Given a declaration whose colour is written as <syntax>
    When the stylesheet is minified
    Then the colour is the same colour as hex, rounded as lightningcss rounded it

    Examples:
      | syntax                        |
      | hsl(222.2, 84%, 4.9%)         |
      | hsl(210 40% 96%/0.5)          |
      | rgb(0 0 0/50%)                |
      | rgba(0,0,0,0.5)               |

  @covers(pdfcn-styles/src/minify.rs)
  Scenario: Leaving what it does not recognise alone
    Given an oklch() colour, a url() or a malformed colour call
    When the stylesheet is minified
    Then it is kept exactly as written, and malformed CSS never panics

  @covers(pdfcn-core/tests/css_equivalence.rs)
  Scenario: Drawing what the old minifier drew
    Given every utility pdfcn-styles recognises, crossed with its scales, colours and arbitrary values
    When each block of them is rendered with the new stylesheet and with what the old code shipped
    Then the pages draw the same operations, colours within 1/255

  @covers(pdfcn-core/tests/css_equivalence.rs)
  Scenario: Fixing what the old unminified fallback broke
    Given a document with a class that sent the old stylesheet out unminified, such as w-1/2
    When it also uses a space-separated colour or shadcn's border token
    Then the colour is painted and the rules after it survive
    And the border takes its token colour instead of black
