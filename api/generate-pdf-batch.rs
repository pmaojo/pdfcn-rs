//! `/api/generate-pdf-batch`: N documents, one request, one response.
//! Vercel auto-builds every `[[bin]]` target under `api/` via its built-in
//! Rust runtime, same as [`crate`-sibling] `generate-pdf.rs`.
//!
//! Why a batch endpoint at all: rendering many documents from one template
//! (a run of invoices, monthly statements) through the single-document
//! endpoint pays HTTP + font-embed + parse overhead per document and
//! serializes on the caller's request loop. Here the documents share one
//! auth check, one decoded asset map, one deduplicated remote-image fetch
//! pass, and -- thanks to `pdfcn-core`'s parse cache -- one parse of an
//! identical template, while the CPU-bound renders fan out across cores
//! with rayon.
//!
//! Semantics mirror `/api/generate-pdf` where they can: a problem with the
//! request itself (bad JSON, bad base64, malformed theme, more than
//! [`MAX_DOCUMENTS`] entries) is a 400 before any rendering starts; a
//! failure *inside* one document's render is not -- it becomes an `error`
//! entry in the JSON response and its siblings still ship. A batch of 500
//! statements must not die because row 137 had a typo.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use http_body_util::BodyExt;
use pdfcn_core::{
    img_srcs, render_html_with_theme, render_pdf_with_assets, theme_from_json, EvalError,
    Orientation, PageConfig, PageSize, PartialLoader, Theme,
};
use pdfcn_vercel::auth::{api_key, authorized};
use pdfcn_vercel::remote_image::{decode_base64_map, fetch_remote_image};
use rayon::prelude::*;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use vercel_runtime::{run, service_fn, Error, Request, Response};

/// Upper bound on documents per request. Serverless functions have hard
/// wall-clock/memory limits; past this, a caller should split into several
/// requests rather than one of them silently timing out with nothing to
/// show for it.
const MAX_DOCUMENTS: usize = 50;

/// Total remote-image fetches one batch request may make, across all of
/// its documents combined. Identical `<img src>` values across documents
/// are fetched once and shared (the common case -- one logo, many
/// invoices), which is what keeps this small.
const MAX_REMOTE_IMAGES_PER_BATCH: usize = 24;

#[derive(Clone, Deserialize)]
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
struct DocumentDto {
    /// HAML-like template source.
    template: String,
    /// Data context merged over the request-level `data` (top-level keys
    /// here win). Omitted means "exactly the shared data".
    #[serde(default)]
    data: Option<serde_json::Value>,
    #[serde(default)]
    page: Option<PageConfigDto>,
    /// Filename reported for this document's result entry. Unnamed
    /// documents default to the request-level `filename` (single-document
    /// batches) or `document-N.pdf` by position (multi-document batches).
    #[serde(default)]
    filename: Option<String>,
}

#[derive(Deserialize)]
struct BatchRequest {
    /// One entry per output PDF.
    documents: Vec<DocumentDto>,
    /// Shared data context underneath every document's own `data`.
    #[serde(default = "default_data")]
    data: serde_json::Value,
    /// Shared page config for documents without their own.
    #[serde(default)]
    page: Option<PageConfigDto>,
    /// Default result filename for a single unnamed document.
    #[serde(default = "default_filename")]
    filename: String,
    /// Shared document theme (see `/api/generate-pdf`'s `theme` field).
    #[serde(default)]
    theme: Option<serde_json::Value>,
    /// Caller-supplied fonts, shared by every document.
    #[serde(default)]
    fonts: HashMap<String, String>,
    /// Caller-supplied images, shared by every document.
    #[serde(default)]
    images: HashMap<String, String>,
    /// Caller-supplied partials for `- include`, shared by every document.
    #[serde(default)]
    partials: BTreeMap<String, String>,
}

fn default_data() -> serde_json::Value {
    serde_json::Value::Object(Default::default())
}

fn default_filename() -> String {
    "document.pdf".to_string()
}

/// Top-level shallow merge: keys in `over` win, everything else comes from
/// `base`. Deliberately not deep -- a per-document override replaces a
/// whole subtree (`{"invoice": {...}}` swaps the invoice object wholesale),
/// which is the predictable behavior for "this row's data".
///
/// Returns a [`Cow`] so the common cases allocate nothing: a document
/// without its own `data` renders against the shared context by reference,
/// and a non-object override borrows wholesale. Only an actual key-level
/// merge clones -- and then only the base map, once, for that document.
fn merged_data<'a>(
    base: &'a serde_json::Value,
    over: Option<&'a serde_json::Value>,
) -> std::borrow::Cow<'a, serde_json::Value> {
    match over {
        // No override at all: render against the shared context directly.
        None => std::borrow::Cow::Borrowed(base),
        Some(over) => match over.as_object() {
            // A non-object override replaces the whole context wholesale.
            None => std::borrow::Cow::Borrowed(over),
            // An empty override merges nothing; borrow the base.
            Some(map) if map.is_empty() => std::borrow::Cow::Borrowed(base),
            Some(over_map) => {
                let mut merged = base.as_object().cloned().unwrap_or_default();
                for (key, value) in over_map {
                    merged.insert(key.clone(), value.clone());
                }
                std::borrow::Cow::Owned(serde_json::Value::Object(merged))
            }
        },
    }
}

