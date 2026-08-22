//! FR-5: a native `/api/generate-pdf` handler for Vercel Functions.
//! Vercel auto-builds every `[[bin]]` target under `api/` via its built-in
//! Rust runtime (https://vercel.com/docs/functions/runtimes/rust) -- no
//! vercel.json, no custom build script. No Chromium, no dynamic system
//! dependencies (NFR-3) -- the whole request/response cycle is
//! `pdfcn-core` plus this thin HTTP adapter.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use http_body_util::BodyExt;
use pdfcn_core::{
    img_srcs, render_html_with_theme, render_pdf_with_assets, theme_from_json, EvalError,
    Orientation, PageConfig, PageSize, PartialLoader, Theme,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::OnceLock;
use std::time::Duration;
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

/// Cap on a single fetched image and on how many distinct http(s) sources
/// one request will fetch, so one template can't turn a single PDF render
/// into an unbounded number of slow/large outbound requests.
const MAX_REMOTE_IMAGE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_REMOTE_IMAGES_PER_REQUEST: usize = 12;
const FETCH_TIMEOUT: Duration = Duration::from_secs(8);

/// The optional shared-secret gate, read exactly once per isolate. Reading
/// it inside the handler would hide the dependency from tests and re-parse
/// the environment on every request; a serverless isolate is long-lived
/// enough that startup-time capture is the composition root.
static API_KEY: OnceLock<Option<String>> = OnceLock::new();

fn api_key() -> Option<&'static str> {
    API_KEY
        .get_or_init(|| {
            std::env::var("PDFCN_API_KEY")
                .ok()
                .filter(|k| !k.is_empty())
        })
        .as_deref()
}

/// True for an IP a template's image URL must never be allowed to reach:
/// loopback, private/link-local ranges, multicast, unspecified, and IPv4
/// mapped into IPv6. `template`/`data` are caller-controlled input on a
/// public endpoint, so an unresolved image src is exactly the SSRF vector
/// that let a template probe internal services or, on AWS (which Vercel
/// Functions run on), the 169.254.169.254 instance-metadata endpoint --
/// this is what keeps that closed off.
fn is_disallowed_target(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_multicast()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
        }
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_disallowed_target(IpAddr::V4(mapped));
            }
            v6.is_loopback()
                || v6.is_multicast()
                || v6.is_unspecified()
                || (v6.segments()[0] & 0xfe00) == 0xfc00 // fc00::/7, unique local
                || (v6.segments()[0] & 0xffc0) == 0xfe80 // fe80::/10, link-local
        }
    }
}

/// Resolves host:port, rejecting the target if it has no public IP among
/// its resolved addresses (a hostname can resolve to several; every one of
/// them must be public) or fails to resolve at all.
async fn resolve_public_addr(host: &str, port: u16) -> Result<SocketAddr, &'static str> {
    let addrs = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| "DNS resolution failed")?
        .collect::<Vec<_>>();
    addrs
        .into_iter()
        .find(|addr| !is_disallowed_target(addr.ip()))
        .ok_or("resolves only to a private/internal address")
}

