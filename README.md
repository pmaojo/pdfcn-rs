# pdfcn-rs

HAML-like templates + Shadcn-style UI components + Tailwind-style utility
classes, compiled to PDF bytes in memory. Pure Rust, no headless browser, no
process spawning — built to run inside a Vercel Function / AWS Lambda.

```
%DocumentLayout(size="a4")
  %Header(title="Invoice {{ invoice.number }}")
  %Card(title="Bill To")
    %p {{ customer.name }}
  %InvoiceTable(rows={{ invoice.items }} columns={{ invoice.columns }})
  - if invoice.paid
    %Badge(variant="success" label="Paid")
```

## Workspace layout

| Crate | Responsibility |
|---|---|
| `pdfcn-parser` | Indentation-based HAML-like lexer/parser (`winnow`) → AST |
| `pdfcn-template` | `{{ interpolation }}`, `- for`, `- if`, `- include` (`minijinja` as an expression engine) |
| `pdfcn-components` | `%InvoiceTable`, `%Badge`, `%Card`, ... registry, expanding to `maud` markup |
| `pdfcn-styles` | Zero-Node Tailwind-style utility scanner + print-safe CSS (`lightningcss`) |
| `pdfcn-core` | Orchestrates the pipeline; HTML → PDF via `printpdf`'s pure-Rust HTML/CSS layout engine |
| `pdfcn-cli` | `pdfcn new / add / build / dev` |
| `api/generate-pdf.rs` | `vercel_runtime` handler for `/api/generate-pdf`, auto-built by Vercel's built-in Rust runtime |
| `pdfcn-node` | `napi-rs` bindings for calling the core directly from Next.js |

## CLI

```sh
pdfcn new invoice                       # scaffold invoice/invoice.haml + data.json
pdfcn add InvoiceTable                  # copy a component snippet into ./templates/components/
pdfcn build invoice.haml -d data.json -o out.pdf
pdfcn dev invoice.haml -d data.json     # live-reload browser preview
```

Try the bundled examples:

```sh
cargo run -p pdfcn-cli -- build examples/invoice.haml -d examples/invoice.json -o /tmp/invoice.pdf
cargo run -p pdfcn-cli -- build examples/catalog.haml -d examples/catalog.json -o /tmp/catalog.pdf
```

## Composing with images

An `<img src="...">` — from a plain `%img` or from a component's `image`
attribute — resolves against caller-supplied bytes, never a network fetch
(NFR-3: `render_pdf`/`render_with_assets`, the API `/api/generate-pdf`
uses, make zero outbound requests). `pdfcn build`/`pdfcn dev` resolve a
relative `src` from disk automatically, relative to the template's own
directory:

```haml
%img(src="cover.jpg" style="width:100%;height:220px;object-fit:cover")
```

**A client's own image URLs** (a CMS, a stock-photo host) work too, without
hand-downloading and base64-encoding them yourself — opt in with
`--fetch-remote-images` on `pdfcn build`:

```sh
pdfcn build catalog.haml -d catalog.json -o out.pdf --fetch-remote-images
```

This is deliberately **opt-in and CLI-only**, never the default and never
available from `render_pdf`/`/api/generate-pdf`: fetching a client-supplied
URL server-side is an SSRF surface, so it belongs to a flag a developer
turns on for their own trusted machine, not to a hosted endpoint that takes
arbitrary input. A fetch that fails (network error, non-2xx, or over the
20MB cap) degrades to a broken-image placeholder rather than failing the
build, the same as a local file that isn't found.

**`%Card(image="...")`** gives a component a full-bleed cover photo above
its body — the shadcn "product card" composition:

```haml
%Card(title="Trail Runner" image="sneaker.png" image-alt="Trail Runner shoe")
  %p $89.00
```

The card's wrapper carries `relative` + `overflow-hidden`, so a child
marked `.absolute` composes *on top of* the image (a discount ribbon, a
price tag) while staying clipped to the card's rounded corners:

```haml
%Card(image="sneaker.png" class="m-2")
  .absolute.top-2.right-2.z-10.rounded-full.bg-destructive.text-white.text-xs.font-semibold(style="padding:2px 10px") -20%
  %p $89.00
```

