# Spike 003 — which utility classes the layout engine actually honors

Written before widening `pdfcn-styles`' utility table, per the repo rule:
check the real upstream (`azul-css`/`azul-layout` 0.0.14 via `printpdf`
0.12.6) instead of assuming a property that parses also paints.

## Method

1. **Parse support** — read `azul-css-0.0.14/src/props/property.rs` (the
   `CssPropertyType` key table) and `parser2.rs`. A property absent here is
   dropped at parse time and can never render.
2. **Paint/layout support** — render a minimal document with and without the
   class through `pdfcn_core::render_pdf`, re-parse the PDF with
   `printpdf::parse_pdf_from_bytes`, and compare the page `ops` (or the
   `render_to_svg` output). Raw PDF bytes are *not* a valid oracle: they
   differ run to run. The page ops are deterministic (verified: same input
   twice → identical ops).

"Renders" below means the drawing operations changed; it does not prove the
result looks right, only that the engine reacted to the property.

## Results

| Utility (CSS) | Parses | Changes output |
|---|---|---|
| `uppercase`, `capitalize`, `text-center`, `text-justify`, `text-6xl` | yes | yes |
| `truncate` (`overflow`, `text-overflow`, `white-space`), `whitespace-nowrap`, `break-words` | yes | yes |
| `font-mono`, `font-serif` | yes | yes |
| `invisible` (`visibility`), `opacity-*` | yes | yes |
| `bg-black/50` (`rgba`), `hsla` tokens | yes | yes |
| `border-dashed`, `border-dotted` | yes | yes |
| `rounded-t-*`, `rounded-bl-*` (per-corner radius, px) | yes | yes |
| `mx-auto`, `-mt-*`, `w-1/2`, `min-w-*`, `size-*` | yes | yes |
| `justify-around`, `justify-evenly`, `flex-row-reverse`, `items-*` | yes | yes |
| `self-*`, `grow`, `flex-none` | yes | yes |
| `first:`, `last:`, `odd:`, `even:` (structural pseudo-classes) | yes | yes |
| `underline`, `line-through` (`text-decoration`) | yes | **no** |
| `border-collapse` utility | yes | **no** (the document-level `table{border-collapse}` rule is what takes effect) |
| `align-middle` etc. (`vertical-align`) | yes | **no** |

`hover:`/`focus:`/`md:` are deliberately not resolved: a PDF has no pointer
and one fixed page size.

## Consequences

- `underline`, `line-through`, `border-collapse` and `align-*` still resolve
  to their standard CSS, so templates stay portable, but have no visible
  effect today (see README → Known limitations). If a visible underline is
  needed, use `border-b` on a block/inline-block element.
- `print:` is unconditional (output is always print); `screen:` never applies.
- Not yet probed and therefore not implemented: `divide-*` (needs a child
  combinator selector), `ring-*`/`outline-*`, `grid-rows-*`/`row-span-*`,
  gradients, `transform`. Probe them the same way before adding.
