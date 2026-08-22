//! FR-5: a native `/api/generate-pdf` handler for Vercel Functions.
//! Vercel auto-builds every `[[bin]]` target under `api/` via its built-in
//! Rust runtime (https://vercel.com/docs/functions/runtimes/rust) -- no
//! vercel.json, no custom build script. No Chromium, no dynamic system
//! dependencies (NFR-3) -- the whole request/response cycle is
//! `pdfcn-core` plus this thin HTTP adapter.

use http_body_util::BodyExt;
use pdfcn_core::{
    img_srcs, render_html_with_theme, render_pdf_with_assets, theme_from_json, EvalError,
    Orientation, PageConfig, PageSize, PartialLoader, RenderOptions, Theme,
};
use pdfcn_vercel::auth::{api_key, authorized};
use pdfcn_vercel::dto::{ImageOptimizationDto, MetadataDto};
use pdfcn_vercel::remote_image::{decode_base64_map, fetch_remote_image};
use serde::Deserialize;
use std::collections::BTreeMap;
use vercel_runtime::{run, service_fn, Error, Request, Response};

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
    /// typefaces, so `template` can use font-family "<name>" for any key
    /// given here -- e.g. a client's brand font.
    #[serde(default)]
    fonts: std::collections::HashMap<String, String>,
    /// Caller-supplied images: img src value -> base64-encoded JPEG/PNG
    /// bytes. `template` references a key via an img tag with that src.
    /// Preferred over a remote URL when both name the same src -- no fetch
    /// is attempted for a src already resolved this way.
    #[serde(default)]
    images: std::collections::HashMap<String, String>,
    /// Caller-supplied partials for `- include "name"`: partial name (as
    /// used in the include path) -> HAML-like source. Lets one request
    /// compose a multi-partial document without hosting any template files.
    #[serde(default)]
    partials: std::collections::BTreeMap<String, String>,
    /// Optional document theme: mode picks shadcn's light or dark token
    /// table; overrides rebrand individual semantic tokens (e.g. primary
    /// to a brand hex), recoloring every utility and component variant
    /// built on them without touching the template.
    #[serde(default)]
    theme: Option<serde_json::Value>,
    /// Repeated on every page (see `skip_first_page`). Currently has no
    /// visible effect -- printpdf 0.12.6 doesn't render it yet; accepted
    /// and wired through for forward compatibility (see
    /// `pdfcn_core::RenderOptions::header_text`'s doc comment).
    #[serde(default)]
    header_text: Option<String>,
    /// Repeated on every page. Same current no-op as `header_text`.
    #[serde(default)]
    footer_text: Option<String>,
    /// Appends "Page X of Y" to the footer. Same current no-op.
    #[serde(default)]
    show_page_numbers: bool,
    /// Suppresses header/footer/page-numbers on the first page (a cover).
    /// Moot while the above render nothing.
    #[serde(default)]
    skip_first_page: bool,
    /// Tunes image re-encoding at save time. Compression is already on by
    /// default (printpdf's own quality 0.85 / 2MB cap); omitting this
    /// keeps that default rather than disabling it.
    #[serde(default)]
    image_optimization: Option<ImageOptimizationDto>,
    /// Plain PDF document metadata (title/author/subject/keywords/producer).
    #[serde(default)]
    metadata: Option<MetadataDto>,
}

/// Resolves `- include` against a request's `partials` map.
struct MemoryPartials {
    partials: BTreeMap<String, pdfcn_parser::Document>,
}

impl PartialLoader for MemoryPartials {
    fn load(&self, path: &str) -> Result<pdfcn_parser::Document, EvalError> {
        self.partials
            .get(path)
            .cloned()
            .ok_or_else(|| EvalError::PartialNotFound(path.to_string()))
    }
}

fn default_data() -> serde_json::Value {
    serde_json::Value::Object(Default::default())
}

fn default_filename() -> String {
    "document.pdf".to_string()
}

/// How many distinct http(s) sources one request will fetch, so one
/// template can't turn a single PDF render into an unbounded number of
/// slow/large outbound requests. Per-image size and timeout are shared with
/// the batch endpoint -- see `pdfcn_vercel::remote_image`.
const MAX_REMOTE_IMAGES_PER_REQUEST: usize = 12;

async fn handler(event: Request) -> Result<Response<Vec<u8>>, Error> {
    // Optional shared-secret gate: when PDFCN_API_KEY is set (production
    // env vars, not sandbox .env), every request must present it in the
    // x-api-key header. Unset means the endpoint stays open -- the default
    // for the public sandbox.
    if let Some(expected) = api_key() {
        if !authorized(event.headers(), expected) {
            return Ok(Response::builder()
                .status(401)
                .body(b"missing or invalid x-api-key header".to_vec())?);
        }
    }
    let body_bytes = event.into_body().collect().await?.to_bytes();
    // A JSON body must be UTF-8 anyway; anything else is a 400.
    match std::str::from_utf8(&body_bytes) {
        Ok(body) => handle_body(body).await,
        Err(_) => Ok(Response::builder()
            .status(400)
            .body(b"request body must be UTF-8 JSON".to_vec())?),
    }
}