That relies on the same positioning utilities every other element can use
to sit anywhere on the page, not just inside a card — `absolute` / `fixed`
/ `relative`, `top-*` / `right-*` / `bottom-*` / `left-*` / `inset-*`
(including negative offsets like `-top-2`), and `z-*` for stacking order.
`examples/catalog.haml` is a worked example: a product grid of `%Card`s
with real cover photos and an overlaid discount badge.

**Two rules for `.absolute` to actually land where you put it,** both
learned the hard way from `printpdf`/`azul-layout`'s early-stage layout
solver, not obvious from the CSS alone:

- Give the overlay its own styling directly (`div.absolute.rounded-full...`
  holding its text straight away). Don't wrap it around another component
  and don't use `%Badge` for it (directly or nested) — `%Badge` is
  `display:inline-flex`, and an `inline-flex` element anywhere inside the
  same card as an absolutely positioned element makes the renderer keep
  that element's normal-flow position instead of moving it, landing it on
  top of the title text instead of the image. A `display:flex` container
  (like the price/stock row) is fine as long as nothing *inside* it is
  `inline-flex`.
- Put spacing (`class="m-2"` for a grid of cards) on `%Card`'s own `class`
  attribute, not on a wrapper `div` around `%Card` — an extra ancestor
  level between the grid and the card breaks the same positioning math.

