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

### Preparing photos: `pdfcn images`

Straight from a phone, a photo is 3-5 MB, carries EXIF (often **GPS
coordinates**) and is rotated by an EXIF flag instead of in its pixels.
pdfcn already handles the rendering side: it applies EXIF orientation,
center-crops `object-fit:cover` boxes, downsamples to ~3x the box size
(Lanczos3, 4096 px cap) and re-encodes, which drops EXIF/GPS/ICC from the
PDF. The embedded-image budget defaults to 64 MB of *decoded* pixels (set
`--image-max-size` to change it).

To keep the repository small and builds fast, prepare originals once:

```sh
pdfcn images photos/*.jpg --out examples/assets/ --max-edge 1800 --quality 82
# sea.jpg: 3060x4080 -> 1350x1800, 4.0 MB -> 527 KB  [removed EXIF, GPS location]
```

Rotation is baked in, metadata removed, long edge capped. It never
overwrites its inputs. `examples/brochure.haml` uses three prepared photos.

**`object-fit:cover` needs a pixel box** (`width:638px;height:185px`).
With a percentage width the engine stretches the picture instead of
cropping it; the A4 content width at the default margin is 638 px.

## Hyperlinks

`%a(href="https://…")`, `mailto:`/`tel:` and in-document jumps (`%a(href="#sec")` to `%h2#sec`)
become real PDF link annotations; see `examples/links.haml`. Other schemes stay
plain text. Links are located with a marker-colored probe render (`pdfcn-core/src/links.rs`),
so documents with links render twice.

## Charts, barcodes and raw SVG (`vector` Cargo feature, on by default)

`%Barcode(scheme="…")` supports `code128`, `ean13`, `ean8`, `upca`, `code39` and
`itf` (EAN/UPC check digits are validated or computed); `%QRCode` covers QR.

`%LineChart`, `%StackedBarChart`, `%PieChart`, `%Sparkline`, `%Barcode` and a
generic `%Vector` escape hatch all render through one shared substrate: the
component expands to an `<img src="pdfcn-chart:...">` / `pdfcn-barcode:...` /
`pdfcn-vector:{id}` placeholder, and an asset-preparation pass rasterizes it
to a print-density PNG (`resvg`, ~300dpi) before layout. It's a real Cargo
feature (`vector`) rather than always-on code, but ships on by default —
it's not a heavy addition (~1.8MB on the serverless binary). A deployment
that wants the smallest possible build opts back out explicitly:

```sh
cargo build -p pdfcn-cli --no-default-features   # drops vector + factur-x
```

```sh
cargo run -p pdfcn-cli -- build examples/charts.haml \
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
the `vector` feature (e.g. built with `--no-default-features`), every one
of these components expands to an explicit marker naming the disabled
feature instead of a silent no-op.

`POST /api/generate-pdf` takes the same `%Vector` SVG side channel as a
top-level `svgs` request field (id → SVG text); `%LineChart`/`%Barcode`/…
need no special field at all, just the component in `template`. `index.html`'s
live sandbox has a "Charts, barcode & vector logo" example exercising all
of it against the real endpoint.

## Factur-X invoice embedding (`factur-x` Cargo feature, on by default)

`pdfcn build --factur-x-xml invoice.xml` splices an EN 16931/CII invoice
XML into the rendered PDF as a Factur-X-compliant attachment -- the same
file is then both the human-readable invoice and the machine-readable
e-invoice a client's accounting system parses. Ships on by default (its
`lopdf` dependency is only for direct object surgery on printpdf's own
output, which has no embedded-file support of its own; see
`--no-default-features` above to opt out):

```sh
cargo run -p pdfcn-cli -- build examples/invoice.haml \
  -d examples/invoice.json -o out.pdf \
  --factur-x-xml examples/invoice-en16931.xml --factur-x-profile en16931
