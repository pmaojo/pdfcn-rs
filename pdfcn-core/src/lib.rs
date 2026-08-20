mod data;
mod error;
mod html_render;
mod page;

pub use data::{load_data, DataFormat};
pub use error::CoreError;
pub use page::{Orientation, PageConfig, PageSize};
pub use pdfcn_template::{NoPartials, PartialLoader};

use std::path::{Path, PathBuf};

use printpdf::{Base64OrRaw, GeneratePdfOptions, PdfDocument, PdfSaveOptions};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;

/// The default document font, embedded so PDF generation has no dependency
/// on fonts being installed on the host (NFR-3). Serverless runtimes like
/// Vercel Functions / AWS Lambda ship no system fonts at all, so relying on
/// `printpdf`'s fontconfig-based system font scan crashes there even though
/// it works on a developer machine with fonts installed. See
/// `assets/fonts/LICENSE` for the embedded font's license.
const DEFAULT_FONT: &[u8] = include_bytes!("../assets/fonts/DejaVuSans.ttf");
const DEFAULT_FONT_BOLD: &[u8] = include_bytes!("../assets/fonts/DejaVuSans-Bold.ttf");
const DEFAULT_FONT_FAMILY: &str = "DejaVu Sans";

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
    let body = html_render::render_body(&resolved);
    let body_html = body.clone().into_string();
    let stylesheet = pdfcn_styles::build_stylesheet(&body_html);
    Ok(html_render::wrap_document(&body, &stylesheet))
}

/// Renders a complete HTML document (as produced by [`render_html`]) to PDF
/// bytes in memory (FR-4), honoring `page`'s size/orientation/margins. Pure
/// Rust: no headless browser, no external process, Vercel-safe (NFR-3).
pub fn render_pdf(html: &str, page: &PageConfig) -> Result<Vec<u8>, CoreError> {
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
    fonts.insert(
        DEFAULT_FONT_FAMILY.to_string(),
        Base64OrRaw::Raw(DEFAULT_FONT.to_vec()),
    );
    fonts.insert(
        format!("{DEFAULT_FONT_FAMILY} Bold"),
        Base64OrRaw::Raw(DEFAULT_FONT_BOLD.to_vec()),
    );

    let mut warnings = Vec::new();
    let doc = PdfDocument::from_html(html, &Default::default(), &fonts, &options, &mut warnings)
        .map_err(CoreError::Render)?;

    let mut save_warnings = Vec::new();
    Ok(doc.save(&PdfSaveOptions::default(), &mut save_warnings))
}

/// End-to-end: `.haml` source + data context + page config -> PDF bytes.
pub fn render(
    source: &str,
    data: &JsonValue,
    page: &PageConfig,
    loader: &dyn PartialLoader,
) -> Result<Vec<u8>, CoreError> {
    let html = render_html(source, data, loader)?;
    render_pdf(&html, page)
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
}