- A grid row that might not fit on the current page (e.g. many products at
  `%Grid(cols="2")`) needs its own `.break-inside-avoid` wrapper per row,
  rather than one `%Grid` around every item — a row that has to fragment
  across a page boundary gets laid out with a wildly inflated height
  instead of moving cleanly to the next page. Group the data into rows
  ahead of time (`examples/catalog.json`'s `rows: [[...], [...]]`) and loop
  `- for row in rows` around a `.grid.grid-cols-2.break-inside-avoid` per
  row, as `examples/catalog.haml` does.

Both are demonstrated in `examples/catalog.haml`.

## Charts, barcodes and raw SVG (opt-in `vector` feature)

`%LineChart`, `%StackedBarChart`, `%PieChart`, `%Sparkline`, `%Barcode` and a
generic `%Vector` escape hatch all render through one shared substrate: the
component expands to an `<img src="pdfcn-chart:...">` / `pdfcn-barcode:...` /
`pdfcn-vector:{id}` placeholder, and an asset-preparation pass rasterizes it
to a print-density PNG (`resvg`, ~300dpi) before layout. None of this ships
in the default build — it's entirely behind the `vector` Cargo feature, kept
off by default so the serverless binary (`pdfcn-vercel`/`api/generate-pdf.rs`)
never links `resvg` unless a deployment opts in:

```sh
cargo run -p pdfcn-cli --features vector -- build examples/charts.haml \
  -d examples/charts.json -o /tmp/charts.pdf --svg logo=examples/logo.svg
```

```haml
%LineChart(values={{ monthly_revenue }} xlabels={{ months }} w="480px" h="200px")
%PieChart(values={{ channel_mix }} labels={{ channels }} donut="true" w="220px" h="180px")
%Sparkline(values={{ signup_trend }} w="220px" h="60px")
%Barcode(scheme="ean13" value="{{ shipment_id }}" w="240px" h="60px")
%Vector(id="logo" w="90px" h="45px" alt="Company logo")
```

`%Vector` renders arbitrary caller-supplied SVG (a logo, a diagram) rather
than a chart spec — its source travels through `RenderOptions.svg_assets`
(id → SVG text) rather than inline markup, since SVG can be arbitrarily
large. `examples/charts.haml` (data: `examples/charts.json`, SVG:
`examples/logo.svg`) is a worked example combining all five. Built without
the `vector` feature, every one of these components expands to an explicit
marker naming the disabled feature instead of a silent no-op.

## Factur-X invoice embedding (opt-in `factur-x` feature)

`pdfcn build --factur-x-xml invoice.xml` splices an EN 16931/CII invoice
XML into the rendered PDF as a Factur-X-compliant attachment -- the same
file is then both the human-readable invoice and the machine-readable
e-invoice a client's accounting system parses. Behind its own opt-in
`factur-x` Cargo feature (never in the default serverless build: it pulls
in `lopdf`, only for direct object surgery on printpdf's own output,
which has no embedded-file support of its own):

```sh
cargo run -p pdfcn-cli --features factur-x -- build invoice.haml \
  -d invoice.json -o out.pdf --factur-x-xml invoice-en16931.xml \
  --factur-x-profile en16931
```

`--factur-x-profile` accepts `minimum`, `basic-wl`, `basic`, `en16931`
(default) or `extended` -- these only change the profile's XMP
declaration, since every one of them maps to the same PDF/A-3B container.
`pdfcn_core::embed_factur_x_invoice` is the same primitive for the API/
napi bindings, taking already-rendered PDF bytes plus the XML.

**PDF/A's `/OutputIntent` needs a real, caller-supplied sRGB ICC
profile.** `--factur-x-icc profile.icc` embeds it; without it, the output
is still Factur-X-shaped (embedded XML, correct `/AF`/`/Names`, correct
XMP conformance claims) but not a fully validator-clean PDF/A-3 file.
pdfcn deliberately never fabricates an ICC profile itself -- see
`docs/spikes/002-factur-x-embedding.md` for why a subtly wrong one is
worse than an honestly absent one.

Not yet validated against a real Factur-X/PDF-A validator (veraPDF,
Mustang) in this environment -- see the spike doc for exactly what is and
isn't confirmed.

## Known limitations

- **`gap` (flex and grid) has no effect.** Neither `display:flex` nor
  `display:grid` containers apply their `gap`/`gap-*` utility in the
  current `azul-layout` engine — items render flush against each other.
  Use margin on the items instead (`examples/catalog.haml` gives each
  `%Card` `class="m-2"` rather than `gap-4` on the grid).
- **`.absolute` + `inline-flex` (i.e. `%Badge`).** See the two rules above.
- **A `%Grid` row that fragments across a page boundary renders with a
  hugely inflated height** instead of the row cleanly moving to the next
  page. See the row-grouping note above for the workaround.
- **`box-shadow` has no effect.** `shadow-sm`/`shadow-md`/etc. resolve to a
  real CSS declaration, but the renderer doesn't paint it — cards render
  flat, without a drop shadow.
- **`object-fit` has no effect in the renderer itself.** An image is scaled
  to exactly fill its box on both axes (not cropped to cover, nor
  letterboxed to contain). `pdfcn-core`'s asset-preparation pass works
  around this by center-cropping the source ahead of time when *both* the
  box's width and height are known in px (inline `style=""`, not classes) --
  but a box with only one dimension known in px (e.g. `%Card`'s cover
  image, whose width is the relative `w-full`) still can't be cropped, only
  resolution-capped. Pick a source image close to the box's aspect ratio to
  keep the distortion imperceptible in that case.
- **`border-radius` needs a `px` value, not `rem`.** `pdfcn-styles`'
  `rounded-*` scale already emits `px` for this reason; a hand-written
  `style="border-radius:0.5rem"` renders square corners; `style="border-radius:8px"`
  renders the real rounded corners.
- **`RenderOptions::header_text`/`footer_text`/`show_page_numbers`/
  `skip_first_page` currently render nothing.** These map directly to
  printpdf's own `GeneratePdfOptions` fields, but printpdf 0.12.6's actual
  HTML-to-PDF call path (`layout_document_paged_v2`) never draws them,
  even though the field is threaded all the way to a `FakePageConfig` and
  azul-layout does contain header/footer-drawing code elsewhere, unused by
  that path. Verified with a minimal reproduction outside pdfcn entirely.
  Kept wired rather than removed, since it costs nothing and starts working
  automatically if a future printpdf release fixes it.

## For coding agents

`AGENTS.md` has build/test/lint commands and this repo's conventions. A
Claude Code plugin lives at `.claude-plugin/` + `skills/` (load with
`claude --plugin-dir .`), with skills covering the HAML template syntax/
components (`haml-syntax`) and the opt-in `vector`/`factur-x` Cargo
features (`cargo-features`).

## Design notes

- **No Chromium.** `pdfcn-core::render_pdf` calls `printpdf::PdfDocument::from_html`,
  which lays out HTML/CSS with `azul-layout`'s pure-Rust engine — no browser
  process, no dynamic system dependency (NFR-3).
- **Escaping.** `pdfcn-template` resolves data but never escapes; escaping
  happens once, at the edge, via `maud`'s auto-escaping when the resolved
  tree is rendered to HTML (NFR-4).
- **Pagination.** `pdfcn-styles` injects `@page`, `print-color-adjust:
  exact`, and `break-inside: avoid` on table rows/cards by default (NFR-5).
