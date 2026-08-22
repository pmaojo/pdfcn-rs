use std::path::Path;
use std::time::Duration;

use pdfcn_core::{Orientation, PageConfig, PageSize};

/// Cap on a single fetched image, generous for a photo but not for an
/// accidental video/zip a misconfigured URL might point at.
const MAX_REMOTE_IMAGE_BYTES: u64 = 20 * 1024 * 1024;

/// Fetches one `http(s)://` image URL for `--fetch-remote-images`. Returns
/// `None` (leaving the `<img>` unresolved, same as a missing local file)
/// on any error or an oversized response rather than failing the whole
/// build over one bad image.
fn fetch_remote_image(url: &str) -> Option<Vec<u8>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .ok()?;
    let resp = client.get(url).send().ok()?.error_for_status().ok()?;
    if resp
        .content_length()
        .is_some_and(|len| len > MAX_REMOTE_IMAGE_BYTES)
    {
        eprintln!("warning: skipping {url} ({MAX_REMOTE_IMAGE_BYTES} byte limit exceeded)");
        return None;
    }
    let bytes = resp.bytes().ok()?;
    if bytes.len() as u64 > MAX_REMOTE_IMAGE_BYTES {
        eprintln!("warning: skipping {url} ({MAX_REMOTE_IMAGE_BYTES} byte limit exceeded)");
        return None;
    }
    Some(bytes.to_vec())
}

fn parse_size(s: &str) -> anyhow::Result<PageSize> {
    match s.to_ascii_lowercase().as_str() {
        "a4" => Ok(PageSize::A4),
        "letter" => Ok(PageSize::Letter),
        custom => {
            let (w, h) = custom.split_once('x').ok_or_else(|| {
                anyhow::anyhow!("invalid --size '{s}', expected a4, letter, or <width>x<height>")
            })?;
            Ok(PageSize::Custom {
                width_mm: w.parse()?,
                height_mm: h.parse()?,
            })
        }
    }
}

fn parse_orientation(s: &str) -> anyhow::Result<Orientation> {
    match s.to_ascii_lowercase().as_str() {
        "portrait" => Ok(Orientation::Portrait),
        "landscape" => Ok(Orientation::Landscape),
        other => anyhow::bail!("invalid --orientation '{other}', expected portrait or landscape"),
    }
}

pub fn run(
    template: &Path,
    data: &Path,
    out: &Path,
    size: &str,
    orientation: &str,
    margin_mm: f32,
    fetch_remote_images: bool,
) -> anyhow::Result<()> {
    let page = PageConfig {
        size: parse_size(size)?,
        orientation: parse_orientation(orientation)?,
        margin_mm,
    };
    let fetcher: Option<&pdfcn_core::RemoteImageFetcher> =
        fetch_remote_images.then_some(&fetch_remote_image);
    let bytes = pdfcn_core::render_files_with_remote_images(template, data, &page, fetcher)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    std::fs::write(out, &bytes)?;
    println!("Wrote {} ({} bytes)", out.display(), bytes.len());
    Ok(())
}
