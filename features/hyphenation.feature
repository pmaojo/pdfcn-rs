Feature: Automatic hyphenation as an opt-in feature
  As a developer deploying pdfcn where every megabyte counts
  I want the ~3 MB of hyphenation dictionaries out of the default build
  So that only documents that ask for hyphens: auto pay for them

  @covers(pdfcn-core/tests/hyphens.rs)
  Scenario: Rendering hyphens: auto with or without the dictionaries
    Given a paragraph styled with hyphens: auto
    When it is rendered with the hyphenation feature on or off
    Then the text is drawn either way, and without the feature it is never an error
