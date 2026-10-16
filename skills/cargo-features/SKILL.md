---
name: cargo-features
description: Build and use pdfcn-rs's Cargo features — vector (Charts v2, %Barcode, %Vector, %Icon, QR as PDF vectors), factur-x (EN 16931 invoice embedding), jpeg-passthrough (photos embedded as-is) and cache (content-hash caches), all on by default — plus the PDFCN_THREADS / PDFCN_CACHE_MB environment variables, CLI flags, how to opt out with --no-default-features, and why they're feature-gated at all. Use when building the CLI, adding a chart/barcode to a template, embedding a Factur-X invoice, or tuning a long-lived process.
---

# pdfcn-rs Cargo features: vector, factur-x, jpeg-passthrough, cache

pdfcn-rs ships as the serverless `pdfcn-vercel` function, and CI enforces
a binary-size tripwire on it (~27MB with `opt-level = 3`, against a 60MB
tripwire). Four capabilities live behind named Cargo features rather than
always-on code, but all ship **on by default** since none is heavy: a
deployment that wants the smallest possible binary opts back out
explicitly:

```sh
cargo build -p pdfcn-cli --no-default-features   # drops all four
cargo test -p pdfcn-cli --no-default-features    # test this combination too
```

Use `-p pdfcn-cli`, not `--workspace`, for the opt-out: the deployed
`pdfcn-vercel` function and the `pdfcn-node` napi bindings each request
every feature unconditionally on their own dependency edge (no opt-out
for those two, by design), so a `--workspace --no-default-features`
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

SVG generated in-process (or supplied, for `%Vector`), converted to PDF
vectors with `svg2pdf` before layout; the layout engine places each one by
size alone and the Form XObject replaces that slot after saving
(`docs/spikes/004-vector-form-xobjects.md`, `006-zero-copy-and-placeholders.md`).
Only an SVG that cannot be converted is rasterized to a ~300dpi PNG with
`resvg`, as a fallback.

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

## `jpeg-passthrough` — photos embedded as they are

A baseline/progressive 8-bit JPEG goes into the PDF as its own file
(`/DCTDecode`), never decoded or re-encoded: laid out by its header's size,
spliced in after saving. CMYK, 12-bit, arithmetic-coded or EXIF-rotated
files, and any image option asking for a re-encode (`--image-quality`,
`--image-max-size`, greyscale, lossless), keep printpdf's own path
(`docs/spikes/005-image-fast-path.md`).

## `cache` — content-hash caches for warm processes

Parsed templates, built stylesheets, converted SVGs and prepared (cropped,
rotated, shrunk) photos are kept per process, keyed by a hash of what went
in; each cache has a byte budget with LRU eviction. A changed input is a
different key, so nothing is ever stale. Helps `pdfcn dev`, servers and
batches; a one-shot `pdfcn build` pays one hash per input
(`docs/spikes/008-parallel-and-caches.md`).

## `hyphenation` — CSS `hyphens: auto` (off by default)

Automatic hyphenation with the dictionaries of ~70 languages embedded
(~3 MB). No utility class or example uses it, so the default build and the
serverless function leave it out; `hyphens: auto` in author CSS is then laid
out unhyphenated, never an error. Turn it on with `cargo build -p pdfcn-cli
--features hyphenation` (or `features = ["hyphenation"]` on the
`pdfcn-core` dependency).

## Environment variables (no feature needed)

- `PDFCN_THREADS=N` — threads for per-image work in one render (header
  reads, crops, SVG conversion). Default: the machine's parallelism. The
  output is byte-identical whatever the value. Library callers that already
  spread documents over threads wrap each render in
  `pdfcn_core::with_threads(1, || ...)` (the batch endpoint does).
- `PDFCN_CACHE_MB=N` — budget of each `cache` cache, in MB (default 64;
  `0` disables caching).

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
