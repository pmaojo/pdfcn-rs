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
| `underline`, `line-through` on an **inline** element (`<span>`, `<u>`, `<a>`) | yes | yes |
| `underline`, `line-through` on a **block** (`<p class="underline">`) | yes | **no** -- engine draws it only for inline elements; fixed by `pdfcn_styles::rewrite_text_decorations` |
| `border-collapse` utility | yes | **no** (the document-level `table{border-collapse}` rule is what takes effect) |
| `align-middle` etc. (`vertical-align`) | yes | **no** |

`hover:`/`focus:`/`md:` are deliberately not resolved: a PDF has no pointer
and one fixed page size.

## Consequences

- `underline`/`line-through`: the first probe wrongly reported "no effect"
  because it put the class on a `<p>`. Re-probed with `<u>`, `<a>` and
  `<span>`: the engine does draw them (it emits `Underline`/`Strikethrough`
  display-list items); it just doesn't propagate the decoration from a
  block. A HTML rewrite pass (like `gap.rs`) now wraps the text in a
  decorated `<span>`.
- `box-shadow`: layout emits `DisplayListItem::BoxShadow`, but printpdf's
  `html/bridge.rs` drops it (it falls into the `_ => {}` arm). Fixed by
  vendoring printpdf (`vendor/printpdf`, `[patch.crates-io]`) and painting the
  item there -- patching the bridge, not azul-layout, was enough. The engine
  also only honors ONE shadow layer: Tailwind's two-layer `shadow-md/lg/xl`
  parsed to nothing, so `tokens::SHADOW` folds them into one layer.
- **Where the leverage is:** `printpdf::html::xml_to_pdf_pages` is
  layout -> `DisplayList` -> `bridge` -> PDF ops. The display list already
  carries `Underline`, `Strikethrough`, `BoxShadow`, gradients, borders and
  opacity; the gap is mostly in the small bridge, which is a few hundred
  lines we can patch, not in the layout engine. A new renderer is not needed
  for these.
- Still open: `vertical-align` (no effect even inline) and `object-fit`
  (the layout engine has no display-list item for it).
- `border-collapse` is already forced by the document stylesheet
  (`table{border-collapse:collapse}`), so the utility only matters to
  switch it off.
- `align-*` (`vertical-align`) has no effect even on inline elements.
- `print:` is unconditional (output is always print); `screen:` never applies.
- Not yet probed and therefore not implemented: `divide-*` (needs a child
  combinator selector), `ring-*`/`outline-*`, `grid-rows-*`/`row-span-*`,
  gradients, `transform`. Probe them the same way before adding.
