//! Everything needed to lay out and save a document to PDF bytes, beyond
//! the already-rendered HTML: page geometry, the semantic theme used one
//! stage earlier to build its stylesheet, and the printpdf-level knobs this
//! crate exposes as pdfcn's own vocabulary -- [`PageConfig`]/[`Theme`]
//! already follow that convention (never leak a `printpdf` type through
//! this crate's public surface); the types below extend it to printpdf's
//! page-decoration and image-compression knobs.
//!
//! `render_pdf_with_assets` (the one place every entry point funnels
//! through) used to take a bare `page: &PageConfig`, and every option this
//! crate wanted to add on top would have meant widening that same
//! function's argument list, and the 7 wrappers around it, again. This
//! struct is that consolidation, done once, ahead of adding anything to it.

use crate::page::PageConfig;
use pdfcn_styles::Theme;

/// The full set of rendering choices threaded through the `render*` family:
/// page geometry and theme (consumed at the HTML-generation stage), plus
/// the printpdf-level knobs consumed when that HTML is laid out and saved
/// to PDF bytes. `Default` reproduces the pipeline's behavior from before
/// this struct existed -- a light-themed A4 document with no header/
/// footer/page-numbers and no metadata -- so introducing it changes no
/// existing output.
#[derive(Debug, Clone, Default)]
pub struct RenderOptions {
    pub page: PageConfig,
    pub theme: Theme,
    /// Wired through to `GeneratePdfOptions::header_text` -- printpdf's own
    /// declared, documented mechanism for a repeated page header.
    ///
    /// **Currently has no visible effect.** Verified directly against
    /// printpdf 0.12.6 / azul-layout 0.0.14: the field reaches
    /// `FakePageConfig` (`printpdf::html::build_page_config`), and
    /// azul-layout does contain header/footer-drawing code
    /// (`solver3::display_list::paginate_pages_impl`) -- but the PDF path
    /// this crate actually calls, `layout_document_paged_v2`
    /// (`solver3::paged_layout`), never calls it; that function is only
    /// referenced from a doc comment elsewhere in the crate. A synthetic
    /// document with only this field set, no other content, confirmed the
    /// text never reaches the rendered page on any of 12 forced pages.
    /// This is an upstream gap, not a wiring bug on this side -- kept wired
    /// (rather than removed) so a future printpdf patch starts working with
    /// no change here; see also `%PageFooter`'s own doc comment.
    pub header_text: Option<String>,
    /// Same as `header_text`, but for the bottom of the page. Same current
    /// no-op.
    pub footer_text: Option<String>,
    /// Appends "Page X of Y" to the footer. Same current no-op.
    pub show_page_numbers: bool,
    /// Suppresses header/footer/page-numbers on the first page -- the
    /// usual shape for a cover page. Moot while the three fields above
    /// render nothing.
    pub skip_first_page: bool,
    /// Governs `doc.save`'s image re-encoding. `None` keeps printpdf's own
    /// default (quality 0.85, a 2MB cap, format chosen automatically) --
    /// compression is already on by default; this is what makes it
    /// tunable rather than what turns it on.
    pub image_optimization: Option<ImageOptimization>,
    pub metadata: DocumentMetadata,
}

impl RenderOptions {
    /// The common case: everything but page geometry stays at its default.
    pub fn from_page(page: PageConfig) -> Self {
        Self {
            page,
            ..Self::default()
        }
    }
}

impl From<PageConfig> for RenderOptions {
    fn from(page: PageConfig) -> Self {
        Self::from_page(page)
    }
}

/// The compression family for an embedded image, mirroring the practically
/// useful subset of printpdf's `ImageCompression` (its legacy/exotic
/// variants -- JPEG2000, LZW, run-length -- aren't exposed here).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImageFormat {
    /// Let printpdf choose based on the image's own content.
    #[default]
    Auto,
    /// Force lossy JPEG compression.
    Jpeg,
    /// Force lossless (flate) compression -- larger, pixel-exact.
    Lossless,
    /// Store raw, uncompressed pixels.
    Raw,
}

/// Tunes `PdfSaveOptions.image_optimization` (see [`RenderOptions::image_optimization`]).
/// Every field left `None` keeps printpdf's own default for that setting.
#[derive(Debug, Clone, Default)]
pub struct ImageOptimization {
    /// 0.0-1.0; meaningful for JPEG-family compression only.
    pub quality: Option<f32>,
    /// A size budget like `"300kb"` or `"2MB"` -- printpdf parses the unit.
    pub max_size: Option<String>,
    /// Force-converts to greyscale before compressing.
    pub greyscale: Option<bool>,
    pub format: Option<ImageFormat>,
}

/// Plain PDF document metadata, set directly on the saved document rather
/// than smuggled through the template's HTML: printpdf's own
/// `<meta name="pdf.metadata.*">` mechanism (`extract_html_config`/
/// `apply_html_config`) is never invoked by `from_html_with_cache` -- it
/// exists in `printpdf::html` but nothing in printpdf's own render path
/// calls it -- and reimplementing that glue ourselves would just be a
/// second way to pass in the same typed values every other pdfcn-core
/// option already uses (`PageConfig`, `Theme`, fonts, images).
///
/// Every field left at its default (`None`/empty) keeps printpdf's own
/// blank default for that field.
#[derive(Debug, Clone, Default)]
pub struct DocumentMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Vec<String>,
    pub producer: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_reproduces_the_pre_existing_pipeline_behavior() {
        let options = RenderOptions::default();
        assert_eq!(options.page, PageConfig::default());
        assert_eq!(options.theme, Theme::light());
        assert_eq!(options.header_text, None);
        assert_eq!(options.footer_text, None);
        assert!(!options.show_page_numbers);
        assert!(!options.skip_first_page);
        assert!(options.image_optimization.is_none());
    }

    #[test]
    fn from_page_leaves_everything_else_at_its_default() {
        let page = PageConfig {
            margin_mm: 20.0,
            ..PageConfig::default()
        };
        let options = RenderOptions::from_page(page);
        assert_eq!(options.page.margin_mm, 20.0);
        assert_eq!(options.theme, Theme::light());
    }
}
