---
name: haml-syntax
description: Write pdfcn-rs HAML-like templates — tag/class/attribute syntax, interpolation, control flow, and the built-in shadcn-style components and Tailwind-style utilities. Use when writing or editing a .haml template for pdfcn-rs, or explaining its syntax.
---

# pdfcn-rs template syntax

Indentation-based, HAML-inspired. No closing tags — a line's indentation
level is its nesting.

## Core syntax

| Syntax | Meaning |
|---|---|
| `%tag` | An element, e.g. `%h1`, `%p`, `%div`. |
| `.class-name` | A bare `div` with that class. Chain classes: `.flex.gap-2.mt-4`. |
| `%tag.class-name` | Tag plus classes, combinable: `%p.text-sm.text-slate-500`. |
| `%Tag(attr="value" other="{{ x }}")` | Attributes in parentheses, double-quoted. Capitalized tag names are components (see below); lowercase are plain HTML elements. |
| `{{ path.to.value }}` | Interpolates from the data context. Valid in text and inside attribute values. Auto-escaped — never write raw HTML through it. |
| `- if cond` / `- else` | Conditional on a truthy value from data. The branch body is the indented block under it. |
| `- for x in list` | Repeats the indented block once per item, binding `x` (and `x.first`/`x.last`/`x.index` inside a loop, per pdfcn-template's loop metadata). |
| `- include "name"` | Inlines a caller-supplied partial. Requires a `Partials` resolver on the embedding side (`pdfcn build`/`pdfcn dev` resolve from `./templates/components/`; the HTTP API uses `NoPartials` and rejects them). |

```haml
%DocumentLayout(size="a4")
  %Header(title="Invoice {{ invoice.number }}" subtitle="{{ invoice.date }}")
    - if invoice.paid
      %Badge(variant="success" label="Paid")
    - else
      %Badge(variant="destructive" label="Unpaid")

  %Card(title="Bill To")
    %p {{ customer.name }}

  %InvoiceTable(rows={{ invoice.items }} columns={{ invoice.columns }})

  - for tag in product.tags
    %Badge(variant="outline" label="{{ tag }}")
```

## Built-in components (always available)

`DocumentLayout` `Header` `Card` `Table` `Grid` `Badge` `Separator`
`SignatureBlock` `InvoiceTable` `Alert` `Avatar` `Input` `Textarea`
`Select` `Label` `Checkbox` `RadioItem` `Progress` `Breadcrumb`
`Pagination` `BarChart` `QRCode` `PageFooter`

Behind the opt-in `vector` Cargo feature (see the `cargo-features`
skill): `LineChart` `StackedBarChart` `PieChart` `Sparkline` `Barcode`
`Vector`. Built without that feature, these expand to an explicit marker
naming the disabled feature — never a silent no-op.

`%Card(image="cover.jpg")` gives a card a full-bleed cover photo; the
card's wrapper is `relative`+`overflow-hidden`, so a child marked
`.absolute` composes on top of the image (a badge, a price tag) while
staying clipped to the rounded corners.

## Images

`<img src="...">` (plain `%img` or a component's `image` attribute)
resolves against caller-supplied bytes — never a network fetch at render
time. `pdfcn build`/`pdfcn dev` resolve a relative `src` from disk,
relative to the template's own directory. `pdfcn build
--fetch-remote-images` opts into fetching `http(s)://` sources (CLI-only,
never the hosted API — that's an SSRF surface a server-side default
must not have).

## Utility classes

Tailwind-style: `flex`, `grid`, `grid-cols-N`, `gap-N`, `p-N`/`px-N`/…,
`m-N`, `w-full`, `h-N`, `text-sm`/`text-lg`/…, `font-bold`, `rounded`,
`border`, `bg-*`/`text-*` semantic tokens (`bg-primary`,
`text-destructive`, …), `absolute`/`relative`/`fixed` + `top-*`/`right-*`/
`bottom-*`/`left-*`/`inset-*` (including negative offsets like `-top-2`),
`z-*`, `space-x-*`/`space-y-*` (child-margin injection; `space-y` works
on a plain block container, `space-x` requires a flex row — matches real
CSS), `break-inside-avoid`, `break-before-page`. Only classes actually
used in a template are emitted into the stylesheet.

**Known renderer limitations** (`azul-layout`, not pdfcn's own bug —
don't "fix" these by changing pdfcn, work around them in the template):
`gap`/`gap-*` has no visual effect on flex/grid containers (use margin on
items instead, e.g. `class="m-2"` on each grid child); `box-shadow` has
no effect; `object-fit` isn't respected by the renderer itself (pdfcn's
asset pipeline pre-crops when both width and height are known in px via
inline `style=""`, not classes); `border-radius` needs a `px` value, not
`rem`; a `%Grid` row that might fragment across a page boundary needs its
own `.break-inside-avoid` wrapper per row, grouped ahead of time in the
data rather than looping one `%Grid` around every item.

## Reference examples in this repo

`examples/invoice.haml` (base pipeline), `examples/catalog.haml` (`%Card`
+ absolute overlays + row grouping), `examples/showcase.haml` (full
component sweep), `examples/charts.haml` (Charts v2 + `%Barcode` +
`%Vector`, needs the `vector` feature).
