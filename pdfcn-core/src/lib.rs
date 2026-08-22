mod assets;
mod data;
mod error;
mod html_render;
mod page;

pub use data::{load_data, DataFormat};
pub use error::CoreError;
pub use page::{Orientation, PageConfig, PageSize};
pub use pdfcn_styles::{Theme, ThemeMode};
pub use pdfcn_template::{EvalError, NoPartials, PartialLoader};

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use printpdf::html::rust_fontconfig::{FcFontCache, FcParseFontBytes};
use printpdf::html::SharedFontPool;
use printpdf::{Base64OrRaw, GeneratePdfOptions, PdfDocument, PdfSaveOptions};
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

/// Resolves `- include "partials/x.haml"` against a base directory on disk.
pub struct FsPartialLoader {
    base_dir: PathBuf,
}

impl FsPartialLoader {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
        }
    }
}

impl PartialLoader for FsPartialLoader {
    fn load(&self, path: &str) -> Result<pdfcn_parser::Document, pdfcn_template::EvalError> {
        let full = self.base_dir.join(path);
        let source = std::fs::read_to_string(&full)
            .map_err(|_| pdfcn_template::EvalError::PartialNotFound(path.to_string()))?;
        pdfcn_parser::parse_document(&source)
            .map_err(|_| pdfcn_template::EvalError::PartialNotFound(path.to_string()))
    }
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

/// Bounded process-wide cache of parsed template ASTs, keyed by a hash of
/// the source. Server-side callers rendering many documents from one
/// template (a batch endpoint, a monthly-statements loop) pay the lex/parse
/// cost once instead of per document; one-off renders pay one extra hash.
/// Entries are evicted FIFO past [`PARSE_CACHE_CAP`] so memory stays
/// bounded regardless of how many distinct templates a long-lived process
/// sees. Only successful parses are cached -- a malformed template keeps
/// failing identically.
const PARSE_CACHE_CAP: usize = 128;
type ParseCache = Mutex<(HashMap<u64, pdfcn_parser::Document>, VecDeque<u64>)>;
static PARSE_CACHE: OnceLock<ParseCache> = OnceLock::new();

fn cached_parse(source: &str) -> Result<pdfcn_parser::Document, CoreError> {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    let key = hasher.finish();
    if let Some(cache) = PARSE_CACHE.get() {
        if let Ok(guard) = cache.lock() {
            if let Some(doc) = guard.0.get(&key) {
                return Ok(doc.clone());
            }
        }
    }
    let doc = pdfcn_parser::parse_document(source)?;
    let cache = PARSE_CACHE.get_or_init(|| Mutex::new((HashMap::new(), VecDeque::new())));
    if let Ok(mut guard) = cache.lock() {
        if !guard.0.contains_key(&key) {
            while guard.0.len() >= PARSE_CACHE_CAP {
                match guard.1.pop_front() {
                    Some(oldest) => {
                        guard.0.remove(&oldest);
                    }
                    None => break,
                }
            }
            guard.0.insert(key, doc.clone());
            guard.1.push_back(key);
        }
    }
    Ok(doc)
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
    let doc = cached_parse(source)?;
    let resolved = pdfcn_template::evaluate(&doc, data, loader)?;
    let body = html_render::render_body(&resolved)?;
    // The layout engine ignores the CSS `gap` declaration, so flex/grid
    // `gap-*` utilities are rewritten into equivalent margins on children
    // before the stylesheet is built — which also means the injected
    // `mr-*`/`mb-*` classes are picked up by the same used-classes scan as
    // every other utility (see pdfcn_styles::rewrite_gaps).
    let body_html = pdfcn_styles::rewrite_gaps(&body.into_string());
    let stylesheet = pdfcn_styles::build_stylesheet_with_theme(&body_html, theme);
    Ok(html_render::wrap_document_str(&body_html, &stylesheet))
}

/// Renders a complete HTML document (as produced by [`render_html`]) to PDF
/// bytes in memory (FR-4), honoring `page`'s size/orientation/margins. Pure
/// Rust: no headless browser, no external process, Vercel-safe (NFR-3).
/// Uses only the built-in typefaces ([`BUILTIN_FONTS`]); for a document that
/// needs a caller-supplied font (a brand typeface), use
/// [`render_pdf_with_fonts`].
pub fn render_pdf(html: &str, page: &PageConfig) -> Result<Vec<u8>, CoreError> {
    render_pdf_with_fonts(html, page, &BTreeMap::new())
}

/// Like [`render_pdf`], but `custom_fonts` (family name -> TTF/OTF bytes) is
/// embedded alongside the built-in typefaces, so the document's CSS can
/// reference `font-family: "<name>"` for any family name used as a key. A
/// custom entry sharing a built-in family name (e.g. `"Inter"`) overrides
/// that built-in weight/style rather than adding a duplicate.
pub fn render_pdf_with_fonts(
    html: &str,
    page: &PageConfig,
    custom_fonts: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<u8>, CoreError> {
    render_pdf_with_assets(html, page, custom_fonts, &BTreeMap::new())
}

/// Like [`render_pdf_with_fonts`], but `images` (the `<img src="...">` value
/// -> raw image bytes, JPEG/PNG) is embedded alongside the document, so an
/// `%img(src="cover.jpg")` element resolves to that image rather than a
/// broken-image placeholder. Matches `custom_fonts`' "supply the bytes
/// yourself" convention -- no network access at render time (NFR-3).
pub fn render_pdf_with_assets(
    html: &str,
    page: &PageConfig,
    custom_fonts: &BTreeMap<String, Vec<u8>>,
    images: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<u8>, CoreError> {
    // Post-render asset preparation runs here — the one choke point every
    // entry point (CLI, HTTP API, napi bindings) funnels through — so QR
    // placeholders get real bytes and object-fit:cover images get cropped
    // before the layout engine ever sees them.
    let mut prepared_images = images.clone();
    let html = assets::prepare_assets(html, &mut prepared_images);
    let html = html.as_str();

    let (width_mm, height_mm) = page.page_size_mm();
    let options = GeneratePdfOptions {
        page_width: Some(width_mm),
        page_height: Some(height_mm),
        margin_top: Some(page.margin_mm),
        margin_right: Some(page.margin_mm),
        margin_bottom: Some(page.margin_mm),
        margin_left: Some(page.margin_mm),
        ..Default::default()
    };
    // Embed only the fonts the document can reach. Every built-in family
    // is ~300-500KB of TTF, and a typical invoice uses two of the twelve;
    // embedding all of them on every render was the single largest line
    // item in output size. A family survives if the document's CSS names
    // it -- or if it's Inter, the default body font `wrap_document` always
    // sets. Caller-supplied fonts are always kept (they were supplied
    // deliberately, and their bytes aren't ours to second-guess).
    let used_families = used_font_families(html);
    let mut fonts = BTreeMap::new();
    let mut raw_fonts: BTreeMap<String, &[u8]> = BTreeMap::new();
    for (family, bytes) in BUILTIN_FONTS {
        if *family != DEFAULT_FONT_FAMILY && !used_families.contains(*family) {
            continue;
        }
        fonts.insert(family.to_string(), Base64OrRaw::Raw(bytes.to_vec()));
        raw_fonts.insert(family.to_string(), bytes);
    }
    for (family, bytes) in custom_fonts {
        fonts.insert(family.clone(), Base64OrRaw::Raw(bytes.clone()));
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

    let mut image_map: BTreeMap<String, Base64OrRaw> = BTreeMap::new();
    for (src, bytes) in &prepared_images {
        image_map.insert(src.clone(), Base64OrRaw::Raw(bytes.clone()));
    }

    let mut warnings = Vec::new();
    let doc = PdfDocument::from_html_with_cache(
        html,
        &image_map,
        &fonts,
        &options,
        &mut warnings,
        Some(pool),
    )
    .map_err(CoreError::Render)?;

    let mut save_warnings = Vec::new();
    Ok(doc.save(&PdfSaveOptions::default(), &mut save_warnings))
}

/// End-to-end: `.haml` source + data context + page config -> PDF bytes.
/// Uses only the built-in typefaces; see [`render_with_fonts`] for a
/// caller-supplied font.
pub fn render(
    source: &str,
    data: &JsonValue,
    page: &PageConfig,
    loader: &dyn PartialLoader,
) -> Result<Vec<u8>, CoreError> {
    render_with_fonts(source, data, page, loader, &BTreeMap::new())
}

/// Like [`render`], but `custom_fonts` (family name -> TTF/OTF bytes) is
/// embedded alongside the built-in typefaces. See [`render_pdf_with_fonts`].
pub fn render_with_fonts(
    source: &str,
    data: &JsonValue,
    page: &PageConfig,
    loader: &dyn PartialLoader,
    custom_fonts: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<u8>, CoreError> {
    render_with_assets(source, data, page, loader, custom_fonts, &BTreeMap::new())
}

/// Like [`render_with_fonts`], but also embeds `images` (see
/// [`render_pdf_with_assets`]).
pub fn render_with_assets(
    source: &str,
    data: &JsonValue,
    page: &PageConfig,
    loader: &dyn PartialLoader,
    custom_fonts: &BTreeMap<String, Vec<u8>>,
    images: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<u8>, CoreError> {
    let html = render_html(source, data, loader)?;
    render_pdf_with_assets(&html, page, custom_fonts, images)
}

/// Convenience for the CLI: reads `template_path` and `data_path` from
/// disk, using the template's directory as the base for `- include`. Every
/// `<img src="...">` (from `%img` or a component like `%Card`'s `image`
/// attribute) whose `src` is a relative filesystem path -- not `http(s):`
/// or `data:`, which this pipeline never fetches (NFR-3) -- is read from
/// disk relative to `template_path`'s directory and embedded automatically,
/// so `pdfcn build` composes real images into the PDF without the caller
/// having to call the lower-level `render_with_assets` API by hand.
///
/// `http(s):` sources are left unresolved here; use
/// [`render_files_with_remote_images`] to also resolve those, via a
/// caller-supplied fetcher.
pub fn render_files(
    template_path: &Path,
    data_path: &Path,
    page: &PageConfig,
) -> Result<Vec<u8>, CoreError> {
    render_files_with_remote_images(template_path, data_path, page, None)
}

/// A caller-supplied `http(s):` image fetcher for
/// [`render_files_with_remote_images`]: given a `src` URL, returns its
/// bytes, or `None` to leave that `<img>` unresolved.
pub type RemoteImageFetcher = dyn Fn(&str) -> Option<Vec<u8>>;

/// Like [`render_files`], but a `src="https://..."` (or `http://`) that
/// isn't resolved by a local file is instead passed to `fetch_remote`,
/// which returns the image bytes (or `None` to leave it unresolved, same
/// as a local file that isn't found). This is how a client's own image
/// URLs (e.g. from a CMS or a stock-photo host) get composed into a PDF
/// without a manual base64-encode-and-embed round trip -- while keeping
/// [`render_pdf`]/[`render_with_assets`] themselves free of any network
/// access (NFR-3): only a caller who explicitly supplies `fetch_remote`
/// (the CLI's `--fetch-remote-images`, opt-in) makes outbound requests.
pub fn render_files_with_remote_images(
    template_path: &Path,
    data_path: &Path,
    page: &PageConfig,
    fetch_remote: Option<&RemoteImageFetcher>,
) -> Result<Vec<u8>, CoreError> {
    let source = std::fs::read_to_string(template_path)
        .map_err(|e| CoreError::Render(format!("reading {template_path:?}: {e}")))?;
    let data_source = std::fs::read_to_string(data_path)
        .map_err(|e| CoreError::Render(format!("reading {data_path:?}: {e}")))?;
    let format = data_path
        .extension()
        .and_then(|e| e.to_str())
        .and_then(DataFormat::from_extension)
        .unwrap_or(DataFormat::Json);
    let data = load_data(&data_source, format)?;
    let base_dir = template_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let loader = FsPartialLoader::new(base_dir.clone());
    let html = render_html(&source, &data, &loader)?;
    let images = load_images(&html, &base_dir, fetch_remote);
    render_pdf_with_assets(&html, page, &BTreeMap::new(), &images)
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

/// Reads every image `src` referenced in `html`: a local (non-`http(s)`,
/// non-`data:`) path from disk relative to `base_dir`, or -- when
/// `fetch_remote` is given -- an `http(s):` source via that callback. A
/// `src` left unresolved either way (not found on disk, no fetcher given,
/// or the fetcher returns `None`) is silently skipped (same convention as
/// an unknown utility class: it degrades, it doesn't fail the render) --
/// `printpdf` renders it as a broken-image placeholder. `data:` sources are
/// always skipped here since `printpdf` resolves those itself.
fn load_images(
    html: &str,
    base_dir: &Path,
    fetch_remote: Option<&RemoteImageFetcher>,
) -> BTreeMap<String, Vec<u8>> {
    let mut images = BTreeMap::new();
    for src in img_srcs(html) {
        if src.starts_with("data:") {
            continue;
        }
        if src.starts_with("http://") || src.starts_with("https://") {
            if let Some(bytes) = fetch_remote.and_then(|fetch| fetch(&src)) {
                images.insert(src, bytes);
            }
            continue;
        }
        if let Ok(bytes) = std::fs::read(base_dir.join(&src)) {
            images.insert(src, bytes);
        }
    }
    images
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
    fn load_images_skips_remote_and_data_uri_sources_without_a_fetcher() {
        let html = concat!(
            r#"<img src="https://example.com/a.png">"#,
            r#"<img src="data:image/png;base64,AAAA">"#,
            r#"<img src="does-not-exist-on-disk.png">"#,
        );
        let images = load_images(html, Path::new("."), None);
        assert!(images.is_empty());
    }

    #[test]
    fn load_images_reads_a_relative_src_from_the_base_dir() {
        let dir = std::env::temp_dir().join(format!(
            "pdfcn-core-test-{}-{}",
            std::process::id(),
            "load_images_reads_a_relative_src_from_the_base_dir"
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("cover.png"), b"not-really-a-png-just-bytes").unwrap();

        let html = r#"<img src="cover.png">"#;
        let images = load_images(html, &dir, None);

        assert_eq!(
            images.get("cover.png").map(Vec::as_slice),
            Some(b"not-really-a-png-just-bytes".as_slice())
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_images_uses_the_fetcher_for_http_s_sources_and_skips_data_uris() {
        let html = concat!(
            r#"<img src="https://example.com/a.png">"#,
            r#"<img src="data:image/png;base64,AAAA">"#,
            r#"<img src="https://example.com/missing.png">"#,
        );
        let fetch = |src: &str| -> Option<Vec<u8>> {
            (src == "https://example.com/a.png").then(|| b"fetched-bytes".to_vec())
        };
        let images = load_images(html, Path::new("."), Some(&fetch));

        assert_eq!(
            images.get("https://example.com/a.png").map(Vec::as_slice),
            Some(b"fetched-bytes".as_slice())
        );
        assert!(!images.contains_key("data:image/png;base64,AAAA"));
        assert!(!images.contains_key("https://example.com/missing.png"));
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
        let bytes = render(source, &data, &PageConfig::default(), &NoPartials).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
    }

    /// Wave 0's shadcn tokens (semantic theme colors, full neutral/accent
    /// scales, radius/shadow scales) resolve to plain CSS declarations, but
    /// the pipeline still has to carry that CSS through `lightningcss`
    /// minification and `printpdf`'s HTML/CSS layout engine end to end.
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

        let bytes = render_pdf(&html, &PageConfig::default()).unwrap();
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
        let bytes = render_pdf(&html, &PageConfig::default()).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
    }

    #[test]
    fn a_serif_document_keeps_its_named_font_embedded() {
        let source = "%p(style=\"font-family:'Source Serif 4'\") Serif body";
        let html = render_html(source, &json!({}), &NoPartials).unwrap();
        assert!(used_font_families(&html).contains("Source Serif 4"));
        let bytes = render_pdf(&html, &PageConfig::default()).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
    }

    /// The parse cache must be transparent: identical renders before and
    /// after caching (including a second render of the same source).
    #[test]
    fn cached_parse_is_transparent_for_repeated_sources() {
        let source = "%p Cache me {{ n }}";
        let first = cached_parse(source).unwrap();
        let second = cached_parse(source).unwrap();
        assert_eq!(first, second);
        // And the cached AST still evaluates correctly.
        let resolved = pdfcn_template::evaluate(&second, &json!({ "n": 7 }), &NoPartials).unwrap();
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

    /// The theme request field drives token resolution end to end: dark
    /// mode flips `bg-background`, and an override rebrands `bg-primary`.
    /// The theme request field drives token resolution end to end: dark
    /// mode flips `bg-background` (and the primary ink on top of it), and
    /// an override rebrands `bg-primary`. Assertions target the *minified*
    /// stylesheet values -- lightningcss rewrites `hsl(...)` tokens to hex --
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
