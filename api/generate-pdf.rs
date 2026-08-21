//! FR-5: a native `/api/generate-pdf` handler for Vercel Functions.
//! Vercel auto-builds every `[[bin]]` target under `api/` via its built-in
//! Rust runtime (https://vercel.com/docs/functions/runtimes/rust) -- no
//! vercel.json, no custom build script. No Chromium, no dynamic system
//! dependencies (NFR-3) -- the whole request/response cycle is
//! `pdfcn-core` plus this thin HTTP adapter.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use http_body_util::BodyExt;
use pdfcn_core::{img_srcs, render_html, render_pdf_with_assets, NoPartials, Orientation, PageConfig, PageSize};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr};
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
    /// typefaces, so `template` can use `font-family: "<name>"` for any key
    /// given here -- e.g. a client's brand font.
    #[serde(default)]
    fonts: std::collections::HashMap<String, String>,
    /// Caller-supplied images: `<img src="...">` value -> base64-encoded
    /// JPEG/PNG bytes. `template` references a key via `%img(src="<key>")`
    /// (or plain `<img src="<key>">`). Preferred over a remote URL when
    /// both name the same `src` -- no fetch is attempted for a `src`
    /// already resolved this way.
    #[serde(default)]
    images: std::collections::HashMap<String, String>,
}

fn default_data() -> serde_json::Value {
    serde_json::Value::Object(Default::default())
}

fn default_filename() -> String {
    "document.pdf".to_string()
}

/// Cap on a single fetched image and on how many distinct `http(s):`
/// sources one request will fetch, so one template can't turn a single PDF
/// render into an unbounded number of slow/large outbound requests.
const MAX_REMOTE_IMAGE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_REMOTE_IMAGES_PER_REQUEST: usize = 12;
const FETCH_TIMEOUT: Duration = Duration::from_secs(8);

/// True for an IP a template's image URL must never be allowed to reach:
/// loopback, private/link-local ranges, multicast, unspecified, and IPv4
/// mapped into IPv6. `template`/`data` are caller-controlled input on a
/// public endpoint, so an unresolved image `src` is exactly the SSRF
/// vector that let a template probe internal services or, on AWS (which
/// Vercel Functions run on), the `169.254.169.254` instance-metadata
/// endpoint -- this is what keeps that closed off.
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

/// Resolves `host:port`, rejecting the target if it has no public IP among
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

