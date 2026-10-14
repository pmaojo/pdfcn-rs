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
  patching printpdf's bridge (first as a vendored copy, now the
  `pmaojo/printpdf` fork, branch `fix/box-shadow`) -- the bridge, not
  azul-layout, was enough. The engine
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

## Round 3 (composites, grid, and what still needs an engine patch)

Probed the same way (ops with/without):

| Utility | Engine honors it? | Done how |
|---|---|---|
| `divide-x/y` (`> * + *`) | no combinator selector; per-child borders work | `composite.rs`: border on each non-first child |
| linear/radial gradient | yes (`background: linear-gradient(...)`) | `composite.rs`: `bg-gradient-to-*` + `from/via/to-*` -> one inline `background` |
| `outline` | **no** | `composite.rs`: zero-blur spread `box-shadow`, same as `ring-*` |
| `ring-*` | via `box-shadow` | `composite.rs` (a ring/outline replaces a `shadow-*` on the same element) |
| `grid-template-rows`, `grid-row: span N` | yes | `grid-rows-N`, `row-span-N` in `utilities.rs` |
| `grid-column-start/end` | **no** | not implemented |
| `transform` (rotate/scale/translate) | **no** (`PushReferenceFrame` is dropped by the bridge) | not implemented |
| `opacity` | **no** | see below |
| `text-shadow` | **no** (`PushTextShadow` dropped by the bridge) | not implemented |
| `vertical-align` (also `<sub>`) | **no** | not implemented |
| `object-fit` | **no** (no display-list item for it) | not implemented |
| `letter-spacing`, `white-space: pre` | yes | already resolved |

**`opacity` root cause:** the layout engine does emit
`PushOpacity`/`PopOpacity`, and the printpdf bridge handles them -- but
`azul-layout`'s page splitter (`clip_and_offset_display_item`,
`solver3/display_list.rs`) returns `None` for them ("filter effects - skip for
now"), so paged output never sees them. Fixing that needs a patch in
`azul-layout` itself (keep push/pop, rebalance them per page), not in the
printpdf bridge. `azul-layout` is 14 MB of source, so vendoring it is a
deliberate decision rather than a drive-by; the same patch point would also be
where `transform` and `vertical-align` get fixed.

## Round 4: forks, and `opacity` fixed

printpdf and azul now come from forks (`pmaojo/printpdf`, `pmaojo/azul`,
integration branch `pdfcn` in each), each the sum of small `fix/*` branches
meant for upstream PRs:

| Repo | Branch | Fix |
|---|---|---|
| printpdf | `fix/box-shadow` | paint outset `BoxShadow` in the HTML bridge |
| printpdf | `fix/deterministic-resource-ids` | ExtGState/shading names from content, not random (two renders of one document differed byte-for-byte; surfaced once shadows made the invoice use translucent fills) |
| azul | `fix/paged-opacity` | `PushOpacity`/`PopOpacity` are structure markers in the paginator, re-emitted and balanced per page |
| azul | `fix/feature-gating` | azul-layout `master` did not build with `default-features = false` (`agg_rust` / `tile_mask` used without the `cpurender` gate); printpdf builds it that way |

`master` of azul (0.0.16) already re-emits clips, scroll frames, stacking
contexts, text shadows and image-mask clips across pages (a marker stack that
0.0.14 lacked). Opacity and reference frames (`transform`) were simply not on
that list. Opacity now is; `transform` (`PushReferenceFrame`) is still not, and
the printpdf bridge also ignores it, so it needs both.

Moving from printpdf 0.12.6/azul 0.0.14 to 0.12.8/0.0.16 changed the golden
snapshots (one line each: engine-version noise); the pages were checked
visually.

## Round 5: `object-fit` fixed in the engine

Re-probed on azul `master` (0.0.16) before assuming round-3's results held:

| Utility | Result on 0.0.16 |
|---|---|
| `opacity` | fixed (round 4) |
| `object-fit`, `object-position` | **were ignored** -> `fix/image-object-fit` (azul) + `fix/img-intrinsic-size` (printpdf) |
| `vertical-align` (sub/super/length/%) | still no effect in printpdf's paged output even though text3 implements it and azul's own tests cover it; not yet root-caused (`<sup>`/`<sub>` also unchanged) |
| `transform`, `text-shadow` | still ignored (`PushReferenceFrame`/`PushTextShadow` are no-ops in the bridge) |
| `outline`, `grid-column-start` | still ignored |

`object-fit` had two causes. azul read the property but never used it
(`DisplayListItem::Image` always stretched; the inline path hardcodes
`ObjectFit::Fill`), and printpdf hands layout `<img>` as a tagged 0x0
placeholder, so there was no aspect ratio to fit. The first attempt, giving
`<img>` `width`/`height` attributes from the decoded size, broke percentage
widths: azul feeds the intrinsic width into the containing block, so catalog
cards inflated to 480 px. Fix instead: printpdf registers the decoded sizes in
the `ImageCache` it passes to layout, and azul looks a placeholder's tag up
there at paint time. Box sizes are unchanged. `fix/image-object-fit` clips the
fitted image to its box with a `PushClip`/`PopClip` pair, which pagination
already keeps balanced.

Lesson recorded for the next probe: `cargo` treats git dependencies as
immutable, so editing `~/.cargo/git/checkouts/...` does not rebuild;
`cargo clean -p <crate>` first.

### `vertical-align` root-cause lead (not fixed)

Narrowed with a throwaway probe against the patched engine:

- The property reaches the span's style: `get_vertical_align_for_node` returns
  `Exact(Superscript)` for the `<span>`.
- text3 applies it per run: a two-run `perform_fragment_layout` with the
  second run `VerticalAlign::Super` moves its clusters 6.4 px up.
- But the glyph positions printpdf draws are identical with and without it.
  The remaining suspect is the paged path: `paged_layout.rs` /
  `text3/dense.rs` store a line as one shared `baseline_y` plus per-run
  ascent (`DenseText::resolved_run_ascent`), and `dense.rs` has no
  `vertical_align`/baseline-shift field, so a per-run shift would be lost when
  the layout is densified. Next step: confirm whether the paged display list
  reads glyph positions from the `DenseText`, then add a per-run `dy` to
  `DenseRun` (set in `from_unified`, applied in `to_unified_items` and
  `get_glyph_runs_pdf_dense`). Larger than the other fixes, so left open.
