use std::path::Path;

use pdfcn_core::{Orientation, PageConfig, PageSize};

fn parse_size(s: &str) -> anyhow::Result<PageSize> {
    match s.to_ascii_lowercase().as_str() {
        "a4" => Ok(PageSize::A4),
        "letter" => Ok(PageSize::Letter),
        custom => {
            let (w, h) = custom
                .split_once('x')
                .ok_or_else(|| anyhow::anyhow!("invalid --size '{s}', expected a4, letter, or <width>x<height>"))?;
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
) -> anyhow::Result<()> {
    let page = PageConfig {
        size: parse_size(size)?,
        orientation: parse_orientation(orientation)?,
        margin_mm,
    };
    let bytes = pdfcn_core::render_files(template, data, &page)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    std::fs::write(out, &bytes)?;
    println!("Wrote {} ({} bytes)", out.display(), bytes.len());
    Ok(())
}
