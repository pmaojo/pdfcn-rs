//! Audits the utility classes the repo's own templates and components use
//! against what `pdfcn-styles` can resolve. An unresolved class is dropped
//! silently when building a stylesheet, so without this check a typo or a
//! missing utility only shows up as a PDF that looks subtly wrong.

use std::fs;
use std::path::{Path, PathBuf};

use pdfcn_styles::{unresolved_classes, Theme};

fn collect(dir: &Path, exts: &[&str], out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, exts, out);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| exts.contains(&e))
        {
            out.push(path);
        }
    }
}

/// Pulls every `class="..."` value out of a source file's text (HTML in a
/// `.haml` template, or a Rust string literal in a component).
fn class_attrs(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(pos) = rest.find("class=\"") {
        let after = &rest[pos + 7..];
        let Some(end) = after.find('"') else { break };
        out.push_str(&format!("<i class=\"{}\">", &after[..end]));
        rest = &after[end + 1..];
    }
    out
}

#[test]
fn repo_templates_and_components_only_use_resolvable_utilities() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    collect(&root.join("examples"), &["haml"], &mut files);
    collect(&root.join("pdfcn-components/src"), &["rs"], &mut files);
    assert!(!files.is_empty(), "no sources found to audit");

    let mut html = String::new();
    for f in &files {
        if let Ok(text) = fs::read_to_string(f) {
            html.push_str(&class_attrs(&text));
        }
    }
    let unresolved = unresolved_classes(&html, &Theme::light());
    // Semantic marker classes the components emit for structure/hooks; they
    // are intentionally not utilities. Anything else unresolved is a typo or
    // a missing utility.
    let unexpected: Vec<&String> = unresolved.iter().filter(|c| !is_marker(c)).collect();
    assert!(
        unexpected.is_empty(),
        "classes used by examples/components that resolve to no CSS: {unexpected:?}"
    );
}

fn is_marker(class: &str) -> bool {
    // A haml-style selector pasted into a doc comment, not a real class.
    if class.contains('.') && class.contains("bg-destructive") {
        return true;
    }
    const PREFIXES: &[&str] = &[
        "alert-",
        "breadcrumb",
        "card-",
        "checkbox-",
        "form-field",
        "invoice-",
        "page-footer-",
        "pagination",
        "pdfcn-",
        "progress",
        "radio-",
        "signature-",
    ];
    const EXACT: &[&str] = &["barchart", "qrcode"];
    EXACT.contains(&class) || PREFIXES.iter().any(|p| class.starts_with(p))
}
