mod utilities;

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
pub fn build_stylesheet(html: &str) -> String {
    let classes = extract_classes(html);
    let mut css = String::from(PRINT_RULES);
    for class in &classes {
        if let Some(decl) = utilities::resolve(class) {
            let escaped = css_escape_class(class);
            css.push_str(&format!(".{escaped}{{{decl}}}\n"));
        }
    }
    minify(&css).unwrap_or(css)
}

fn css_escape_class(class: &str) -> String {
    class
        .chars()
        .map(|c| {
            if c == '.' || c == ':' || c == '/' {
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
}
