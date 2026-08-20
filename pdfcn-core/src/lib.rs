mod data;
mod error;
mod html_render;
mod page;

pub use data::{load_data, DataFormat};
pub use error::CoreError;
pub use page::{Orientation, PageConfig, PageSize};
pub use pdfcn_template::{NoPartials, PartialLoader};

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use printpdf::html::rust_fontconfig::{FcFontCache, FcParseFontBytes};
use printpdf::html::SharedFontPool;
use printpdf::{Base64OrRaw, GeneratePdfOptions, PdfDocument, PdfSaveOptions};
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, HashMap};

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
    ("Inter", include_bytes!("../assets/fonts/inter/Inter-Regular.ttf")),
    ("Inter Bold", include_bytes!("../assets/fonts/inter/Inter-Bold.ttf")),
    ("Inter Italic", include_bytes!("../assets/fonts/inter/Inter-Italic.ttf")),
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

/// Runs the full FR-1/FR-2/FR-3 pipeline: parses `source`, evaluates it
/// against `data`, expands components, and returns a complete HTML
/// document string with an embedded, minified, print-safe stylesheet.
pub fn render_html(
    source: &str,
    data: &JsonValue,
    loader: &dyn PartialLoader,
) -> Result<String, CoreError> {
    let doc = pdfcn_parser::parse_document(source)?;
    let resolved = pdfcn_template::evaluate(&doc, data, loader)?;
    let body = html_render::render_body(&resolved)?;
    let body_html = body.clone().into_string();
    let stylesheet = pdfcn_styles::build_stylesheet(&body_html);
    Ok(html_render::wrap_document(&body, &stylesheet))
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
    let mut fonts = BTreeMap::new();
    let mut raw_fonts: BTreeMap<String, &[u8]> = BTreeMap::new();
    for (family, bytes) in BUILTIN_FONTS {
        fonts.insert(family.to_string(), Base64OrRaw::Raw(bytes.to_vec()));
        raw_fonts.insert(family.to_string(), bytes);
    }
    for (family, bytes) in custom_fonts {
        fonts.insert(family.clone(), Base64OrRaw::Raw(bytes.clone()));
        raw_fonts.insert(family.clone(), bytes);
    }
    let pool = font_pool(&raw_fonts);

    let mut warnings = Vec::new();
    let doc = PdfDocument::from_html_with_cache(
        html,
        &Default::default(),
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
    let html = render_html(source, data, loader)?;
    render_pdf_with_fonts(&html, page, custom_fonts)
}

/// Convenience for the CLI: reads `template_path` and `data_path` from
/// disk, using the template's directory as the base for `- include`.
pub fn render_files(
    template_path: &Path,
    data_path: &Path,
    page: &PageConfig,
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
    let loader = FsPartialLoader::new(base_dir);
    render(&source, &data, page, &loader)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
        assert!(html.contains("border-radius:1rem"));
        assert!(html.contains("box-shadow"));

        let bytes = render_pdf(&html, &PageConfig::default()).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
    }

    #[test]
    fn interactive_only_component_is_rejected_with_an_explicit_error() {
        let source = "%Dialog";
        let err = render_html(source, &json!({}), &NoPartials).unwrap_err();
        assert!(err
            .to_string()
            .contains("interactive-only, unsupported in static PDF output"));
    }
}
