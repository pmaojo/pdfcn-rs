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
(NFR-3). `pdfcn build`/`pdfcn dev` resolve a relative `src` from disk
automatically, relative to the template's own directory:

```haml
%img(src="cover.jpg" style="width:100%;height:220px;object-fit:cover")
```

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
%Card(image="sneaker.png")
  .absolute.top-2.right-2.z-10
    %Badge(variant="destructive" label="-20%")
  %p $89.00
```

That relies on the same positioning utilities every other element can use
to sit anywhere on the page, not just inside a card — `absolute` / `fixed`
/ `relative`, `top-*` / `right-*` / `bottom-*` / `left-*` / `inset-*`
(including negative offsets like `-top-2`), and `z-*` for stacking order —
plus `object-cover` / `object-contain` / `object-{top,bottom,center,left,right}`
for how an image fills its box. `examples/catalog.haml` is a worked example:
a product grid of `%Card`s with real cover photos and an overlaid discount
badge.

## Design notes

- **No Chromium.** `pdfcn-core::render_pdf` calls `printpdf::PdfDocument::from_html`,
  which lays out HTML/CSS with `azul-layout`'s pure-Rust engine — no browser
  process, no dynamic system dependency (NFR-3).
- **Escaping.** `pdfcn-template` resolves data but never escapes; escaping
  happens once, at the edge, via `maud`'s auto-escaping when the resolved
  tree is rendered to HTML (NFR-4).
- **Pagination.** `pdfcn-styles` injects `@page`, `print-color-adjust:
  exact`, and `break-inside: avoid` on table rows/cards by default (NFR-5).
