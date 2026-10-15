//! Loading icon sets for `%Icon`, and listing what is available.

use std::path::Path;

/// Registers every `*.svg` in `dir` as an icon named after the file
/// (`receipt.svg` -> `receipt`). Names that are not valid icon names, or files
/// that are not acceptable icons, are reported and skipped rather than ending
/// the build: a folder from a package usually holds a few oddities.
pub fn load_dir(dir: &Path) -> anyhow::Result<usize> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| anyhow::anyhow!("--icons-dir: cannot read {dir:?}: {e}"))?;
    let mut loaded = 0;
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("svg") {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let svg = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("--icons-dir: cannot read {path:?}: {e}"))?;
        match pdfcn_core::register_icon(name, &svg) {
            Ok(()) => loaded += 1,
            Err(reason) => eprintln!("warning: --icons-dir skipped {path:?}: {reason}"),
        }
    }
    if loaded == 0 {
        anyhow::bail!("--icons-dir: no usable .svg icons in {dir:?}");
    }
    Ok(loaded)
}

/// `pdfcn icons [filter]`: the names `%Icon(name="...")` accepts.
pub fn list(filter: Option<&str>) {
    let names = pdfcn_core::icon_names();
    let shown: Vec<&String> = names
        .iter()
        .filter(|n| filter.is_none_or(|f| n.contains(&f.to_lowercase())))
        .collect();
    for name in &shown {
        println!("{name}");
    }
    eprintln!(
        "{} of {} icons (Tabler Icons {}, MIT; add others with `pdfcn build --icons-dir <dir>`)",
        shown.len(),
        names.len(),
        pdfcn_core::tabler_version()
    );
}
