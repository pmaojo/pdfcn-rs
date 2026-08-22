---
name: cargo-features
description: Build and use pdfcn-rs's opt-in Cargo features — vector (Charts v2, %Barcode, %Vector) and factur-x (EN 16931 invoice embedding) — including CLI flags, function signatures, and why they're feature-gated. Use when building the CLI with extra features, adding a chart/barcode to a template, or embedding a Factur-X invoice.
---

# pdfcn-rs opt-in Cargo features

pdfcn-rs's default build is deliberately minimal — it's what ships as the
serverless `pdfcn-vercel` function, and CI enforces a binary-size
tripwire on it. Two capabilities are real but live behind Cargo features,
off by default, so they never reach that build unless explicitly enabled:

```sh
cargo build -p pdfcn-cli --features vector,factur-x
cargo test --workspace --features pdfcn-core/vector,pdfcn-components/vector,pdfcn-cli/vector
cargo test --workspace --features pdfcn-core/factur-x,pdfcn-cli/factur-x
```

Adding a new heavy capability to this project should follow the same
pattern: a new named feature in `pdfcn-core/Cargo.toml`, forwarded through
`pdfcn-cli/Cargo.toml` and `pdfcn-components/Cargo.toml` as needed, never
added to `pdfcn-vercel`'s default dependency graph. Verify with `cargo
tree --edges features -p pdfcn-vercel`.

## `vector` — Charts v2, %Barcode, %Vector

SVG generated in-process, rasterized to PNG at ~300dpi via `resvg` just
before layout. See `docs/spikes/001-vector-vs-raster.md` for why
rasterized (not vectorial via `svg2pdf`) was chosen.

```haml
%LineChart(values={{ monthly_revenue }} xlabels={{ months }} w="480px" h="200px")
%StackedBarChart(values={{ series }} xlabels={{ months }})
%PieChart(values={{ channel_mix }} labels={{ channels }} donut="true")
%Sparkline(values={{ signup_trend }} w="220px" h="60px")
%Barcode(scheme="ean13" value="{{ shipment_id }}" w="240px" h="60px")
%Vector(id="logo" w="90px" h="45px" alt="Company logo")
```

`%Barcode` supports `code128` (auto B/C switching) and `ean13` (checksum
enforced — an invalid check digit is an explicit invalid-component
marker, never a silently-wrong barcode). `%Vector` renders arbitrary
caller-supplied SVG (a logo, a diagram) rather than a generated chart —
its source travels through `RenderOptions.svg_assets` (id → SVG text),
not inline markup:

```sh
pdfcn build charts.haml -d charts.json -o out.pdf --svg logo=logo.svg
```

DataMatrix/PDF417 are deliberately NOT implemented: a subtly wrong ECC
symbol scans sometimes, which is worse than an explicit "unsupported
scheme" marker.

## `factur-x` — EN 16931/CII invoice embedding

Splices a caller-supplied invoice XML into an already-rendered PDF as a
Factur-X-compliant attachment: `/Type /EmbeddedFile` named exactly
`factur-x.xml`, referenced from both `/Names/EmbeddedFiles` and the
document `/AF` array, plus XMP declaring `pdfaid:part`/`conformance` and
the Factur-X `fx:` schema. See `docs/spikes/002-factur-x-embedding.md`
for why this is a post-processing pass over printpdf's own output using
`lopdf` directly — printpdf 0.12.6 has no embedded-file support at all.

```sh
pdfcn build invoice.haml -d invoice.json -o out.pdf \
  --factur-x-xml invoice-en16931.xml --factur-x-profile en16931
```

`--factur-x-profile`: `minimum`, `basic-wl`, `basic`, `en16931` (default),
`extended` — all map to the same PDF/A-3B container, only the XMP
conformance-level string differs. `--factur-x-icc profile.icc` embeds a
genuine sRGB ICC profile as the PDF/A `/OutputIntent`; **never fabricate
one** if the caller doesn't supply it — omit the OutputIntent instead
(same principle as the DataMatrix/PDF417 exclusion above: a wrong binary
profile is worse than an honestly absent one).

The equivalent library call is `pdfcn_core::embed_factur_x_invoice(pdf:
&[u8], invoice_xml: &[u8], profile: FacturXProfile, icc_srgb_profile:
Option<&[u8]>) -> Result<Vec<u8>, FacturXError>`, taking already-rendered
PDF bytes.

**Not yet validated** against a real PDF/A validator (veraPDF, Mustang)
in the sandbox this was built in — check before relying on it for actual
regulatory e-invoicing submissions.
