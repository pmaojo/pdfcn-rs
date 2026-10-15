//! `pdfcn images`: prepare photos once, before they go into a template.

use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::{Args, ValueEnum};
use pdfcn_core::{prepare_image, PrepareFormat, PrepareOptions};

#[derive(Copy, Clone, ValueEnum)]
pub enum Format {
    /// JPEG stays JPEG, PNG stays PNG
    Keep,
    Jpeg,
    Png,
}

#[derive(Args)]
pub struct ImagesArgs {
    /// Image files, or folders of them (JPEG and PNG; folders are not searched deeper)
    #[arg(required = true)]
    inputs: Vec<PathBuf>,
    /// Folder to write the prepared images to. Originals are never overwritten:
    /// this may not be the folder they are in
    #[arg(short, long)]
    out: PathBuf,
    /// Longest side of the result in pixels (never enlarges). 2400 is a
    /// full-width A4 photo at about 300 dpi
    #[arg(long, default_value_t = 2400)]
    max_edge: u32,
    /// JPEG quality, 1-100
    #[arg(long, default_value_t = 85)]
    quality: u8,
    /// Output format
    #[arg(long, value_enum, default_value = "keep")]
    format: Format,
}

fn is_image(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("jpg" | "jpeg" | "png")
    )
}

fn collect(inputs: &[PathBuf]) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for input in inputs {
        if input.is_dir() {
            let mut found: Vec<PathBuf> = std::fs::read_dir(input)
                .with_context(|| format!("cannot read {input:?}"))?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_file() && is_image(p))
                .collect();
            found.sort();
            files.extend(found);
        } else if input.is_file() {
            files.push(input.clone());
        } else {
            anyhow::bail!("{input:?} does not exist");
        }
    }
    Ok(files)
}

fn human(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{} KB", (bytes + 512) / 1024)
    }
}

pub fn run(args: ImagesArgs) -> anyhow::Result<()> {
    let files = collect(&args.inputs)?;
    if files.is_empty() {
        anyhow::bail!("no .jpg, .jpeg or .png images found");
    }
    std::fs::create_dir_all(&args.out).with_context(|| format!("cannot create {:?}", args.out))?;
    let out_dir = std::fs::canonicalize(&args.out)?;
    let options = PrepareOptions {
        max_edge: args.max_edge,
        quality: args.quality,
        format: match args.format {
            Format::Keep => PrepareFormat::Keep,
            Format::Jpeg => PrepareFormat::Jpeg,
            Format::Png => PrepareFormat::Png,
        },
    };

    let (mut before, mut after, mut done) = (0usize, 0usize, 0usize);
    for file in &files {
        let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        let source = std::fs::read(file).with_context(|| format!("cannot read {file:?}"))?;
        let prepared = match prepare_image(&source, &options) {
            Ok(p) => p,
            Err(reason) => {
                eprintln!("skipped {name}: {reason}");
                continue;
            }
        };
        let target = out_dir.join(Path::new(name).with_extension(prepared.extension()));
        if std::fs::canonicalize(file).ok().as_deref() == Some(target.as_path()) {
            anyhow::bail!("{name}: the output would overwrite the original; choose another --out");
        }
        std::fs::write(&target, &prepared.bytes)
            .with_context(|| format!("cannot write {target:?}"))?;
        let mut notes = Vec::new();
        if prepared.rotated {
            notes.push("turned upright".to_string());
        }
        if !prepared.removed.is_empty() {
            notes.push(format!("removed {}", prepared.removed.join(", ")));
        }
        println!(
            "{name}: {}x{} -> {}x{}, {} -> {}{}",
            prepared.original_width,
            prepared.original_height,
            prepared.width,
            prepared.height,
            human(source.len()),
            human(prepared.bytes.len()),
            if notes.is_empty() {
                String::new()
            } else {
                format!("  [{}]", notes.join("; "))
            }
        );
        if prepared.removed.contains(&"ICC colour profile") {
            eprintln!(
                "note: {name} had a colour profile; it was dropped, so the result is read as sRGB"
            );
        }
        before += source.len();
        after += prepared.bytes.len();
        done += 1;
    }
    if done == 0 {
        anyhow::bail!("nothing could be prepared");
    }
    println!(
        "{done} image{} -> {}: {} -> {}",
        if done == 1 { "" } else { "s" },
        args.out.display(),
        human(before),
        human(after)
    );
    Ok(())
}