```

`--factur-x-profile` accepts `minimum`, `basic-wl`, `basic`, `en16931`
(default) or `extended` -- these only change the profile's XMP
declaration, since every one of them maps to the same PDF/A-3B container.
`pdfcn_core::embed_factur_x_invoice` is the same primitive for napi
bindings, taking already-rendered PDF bytes plus the XML.
`POST /api/generate-pdf` exposes it as a top-level `factur_x` request
field: `{ "xml": "...", "profile"?: "en16931", "icc_base64"?: "..." }`.

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

## Digital signatures (`sign` Cargo feature, opt-in)

`pdfcn build --sign-p12 cert.p12 ...` signs the finished PDF with an X.509
certificate: a PAdES baseline-B signature (`ETSI.CAdES.detached`, SHA-256,
with `signing-certificate-v2`). RSA and ECDSA P-256 keys work, from a PKCS#12
file or from PEM:

```sh
cargo install --path pdfcn-cli --features sign

export PDFCN_SIGN_PASSWORD=...        # never a command-line argument
pdfcn build invoice.haml --data invoice.json \
  --factur-x-xml invoice.xml \
  --sign-p12 company.p12 --sign-reason "Invoice issued" --sign-location Madrid

# or PEM: the chain (signer first) and the key (PKCS#8, PKCS#1 or SEC1)
pdfcn build ... --sign-cert chain.pem --sign-key key.pem
```

Signing is the last step, after Factur-X, and is added as an incremental
update: every byte of the rendered PDF is left as it was and is covered by the
signature. Signing the output again adds a second signature. From Rust:
`SigningIdentity::from_pkcs12` / `from_pem`, then `sign_pdf`.

### Trusted timestamp (PAdES-T)

`--timestamp-url https://tsa.example/tsr` asks an RFC 3161 time-stamping
authority to stamp the signature, which keeps it checkable after the
certificate expires (the stamp proves it was made while the certificate was
valid). Signing fails if the authority is unreachable, refuses, or answers
for a different signature or nonce: a missing timestamp is never skipped
silently. Many authorities are free; qualified ones (the ones with legal weight
in the EU) are usually paid and may need credentials, which this does not send.
From Rust: `sign_pdf_timestamped(pdf, &identity, &options, &|request| ...)`;
pdfcn-core does no network I/O, so the transport is yours.

Verified with poppler's `pdfsig` and pyHanko (signature and timestamp valid,
whole file covered), against an OpenSSL time-stamping server.
What it does **not** do: embed revocation data (PAdES-B-LT, for archives that
must outlive the certificate chain), or sign with a key that lives in a smart
card or HSM. A signature is only as trustworthy as the certificate behind it;
for legal weight use a qualified certificate.

## Icons: `%Icon`

```haml
%Icon(name="receipt" size="20px" color="#0f766e" stroke="1.5")
```

