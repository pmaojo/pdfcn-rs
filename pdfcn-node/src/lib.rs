//! FR-5 (optional): NAPI-RS bindings so a Next.js/Node app can call
//! `pdfcn-core` in-process — no child process, no HTTP hop.

#![deny(clippy::all)]

use napi::bindgen_prelude::*;
use napi_derive::napi;
use pdfcn_core::{render, NoPartials, Orientation, PageConfig, PageSize};

#[napi(object)]
pub struct PageOptions {
    /// "a4" | "letter" | "<width>x<height>" (mm)
    pub size: Option<String>,
    /// "portrait" | "landscape"
    pub orientation: Option<String>,
    pub margin_mm: Option<f64>,
}

fn to_page_config(opts: Option<PageOptions>) -> PageConfig {
    let default = PageConfig::default();
    let Some(opts) = opts else { return default };

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
    PageConfig {
        size,
        orientation,
        margin_mm: opts.margin_mm.map(|m| m as f32).unwrap_or(default.margin_mm),
    }
}

/// Renders a `.haml`-style `template` against a JSON-encoded `data_json`
/// context and returns PDF bytes.
#[napi]
pub fn render_pdf(template: String, data_json: String, options: Option<PageOptions>) -> Result<Buffer> {
    let data: serde_json::Value = serde_json::from_str(&data_json)
        .map_err(|e| Error::from_reason(format!("invalid data_json: {e}")))?;
    let page = to_page_config(options);
    let bytes = render(&template, &data, &page, &NoPartials)
        .map_err(|e| Error::from_reason(format!("render error: {e}")))?;
    Ok(bytes.into())
}
