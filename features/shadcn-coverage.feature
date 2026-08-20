Feature: shadcn/ui look-and-feel coverage — closing the design-token and component gaps
  As a developer building documents that must visually match a shadcn/ui product
  I want the component registry and utility resolver to reproduce shadcn's
  default theme tokens and its print-appropriate components
  So that "shadcn look and feel" is a faithful, reviewable subset rather than
  a handful of inspired-by primitives, with an explicit documented boundary
  for what a static PDF cannot reproduce (menus, dialogs, tooltips, and other
  interactive-only primitives)

  # Wave 0 — design tokens. Every component below inherits its fidelity from
  # this layer, so it lands before any new component.
  @covers(pdfcn-styles/src/tokens.rs)
  Scenario: Resolving shadcn's default theme color tokens
    Given the utility resolver is asked for "bg-primary", "text-primary-foreground" and "border-input"
    When each class is resolved
    Then it returns shadcn's default theme CSS values, not just the hand-picked palette subset

  @covers(pdfcn-styles/src/tokens.rs)
  Scenario Outline: Resolving the full neutral and accent color scales
    Given the utility resolver is asked for "<class>"
    When the class is resolved
    Then it returns a declaration for the shadcn/Tailwind "<scale>" scale at shade "<shade>"

    Examples:
      | class          | scale   | shade |
      | bg-slate-950   | slate   | 950   |
      | bg-zinc-100    | zinc    | 100   |
      | bg-neutral-500 | neutral | 500   |
      | bg-stone-800   | stone   | 800   |

  @covers(pdfcn-styles/src/tokens.rs)
  Scenario: Resolving the shadow and radius scales
    Given the utility resolver is asked for "shadow-sm", "shadow-md", "shadow-lg", "shadow-xl", "rounded-2xl" and "rounded-3xl"
    When each class is resolved
    Then each returns a distinct declaration matching shadcn's elevation and radius scale

  # Wave 1 — first tranche of print-appropriate shadcn components
  @covers(pdfcn-components/src/alert.rs)
  Scenario Outline: Rendering an Alert in each shadcn variant
    Given a "%Alert(variant=\"<variant>\" title=\"<title>\")" component instance
    When rendered
    Then the emitted markup uses the "<variant>" variant's shadcn utility classes
    And it exposes an icon slot consistent with shadcn's Alert anatomy

    Examples:
      | variant     | title       |
      | default     | Heads up    |
      | destructive | Payment due |

  @covers(pdfcn-components/src/avatar.rs)
  Scenario: Rendering an Avatar with an image source
    Given a "%Avatar(src=\"https://example.com/a.png\" alt=\"Ada\")" component instance
    When rendered
    Then the emitted markup is a circular image matching shadcn's Avatar sizing

  @covers(pdfcn-components/src/avatar.rs)
  Scenario: Falling back to initials when an Avatar has no image
    Given a "%Avatar(name=\"Ada Lovelace\")" component instance with no "src"
    When rendered
    Then the emitted markup shows the "AL" initials fallback, matching shadcn's AvatarFallback

  @covers(pdfcn-components/src/form_field.rs)
  Scenario Outline: Rendering a static filled form field
    Given a "%<component>(label=\"<label>\" value=\"<value>\")" component instance
    When rendered
    Then it emits a labeled, boxed field matching shadcn's Input/Textarea/Select anatomy

    Examples:
      | component | label     | value        |
      | Input     | Full name | Ada Lovelace |
      | Textarea  | Notes     | Paid in full |
      | Select    | Status    | Approved     |

  @covers(pdfcn-components/src/form_field.rs)
  Scenario Outline: Rendering a static Checkbox / RadioItem glyph
    Given a "%<component>(checked=\"<checked>\" label=\"<label>\")" component instance
    When rendered
    Then it emits the "<checked>" glyph state matching shadcn's checked/unchecked styling

    Examples:
      | component | checked | label        |
      | Checkbox  | true    | Terms agreed |
      | Checkbox  | false   | Newsletter   |
      | RadioItem | true    | Option A     |

  @covers(pdfcn-components/src/progress.rs)
  Scenario: Rendering a static Progress bar at a given percentage
    Given a "%Progress(value=\"65\")" component instance
    When rendered
    Then the emitted markup's filled track width is 65% of the track, matching shadcn's Progress anatomy

  @covers(pdfcn-components/src/nav.rs)
  Scenario: Rendering a Breadcrumb trail
    Given a "%Breadcrumb(items={{ trail }})" component instance over 3 crumbs
    When rendered
    Then the emitted markup separates each crumb with shadcn's chevron separator
    And the last crumb is rendered as the current page, not a link

  @covers(pdfcn-components/src/nav.rs)
  Scenario: Rendering a Pagination footer
    Given a "%Pagination(current=\"3\" total=\"12\")" component instance
    When rendered
    Then the emitted markup shows "Page 3 of 12" styled with shadcn's Pagination anatomy

  # The boundary: interactive-only shadcn primitives get a clear rejection,
  # not a silent no-op, so the gap is documented in behavior, not just prose.
  @covers(pdfcn-components/src/lib.rs)
  Scenario Outline: Rejecting an interactive-only shadcn component with a clear message
    Given a "%<component>" component instance
    When rendered
    Then the registry returns an explicit "interactive-only, unsupported in static PDF output" error
    And it does not silently fall through as an unknown component

    Examples:
      | component    |
      | Dialog       |
      | Tooltip      |
      | DropdownMenu |
      | Popover      |
      | Sonner       |
