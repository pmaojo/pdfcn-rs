//! Everything that reads the filesystem or the network on the caller's
//! behalf: `- include` partials from disk, and `render_files*`, which read a
//! template, its data and its images. The rest of the crate is the pure
//! pipeline from source text and bytes to PDF bytes (docs/spikes/009).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::{
    img_srcs, load_data, render_html_as, render_pdf_with_assets, CoreError, DataFormat, Format,
    PartialLoader, RenderOptions, Theme,
};

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
        // A partial may be in any supported format, chosen by its extension.
        let format = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .and_then(Format::from_extension);
        pdfcn_formats::parse_document(&source, format)
            .map_err(|_| pdfcn_template::EvalError::PartialNotFound(path.to_string()))
    }
}

/// Convenience for the CLI: reads `template_path` and `data_path` from
/// disk, using the template's directory as the base for `- include`. Every
/// `<img src="...">` (from `%img` or a component like `%Card`'s `image`
/// attribute) whose `src` is a relative filesystem path -- not `http(s):`
/// or `data:`, which this pipeline never fetches (NFR-3) -- is read from
/// disk relative to `template_path`'s directory and embedded automatically,
/// so `pdfcn build` composes real images into the PDF without the caller
/// having to call the lower-level `render_with_assets` API by hand.
///
/// `http(s):` sources are left unresolved here; use
/// [`render_files_with_remote_images`] to also resolve those, via a
/// caller-supplied fetcher.
pub fn render_files(
    template_path: &Path,
    data_path: &Path,
    options: &RenderOptions,
) -> Result<Vec<u8>, CoreError> {
    render_files_with_remote_images(template_path, data_path, options, None)
}

/// Reads `template_path` and `data_path` and runs the pipeline up to the
/// HTML document (no PDF yet), returning it with the template's directory
/// (where relative `include`s and `<img src>` resolve from).
pub fn render_files_html(
    template_path: &Path,
    data_path: &Path,
    theme: &Theme,
) -> Result<(String, PathBuf), CoreError> {
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
    let loader = FsPartialLoader::new(base_dir.clone());
    let template_format = template_path
        .extension()
        .and_then(|e| e.to_str())
        .and_then(Format::from_extension);
    let html = render_html_as(&source, template_format, &data, &loader, theme)?;
    Ok((html, base_dir))
}

/// A caller-supplied `http(s):` image fetcher for
/// [`render_files_with_remote_images`]: given a `src` URL, returns its
/// bytes, or `None` to leave that `<img>` unresolved.
pub type RemoteImageFetcher = dyn Fn(&str) -> Option<Vec<u8>>;

/// Like [`render_files`], but a `src="https://..."` (or `http://`) that
/// isn't resolved by a local file is instead passed to `fetch_remote`,
/// which returns the image bytes (or `None` to leave it unresolved, same
/// as a local file that isn't found). This is how a client's own image
/// URLs (e.g. from a CMS or a stock-photo host) get composed into a PDF
/// without a manual base64-encode-and-embed round trip -- while keeping
/// [`render_pdf`]/[`render_with_assets`] themselves free of any network
/// access (NFR-3): only a caller who explicitly supplies `fetch_remote`
/// (the CLI's `--fetch-remote-images`, opt-in) makes outbound requests.
pub fn render_files_with_remote_images(
    template_path: &Path,
    data_path: &Path,
    options: &RenderOptions,
    fetch_remote: Option<&RemoteImageFetcher>,
) -> Result<Vec<u8>, CoreError> {
    let (html, base_dir) = render_files_html(template_path, data_path, &options.theme)?;
    let images = load_images(&html, &base_dir, fetch_remote);
    render_pdf_with_assets(&html, &BTreeMap::new(), &images, options)
}

/// Reads every image `src` referenced in `html`: a local (non-`http(s)`,
/// non-`data:`) path from disk relative to `base_dir`, or -- when
/// `fetch_remote` is given -- an `http(s):` source via that callback. A
/// `src` left unresolved either way (not found on disk, no fetcher given,
/// or the fetcher returns `None`) is silently skipped (same convention as
/// an unknown utility class: it degrades, it doesn't fail the render) --
/// `printpdf` renders it as a broken-image placeholder. `data:` sources are
/// always skipped here since `printpdf` resolves those itself.
fn load_images(
    html: &str,
    base_dir: &Path,
    fetch_remote: Option<&RemoteImageFetcher>,
) -> BTreeMap<String, Vec<u8>> {
    let mut images = BTreeMap::new();
    for src in img_srcs(html) {
        if src.starts_with("data:") {
            continue;
        }
        if src.starts_with("http://") || src.starts_with("https://") {
            if let Some(bytes) = fetch_remote.and_then(|fetch| fetch(&src)) {
                images.insert(src, bytes);
            }
            continue;
        }
        if let Ok(bytes) = std::fs::read(base_dir.join(&src)) {
            images.insert(src, bytes);
        }
    }
    images
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_images_skips_remote_and_data_uri_sources_without_a_fetcher() {
        let html = concat!(
            r#"<img src="https://example.com/a.png">"#,
            r#"<img src="data:image/png;base64,AAAA">"#,
            r#"<img src="does-not-exist-on-disk.png">"#,
        );
        let images = load_images(html, Path::new("."), None);
        assert!(images.is_empty());
    }

    #[test]
    fn load_images_reads_a_relative_src_from_the_base_dir() {
        let dir = std::env::temp_dir().join(format!(
            "pdfcn-core-test-{}-{}",
            std::process::id(),
            "load_images_reads_a_relative_src_from_the_base_dir"
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("cover.png"), b"not-really-a-png-just-bytes").unwrap();

        let html = r#"<img src="cover.png">"#;
        let images = load_images(html, &dir, None);

        assert_eq!(
            images.get("cover.png").map(Vec::as_slice),
            Some(b"not-really-a-png-just-bytes".as_slice())
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_images_uses_the_fetcher_for_http_s_sources_and_skips_data_uris() {
        let html = concat!(
            r#"<img src="https://example.com/a.png">"#,
            r#"<img src="data:image/png;base64,AAAA">"#,
            r#"<img src="https://example.com/missing.png">"#,
        );
        let fetch = |src: &str| -> Option<Vec<u8>> {
            (src == "https://example.com/a.png").then(|| b"fetched-bytes".to_vec())
        };
        let images = load_images(html, Path::new("."), Some(&fetch));

        assert_eq!(
            images.get("https://example.com/a.png").map(Vec::as_slice),
            Some(b"fetched-bytes".as_slice())
        );
        assert!(!images.contains_key("data:image/png;base64,AAAA"));
        assert!(!images.contains_key("https://example.com/missing.png"));
    }
}