/// Resolves `- include` against the request's `partials` map. `Sync`, not
/// just `Send`: the render phase shares `&loader` across rayon's threads.
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

/// One document's slot in the response: either a rendered PDF (base64) or
/// the reason it failed. Built with `json!` at serialization time so the
/// wire shape lives in one place.
enum DocOutcome {
    Pdf { filename: String, bytes: Vec<u8> },
    Failed(String),
}

async fn handler(event: Request) -> Result<Response<Vec<u8>>, Error> {
    // Same optional shared-secret gate as /api/generate-pdf: when
    // PDFCN_API_KEY is set, every request must present it. A batch is worth
    // at least as much protection as a single render.
    if let Some(expected) = api_key() {
        if !authorized(event.headers(), expected) {
            return Ok(Response::builder()
                .status(401)
                .body(b"missing or invalid x-api-key header".to_vec())?);
        }
    }
    let body_bytes = event.into_body().collect().await?.to_bytes();
    match std::str::from_utf8(&body_bytes) {
        Ok(body) => handle_body(body).await,
        Err(_) => Ok(Response::builder()
            .status(400)
            .body(b"request body must be UTF-8 JSON".to_vec())?),
    }
}

async fn handle_body(body: &str) -> Result<Response<Vec<u8>>, Error> {
    let mut req: BatchRequest = if body.is_empty() {
        return Ok(Response::builder()
            .status(400)
            .body(b"missing JSON body: {\"documents\": [...] }".to_vec())?);
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

    if req.documents.is_empty() {
        return Ok(Response::builder()
            .status(400)
            .body(b"documents must contain at least one entry".to_vec())?);
    }
    if req.documents.len() > MAX_DOCUMENTS {
        return Ok(Response::builder().status(400).body(
            format!(
                "too many documents: {} (max {MAX_DOCUMENTS}); split the batch",
                req.documents.len()
            )
            .into_bytes(),
        )?);
    }

    // Everything shared is validated up front, exactly as the single
    // endpoint validates it: caller mistakes are 400s before any render.
    let fonts = match decode_base64_map(&req.fonts, "font") {
        Ok(fonts) => fonts,
        Err(msg) => return Ok(Response::builder().status(400).body(msg.into_bytes())?),
    };

    let mut images = match decode_base64_map(&req.images, "image") {
        Ok(images) => images,
        Err(msg) => return Ok(Response::builder().status(400).body(msg.into_bytes())?),
    };

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

    let loader: Box<dyn PartialLoader + Send + Sync> = if req.partials.is_empty() {
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

    let default_page: Option<PageConfig> = req.page.map(PageConfigDto::into_page_config);

    // Per-document layout knobs, extracted up front so Phase 1 can own the
    // document list outright: spawn_blocking closures are 'static, they
    // cannot borrow from `req`.
    let per_doc: Vec<(Option<PageConfig>, Option<String>)> = req
        .documents
        .iter()
        .map(|doc| {
            (
                doc.page.as_ref().map(|dto| dto.clone().into_page_config()),
                doc.filename.clone(),
            )
        })
        .collect();

    // Phase 1 -- resolve every document to HTML, in parallel. Identical
    // templates hit pdfcn-core's parse cache, so a one-template-many-rows
    // batch parses once. The rayon fan-out runs inside spawn_blocking:
    // parking a CPU-bound loop on an async worker starves every other
    // future sharing that worker, so the pool gets a dedicated blocking
    // thread instead.
    let total = per_doc.len();
    let rendered: Vec<Result<String, String>> = {
        let documents = std::mem::take(&mut req.documents);
        let base_data = std::mem::take(&mut req.data);
        tokio::task::spawn_blocking(move || {
            documents
                .par_iter()
                .map(|doc| {
                    let data = merged_data(&base_data, doc.data.as_ref());
                    render_html_with_theme(&doc.template, &data, loader.as_ref(), &theme)
                        .map_err(|e| format!("render error: {e}"))
                })
                .collect::<Vec<_>>()
        })
        .await
        .map_err(|e| Error::from(format!("template rendering task failed: {e}")))?
    };

    // Phase 2 -- one deduplicated remote-image pass over every successful
    // HTML. A logo referenced by all 50 documents is one fetch, not fifty.
    // Sequential awaits, bounded by MAX_REMOTE_IMAGES_PER_BATCH: simple,
    // and the caps keep worst-case latency sane without spawning tasks.
    let mut pending_srcs: BTreeSet<String> = BTreeSet::new();
    for html in rendered.iter().flatten() {
        for src in img_srcs(html) {
            if !images.contains_key(&src)
                && (src.starts_with("http://") || src.starts_with("https://"))
            {
                pending_srcs.insert(src);
            }
        }
    }
    for src in pending_srcs.into_iter().take(MAX_REMOTE_IMAGES_PER_BATCH) {
        if let Some(bytes) = fetch_remote_image(&src).await {
            images.insert(src, bytes);
        }
    }

    // Phase 3 -- lay out each resolved HTML to PDF bytes, in parallel,
    // pairing results back with their filenames by position. Font
    // embedding + layout are the heaviest CPU work in the pipeline, so the
    // same spawn_blocking containment as Phase 1 applies.
    let shared_filename = req.filename.clone();
    let outcomes: Vec<DocOutcome> = {
        tokio::task::spawn_blocking(move || {
            rendered
                .into_par_iter()
                .zip(per_doc)
                .enumerate()
                .map(|(index, (html_res, (page, filename)))| match html_res {
                    Err(msg) => DocOutcome::Failed(msg),
                    Ok(html) => {
                        let page = page.or(default_page).unwrap_or_default();
                        match render_pdf_with_assets(&html, &page, &fonts, &images) {
                            Ok(bytes) => {
                                let filename = match filename {
                                    Some(name) => name,
                                    None if total == 1 => shared_filename.clone(),
                                    None => format!("document-{}.pdf", index + 1),
                                };
                                DocOutcome::Pdf { filename, bytes }
                            }
                            Err(e) => DocOutcome::Failed(format!("render error: {e}")),
                        }
                    }
                })
                .collect::<Vec<_>>()
        })
        .await
        .map_err(|e| Error::from(format!("pdf layout task failed: {e}")))?
    };

    let ok_count = outcomes
        .iter()
        .filter(|o| matches!(o, DocOutcome::Pdf { .. }))
        .count();
    let results: Vec<serde_json::Value> = outcomes
        .into_iter()
        .enumerate()
        .map(|(index, outcome)| match outcome {
            DocOutcome::Pdf { filename, bytes } => serde_json::json!({
                "index": index,
                "filename": filename,
                "pdf_base64": STANDARD.encode(bytes),
            }),
            DocOutcome::Failed(msg) => serde_json::json!({
                "index": index,
                "error": msg,
            }),
        })
        .collect();

    let payload = serde_json::json!({
        "count": total,
        "ok": ok_count,
        "failed": total - ok_count,
        "results": results,
    });
    let body = match serde_json::to_vec(&payload) {
        Ok(body) => body,
        // Unreachable in practice (every value is a string/number), but a
        // serialization bug must surface as a 500, never a hung request.
        Err(e) => {
            return Ok(Response::builder()
                .status(500)
                .header("Content-Type", "text/plain")
                .body(format!("failed to encode response: {e}").into_bytes())?)
        }
    };
    Ok(Response::builder()
        .status(200)
        .header("Content-Type", "application/json")
        .body(body)?)
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

    fn json_response(resp: &Response<Vec<u8>>) -> serde_json::Value {
        serde_json::from_slice(resp.body()).unwrap()
    }

    fn minimal_doc(template: &str) -> String {
        serde_json::json!({ "template": template }).to_string()
    }

    #[tokio::test]
    async fn renders_two_documents_in_one_request() {
        let body = serde_json::json!({
            "documents": [
                { "template": "%h1 First\n%p Total: {{ n }}", "data": { "n": 7 } },
                { "template": "%h1 Second" },
            ]
        })
        .to_string();
        let resp = handle_body(&body).await;
        assert_eq!(
            resp.status(),
            200,
            "body: {:?}",
            String::from_utf8_lossy(resp.body())
        );
        let payload = json_response(&resp);
        assert_eq!(payload["count"], 2);
        assert_eq!(payload["ok"], 2);
        assert_eq!(payload["failed"], 0);
        for (i, result) in payload["results"].as_array().unwrap().iter().enumerate() {
            assert_eq!(result["index"], i);
            let b64 = result["pdf_base64"].as_str().unwrap();
            let bytes = STANDARD.decode(b64).unwrap();
            assert!(bytes.starts_with(b"%PDF"));
        }
    }

    #[tokio::test]
    async fn rejects_missing_and_empty_documents_with_400() {
        for body in ["{}", r#"{"documents": []}"#] {
            let resp = handle_body(body).await;
            assert_eq!(resp.status(), 400, "{body}");
        }
    }

    #[tokio::test]
    async fn exceeding_the_document_cap_is_a_400() {
        let doc = minimal_doc("%p hi");
        let documents: Vec<String> = (0..=MAX_DOCUMENTS).map(|_| doc.clone()).collect();
        let body = format!(r#"{{"documents": [{}]}}"#, documents.join(","));
        let resp = handle_body(&body).await;
        assert_eq!(resp.status(), 400);
        assert!(String::from_utf8_lossy(resp.body()).contains("max"));
    }

    /// The point of per-entry errors: one broken row must not sink its
    /// siblings.
    #[tokio::test]
    async fn one_failing_document_does_not_fail_the_batch() {
        let body = serde_json::json!({
            "documents": [
                { "template": "%p Fine" },
                { "template": "%p\n\t%span tab indentation rejected" },
                { "template": "%p Also fine" },
            ]
        })
        .to_string();
        let resp = handle_body(&body).await;
        assert_eq!(
            resp.status(),
            200,
            "body: {:?}",
            String::from_utf8_lossy(resp.body())
        );
        let payload = json_response(&resp);
        assert_eq!(payload["ok"], 2);
        assert_eq!(payload["failed"], 1);
        let failed = &payload["results"][1];
        assert_eq!(failed["index"], 1);
        assert!(failed["error"].as_str().unwrap().contains("render error"));
        assert!(failed.get("pdf_base64").is_none());
    }

    /// Per-document data merges over the shared context: both variables
    /// resolving proves the merge happened in both directions.
    #[tokio::test]
    async fn per_document_data_merges_over_shared_data() {
        let body = serde_json::json!({
            "data": { "company": "Acme", "row": "ignored" },
            "documents": [
                { "template": "%p {{ company }} / {{ row }}", "data": { "row": "42" } },
            ]
        })
        .to_string();
        let resp = handle_body(&body).await;
        assert_eq!(
            resp.status(),
            200,
            "body: {:?}",
            String::from_utf8_lossy(resp.body())
        );
        assert_eq!(json_response(&resp)["ok"], 1);
    }

    #[tokio::test]
    async fn invalid_shared_theme_is_a_400() {
        let body = serde_json::json!({
            "theme": { "mode": "sepia" },
            "documents": [ { "template": "%p hi" } ],
        })
        .to_string();
        let resp = handle_body(&body).await;
        assert_eq!(resp.status(), 400);
        assert!(String::from_utf8_lossy(resp.body()).contains("invalid theme"));
    }

    /// Filenames: explicit per-document names win; unnamed documents in a
    /// multi-document batch are numbered by position; a single unnamed
    /// document gets the request-level default.
    #[tokio::test]
    async fn result_filenames_follow_position_and_explicit_names() {
        let body = serde_json::json!({
            "filename": "statement.pdf",
            "documents": [
                { "template": "%p a" },
                { "template": "%p b", "filename": "custom.pdf" },
            ]
        })
        .to_string();
        let resp = handle_body(&body).await;
        let payload = json_response(&resp);
        assert_eq!(payload["results"][0]["filename"], "document-1.pdf");
        assert_eq!(payload["results"][1]["filename"], "custom.pdf");

        let single = serde_json::json!({
            "filename": "statement.pdf",
            "documents": [ { "template": "%p a" } ],
        })
        .to_string();
        let payload = json_response(&handle_body(&single).await);
        assert_eq!(payload["results"][0]["filename"], "statement.pdf");
    }

    #[test]
    fn merged_data_prefers_the_override_subtree() {
        let base = serde_json::json!({ "a": 1, "b": { "x": 1 } });
        let over = serde_json::json!({ "b": { "y": 2 } });
        let merged = merged_data(&base, Some(&over));
        assert_eq!(merged["a"], 1);
        // Shallow: the whole `b` object is replaced, not deep-merged.
        assert_eq!(merged["b"], serde_json::json!({ "y": 2 }));
        assert_eq!(merged_data(&base, None).into_owned(), base);
        // A non-object override wins wholesale too.
        let scalar = serde_json::json!({ "rows": [1, 2] });
        assert_eq!(merged_data(&base, Some(&scalar))["rows"][1], 2);
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

    // `is_disallowed_target`'s own coverage now lives with the function in
    // `pdfcn_vercel::remote_image` -- see the matching note in
    // `generate-pdf.rs`, whose copy of this test was the other duplicate.

    #[tokio::test]
    async fn a_non_http_scheme_is_never_fetched() {
        assert!(fetch_remote_image("file:///etc/passwd").await.is_none());
        assert!(fetch_remote_image("http://127.0.0.1:9/a.png")
            .await
            .is_none());
    }
}
