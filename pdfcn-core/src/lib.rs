mod assets;
#[cfg(feature = "cache")]
mod cache;
mod data;
mod error;
mod fs;
mod html_render;
mod links;
mod options;
mod page;
mod parallel;
mod prepare;
// The Wave 1 vector substrate (Ola 1.2/1.3): SVG rasterization and the
// chart/barcode SVG generators. Behind the `vector` cargo feature, on by
// default (see Cargo.toml) -- `--no-default-features` compiles it out
// entirely for a deployment that wants the smallest possible binary.
#[cfg(feature = "vector")]
mod barcode;
#[cfg(feature = "vector")]
mod charts_svg;
#[cfg(feature = "vector")]
mod raster;

// Ola 3: post-processes printpdf's own output to splice in a Factur-X
// invoice attachment. Behind the `factur-x` cargo feature, on by default
// (see Cargo.toml) -- same `--no-default-features` opt-out as `vector`.
#[cfg(feature = "factur-x")]
mod factur_x;
#[cfg(feature = "vector")]
mod vector_pdf;
// JPEG photos embedded as their own bytes (`/DCTDecode`) instead of decoded
// and re-encoded by printpdf. `jpeg-passthrough` cargo feature, on by default.
#[cfg(feature = "jpeg-passthrough")]
mod jpeg_passthrough;
#[cfg(any(feature = "vector", feature = "jpeg-passthrough"))]
mod pdf_xobjects;

// PAdES signing of a finished PDF. Opt-in `sign` cargo feature.
#[cfg(feature = "sign")]
mod sign;
#[cfg(feature = "sign")]
pub use sign::{
    sign_pdf, sign_pdf_timestamped, SignError, SignOptions, SigningIdentity, TimestampAuthority,
};

// Fiscal QR profiles (`%FiscalQR`): a JSON description of a country's invoice
// QR, registered at start-up. See pdfcn-components' `fiscal_qr` module.
pub use pdfcn_components::{
    fiscal_profile_ids, icon_names, register_fiscal_profile, register_icon, tabler_version,
};

pub use data::{load_data, DataFormat};
pub use error::CoreError;
#[cfg(feature = "factur-x")]
pub use factur_x::{embed_invoice as embed_factur_x_invoice, FacturXError, FacturXProfile};
pub use fs::{
    render_files, render_files_html, render_files_with_remote_images, FsPartialLoader,
    RemoteImageFetcher,
};
pub use options::{DocumentMetadata, ImageFormat, ImageOptimization, RenderOptions};
pub use page::{Orientation, PageConfig, PageSize};
pub use parallel::{threads, with_threads, THREADS_ENV};
pub use pdfcn_formats::Format;
pub use pdfcn_styles::{unresolved_classes, Theme, ThemeMode};
pub use pdfcn_template::{EvalError, NoPartials, PartialLoader};
pub use prepare::{prepare_image, PrepareFormat, PrepareOptions, Prepared};

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use printpdf::html::rust_fontconfig::{FcFontCache, FcParseFontBytes};
use printpdf::html::{add_xml_to_document_tracked, PageMargins, SharedFontPool, XmlRenderOptions};
use printpdf::{PdfDocument, PdfSaveOptions};
use serde_json::Value as JsonValue;