/// Fetches one `http(s)://` image URL, resolving and validating its host
/// first (see [`is_disallowed_target`]), then requesting that literal
/// validated address with redirects disabled -- a redirect target gets its
/// own validated fetch instead of being followed blindly, closing off
/// SSRF-via-redirect. Errors and oversized responses degrade to `None`
/// (the `<img>` renders as a broken-image placeholder) rather than failing
/// the whole request over one bad image, matching a local file that isn't
/// found.
async fn fetch_remote_image(url_str: &str) -> Option<Vec<u8>> {
    let mut url = reqwest::Url::parse(url_str).ok()?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return None;
    }

    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .ok()?;

    // Bounded, not recursive: each hop is itself resolved and validated
    // before being requested, so a chain can't smuggle a private target in
    // past the first hop.
    for _ in 0..5 {
        let host = url.host_str()?.to_string();
        let port = url.port_or_known_default()?;
        let addr = resolve_public_addr(&host, port).await.ok()?;

        let resp = client
            .get(url.clone())
            // Connect to the address we already validated, not whatever a
            // second DNS lookup inside reqwest might return (DNS
            // rebinding) -- reqwest still sends the original Host header.
            .header(reqwest::header::HOST, format!("{host}:{port}"))
            .send()
            .await
            .ok()?;
        let _ = addr; // resolution success is the check; reqwest does its own connect

        if resp.status().is_redirection() {
            let location = resp.headers().get(reqwest::header::LOCATION)?.to_str().ok()?;
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
    let body_bytes = event.into_body().collect().await?.to_bytes();
    handle_body(&body_bytes).await
}

async fn handle_body(body_bytes: &[u8]) -> Result<Response<Vec<u8>>, Error> {
    let req: GenerateRequest = if body_bytes.is_empty() {
        return Ok(Response::builder()
            .status(400)
            .body(b"missing JSON body: {\"template\": ..., \"data\": ...}".to_vec())?);
    } else {
        match serde_json::from_slice(body_bytes) {
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

    let mut fonts: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for (family, b64) in &req.fonts {
        match STANDARD.decode(b64) {
            Ok(bytes) => {
                fonts.insert(family.clone(), bytes);
            }
            Err(e) => {
                return Ok(Response::builder()
                    .status(400)
                    .body(format!("invalid base64 for font \"{family}\": {e}").into_bytes())?)
            }
        }
    }

    let mut images: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for (src, b64) in &req.images {
        match STANDARD.decode(b64) {
            Ok(bytes) => {
                images.insert(src.clone(), bytes);
            }
            Err(e) => {
                return Ok(Response::builder()
                    .status(400)
                    .body(format!("invalid base64 for image \"{src}\": {e}").into_bytes())?)
            }
        }
    }

    let html = match render_html(&req.template, &req.data, &NoPartials) {
        Ok(html) => html,
        Err(e) => {
            return Ok(Response::builder()
                .status(422)
                .header("Content-Type", "text/plain")
                .body(format!("render error: {e}").into_bytes())?)
        }
    };

    // Any `<img src="http(s)://...">` not already covered by the caller's
    // own `images` map is fetched here -- see `fetch_remote_image` for the
    // SSRF guards. `render_pdf`/`render_with_assets` themselves stay
    // network-free (NFR-3); this fetching is specific to this HTTP
    // handler, which is the one place that both takes arbitrary
    // caller-supplied URLs and has an async runtime to fetch them on.
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

    #[tokio::test]
    async fn generates_a_pdf_for_a_minimal_request() {
        let body =
            br#"{"template": "%h1 Invoice\n%p Total: {{ total }}", "data": {"total": "$42.00"}}"#;
        let resp = handle_body(body).await.unwrap();
        assert_eq!(resp.status(), 200);
        assert!(resp.body().starts_with(b"%PDF"));
    }

    #[tokio::test]
    async fn rejects_empty_body_with_400() {
        let resp = handle_body(b"").await.unwrap();
        assert_eq!(resp.status(), 400);
    }

    #[tokio::test]
    async fn rejects_invalid_json_with_400() {
        let resp = handle_body(b"not json").await.unwrap();
        assert_eq!(resp.status(), 400);
    }

    /// The index.html sandbox's "Catalog example (images)" sends exactly
    /// this shape (template + data + a base64 `images` map) to
    /// `/api/generate-pdf`. This pins that the browser sandbox's request
    /// actually renders, with the images embedded, not just the CLI path.
    #[tokio::test]
    async fn sandbox_catalog_example_request_renders_with_images() {
        let body = include_bytes!("testdata/sandbox_catalog_request.json");
        let resp = handle_body(body).await.unwrap();
        assert_eq!(resp.status(), 200, "body: {:?}", String::from_utf8_lossy(resp.body()));
        assert!(resp.body().starts_with(b"%PDF"));
    }

    #[test]
    fn private_and_loopback_targets_are_disallowed() {
        for ip in [
            "127.0.0.1", "10.0.0.5", "172.16.0.1", "192.168.1.1", "169.254.169.254", "0.0.0.0",
            "::1", "fc00::1", "fe80::1",
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
        assert!(fetch_remote_image("ftp://example.com/a.png").await.is_none());
    }

    #[tokio::test]
    async fn an_unresolvable_or_private_host_is_never_fetched() {
        assert!(fetch_remote_image("http://127.0.0.1:9/a.png").await.is_none());
        assert!(fetch_remote_image("http://169.254.169.254/latest/meta-data/").await.is_none());
        assert!(
            fetch_remote_image("http://this-host-does-not-resolve.invalid/a.png")
                .await
                .is_none()
        );
    }
}