async fn handle_body(body: &str) -> Result<Response<Vec<u8>>, Error> {
    let req: GenerateRequest = if body.is_empty() {
        return Ok(Response::builder()
            .status(400)
            .body(b"missing JSON body: provide template and data".to_vec())?);
    } else {
        match serde_json::from_str(body) {
            Ok(req) => req,
            Err(e) => {
                return Ok(Response::builder()
                    .status(400)
                    .body(format!("invalid request body: {e}").into_bytes())?)
            }
        }
    };

    let page = req
        .page
        .map(PageConfigDto::into_page_config)
        .unwrap_or_default();

    // Caller mistakes are 400s before any rendering starts.
    let fonts = match decode_base64_map(&req.fonts, "font") {
        Ok(fonts) => fonts,
        Err(msg) => return Ok(Response::builder().status(400).body(msg.into_bytes())?),
    };
    let mut images = match decode_base64_map(&req.images, "image") {
        Ok(images) => images,
        Err(msg) => return Ok(Response::builder().status(400).body(msg.into_bytes())?),
    };

    // The theme is validated up front (a malformed one is a client
    // mistake, a 400) rather than discovered mid-render.
    let theme: Theme = match &req.theme {
        Some(value) => match theme_from_json(value) {
            Ok(theme) => theme,
            Err(msg) => {
                return Ok(Response::builder()
                    .status(400)
                    .body(format!("invalid theme: {msg}").into_bytes())?)
            }
        },
        None => Theme::light(),
    };

    let image_optimization = match req
        .image_optimization
        .map(ImageOptimizationDto::into_image_optimization)
    {
        None => None,
        Some(Ok(opts)) => Some(opts),
        Some(Err(msg)) => return Ok(Response::builder().status(400).body(msg.into_bytes())?),
    };
    // `theme` is cloned here rather than moved: `render_html_with_theme`
    // below still needs its own `&theme` borrow. Every other field is
    // moved out of `req` -- only `req.template`/`req.data` (by reference,
    // already used above) and `req.filename` (used later) are touched
    // again, so the partial move is sound.
    let render_options = RenderOptions {
        page,
        theme: theme.clone(),
        header_text: req.header_text,
        footer_text: req.footer_text,
        show_page_numbers: req.show_page_numbers,
        skip_first_page: req.skip_first_page,
        image_optimization,
        metadata: req
            .metadata
            .map(MetadataDto::into_document_metadata)
            .unwrap_or_default(),
    };

    // Partials are validated up front (a syntax error in one is a client
    // mistake, a 400) rather than discovered mid-render as a generic
    // partial-not-found.
    // `+ Send`: this box lives across the image-fetch awaits below, and a
    // non-Send local would poison the whole handler future.
    let loader: Box<dyn PartialLoader + Send> = if req.partials.is_empty() {
        Box::new(pdfcn_core::NoPartials)
    } else {
        let mut parsed = BTreeMap::new();
        for (name, source) in &req.partials {
            match pdfcn_parser::parse_document(source) {
                Ok(doc) => {
                    parsed.insert(name.clone(), doc);
                }
                Err(e) => {
                    return Ok(Response::builder()
                        .status(400)
                        .body(format!("invalid partial \"{name}\": {e}").into_bytes())?)
                }
            }
        }
        Box::new(MemoryPartials { partials: parsed })
    };

    let html = match render_html_with_theme(&req.template, &req.data, loader.as_ref(), &theme) {
        Ok(html) => html,
        Err(e) => {
            return Ok(Response::builder()
                .status(422)
                .header("Content-Type", "text/plain")
                .body(format!("render error: {e}").into_bytes())?)
        }
    };

    // Any http(s) img src not already covered by the caller's own images
    // map is fetched here -- see fetch_remote_image for the SSRF guards.
    // render_pdf/render_with_assets themselves stay network-free (NFR-3);
    // this fetching is specific to this HTTP handler, which is the one
    // place that both takes arbitrary caller-supplied URLs and has an
    // async runtime to fetch them on.
    let remote_srcs: Vec<String> = img_srcs(&html)
        .into_iter()
        .filter(|src| {
            !images.contains_key(src) && (src.starts_with("http://") || src.starts_with("https://"))
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .take(MAX_REMOTE_IMAGES_PER_REQUEST)
        .collect();
    for src in remote_srcs {
        if let Some(bytes) = fetch_remote_image(&src).await {
            images.insert(src, bytes);
        }
    }

    match render_pdf_with_assets(&html, &fonts, &images, &render_options) {
        Ok(bytes) => Ok(Response::builder()
            .status(200)
            .header("Content-Type", "application/pdf")
            .header(
                "Content-Disposition",
                format!("inline; filename=\"{}\"", req.filename),
            )
            .body(bytes)?),
        Err(e) => Ok(Response::builder()
            .status(422)
            .header("Content-Type", "text/plain")
            .body(format!("render error: {e}").into_bytes())?),
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    run(service_fn(handler)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test wrapper: unwraps the transport-level result so assertions can
    /// focus on the HTTP response itself.
    async fn handle_body(body: &str) -> Response<Vec<u8>> {
        super::handle_body(body).await.unwrap()
    }

    #[tokio::test]
    async fn generates_a_pdf_for_a_minimal_request() {
        let body = serde_json::json!({
            "template": "%h1 Invoice\n%p Total: {{ total }}",
            "data": { "total": "$42.00" }
        })
        .to_string();
        let resp = handle_body(&body).await;
        assert_eq!(resp.status(), 200);
        assert!(resp.body().starts_with(b"%PDF"));
    }

    #[tokio::test]
    async fn rejects_empty_body_with_400() {
        let resp = handle_body("").await;
        assert_eq!(resp.status(), 400);
    }

    /// `- include` over HTTP: a `partials` map makes one request able to
    /// compose multiple HAML fragments, with interpolation flowing through.
    #[tokio::test]
    async fn request_partials_are_included_and_rendered() {
        let body = serde_json::json!({
            "template": "%h1 Doc\n- include \"footer\"",
            "data": { "year": 2026 },
            "partials": { "footer": "%p Footer {{ year }}" },
        })
        .to_string();
        let resp = handle_body(&body).await;
        assert_eq!(
            resp.status(),
            200,
            "body: {:?}",
            String::from_utf8_lossy(resp.body())
        );
        assert!(resp.body().starts_with(b"%PDF"));
    }

    #[tokio::test]
    async fn a_syntactically_invalid_partial_is_a_400() {
        let body = serde_json::json!({
            "template": "%h1 Doc",
            "partials": { "broken": "%p\n\t%span tabs rejected" },
        })
        .to_string();
        let resp = handle_body(&body).await;
        assert_eq!(resp.status(), 400);
        assert!(String::from_utf8_lossy(resp.body()).contains("invalid partial"));
    }

    #[tokio::test]
    async fn an_unresolvable_include_is_a_422() {
        let body = serde_json::json!({
            "template": "%h1 Doc\n- include \"missing\"",
        })
        .to_string();
        let resp = handle_body(&body).await;
        assert_eq!(resp.status(), 422);
    }

    #[test]
    fn the_api_key_gate_requires_the_exact_header_value() {
        let mut headers = http::HeaderMap::new();
        assert!(!authorized(&headers, "secret"));
        headers.insert("x-api-key", http::HeaderValue::from_static("wrong"));
        assert!(!authorized(&headers, "secret"));
        headers.insert("x-api-key", http::HeaderValue::from_static("secret"));
        assert!(authorized(&headers, "secret"));
    }

    #[tokio::test]
    async fn rejects_invalid_json_with_400() {
        let resp = handle_body("not json").await;
        assert_eq!(resp.status(), 400);
    }

    /// The index.html sandbox's "Catalog example (images)" sends exactly
    /// this shape (template + data + a base64 images map) to
    /// /api/generate-pdf. This pins that the browser sandbox's request
    /// actually renders, with the images embedded, not just the CLI path.
    #[tokio::test]
    async fn sandbox_catalog_example_request_renders_with_images() {
        let body = include_str!("testdata/sandbox_catalog_request.json");
        let resp = handle_body(body).await;
        assert_eq!(
            resp.status(),
            200,
            "body: {:?}",
            String::from_utf8_lossy(resp.body())
        );
        assert!(resp.body().starts_with(b"%PDF"));
    }

    // `is_disallowed_target`'s own coverage (loopback/private/link-local,
    // IPv4-mapped IPv6, public addresses) now lives with the function
    // itself in `pdfcn_vercel::remote_image`, alongside `generate-pdf-
    // batch.rs`'s copy -- these were the same three tests duplicated a
    // third time.

    #[tokio::test]
    async fn a_non_http_scheme_is_never_fetched() {
        assert!(fetch_remote_image("file:///etc/passwd").await.is_none());
        assert!(fetch_remote_image("ftp://example.com/a.png")
            .await
            .is_none());
    }

    #[tokio::test]
    async fn an_unresolvable_or_private_host_is_never_fetched() {
        assert!(fetch_remote_image("http://127.0.0.1:9/a.png")
            .await
            .is_none());
        assert!(
            fetch_remote_image("http://169.254.169.254/latest/meta-data/")
                .await
                .is_none()
        );
        assert!(
            fetch_remote_image("http://this-host-does-not-resolve.invalid/a.png")
                .await
                .is_none()
        );
    }
}