193 [Tabler Icons](https://tabler.io/icons) (MIT) are built in, drawn as
vectors and rasterised at print density: `pdfcn icons [filter]` lists them.
`size` is any px length, `color` a plain colour value, `stroke` a width from 0.25
to 4 (Tabler draws at 2). An unknown name or a bad value is a visible error in the
document, not a blank space.

Tabler is the pick because its outline set is the largest permissively licensed
one, on one 24x24 grid with one stroke weight, and because stroke and colour are
plain SVG attributes. The built-in subset keeps the binary small (about 37 KB);
the rest of Tabler's ~5,000 icons, or any other set (Lucide, Heroicons, your own),
is one flag away:

```sh
pdfcn build doc.haml -d data.json --icons-dir node_modules/@tabler/icons/icons/outline
```

Each `.svg` in the folder becomes an icon named after the file, replacing a
built-in one of the same name. `currentColor` in the SVG takes the icon's colour
and a `stroke-width` on its root takes `stroke`. Files that use scripts, external
references or `<image>` are skipped. From Rust: `register_icon(name, svg)`.
`scripts/update-icons.py` regenerates the built-in subset from `scripts/icons.txt`.

The bundled drawings are © Paweł Kuna, MIT: see
`pdfcn-components/src/icons/LICENSE-tabler-icons`.

## Invoice QR codes for any country: `%FiscalQR`

Tax authorities each ask for a different QR on invoices. Instead of hard-coding
them, `%FiscalQR` is driven by a **profile**: a JSON file that declares the
fields, how to validate them, and the text the QR encodes. A developer follows
their country's specification by writing one file and passing
`--fiscal-profile` (or `register_fiscal_profile` from Rust); no pdfcn change.
See [docs/fiscal-profiles.md](docs/fiscal-profiles.md) and
`examples/fiscal-profiles/xx-demo.json`. Spain's VERI*FACTU is the one profile
that ships built in, as data.

## Spanish invoices: VERI*FACTU QR

`%VerifactuQR` is `%FiscalQR(profile="es-verifactu" ...)` under its own name. It
prints the QR the tax agency requires on invoices under Real Decreto 1007/2023. It encodes the AEAT verification URL built from the invoice's
issuer NIF, series and number, date and total, so it cannot disagree with what
the page says:

```haml
%VerifactuQR(nif="{{ issuer.nif }}" numserie="{{ invoice.series }}" fecha="{{ invoice.date }}" importe="{{ invoice.total }}")
```

`fecha` takes `dd-mm-yyyy` or `yyyy-mm-dd`; `importe` is a plain number (no
currency symbol or thousands separator). Bad values render a visible error
chip, never a QR that would send the customer to a "no such invoice" page.
`env="test"` points at the AEAT pre-production service,
`mode="no-verifactu"` at the non-VERI*FACTU endpoint (and drops the label).
The symbol is 150 px (about 40 mm) with error correction M and the
`VERI*FACTU` label above it; `size` overrides it. See `examples/verifactu`.

**`importe` is the invoice's "Importe total" (taxable base plus VAT and any
equivalence surcharge), not the amount to pay.** Withholdings (IRPF), suplidos
and similar adjustments change what the customer pays but are not part of the
invoice record, and the AEAT's cotejo service compares the QR against that
record. The AEAT's developer FAQ (section 20) suggests printing both figures
labelled, e.g. "Total factura" and "Total a pagar"; pass the first one here.

### What this is and is not, legally

The AEAT's developer FAQ (4 December 2025, section 5) treats any component that
takes part in *printing invoices* or *generating the QR* as a billing component
("componente de facturación") of the invoicing system (SIF). Used for
VERI*FACTU invoices, pdfcn is such a component, which means:

- the main billing system that calls it must be certified with a *declaración
  responsable* that names pdfcn and the exact version it invokes, and
- it must call pdfcn on every invoice, as part of issuing it, so that no invoice
  is printed without its record and no record exists without its invoice.

pdfcn does **not** create the invoice record, chain its hash, keep the records or
send them to the AEAT, and it is not meant to: those need persistent state, an
event log and a qualified certificate, which belong to the billing system. This
README is not legal advice; check the requirements for your case with your
adviser or the AEAT.

The URL format follows the specification as described by several independent
implementations and the AEAT's developer FAQ; the AEAT's own QR specification
document has not been checked against it, so compare before relying on it.

## Examples

`examples/` holds runnable templates; `pdfcn build examples/<name>.haml -d examples/<name>.json -o out.pdf`.
The live demo (`index.html`) loads the same ones in its editor.

| Example | Shows |
|---|---|
| `dashboard` | gradient hero, KPI cards with shadows, bar chart, progress, `divide-y` list, ring callout |
| `pricing` | three plan cards, featured card with `ring-4` and gradient header, dashed CTA |
| `certificate` | serif type, double border, gradient background, circular seal, rotated stamp |
| `ticket` | gradient boarding-pass header, QR code, dashed tear-off line |
| `icons` | `%Icon`: sizes, colours, stroke widths, cards and a gallery of built-in icons |
| `utilities` | `space-x/y`, `list-none`, columns, float, arbitrary values |
| `verifactu` | Spanish invoice with the AEAT VERI*FACTU QR |
| `invoice`, `catalog`, `showcase`, `charts` | tables and signature, `object-fit` images, a long mixed document, vector charts and barcodes |

