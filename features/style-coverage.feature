Feature: Utility-class coverage — wider Tailwind subset, variants and unresolved-class audit

  @covers(pdfcn-styles/src/utilities.rs)
  Scenario Outline: Resolving a utility class to its CSS declaration
    Given a template using the class "<class>"
    When the stylesheet is built
    Then it contains the declaration "<declaration>"

    Examples:
      | class         | declaration                       |
      | mx-auto       | margin-left:auto                  |
      | -mt-2         | margin-top:-0.5rem                |
      | w-1/2         | width:50%                         |
      | min-w-0       | min-width:0                       |
      | size-8        | width:2rem                        |
      | bg-black/50   | background-color:rgba(0,0,0,0.5)  |
      | underline     | text-decoration:underline         |
      | truncate      | text-overflow:ellipsis            |
      | text-center   | text-align:center                 |
      | text-6xl      | font-size:3.75rem                 |
      | border-dashed | border-style:dashed               |
      | rounded-t-lg  | border-top-left-radius:8px        |
      | justify-around| justify-content:space-around      |
      | self-center   | align-self:center                 |

  @covers(pdfcn-styles/src/lib.rs)
  Scenario: Print and structural variants
    Given a template using the classes "print:hidden first:pt-0 last:border-b-0"
    When the stylesheet is built
    Then "print:hidden" applies unconditionally as display:none
    And "first:" and "last:" emit :first-child and :last-child rules
    And "hover:" and "md:" variants are not recognized

  @covers(pdfcn-styles/src/lib.rs)
  Scenario: Reporting classes that resolve to no CSS
    Given a template using the classes "p-4 flexx"
    When unresolved classes are requested
    Then "flexx" is reported and "p-4" is not

  @covers(pdfcn-styles/src/utilities.rs)
  Scenario: Every utility used by examples and components resolves
    Given the templates in "examples/" and the component sources
    When their class attributes are audited against the stylesheet builder
    Then only the known semantic marker classes are left unresolved

  @covers(pdfcn-cli/src/build.rs)
  Scenario: Warning about, or failing on, unrecognized utility classes
    Given a template using the classes "p-4 flexx hover:bg-red-500"
    When "pdfcn build" runs without "--strict"
    Then it warns that "flexx" and "hover:bg-red-500" were ignored and still writes the PDF
    When "pdfcn build" runs with "--strict"
    Then it fails naming the unrecognized classes

  @covers(pdfcn-styles/src/decoration.rs)
  Scenario: Underline and line-through on a block element reach the output
    Given a "%p" with the class "underline" or "line-through"
    When the document is rendered
    Then the text is wrapped in decorated inline spans so the engine draws the line

  @covers(pdfcn-styles/src/tokens.rs)
  Scenario Outline: Shadow utilities are painted
    Given a white box with the class "<shadow>"
    When the document is rendered
    Then the drawn output differs from the same box without a shadow

    Examples:
      | shadow    |
      | shadow-sm |
      | shadow    |
      | shadow-md |
      | shadow-lg |
      | shadow-2xl|

  @covers(pdfcn-styles/src/composite.rs)
  Scenario Outline: Composite utilities are painted
    Given a template using the classes "<classes>"
    When the document is rendered
    Then the drawn output differs from the same markup without them
    And the composed classes are not reported as unrecognized

    Examples:
      | classes                                      |
      | divide-y divide-slate-300                    |
      | flex divide-x-2 divide-red-500               |
      | bg-gradient-to-r from-blue-500 to-red-500    |
      | ring-2 ring-red-500                          |
      | outline outline-blue-500                     |

  @covers(pdfcn-styles/src/utilities.rs)
  Scenario Outline: Grid row utilities
    Given a grid using the class "<class>"
    When the document is rendered
    Then the row layout differs from the same grid without it

    Examples:
      | class       |
      | grid-rows-4 |
      | row-span-2  |

  @covers(pdfcn-core/src/lib.rs)
  Scenario Outline: Opacity utilities are painted, also across page breaks
    Given a box with the class "<opacity>"
    When the document is rendered
    Then the drawn output differs from the same box without it

    Examples:
      | opacity    |
      | opacity-50 |
      | opacity-25 |