/// Fetches one http(s) image URL, resolving and validating its host first
/// (see [`is_disallowed_target`]), then requesting that literal validated
/// address with redirects disabled -- a redirect target gets its own
/// validated fetch instead of being followed blindly, closing off
/// SSRF-via-redirect (and, because every hop is re-validated against the
/// same scheme/host/IP rules, this is a fetch-side guard rather than an
/// open redirect: no user-facing Location is ever echoed). The validated
/// address is pinned onto the reqwest client via resolve(), so the
/// connection cannot land anywhere other than the vetted IP -- without the
/// pin, reqwest's internal DNS lookup re-opens a TOCTOU window that a
/// short-TTL rebinding record can slip a private address through. Errors
/// and oversized responses degrade to None (the img renders as a
/// broken-image placeholder) rather than failing the whole request over
/// one bad image, matching a local file that isn't found.
async fn fetch_remote_image(url_str: &str) -> Option<Vec<u8>> {
    let mut url = reqwest::Url::parse(url_str).ok()?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return None;
    }

    // Pinned to the last validated origin, rebuilt only when a redirect
    // changes host or port -- not on every hop.
    let mut client: Option<reqwest::Client> = None;
    let mut pinned_host = String::new();
    let mut pinned_port = 0u16;

    // Bounded, not recursive: each hop is itself resolved and validated
    // before being requested, so a chain can't smuggle a private target in
    // past the first hop. The scheme check repeats per hop for the same
    // reason: reqwest only speaks http(s), but saying so here makes the
    // invariant local instead of incidental.
    for _ in 0..5 {
        if url.scheme() != "http" && url.scheme() != "https" {
            return None;
        }
        let host = url.host_str()?.to_string();
        let port = url.port_or_known_default()?;
        let addr = resolve_public_addr(&host, port).await.ok()?;

        let rebuild = match &client {
            Some(_) => pinned_host != host || pinned_port != port,
            None => true,
        };
        if rebuild {
            // resolve() pins the exact address just validated: reqwest
            // connects to addr and skips its own DNS lookup entirely.
            // Without this there is a TOCTOU window between the validation
            // above and reqwest's internal resolution -- a short-TTL
            // rebinding record can answer the first lookup with a public
            // IP and the second with 127.0.0.1. The explicit Host header
            // below keeps the request semantically addressed to the
            // original hostname.
            let built = reqwest::Client::builder()
                .timeout(FETCH_TIMEOUT)
                .redirect(reqwest::redirect::Policy::none())
                .resolve(host.as_str(), addr)
                .build()
                .ok()?;
            client = Some(built);
            pinned_host = host;
            pinned_port = port;
        }
        let bound = client.as_ref()?;

        let resp = bound
            .get(url.clone())
            .header(reqwest::header::HOST, format!("{}:{port}", pinned_host))
            .send()
            .await
            .ok()?;

        if resp.status().is_redirection() {
            let location = resp
                .headers()
                .get(reqwest::header::LOCATION)?
                .to_str()
                .ok()?;
            url = url.join(location).ok()?;
            continue;
        }
        if !resp.status().is_success() {
            return None;
        }
        if resp
            .content_length()
            .is_some_and(|len| len > MAX_REMOTE_IMAGE_BYTES)
        {
            return None;
        }
        let bytes = resp.bytes().await.ok()?;
        if bytes.len() as u64 > MAX_REMOTE_IMAGE_BYTES {
            return None;
        }
        return Some(bytes.to_vec());
    }
    None
}

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

/// The auth decision, isolated from HTTP plumbing so it's testable without
/// a live request: a request is authorized iff its x-api-key header equals
/// the configured key.
fn authorized(headers: &http::HeaderMap, expected_key: &str) -> bool {
    headers.get("x-api-key").and_then(|v| v.to_str().ok()) == Some(expected_key)
}

/// Decodes one base64-keyed map (fonts or images), rejecting the request
/// with a client-facing message if any value is malformed.
fn decode_base64_map(
    raw: &std::collections::HashMap<String, String>,
    kind: &str,
) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut out = BTreeMap::new();
    for (key, b64) in raw {
        match STANDARD.decode(b64) {
            Ok(bytes) => {
                out.insert(key.clone(), bytes);
            }
            Err(e) => return Err(format!("invalid base64 for {kind} \"{key}\": {e}")),
        }
    }
    Ok(out)
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

    match render_pdf_with_assets(&html, &page, &fonts, &images) {
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

    #[test]
    fn private_and_loopback_targets_are_disallowed() {
        for ip in [
            "127.0.0.1",
            "10.0.0.5",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "0.0.0.0",
            "::1",
            "fc00::1",
            "fe80::1",
        ] {
            let ip: IpAddr = ip.parse().unwrap();
            assert!(is_disallowed_target(ip), "{ip} should be disallowed");
        }
    }

    #[test]
    fn public_targets_are_allowed() {
        for ip in ["93.184.216.34", "8.8.8.8", "2606:4700:4700::1111"] {
            let ip: IpAddr = ip.parse().unwrap();
            assert!(!is_disallowed_target(ip), "{ip} should be allowed");
        }
    }

    #[test]
    fn ipv4_mapped_ipv6_private_targets_are_disallowed() {
        let ip: IpAddr = "::ffff:127.0.0.1".parse().unwrap();
        assert!(is_disallowed_target(ip));
    }

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
