---
name: cargo-features
description: Build and use pdfcn-rs's vector (Charts v2, %Barcode, %Vector) and factur-x (EN 16931 invoice embedding) Cargo features, both on by default — CLI flags, function signatures, how to opt out with --no-default-features, and why they're feature-gated at all. Use when building the CLI, adding a chart/barcode to a template, or embedding a Factur-X invoice.
---

# pdfcn-rs Cargo features: vector and factur-x

pdfcn-rs ships as the serverless `pdfcn-vercel` function, and CI enforces
a binary-size tripwire on it (currently ~18.4MB against a 60MB
tripwire — plenty of headroom). Two capabilities live behind named Cargo
features rather than always-on code, but both ship **on by default**
since neither is heavy (~1.8MB combined): a deployment that wants the
smallest possible binary opts back out explicitly:

```sh
cargo build -p pdfcn-cli --no-default-features   # drops both vector and factur-x
cargo test -p pdfcn-cli --no-default-features    # test this combination too
```

Use `-p pdfcn-cli`, not `--workspace`, for the opt-out: the deployed
`pdfcn-vercel` function and the `pdfcn-node` napi bindings each request
`vector`/`factur-x` unconditionally on their own dependency edge (no
opt-out for those two, by design), so a `--workspace --no-default-features`
build still ends up compiling `pdfcn-core` with both features anyway --
Cargo unifies a shared dependency's features across every workspace
member being built together.

The plain default build already has both:

```sh
cargo build -p pdfcn-cli
```

Adding a new heavy capability to this project should follow the same
gating pattern (a new named feature in `pdfcn-core/Cargo.toml`, forwarded
through `pdfcn-cli/Cargo.toml` and `pdfcn-components/Cargo.toml` as
needed) so it can be compiled out with `--no-default-features` -- but
whether the new feature defaults on or off is a real judgment call
against the size gate, not an automatic "off" the way it would be for a
heavier addition. Verify either way with `cargo tree --edges features -p
pdfcn-vercel` so the default build's actual dependency graph is never a
surprise.

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
