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

Try the bundled example:

```sh
cargo run -p pdfcn-cli -- build examples/invoice.haml -d examples/invoice.json -o /tmp/invoice.pdf
```

## Design notes

- **No Chromium.** `pdfcn-core::render_pdf` calls `printpdf::PdfDocument::from_html`,
  which lays out HTML/CSS with `azul-layout`'s pure-Rust engine — no browser
  process, no dynamic system dependency (NFR-3).
- **Escaping.** `pdfcn-template` resolves data but never escapes; escaping
  happens once, at the edge, via `maud`'s auto-escaping when the resolved
  tree is rendered to HTML (NFR-4).
- **Pagination.** `pdfcn-styles` injects `@page`, `print-color-adjust:
  exact`, and `break-inside: avoid` on table rows/cards by default (NFR-5).