/// The built-in document typefaces, embedded so PDF generation has no
/// dependency on fonts being installed on the host (NFR-3): the render path
/// stays a single static binary regardless of what fonts the deploy target
/// happens to ship. Inter is shadcn/ui's own default UI typeface (FR-2's
/// "shadcn look and feel" goal), paired with a serif and a monospace family
/// for document bodies that need one. All three are Google Fonts, Open Font
/// License (redistributable/embeddable) -- see each family's `OFL.txt` under
/// `assets/fonts/`.
///
/// `render_pdf`/`render` use only these. A caller with its own typeface
/// (brand fonts, a client's house font) uses `render_pdf_with_fonts`/
/// `render_with_fonts` instead, passing `family name -> TTF/OTF bytes`;
/// custom fonts are added alongside the built-ins (a custom entry with the
/// same family name shadows the built-in one), and the document's CSS
/// selects a family the normal way (`font-family: "My Brand Font"`).
const BUILTIN_FONTS: &[(&str, &[u8])] = &[
    (
        "Inter",
        include_bytes!("../assets/fonts/inter/Inter-Regular.ttf"),
    ),
    (
        "Inter Bold",
        include_bytes!("../assets/fonts/inter/Inter-Bold.ttf"),
    ),
    (
        "Inter Italic",
        include_bytes!("../assets/fonts/inter/Inter-Italic.ttf"),
    ),
    (
        "Inter Bold Italic",
        include_bytes!("../assets/fonts/inter/Inter-BoldItalic.ttf"),
    ),
    (
        "Source Serif 4",
        include_bytes!("../assets/fonts/source-serif-4/SourceSerif4-Regular.ttf"),
    ),
    (
        "Source Serif 4 Bold",
        include_bytes!("../assets/fonts/source-serif-4/SourceSerif4-Bold.ttf"),
    ),
    (
        "Source Serif 4 Italic",
        include_bytes!("../assets/fonts/source-serif-4/SourceSerif4-Italic.ttf"),
    ),
    (
        "Source Serif 4 Bold Italic",
        include_bytes!("../assets/fonts/source-serif-4/SourceSerif4-BoldItalic.ttf"),
    ),
    (
        "JetBrains Mono",
        include_bytes!("../assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf"),
    ),
    (
        "JetBrains Mono Bold",
        include_bytes!("../assets/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    ),
    (
        "JetBrains Mono Italic",
        include_bytes!("../assets/fonts/jetbrains-mono/JetBrainsMono-Italic.ttf"),
    ),
    (
        "JetBrains Mono Bold Italic",
        include_bytes!("../assets/fonts/jetbrains-mono/JetBrainsMono-BoldItalic.ttf"),
    ),
];

/// The default document font-family, set on `body` by [`html_render::wrap_document`].
pub const DEFAULT_FONT_FAMILY: &str = "Inter";

/// The family a built-in face belongs to: `"Inter Bold Italic"` -> `"Inter"`.
fn family_of(face: &str) -> &str {
    let mut family = face;
    while let Some(shorter) = family
        .strip_suffix(" Italic")
        .or_else(|| family.strip_suffix(" Bold"))
    {
        family = shorter;
    }
    family
}

/// Builds a font pool from raw font bytes with **no system font scan**.
///
/// `printpdf::PdfDocument::from_html` builds one internally (via
/// `rust_fontconfig::FcFontCache::build()`, a real filesystem scan for
/// installed fonts) whenever no `font_pool` is supplied -- on every single
/// render call, since nothing here reuses it across calls. On a sandboxed
/// serverless filesystem that scan can hang indefinitely rather than fail
/// fast (this is what made `/api/generate-pdf` hang in production). Every
/// family this pipeline needs is already embedded ([`BUILTIN_FONTS`] plus
/// any caller-supplied font), so there is nothing for a system scan to add:
/// starting from an empty `FcFontCache` and registering only those bytes
/// removes the filesystem dependency entirely (NFR-3), at the cost of no
/// system-font fallback for a glyph none of the embedded fonts cover.
fn font_pool(fonts: &BTreeMap<String, &[u8]>) -> SharedFontPool {
    let fc_cache = FcFontCache::default();
    for (name, bytes) in fonts {
        if let Some(parsed) = FcParseFontBytes(bytes, name) {
            fc_cache.with_memory_fonts(parsed);
        }
    }
    SharedFontPool {
        fc_cache: Arc::new(fc_cache),
        parsed_fonts: Arc::new(Mutex::new(HashMap::new())),
    }
}

/// The built-in-only font pool, built once per process and shared by every
/// render that doesn't bring custom fonts. `printpdf`'s `SharedFontPool` is
/// designed for exactly this: its `parsed_fonts` map fills lazily during a
/// layout pass and is reused by every later pass sharing the pool, so
/// rebuilding the pool per render (as this used to) re-parses all twelve
/// embedded TTFs on every single document. Server-side callers rendering in
/// a loop pay the font-parse cost once instead of per request.
static BUILTIN_FONT_POOL: OnceLock<SharedFontPool> = OnceLock::new();

fn builtin_font_pool() -> &'static SharedFontPool {
    BUILTIN_FONT_POOL.get_or_init(|| {
        let raw: BTreeMap<String, &[u8]> = BUILTIN_FONTS
            .iter()
            .map(|(name, bytes)| (name.to_string(), *bytes))
            .collect();
        font_pool(&raw)
    })
}

/// Parses a named partial in the format its extension implies (`.html`,
/// `.md`, `.yaml`, ...), or HAML/auto-detected when it has none. For callers
/// that hold partials in memory rather than on disk (the HTTP API).
pub fn parse_partial(name: &str, source: &str) -> Result<pdfcn_parser::Document, CoreError> {
    let format = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .and_then(Format::from_extension);
    Ok(pdfcn_formats::parse_document(source, format)?)
}

/// Builds a [`Theme`] from its JSON form, as accepted by the HTTP API's
/// optional `theme` request field:
///
/// ```json
/// { "mode": "dark", "overrides": { "primary": "#2563eb" } }
/// ```
///
/// `mode` defaults to light; `overrides` maps bare semantic token names to
/// literal CSS colors. Returns `Err` with a client-facing message for a
/// malformed shape.
pub fn theme_from_json(value: &JsonValue) -> Result<Theme, String> {
    let obj = value
        .as_object()
        .ok_or_else(|| "theme must be an object".to_string())?;
    let mut theme = match obj.get("mode").and_then(JsonValue::as_str) {
        None | Some("light") => Theme::light(),
        Some("dark") => Theme::dark(),
        Some(other) => {
            return Err(format!(
                "unknown theme mode \"{other}\" (expected \"light\" or \"dark\")"
            ))
        }
    };
    match obj.get("overrides") {
        None | Some(JsonValue::Null) => {}
        Some(JsonValue::Object(map)) => {
            for (token, color) in map {
                let Some(color) = color.as_str() else {
                    return Err(format!("theme override \"{token}\" must be a string"));
                };
                theme.overrides.insert(token.clone(), color.to_string());
            }
        }
        Some(_) => {
            return Err("theme overrides must be an object of token name to CSS color".to_string())
        }
    }
    Ok(theme)
}

/// Parsed template ASTs, keyed by the source text and its format (see
/// `cache`). Server-side callers rendering many documents from one template
/// (a batch endpoint, a monthly-statements loop) and `pdfcn dev` pay the
/// lex/parse cost once; a hit is one refcount bump, not a deep clone of the
/// tree. Only successful parses are cached -- a malformed template keeps
/// failing identically. `pdfcn_template::evaluate` only ever needs
/// `&Document`, and `&Arc<Document>` derefs to that for free.
#[cfg(feature = "cache")]
static PARSE_CACHE: cache::Shared<pdfcn_formats::Parsed> = cache::Shared::new();

fn cached_parse(
    source: &str,
    format: Option<Format>,
) -> Result<Arc<pdfcn_formats::Parsed>, CoreError> {
    #[cfg(feature = "cache")]
    {
        // The same text can mean different things in different formats.
        let key = cache::key((format.map(|f| f as u8), source), source.len());
        if let Some(hit) = PARSE_CACHE.get(key) {
            return Ok(hit);
        }
        let parsed = pdfcn_formats::parse(source, format)?;
        // An AST is a few times the size of its source.
        Ok(PARSE_CACHE.insert(key, parsed, source.len().saturating_mul(4)))
    }
    #[cfg(not(feature = "cache"))]
    Ok(Arc::new(pdfcn_formats::parse(source, format)?))
}

/// Built stylesheets, keyed by the distinct utility classes a document uses
/// plus its theme -- not by the document's literal HTML. A "one template,
/// many rows" batch almost always resolves to the same class set across every
/// row even though the text differs per row, so this is the case that
/// actually recurs.
#[cfg(feature = "cache")]
static STYLESHEET_CACHE: cache::Shared<String> = cache::Shared::new();

fn cached_stylesheet(classes: &BTreeSet<String>, theme: &Theme) -> Arc<String> {
    let build = || pdfcn_styles::build_stylesheet_from_classes(classes, theme);
    #[cfg(feature = "cache")]
    {
        STYLESHEET_CACHE.get_or_compute(
            cache::key((classes, theme), classes.len()),
            build,
            String::len,
        )
    }
    #[cfg(not(feature = "cache"))]
    Arc::new(build())
}

/// Runs the full FR-1/FR-2/FR-3 pipeline: parses `source`, evaluates it
/// against `data`, expands components, and returns a complete HTML
/// document string with an embedded, minified, print-safe stylesheet.
pub fn render_html(
    source: &str,
    data: &JsonValue,
    loader: &dyn PartialLoader,
) -> Result<String, CoreError> {
    render_html_with_theme(source, data, loader, &Theme::light())
}

/// Like [`render_html`], but semantic token utilities resolve through
/// `theme`: its mode picks shadcn's light or dark token table and its
/// per-token overrides rebrand the document (`bg-primary`, `%Badge`,
/// borders, ...) without touching the template. See [`theme_from_json`]
/// for the HTTP-facing shape.
pub fn render_html_with_theme(
    source: &str,
    data: &JsonValue,
    loader: &dyn PartialLoader,
    theme: &Theme,
) -> Result<String, CoreError> {
    render_html_as(source, None, data, loader, theme)
}

/// Like [`render_html_with_theme`], with the template's format given
/// explicitly (`None` detects it from the text, see [`Format::detect`]).
/// HAML, HTML, Markdown, JSON/YAML block trees and the Typst/LaTeX subsets
/// all lower to the same AST, so everything after parsing is shared.
pub fn render_html_as(
    source: &str,
    format: Option<Format>,
    data: &JsonValue,
    loader: &dyn PartialLoader,
    theme: &Theme,
) -> Result<String, CoreError> {
    let parsed = cached_parse(source, format)?;
    // Markdown front matter is default data: what the caller supplies wins.
    let merged;
    let data = match &parsed.front_matter {
        Some(front) => {
            merged = pdfcn_formats::merge_defaults(front, data);
            &merged
        }
        None => data,
    };
    let resolved = pdfcn_template::evaluate(&parsed.document, data, loader)?;
    let body = html_render::render_body(&resolved)?;
    // The layout engine ignores the CSS `gap` declaration, so flex/grid
    // `gap-*` utilities are rewritten into equivalent margins on children
    // before the stylesheet is built — which also means the injected
    // `mr-*`/`mb-*` classes are picked up by the same used-classes scan as
    // every other utility (see pdfcn_styles::rewrite_gaps).
    let (body_html, author_css) = html_render::hoist_styles(&body.into_string());
    let body_html = pdfcn_styles::rewrite_composites(
        &pdfcn_styles::rewrite_text_decorations(&pdfcn_styles::rewrite_gaps(&body_html)),
        theme,
    );
    // Last step, after every pass that reads utility classes: brackets in a
    // class name would make the engine drop the rules after their selector.
    let body_html = pdfcn_styles::encode_class_brackets(&body_html);
    let classes = pdfcn_styles::extract_classes(&body_html);
    // Author `<style>` blocks come last so they win ties with utilities.
    let stylesheet = format!("{}{}", cached_stylesheet(&classes, theme), author_css);
    Ok(html_render::wrap_document_str(&body_html, &stylesheet))
}

/// Renders a complete HTML document (as produced by [`render_html`]) to PDF
/// bytes in memory (FR-4), honoring `options`'s page geometry, page
/// decoration (header/footer/page-numbers) and metadata. Pure Rust: no
/// headless browser, no external process, Vercel-safe (NFR-3). Uses only
/// the built-in typefaces ([`BUILTIN_FONTS`]); for a document that needs a
/// caller-supplied font (a brand typeface), use [`render_pdf_with_fonts`].
pub fn render_pdf(html: &str, options: &RenderOptions) -> Result<Vec<u8>, CoreError> {
    render_pdf_with_fonts(html, options, &BTreeMap::new())
}

/// Like [`render_pdf`], but `custom_fonts` (family name -> TTF/OTF bytes) is
/// embedded alongside the built-in typefaces, so the document's CSS can
/// reference `font-family: "<name>"` for any family name used as a key. A
/// custom entry sharing a built-in family name (e.g. `"Inter"`) overrides
/// that built-in weight/style rather than adding a duplicate.
pub fn render_pdf_with_fonts(
    html: &str,
    options: &RenderOptions,
    custom_fonts: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<u8>, CoreError> {
    render_pdf_with_assets(html, custom_fonts, &BTreeMap::new(), options)
}

/// Like [`render_pdf_with_fonts`], but `images` (the `<img src="...">` value
/// -> raw image bytes, JPEG/PNG) is embedded alongside the document, so an
/// `%img(src="cover.jpg")` element resolves to that image rather than a
/// broken-image placeholder. Matches `custom_fonts`' "supply the bytes
/// yourself" convention -- no network access at render time (NFR-3).
pub fn render_pdf_with_assets(
    html: &str,
    custom_fonts: &BTreeMap<String, Vec<u8>>,
    images: &BTreeMap<String, Vec<u8>>,
    options: &RenderOptions,
) -> Result<Vec<u8>, CoreError> {
    // Post-render asset preparation runs here — the one choke point every
    // entry point (CLI, HTTP API, napi bindings) funnels through — so QR
    // placeholders get real bytes and object-fit:cover images get cropped
    // before the layout engine ever sees them. Every entry borrows the
    // caller's bytes until a pass replaces it (docs/spikes/006).
    let mut prepared_images: assets::Images<'_> = images
        .iter()
        .map(|(src, bytes)| (src.clone(), Cow::Borrowed(bytes.as_slice())))
        .collect();
    let (width_mm, height_mm) = options.page.page_size_mm();
    let page_bound = content_box_px(&options.page);
    let mut vector_sources = BTreeMap::new();
    let html = assets::prepare_assets_with_vectors(
        html,
        &mut prepared_images,
        &options.svg_assets,
        Some(page_bound),
        options.shrink_images,
        &mut vector_sources,
    );
    let html = html.as_str();
    // Images the layout engine lays out by size alone: their XObject is
    // replaced once the document is saved (`pdf_xobjects::finish`).
    #[allow(unused_mut)]
    let mut placeholders: BTreeMap<String, (u32, u32)> = BTreeMap::new();
    #[cfg(feature = "vector")]
    let forms = vector_pdf::prepare(&vector_sources, &mut prepared_images, &mut placeholders);
    #[cfg(not(feature = "vector"))]
    let _ = vector_sources;
    // Last, after every pass that may re-encode a photo: whatever JPEG is
    // left goes into the PDF as it is (see `jpeg_passthrough`).
    #[cfg(feature = "jpeg-passthrough")]
    let jpegs = if jpeg_passthrough::allowed(options.image_optimization.as_ref()) {
        jpeg_passthrough::take(&mut prepared_images, &mut placeholders)
    } else {
        BTreeMap::new()
    };

    // header_text/footer_text/show_page_numbers/skip_first_page are wired
    // through exactly as printpdf's own API declares them, but currently
    // render nothing -- see `RenderOptions::header_text`'s doc comment for
    // the verified upstream gap. Left wired rather than dropped: correct
    // today (a documented no-op) and self-healing if printpdf starts
    // drawing them.
    //
    // Built here rather than through `PdfDocument::from_html_with_cache`,
    // which copies every image and font into an `XmlRenderOptions` of its
    // own: this one owns them already, and `add_xml_to_document_tracked`
    // only reads it.
    let mut xml_options = XmlRenderOptions {
        page_width: printpdf::Mm(width_mm),
        page_height: printpdf::Mm(height_mm),
        margins: PageMargins::new(
            printpdf::Mm(options.page.margin_mm),
            printpdf::Mm(options.page.margin_mm),
            printpdf::Mm(options.page.margin_mm),
            printpdf::Mm(options.page.margin_mm),
        ),
        header_text: options.header_text.clone(),
        footer_text: options.footer_text.clone(),
        show_page_numbers: options.show_page_numbers,
        skip_first_page: options.skip_first_page,
        image_placeholders: placeholders,
        ..XmlRenderOptions::default()
    };
    let (fonts, pool) = embedded_fonts(html, custom_fonts);
    xml_options.fonts = fonts;
    xml_options.font_pool = Some(pool);
    // Whatever images are left are decoded by printpdf: a borrowed one is
    // copied into its options once, an owned one (a crop, a raster) moves.
    xml_options.images = prepared_images
        .into_iter()
        .map(|(src, bytes)| (src, bytes.into_owned()))
        .collect();

    // Hyperlinks: the links (and their jump targets) carry a tracking class,
    // and the one layout of the document reports where each landed; the
    // annotations go onto that same document. See `links`.
    let link_plan = links::plan(html);
    if link_plan.is_some() {
        xml_options.track_class_prefix = Some(links::TRACK_PREFIX.to_string());
    }
    let html = link_plan
        .as_ref()
        .map_or(html, |plan| plan.marked_html.as_str());
    let mut doc = PdfDocument::new("PDF Document");
    // A failed layout leaves the document without pages, exactly as
    // `from_html_with_cache` (which this replaces) does.
    let tracked = add_xml_to_document_tracked(&mut doc, html, &xml_options).unwrap_or_default();
    if let Some(plan) = &link_plan {
        links::apply(plan, &tracked, &mut doc);
    }

    apply_metadata(&mut doc, &options.metadata);

    #[allow(unused_mut)]
    let mut image_optimization =
        to_printpdf_image_optimization(options.image_optimization.as_ref());
    // When every image printpdf decoded is one that a later pass replaces
    // (a QR code's PNG that becomes a vector), compressing its pixels is pure
    // waste. Write them raw; `pdf_xobjects::finish` compresses whatever is
    // left. Size-only placeholders have no pixels to compress either way.
    #[cfg(feature = "vector")]
    let raw_placeholders = !xml_options.images.is_empty()
        && xml_options.images.keys().all(|src| forms.contains_key(src));
    #[cfg(not(feature = "vector"))]
    let raw_placeholders = false;
    if raw_placeholders {
        image_optimization.format = Some(printpdf::ImageCompression::None);
    }
    // The layout is done: printpdf's copy of the input bytes is not needed to
    // save (it keeps its own decoded images).
    drop(xml_options);
    let save_options = PdfSaveOptions {
        image_optimization: Some(image_optimization),
        ..Default::default()
    };
    let mut save_warnings = Vec::new();
    let pdf = doc.save(&save_options, &mut save_warnings);
    drop(doc);
    // The layout placed every vector and passed-through JPEG as a stand-in;
    // put the real objects in, in one load and one save (see `pdf_xobjects`).
    #[cfg(any(feature = "vector", feature = "jpeg-passthrough"))]
    let pdf = pdf_xobjects::finish(
        pdf,
        pdf_xobjects::Replacements {
            #[cfg(feature = "vector")]
            forms,
            #[cfg(feature = "jpeg-passthrough")]
            jpegs,
            compress_leftovers: raw_placeholders,
            borrows: std::marker::PhantomData,
        },
    );
    #[cfg(not(any(feature = "vector", feature = "jpeg-passthrough")))]
    let _ = raw_placeholders;
    Ok(pdf)
}

/// What a percentage-sized image can at most occupy: the page minus margins,
/// in CSS px (96 per inch).
fn content_box_px(page: &PageConfig) -> (f64, f64) {
    let (width_mm, height_mm) = page.page_size_mm();
    let margin_mm = page.margin_mm;
    let px_per_mm = 96.0 / 25.4;
    (
        f64::from((width_mm - 2.0 * margin_mm).max(1.0)) * px_per_mm,
        f64::from((height_mm - 2.0 * margin_mm).max(1.0)) * px_per_mm,
    )
}

/// The fonts a render embeds (family -> bytes) and the pool printpdf parses
/// them from.
fn embedded_fonts(
    html: &str,
    custom_fonts: &BTreeMap<String, Vec<u8>>,
) -> (BTreeMap<String, Vec<u8>>, SharedFontPool) {
    // Embed only the fonts the document can reach. Every built-in family
    // is ~300-500KB of TTF, and a typical invoice uses two of the twelve;
    // embedding all of them on every render was the single largest line
    // item in output size. A family survives if the document's CSS names
    // it -- or if it's Inter, the default body font `wrap_document` always
    // sets. Caller-supplied fonts are always kept (they were supplied
    // deliberately, and their bytes aren't ours to second-guess).
    let used_families = used_font_families(html);
    let mut raw_fonts: BTreeMap<String, &[u8]> = BTreeMap::new();
    for (family, bytes) in BUILTIN_FONTS {
        // A face belongs to its family: `font-bold` / `italic` select the
        // Bold / Italic face of the family the CSS names (or of Inter, the
        // default), so those faces must be registered too -- not only when
        // their own spelling ("Inter Bold") is spelled out in the CSS.
        let base = family_of(family);
        if base != DEFAULT_FONT_FAMILY
            && !used_families.contains(*family)
            && !used_families.contains(base)
        {
            continue;
        }
        raw_fonts.insert(family.to_string(), bytes);
    }
    for (family, bytes) in custom_fonts {
        raw_fonts.insert(family.clone(), bytes);
    }
    let pool = if custom_fonts.is_empty() {
        // Shared per-process pool: font parse results accumulate in its
        // `parsed_fonts` cache across renders instead of being thrown away
        // (see `builtin_font_pool`).
        builtin_font_pool().clone()
    } else {
        font_pool(&raw_fonts)
    };
    // printpdf's options own their font bytes: the one copy per render.
    let fonts = raw_fonts
        .into_iter()
        .map(|(family, bytes)| (family, bytes.to_vec()))
        .collect();
    (fonts, pool)
}

fn apply_metadata(doc: &mut PdfDocument, metadata: &DocumentMetadata) {
    // Set directly on the saved document rather than via printpdf's own
    // `<meta name="pdf.metadata.*">` mechanism -- see `DocumentMetadata`'s
    // doc comment for why. Only touches a field the caller actually set,
    // leaving printpdf's own blank defaults for the rest.
    if let Some(title) = &metadata.title {
        doc.metadata.info.document_title = title.clone();
    }
    if let Some(author) = &metadata.author {
        doc.metadata.info.author = author.clone();
    }
    if let Some(subject) = &metadata.subject {
        doc.metadata.info.subject = subject.clone();
    }
    if !metadata.keywords.is_empty() {
        doc.metadata.info.keywords = metadata.keywords.clone();
    }
    if let Some(producer) = &metadata.producer {
        doc.metadata.info.producer = producer.clone();
    }
}

/// The per-image budget, in *uncompressed* bytes, above which printpdf shrinks
/// an image when it saves the document.
///
/// printpdf's own default is 2MB. That is the size of the decoded pixels, not of
/// the file: an RGB picture of 724x965 already reaches it, so every photograph
/// in a document was cut to about 0.7 megapixels no matter how large it is
/// drawn (a full-width photo came out at roughly 110 ppi), by a
/// nearest-neighbour resample at that. pdfcn's own asset pass has already limited
/// each image to what its box can use (about 300 ppi) and to 4096 pixels a side,
/// which is 48MB decoded at the very most, so this budget only has to stay out of
/// the way. The file stays small because photographs are still written as JPEG.
/// A caller who wants the old, tighter budget sets `max_size` (`--image-max-size`).
const DEFAULT_MAX_IMAGE_BYTES: &str = "64MB";

/// Maps pdfcn's own [`ImageOptimization`] to printpdf's
/// `ImageOptimizationOptions`/`ImageCompression`. Always returns a concrete
/// value, falling back to printpdf's own per-field defaults (quality 0.85,
/// format auto -- compression that's already on) for whichever fields the
/// caller left unset, `opts: None` included -- except the size budget, see
/// [`DEFAULT_MAX_IMAGE_BYTES`]. Passing `None` straight through to
/// `PdfSaveOptions.image_optimization` would disable image re-encoding entirely
/// rather than leave it at its default, which is exactly the regression a
/// golden-snapshot diff caught while wiring this.
fn to_printpdf_image_optimization(
    opts: Option<&ImageOptimization>,
) -> printpdf::ImageOptimizationOptions {
    let defaults = printpdf::ImageOptimizationOptions {
        max_image_size: Some(DEFAULT_MAX_IMAGE_BYTES.to_string()),
        ..printpdf::ImageOptimizationOptions::default()
    };
    let Some(opts) = opts else { return defaults };
    printpdf::ImageOptimizationOptions {
        quality: opts.quality.or(defaults.quality),
        max_image_size: opts.max_size.clone().or(defaults.max_image_size),
        dither_greyscale: defaults.dither_greyscale,
        convert_to_greyscale: opts.greyscale.or(defaults.convert_to_greyscale),
        auto_optimize: defaults.auto_optimize,
        format: opts
            .format
            .map(|format| match format {
                ImageFormat::Auto => printpdf::ImageCompression::Auto,
                ImageFormat::Jpeg => printpdf::ImageCompression::Jpeg,
                ImageFormat::Lossless => printpdf::ImageCompression::Flate,
                ImageFormat::Raw => printpdf::ImageCompression::None,
            })
            .or(defaults.format),
    }
}

/// End-to-end: `.haml` source + data context + rendering options -> PDF
/// bytes. Uses only the built-in typefaces; see [`render_with_fonts`] for a
/// caller-supplied font. `options.theme` reaches the stylesheet build here
/// -- this is the entry point CLI/napi callers should use for a themed
/// document, rather than [`render_html`] (always light) plus
/// [`render_pdf`].
pub fn render(
    source: &str,
    data: &JsonValue,
    loader: &dyn PartialLoader,
    options: &RenderOptions,
) -> Result<Vec<u8>, CoreError> {
    render_with_fonts(source, data, loader, &BTreeMap::new(), options)
}

/// Like [`render`], but `custom_fonts` (family name -> TTF/OTF bytes) is
/// embedded alongside the built-in typefaces. See [`render_pdf_with_fonts`].
pub fn render_with_fonts(
    source: &str,
    data: &JsonValue,
    loader: &dyn PartialLoader,
    custom_fonts: &BTreeMap<String, Vec<u8>>,
    options: &RenderOptions,
) -> Result<Vec<u8>, CoreError> {
    render_with_assets(
        source,
        data,
        loader,
        custom_fonts,
        &BTreeMap::new(),
        options,
    )
}

/// Like [`render_with_fonts`], but also embeds `images` (see
/// [`render_pdf_with_assets`]).
pub fn render_with_assets(
    source: &str,
    data: &JsonValue,
    loader: &dyn PartialLoader,
    custom_fonts: &BTreeMap<String, Vec<u8>>,
    images: &BTreeMap<String, Vec<u8>>,
    options: &RenderOptions,
) -> Result<Vec<u8>, CoreError> {
    let html = render_html_with_theme(source, data, loader, &options.theme)?;
    render_pdf_with_assets(&html, custom_fonts, images, options)
}

/// Extracts every `src="..."` value from `<img ...>` tags in already
/// -rendered HTML (as produced by [`render_html`]), in document order.
/// Delegates to the single `<img>` scanner in [`crate::assets`] — the same
/// parse the QR/cover-crop passes use — rather than keeping a second
/// hand-rolled copy. Public so a caller with its own async/runtime-specific
/// fetching (e.g. the `/api/generate-pdf` Vercel handler, which needs
/// `tokio`-async HTTP rather than the blocking fetcher `pdfcn build` uses)
/// can find which `http(s):` sources to resolve without reimplementing
/// this parse.
pub fn img_srcs(html: &str) -> Vec<String> {
    assets::scan_img_tags(html)
        .iter()
        .filter_map(|tag| assets::attr_value(&tag.attrs, "src").map(str::to_string))
        .collect()
}

/// Collects the font families the document's CSS actually asks for: every
/// value in a `font-family:` declaration (the generated stylesheet embeds
/// them minified inside `<style>`, so this scans the whole document),
/// split on commas, unquoted and trimmed. Used by [`render_pdf_with_assets`]
/// to prune the embedded font set down to what the document references.
fn used_font_families(html: &str) -> BTreeSet<String> {
    // Generic CSS family keywords are never embeddable files; naming one
    // in a stack is a fallback instruction, not a font request.
    const GENERIC_FAMILIES: [&str; 9] = [
        "serif",
        "sans-serif",
        "monospace",
        "cursive",
        "fantasy",
        "system-ui",
        "ui-serif",
        "ui-sans-serif",
        "ui-monospace",
    ];
    const MARKER: &str = "font-family:";
    let mut out = BTreeSet::new();
    let mut rest = html;
    while let Some(pos) = rest.find(MARKER) {
        let after = &rest[pos + MARKER.len()..];
        // A declaration ends at `;`, a block close, a tag opening, or an
        // inline attribute's `)"` tail -- but never at a quote or comma,
        // since multi-word families arrive quoted ("Source Serif 4") and
        // stacks arrive comma-separated.
        let end = after.find([';', '}', '<', ')', '>']).unwrap_or(after.len());
        for name in after[..end].split(',') {
            let name = name.trim().trim_matches('"').trim_matches('\'').trim();
            if !name.is_empty() && !GENERIC_FAMILIES.contains(&name.to_ascii_lowercase().as_str()) {
                out.insert(name.to_string());
            }
        }
        rest = &after[end..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn image_optimization_none_keeps_printpdfs_own_defaults_except_the_size_budget() {
        let mapped = to_printpdf_image_optimization(None);
        let printpdf_default = printpdf::ImageOptimizationOptions::default();
        assert_eq!(mapped.quality, printpdf_default.quality);
        assert_eq!(mapped.format, printpdf_default.format);
        assert_eq!(mapped.auto_optimize, printpdf_default.auto_optimize);
        // printpdf's 2MB of decoded pixels would cut every photo to ~0.7 MP.
        assert_eq!(mapped.max_image_size.as_deref(), Some("64MB"));
        assert_ne!(mapped.max_image_size, printpdf_default.max_image_size);
    }

    #[test]
    fn an_explicit_size_budget_still_wins() {
        let opts = ImageOptimization {
            max_size: Some("300kb".into()),
            ..ImageOptimization::default()
        };
        let mapped = to_printpdf_image_optimization(Some(&opts));
        assert_eq!(mapped.max_image_size.as_deref(), Some("300kb"));
    }

    #[test]
    fn image_optimization_only_overrides_fields_the_caller_set() {
        let opts = ImageOptimization {
            quality: Some(0.5),
            ..ImageOptimization::default()
        };
        let mapped = to_printpdf_image_optimization(Some(&opts));
        let defaults = printpdf::ImageOptimizationOptions::default();
        assert_eq!(mapped.quality, Some(0.5));
        assert_eq!(mapped.max_image_size.as_deref(), Some("64MB"));
        assert_eq!(mapped.auto_optimize, defaults.auto_optimize);
    }

    #[test]
    fn image_optimization_maps_every_format_variant() {
        for (pdfcn, printpdf) in [
            (ImageFormat::Auto, printpdf::ImageCompression::Auto),
            (ImageFormat::Jpeg, printpdf::ImageCompression::Jpeg),
            (ImageFormat::Lossless, printpdf::ImageCompression::Flate),
            (ImageFormat::Raw, printpdf::ImageCompression::None),
        ] {
            let opts = ImageOptimization {
                format: Some(pdfcn),
                ..ImageOptimization::default()
            };
            assert_eq!(
                to_printpdf_image_optimization(Some(&opts)).format,
                Some(printpdf)
            );
        }
    }

    #[test]
    fn metadata_only_sets_fields_the_caller_provided() {
        let options = RenderOptions {
            metadata: DocumentMetadata {
                title: Some("Invoice #42".to_string()),
                author: Some("pdfcn".to_string()),
                ..DocumentMetadata::default()
            },
            ..RenderOptions::default()
        };
        let bytes =
            render_pdf_with_assets("<p>hi</p>", &BTreeMap::new(), &BTreeMap::new(), &options)
                .unwrap();
        let mut parse_warnings = Vec::new();
        let doc = printpdf::parse_pdf_from_bytes(
            &bytes,
            &printpdf::PdfParseOptions::default(),
            &mut parse_warnings,
        )
        .unwrap();
        assert_eq!(doc.metadata.info.document_title, "Invoice #42");
        assert_eq!(doc.metadata.info.author, "pdfcn");
        assert_eq!(doc.metadata.info.subject, "");
    }

    #[test]
    fn img_srcs_extracts_every_image_source_in_document_order() {
        let html = concat!(
            r#"<div><img class="a" src="cover.jpg" alt=""><p>x</p>"#,
            r#"<img src="https://example.com/x.png"></div>"#,
        );
        assert_eq!(
            img_srcs(html),
            vec![
                "cover.jpg".to_string(),
                "https://example.com/x.png".to_string()
            ]
        );
    }

    #[test]
    fn renders_html_with_component_and_styles() {
        let source = "%Card(title=\"Invoice\")\n  %p.text-lg Hello {{ name }}";
        let data = json!({ "name": "Ada" });
        let html = render_html(source, &data, &NoPartials).unwrap();
        assert!(html.contains("Hello Ada"));
        assert!(html.contains("card"));
        assert!(html.contains("<style>"));
    }

    #[test]
    fn escapes_untrusted_interpolated_values() {
        let source = "%p {{ payload }}";
        let data = json!({ "payload": "<script>alert(1)</script>" });
        let html = render_html(source, &data, &NoPartials).unwrap();
        assert!(!html.contains("<script>alert"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn renders_a_minimal_document_to_pdf_bytes() {
        let source = "%h1 Invoice\n%p Total: {{ total }}";
        let data = json!({ "total": "$42.00" });
        let bytes = render(source, &data, &NoPartials, &RenderOptions::default()).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
    }

    /// Wave 0's shadcn tokens (semantic theme colors, full neutral/accent
    /// scales, radius/shadow scales) resolve to plain CSS declarations, but
    /// the pipeline still has to carry that CSS through the stylesheet
    /// minifier and `printpdf`'s HTML/CSS layout engine end to end.
    /// This is the sanity check the "Wave 0" scoping asked for before Wave 1
    /// component fidelity can be trusted on top of it.
    #[test]
    fn layout_engine_renders_new_token_css_to_pdf() {
        let source = concat!(
            "%DocumentLayout\n",
            "  %Card(title=\"Report\")\n",
            "    %Badge(variant=\"destructive\" label=\"Overdue\")\n",
            "    %p.bg-primary.text-primary-foreground.shadow-lg.rounded-2xl.bg-zinc-100 Body\n",
            "    %Separator\n",
        );
        let html = render_html(source, &json!({}), &NoPartials).unwrap();
        // Every Wave 0 token class made it into the emitted stylesheet as a
        // real declaration, not silently dropped by the minifier.
        assert!(html.contains("background-color"));
        assert!(html.contains("border-radius:16px"));
        assert!(html.contains("box-shadow"));

        let bytes = render_pdf(&html, &RenderOptions::default()).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
    }

    /// The layout engine ignores the CSS `gap` declaration, so render_html
    /// rewrites flex/grid `gap-*` containers into margins on children before
    /// building the stylesheet — `gap-2` must produce real spacing CSS
    /// (`margin-right` on every child but the last), not a dead declaration.
    #[test]
    fn flex_gap_is_rewritten_into_child_margins_end_to_end() {
        let source = ".flex.gap-2\n  %p A\n  %p B\n  %p C";
        let html = render_html(source, &json!({}), &NoPartials).unwrap();
        assert!(html.contains("<p class=\"mr-2\">A</p>"), "{html}");
        assert!(html.contains("<p class=\"mr-2\">B</p>"), "{html}");
        assert!(html.contains("<p>C</p>"), "{html}");
        // The injected utility was picked up by the used-classes scan.
        assert!(html.contains("margin-right"), "{html}");
    }

    #[test]
    fn grid_gap_follows_row_geometry_end_to_end() {
        let source = concat!(
            ".grid.grid-cols-2.gap-3\n",
            "  %p One\n",
            "  %p Two\n",
            "  %p Three\n",
            "  %p Four",
        );
        let html = render_html(source, &json!({}), &NoPartials).unwrap();
        assert!(html.contains("<p class=\"mr-3 mb-3\">One</p>"), "{html}");
        assert!(html.contains("<p class=\"mb-3\">Two</p>"), "{html}");
        assert!(html.contains("<p class=\"mr-3\">Three</p>"), "{html}");
        assert!(html.contains("<p>Four</p>"), "{html}");
    }

    #[test]
    fn interactive_only_component_is_rejected_with_an_explicit_error() {
        let source = "%Dialog";
        let err = render_html(source, &json!({}), &NoPartials).unwrap_err();
        assert!(err
            .to_string()
            .contains("interactive-only, unsupported in static PDF output"));
    }

    /// Font pruning: only families the document's CSS names (plus Inter,
    /// the default body font) may be embedded. A serif/mono document must
    /// keep those families; a default-styled one embeds just Inter.
    #[test]
    fn used_font_families_collects_every_declared_family() {
        let html = concat!(
            "<style>body{font-family:Inter}",
            "h1{font-family:\"Source Serif 4\",serif}",
            "code{font-family: JetBrains Mono}</style>"
        );
        let used = used_font_families(html);
        assert!(used.contains("Inter"));
        assert!(used.contains("Source Serif 4"));
        assert!(used.contains("JetBrains Mono"));
    }

    #[test]
    fn a_default_styled_document_embeds_only_the_default_font() {
        let source = "%h1 Invoice\\n%p Total: $42";
        let html = render_html(source, &json!({}), &NoPartials).unwrap();
        // The stylesheet names only the default family.
        let used = used_font_families(&html);
        assert_eq!(used, BTreeSet::from([DEFAULT_FONT_FAMILY.to_string()]));
        // End to end: still renders.
        let bytes = render_pdf(&html, &RenderOptions::default()).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
    }

    #[test]
    fn a_serif_document_keeps_its_named_font_embedded() {
        let source = "%p(style=\"font-family:'Source Serif 4'\") Serif body";
        let html = render_html(source, &json!({}), &NoPartials).unwrap();
        assert!(used_font_families(&html).contains("Source Serif 4"));
        let bytes = render_pdf(&html, &RenderOptions::default()).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
    }

    /// The parse cache must be transparent: identical renders before and
    /// after caching (including a second render of the same source).
    #[test]
    fn cached_parse_is_transparent_for_repeated_sources() {
        let source = "%p Cache me {{ n }}";
        let first = cached_parse(source, None).unwrap();
        let second = cached_parse(source, None).unwrap();
        assert_eq!(first.document, second.document);
        // A cache hit shares the same allocation rather than deep-cloning
        // the AST -- the whole point of caching it.
        #[cfg(feature = "cache")]
        assert!(Arc::ptr_eq(&first, &second));
        // And the cached AST still evaluates correctly.
        let resolved =
            pdfcn_template::evaluate(&second.document, &json!({ "n": 7 }), &NoPartials).unwrap();
        match &resolved[0] {
            pdfcn_template::Resolved::Element { children, .. } => {
                assert_eq!(
                    children[0],
                    pdfcn_template::Resolved::Text("Cache me 7".into())
                );
            }
            other => panic!("expected Element, got {other:?}"),
        }
    }

    /// The stylesheet cache keys on the class set + theme, not the literal
    /// HTML -- two different documents that happen to use the same classes
    /// (the common "one template, many rows" shape) must share one build.
    #[test]
    fn stylesheet_cache_hits_on_matching_classes_regardless_of_surrounding_html() {
        // `bg-primary` resolves through the theme's token table, so light
        // and dark must actually disagree on its declaration -- unlike
        // `flex`/`p-4`, which don't depend on the theme at all.
        let classes: BTreeSet<String> = ["p-4", "flex", "bg-primary"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let theme = Theme::light();
        let first = cached_stylesheet(&classes, &theme);
        // Shared, not rebuilt, when the cache is on.
        #[cfg(feature = "cache")]
        assert!(Arc::ptr_eq(&first, &cached_stylesheet(&classes, &theme)));
        assert_eq!(
            *first,
            pdfcn_styles::build_stylesheet_from_classes(&classes, &theme)
        );

        // A different theme is a genuinely different cache entry.
        let dark = Theme::dark();
        let third = cached_stylesheet(&classes, &dark);
        assert!(!Arc::ptr_eq(&first, &third));
        assert_ne!(*first, *third);
    }

    /// The theme request field drives token resolution end to end: dark
    /// mode flips `bg-background`, and an override rebrands `bg-primary`.
    /// The theme request field drives token resolution end to end: dark
    /// mode flips `bg-background` (and the primary ink on top of it), and
    /// an override rebrands `bg-primary`. Assertions target the *minified*
    /// stylesheet values -- the minifier rewrites `hsl(...)` tokens to hex --
    /// so the expected strings are the hex forms: shadcn's dark background
    /// hsl(222.2, 84%, 4.9%) is #020817, dark primary hsl(210, 40%, 98%)
    /// is #f8fafc.
    #[test]
    fn render_html_with_theme_resolves_tokens_end_to_end() {
        let source = "%p.bg-background.text-primary Body";
        let theme = Theme::dark();
        let html = render_html_with_theme(source, &json!({}), &NoPartials, &theme).unwrap();
        assert!(html.contains("#020817"), "{html}");
        assert!(html.contains("#f8fafc"), "{html}");

        let mut branded = Theme::light();
        branded.overrides.insert("primary".into(), "#2563eb".into());
        let html = render_html_with_theme(source, &json!({}), &NoPartials, &branded).unwrap();
        assert!(html.to_lowercase().contains("#2563eb"), "{html}");
    }

    #[test]
    fn theme_from_json_accepts_mode_and_overrides_and_rejects_junk() {
        let theme = theme_from_json(&serde_json::json!({
            "mode": "dark",
            "overrides": { "primary": "#2563eb" }
        }))
        .unwrap();
        assert_eq!(theme.mode, ThemeMode::Dark);
        assert_eq!(theme.token("primary"), Some("#2563eb"));

        assert_eq!(theme_from_json(&json!({})).unwrap(), Theme::light());
        assert!(theme_from_json(&json!("dark")).is_err());
        assert!(theme_from_json(&json!({ "mode": "sepia" })).is_err());
        assert!(theme_from_json(&json!({ "overrides": { "primary": 3 } })).is_err());
    }
}
