//! FR-5 (optional): NAPI-RS bindings so a Next.js/Node app can call
//! `pdfcn-core` in-process — no child process, no HTTP hop.

#![deny(clippy::all)]

use napi::bindgen_prelude::*;
use napi_derive::napi;
use pdfcn_core::{
    render, DocumentMetadata, ImageFormat, ImageOptimization, NoPartials, Orientation, PageConfig,
    PageSize, RenderOptions, Theme,
};

#[napi(object)]
pub struct PageOptions {
    /// "a4" | "letter" | "<width>x<height>" (mm)
    pub size: Option<String>,
    /// "portrait" | "landscape"
    pub orientation: Option<String>,
    pub margin_mm: Option<f64>,
    /// "light" | "dark" -- picks shadcn's built-in token table
    pub theme: Option<String>,
    /// Repeated on every page (see `skip_first_page`). Currently has no
    /// visible effect -- printpdf 0.12.6 doesn't render it yet; accepted
    /// and wired through for forward compatibility (see
    /// `pdfcn_core::RenderOptions::header_text`'s doc comment).
    pub header_text: Option<String>,
    /// Repeated on every page. Same current no-op as `header_text`.
    pub footer_text: Option<String>,
    /// Appends "Page X of Y" to the footer. Same current no-op.
    pub show_page_numbers: Option<bool>,
    /// Suppresses header/footer/page-numbers on the first page (a cover).
    /// Moot while the above render nothing.
    pub skip_first_page: Option<bool>,
    /// JPEG-family compression quality, 0.0-1.0 (printpdf's own default:
    /// 0.85). Compression is already on; this only tunes it.
    pub image_quality: Option<f64>,
    /// Size budget per embedded image, e.g. "300kb" or "2MB" (printpdf's
    /// own default: "2MB")
    pub image_max_size: Option<String>,
    /// Force every embedded image to greyscale before compressing
    pub image_greyscale: Option<bool>,
    /// "auto" | "jpeg" | "lossless" | "raw"
    pub image_format: Option<String>,
    /// PDF document title metadata
    pub title: Option<String>,
    /// PDF document author metadata
    pub author: Option<String>,
    /// PDF document subject metadata
    pub subject: Option<String>,
    /// PDF document keywords metadata
    pub keywords: Option<Vec<String>>,
}

fn to_render_options(opts: Option<PageOptions>) -> Result<RenderOptions> {
    let Some(opts) = opts else {
        return Ok(RenderOptions::default());
    };

    let default = PageConfig::default();
    let size = match opts.size.as_deref() {
        Some("letter") => PageSize::Letter,
        Some("a4") | None => default.size,
        Some(custom) => custom
            .split_once('x')
            .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
            .map(|(width_mm, height_mm)| PageSize::Custom {
                width_mm,
                height_mm,
            })
            .unwrap_or(default.size),
    };
    let orientation = match opts.orientation.as_deref() {
        Some("landscape") => Orientation::Landscape,
        _ => default.orientation,
    };
    let page = PageConfig {
        size,
        orientation,
        margin_mm: opts
            .margin_mm
            .map(|m| m as f32)
            .unwrap_or(default.margin_mm),
    };

    let theme = match opts.theme.as_deref() {
        None | Some("light") => Theme::light(),
        Some("dark") => Theme::dark(),
        Some(other) => {
            return Err(Error::from_reason(format!(
                "invalid theme \"{other}\", expected \"light\" or \"dark\""
            )))
        }
    };

    let image_format = match opts.image_format.as_deref() {
        None => None,
        Some("auto") => Some(ImageFormat::Auto),
        Some("jpeg") => Some(ImageFormat::Jpeg),
        Some("lossless") => Some(ImageFormat::Lossless),
        Some("raw") => Some(ImageFormat::Raw),
        Some(other) => {
            return Err(Error::from_reason(format!(
                "invalid image_format \"{other}\", expected \"auto\", \"jpeg\", \"lossless\", or \"raw\""
            )))
        }
    };
    let image_optimization = if opts.image_quality.is_some()
        || opts.image_max_size.is_some()
        || opts.image_greyscale.is_some()
        || image_format.is_some()
    {
        Some(ImageOptimization {
            quality: opts.image_quality.map(|q| q as f32),
            max_size: opts.image_max_size,
            greyscale: opts.image_greyscale,
            format: image_format,
        })
    } else {
        None
    };

    Ok(RenderOptions {
        page,
        theme,
        header_text: opts.header_text,
        footer_text: opts.footer_text,
        show_page_numbers: opts.show_page_numbers.unwrap_or(false),
        skip_first_page: opts.skip_first_page.unwrap_or(false),
        image_optimization,
        metadata: DocumentMetadata {
            title: opts.title,
            author: opts.author,
            subject: opts.subject,
            keywords: opts.keywords.unwrap_or_default(),
            producer: None,
        },
    })
}

/// Renders a `.haml`-style `template` against a JSON-encoded `data_json`
/// context and returns PDF bytes.
#[napi]
pub fn render_pdf(
    template: String,
    data_json: String,
    options: Option<PageOptions>,
) -> Result<Buffer> {
    let data: serde_json::Value = serde_json::from_str(&data_json)
        .map_err(|e| Error::from_reason(format!("invalid data_json: {e}")))?;
    let render_options = to_render_options(options)?;
    let bytes = render(&template, &data, &NoPartials, &render_options)
        .map_err(|e| Error::from_reason(format!("render error: {e}")))?;
    Ok(bytes.into())
}
