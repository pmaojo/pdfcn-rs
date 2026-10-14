# Local patches to printpdf 0.12.6

This directory is printpdf 0.12.6 (MIT) with `examples/`, `tests/` and
`scripts/` removed, used through `[patch.crates-io]` in the root
`Cargo.toml`. Every change is marked `PDFCN PATCH` in the source so it can be
diffed against the published crate and dropped once upstream carries it.

| File | Change | Why |
|---|---|---|
| `src/html/bridge.rs` | Paint `DisplayListItem::BoxShadow` (outset only): translucent rects, stacked to approximate blur | The layout engine emits box shadows but the bridge ignored them, so `shadow-*` utilities rendered flat. |

A shadow is painted underneath the element box, so an element with a
transparent background shows the shadow through it (CSS would clip it out).
Inset shadows are not drawn.
