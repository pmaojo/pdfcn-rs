mod gap;
pub mod theme;
mod tokens;
mod utilities;

pub use gap::rewrite_gaps;
pub use theme::{Theme, ThemeMode};
// The doc comments below describe this crate's model in terms of `resolve()`
// -- one class in, one flat declaration out. It lives in a private module, so
// until now that vocabulary named something no caller could reach and no
// non-test build referenced.
pub use utilities::{resolve, resolve_with};

use std::collections::BTreeSet;

use lightningcss::stylesheet::{MinifyOptions, ParserOptions, PrinterOptions, StyleSheet};

/// Print-safety rules injected into every generated stylesheet by default
/// (FR-3): forces background/color reproduction and keeps table rows and
/// cards from splitting across a page boundary.
const PRINT_RULES: &str = r#"
@page { margin: 0; }
html, body { print-color-adjust: exact; -webkit-print-color-adjust: exact; }
table { border-collapse: collapse; width: 100%; }
tr, .card { break-inside: avoid; page-break-inside: avoid; }
"#;

/// `table-striped`'s zebra effect needs a `:nth-child` selector, which
/// `utilities::resolve()` can't express (it maps one class to one flat
/// declaration applied to that exact class selector). Emitted as its own
/// rule, only when the class is actually used, same as every other class.
const TABLE_STRIPED_RULE: &str =
    ".table-striped tr:nth-child(even){background-color:hsl(210, 40%, 96.1%)}\n";

/// Scans `class="..."` attributes in already-rendered HTML and returns the
/// distinct utility class names referenced, in the order first seen.
pub fn extract_classes(html: &str) -> BTreeSet<String> {
    let mut classes = BTreeSet::new();
    let mut rest = html;
    while let Some(pos) = rest.find("class=\"") {
        let after = &rest[pos + "class=\"".len()..];
        let Some(end) = after.find('"') else { break };
        for token in after[..end].split_whitespace() {
            classes.insert(token.to_string());
        }
        rest = &after[end + 1..];
    }
    classes
}

/// Builds a minified, self-contained stylesheet for exactly the utility
/// classes referenced in `html`, plus the default print-safety rules.
/// Semantic tokens resolve against shadcn's light theme.
pub fn build_stylesheet(html: &str) -> String {
    build_stylesheet_with_theme(html, &Theme::light())
}

/// Like [`build_stylesheet`], but semantic token utilities resolve through
/// `theme` -- its mode picks shadcn's light or dark token table and its
/// overrides rebrand individual tokens (e.g. `primary` -> `#2563eb`).
pub fn build_stylesheet_with_theme(html: &str, theme: &Theme) -> String {
    let classes = extract_classes(html);
    build_stylesheet_from_classes(&classes, theme)
}

/// Like [`build_stylesheet_with_theme`], but for a caller that already has
/// the distinct utility classes in hand (e.g. as a cache key derived from
/// them) and wants to skip re-scanning the HTML for `class="..."`
/// attributes.
pub fn build_stylesheet_from_classes(classes: &BTreeSet<String>, theme: &Theme) -> String {
    let mut css = String::from(PRINT_RULES);
    for class in classes {
        if let Some(decl) = utilities::resolve_with(class, theme) {
            let escaped = css_escape_class(class);
            css.push_str(&format!(".{escaped}{{{decl}}}\n"));
        }
    }
    if classes.contains("table-striped") {
        css.push_str(TABLE_STRIPED_RULE);
    }
    minify(&css).unwrap_or(css)
}

fn css_escape_class(class: &str) -> String {
    class
        .chars()
        .map(|c| {
            if matches!(c, '.' | ':' | '/' | '[' | ']') {
                format!("\\{c}")
            } else {
                c.to_string()
            }
        })
        .collect()
}

fn minify(css: &str) -> Option<String> {
    let mut sheet = StyleSheet::parse(css, ParserOptions::default()).ok()?;
    sheet.minify(MinifyOptions::default()).ok()?;
    let out = sheet
        .to_css(PrinterOptions {
            minify: true,
            ..PrinterOptions::default()
        })
        .ok()?;
    Some(out.code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_classes_from_multiple_elements() {
        let html = r#"<div class="p-4 flex"><span class="text-lg">hi</span></div>"#;
        let classes = extract_classes(html);
        assert!(classes.contains("p-4"));
        assert!(classes.contains("flex"));
        assert!(classes.contains("text-lg"));
    }

    #[test]
    fn stylesheet_contains_only_used_classes() {
        let html = r#"<div class="p-4"></div>"#;
        let css = build_stylesheet(html);
        assert!(css.contains(".p-4"));
        assert!(!css.contains(".flex{"));
    }

    #[test]
    fn stylesheet_always_includes_print_rules() {
        let css = build_stylesheet("<div></div>");
        assert!(css.contains("@page"));
        assert!(css.to_lowercase().contains("break-inside"));
    }

    /// `table-striped` previously named a variant that resolved to no CSS
    /// declaration at all (`utilities::resolve` had no entry for it), so
    /// choosing it rendered identically to the default table -- a silent,
    /// untested drift. Zebra striping needs a `:nth-child` selector, which
    /// `resolve()`'s one-class-one-flat-declaration shape can't express, so
    /// this is emitted as its own rule rather than through `resolve()`.
    #[test]
    fn table_striped_class_actually_renders_zebra_striping() {
        let css = build_stylesheet(r#"<table class="table-striped"><tr></tr></table>"#);
        assert!(
            css.contains("nth-child"),
            "table-striped must emit an nth-child zebra rule, got: {css}"
        );
    }
}
