//! FR-5: a native `/api/generate-pdf` handler for AWS Lambda / Vercel
//! Functions (cargo-lambda-compatible: builds to a `bootstrap` binary).
//! No Chromium, no dynamic system dependencies (NFR-3) — the whole
//! request/response cycle is `pdfcn-core` plus this thin HTTP adapter.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use lambda_http::{run, service_fn, Body, Error, Request, RequestPayloadExt, Response};
use pdfcn_core::{render_with_fonts, NoPartials, Orientation, PageConfig, PageSize};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct PageConfigDto {
    #[serde(default)]
    size: Option<String>,
    #[serde(default)]
    orientation: Option<String>,
    #[serde(default)]
    margin_mm: Option<f32>,
}

impl PageConfigDto {
    fn into_page_config(self) -> PageConfig {
        let default = PageConfig::default();
        let size = match self.size.as_deref() {
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
        let orientation = match self.orientation.as_deref() {
            Some("landscape") => Orientation::Landscape,
            _ => default.orientation,
        };
        PageConfig {
            size,
            orientation,
            margin_mm: self.margin_mm.unwrap_or(default.margin_mm),
        }
    }
}

#[derive(Deserialize)]
struct GenerateRequest {
    /// HAML-like template source.
    template: String,
    /// Data context to render it against.
    #[serde(default = "default_data")]
    data: serde_json::Value,
    #[serde(default)]
    page: Option<PageConfigDto>,
    /// Filename suggested via Content-Disposition.
    #[serde(default = "default_filename")]
    filename: String,
    /// Caller-supplied fonts: family name -> base64-encoded TTF/OTF bytes.
    /// Embedded alongside (and, per family name, overriding) the built-in
    /// typefaces, so `template` can use `font-family: "<name>"` for any key
    /// given here -- e.g. a client's brand font.
    #[serde(default)]
    fonts: std::collections::HashMap<String, String>,
}

fn default_data() -> serde_json::Value {
    serde_json::Value::Object(Default::default())
}

fn default_filename() -> String {
    "document.pdf".to_string()
}

async fn handler(event: Request) -> Result<Response<Body>, Error> {
    let req: GenerateRequest = match event.payload::<GenerateRequest>() {
        Ok(Some(req)) => req,
        Ok(None) => {
            return Ok(Response::builder()
                .status(400)
                .body(Body::from("missing JSON body: {\"template\": ..., \"data\": ...}"))?)
        }
        Err(e) => {
            return Ok(Response::builder()
                .status(400)
                .body(Body::from(format!("invalid request body: {e}")))?)
        }
    };

    let page = req
        .page
        .map(PageConfigDto::into_page_config)
        .unwrap_or_default();

    let mut fonts: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for (family, b64) in &req.fonts {
        match STANDARD.decode(b64) {
            Ok(bytes) => {
                fonts.insert(family.clone(), bytes);
            }
            Err(e) => {
                return Ok(Response::builder()
                    .status(400)
                    .body(Body::from(format!(
                        "invalid base64 for font \"{family}\": {e}"
                    )))?)
            }
        }
    }

    match render_with_fonts(&req.template, &req.data, &page, &NoPartials, &fonts) {
        Ok(bytes) => Ok(Response::builder()
            .status(200)
            .header("Content-Type", "application/pdf")
            .header(
                "Content-Disposition",
                format!("inline; filename=\"{}\"", req.filename),
            )
            .body(Body::from(bytes))?),
        Err(e) => Ok(Response::builder()
            .status(422)
            .header("Content-Type", "text/plain")
            .body(Body::from(format!("render error: {e}")))?),
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    run(service_fn(handler)).await
}