## Known limitations

- **`gap` and `space-x-*` / `space-y-*` work** (flex rows, flex columns, grids;
  `space-y-*` also on a plain block container). They are applied as margins on
  the children, so a child that already sets its own margin keeps it.
- **Arbitrary values** (`w-[220px]`, `bg-[#0f766e]`, `bg-[rgb(15,118,110)]`,
  `w-[calc(100%-2rem)]`, `grid-cols-[1fr_2fr]`, `font-[600]`, `tracking-[0.05em]`)
  go in a `class="..."` attribute, **not** in the `.dotted` shorthand, which stops
  at the `[`: write `%div(class="w-[220px] p-2")`, not `.w-[220px].p-2`.
- **`list-none`** removes the markers and the indent; `list-disc`,
  `list-decimal`, `list-inside` and `list-outside` work on `ul` and `ol`.
- **Utilities that resolve to nothing are listed, not silently dropped:** the
  build prints them, and `--strict` makes them an error. Classes that only matter
  on screen (`cursor-*`, `select-*`, `pointer-events-*`, `transition-*`,
  `animate-*`...) are accepted without a warning. Common Tailwind classes that are
  still not rendered, because the layout engine ignores the CSS they would need:
  `order-*`, `overline`, `underline-offset-*`, `decoration-*`, `tabular-nums`,
  `line-clamp-*`, `blur`/`backdrop-blur`, `mix-blend-*`, `shadow-inner` and
  `sr-only`. Variants that cannot apply to a PDF (`hover:`, `md:`, `dark:`) are
  dropped by design.
- **A percentage width inside a grid cell** is computed against a larger box than
  the cell (`w-full` and `w-[calc(100%-2rem)]` can overflow it); give grid items
  an explicit width, or avoid `%` there.
- **`.absolute` + `inline-flex` (i.e. `%Badge`).** See the two rules above.
- **A `%Grid` row that fragments across a page boundary renders with a
  hugely inflated height** instead of the row cleanly moving to the next
  page. See the row-grouping note above for the workaround.
- **`box-shadow` is approximated.** The layout engine emits it but upstream
  printpdf dropped it; the `pmaojo/printpdf` fork (see `docs/spikes/003-style-coverage.md`)
  paints outset shadows as stacked translucent rects (blur is a stepped
  approximation, inset shadows are not drawn, and the shadow also shows
  through a transparent-background box). Only a single shadow layer is
  honored, so `shadow-md`/`lg`/`xl` fold Tailwind's two layers into one.
- **Text decoration stops about one glyph short** of the end of the text
  (engine width quirk on the underline/line-through bounds).
- **`object-fit` / `object-position` work through the patched engine**
  (`contain`, `cover`, `none`, `scale-down`, with a clip to the box), also
  for boxes whose size only layout knows (a class like `w-full`). A box with
  both dimensions in px still gets `pdfcn-core`'s asset pass first, which
  center-crops the source to shrink the embedded image; a non-center
  `object-position` skips that pass so the engine anchors the crop itself.
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
components (`haml-syntax`) and the `vector`/`factur-x` Cargo features,
on by default (`cargo-features`).

## Design notes

- **No Chromium.** `pdfcn-core::render_pdf` calls `printpdf::PdfDocument::from_html`,
  which lays out HTML/CSS with `azul-layout`'s pure-Rust engine — no browser
  process, no dynamic system dependency (NFR-3).
- **Escaping.** `pdfcn-template` resolves data but never escapes; escaping
  happens once, at the edge, via `maud`'s auto-escaping when the resolved
  tree is rendered to HTML (NFR-4).
- **Pagination.** `pdfcn-styles` injects `@page`, `print-color-adjust:
  exact`, and `break-inside: avoid` on table rows/cards by default (NFR-5).
