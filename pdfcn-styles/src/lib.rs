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
        let Some((pseudo, base)) = split_variant(class) else {
            continue;
        };
        if let Some(decl) = utilities::resolve_with(base, theme) {
            let selector = class_selector(class);
            css.push_str(&format!("{selector}{pseudo}{{{decl}}}\n"));
        }
    }
    if classes.contains("table-striped") {
        css.push_str(TABLE_STRIPED_RULE);
    }
    minify(&css).unwrap_or(css)
}

/// Splits a Tailwind variant prefix off `class`, returning the pseudo
/// selector to append (empty for none) and the bare utility. The output is
/// always print, so `print:` is unconditional and `screen:` never applies
/// (`None`). Structural variants (`first:`, `last:`, `odd:`, `even:`) map to
/// the matching `:first-child`/`:last-child`/`:nth-child` pseudo-class.
/// Interactive (`hover:`, `focus:`) and responsive (`md:`) variants have no
/// meaning in a PDF and are left unrecognized, like any unknown class.
fn split_variant(class: &str) -> Option<(&'static str, &str)> {
    let Some((variant, base)) = class.split_once(':') else {
        return Some(("", class));
    };
    // `[...]` arbitrary values may themselves contain a colon; only a
    // plain identifier before the colon is a variant.
    let pseudo = match variant {
        "print" => "",
        "first" => ":first-child",
        "last" => ":last-child",
        "odd" => ":nth-child(odd)",
        "even" => ":nth-child(even)",
        _ => return None,
    };
    Some((pseudo, base))
}

/// The utility classes in `html` that resolve to no CSS -- typos, utilities
/// this crate doesn't implement, or variants that can't apply to a PDF.
/// Unknown classes are skipped silently when building the stylesheet (as an
/// unknown Tailwind class would be), so this is the way to find out which
/// ones were dropped. Classes that are plain markers rather than utilities
/// (e.g. `card`, `table-striped`) are reported too; callers filter as needed.
pub fn unresolved_classes(html: &str, theme: &Theme) -> BTreeSet<String> {
    extract_classes(html)
        .into_iter()
        .filter(|class| {
            class != "table-striped"
                && split_variant(class)
                    .and_then(|(_, base)| utilities::resolve_with(base, theme))
                    .is_none()
        })
        .collect()
}

/// Builds the CSS selector for one utility class. A plain class name (the
/// common case) becomes an ordinary `.class` selector. A class containing
/// a character that would need backslash-escaping inside a `.class`
/// selector (`.`, `:`, `/`, `[`, `]` -- real Tailwind-style names like
/// `px-2.5` or `w-1/2`) instead becomes an attribute selector,
/// `[class~="px-2.5"]`, which needs no escaping at all: the class name
/// lives inside a quoted string, not CSS identifier syntax.
///
/// This isn't cosmetic -- verified against the real layout engine, not
/// assumed: `azul-layout`'s CSS parser doesn't handle the backslash escape
/// a `.class` selector would otherwise need for these names, and a
/// malformed escaped selector silently corrupts its parse of every rule
/// that follows it in the same stylesheet (alphabetically later utility
/// classes lost their declarations entirely -- e.g. a `%BarChart` placed
/// anywhere after a `px-2.5`/`py-0.5` class in the same document rendered
/// with invisible, zero-width bars, because `.w-full` sorted after the
/// broken selector). The attribute-selector form sidesteps the escape
/// entirely and both `lightningcss` (this crate's own minifier) and
/// `azul-layout` parse it correctly.
fn class_selector(class: &str) -> String {
    if class
        .chars()
        .any(|c| matches!(c, '.' | ':' | '/' | '[' | ']'))
    {
        let escaped_value = class.replace('\\', "\\\\").replace('"', "\\\"");
        format!("[class~=\"{escaped_value}\"]")
    } else {
        format!(".{class}")
    }
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
    // `class_selector` never emits a backslash (see its own doc comment
    // for why: azul-layout's parser can't handle one and silently drops
    // every later rule in the stylesheet). `lightningcss`'s own minifier
    // can still reintroduce one by unquoting an attribute selector's
    // value back into an escaped bare identifier -- verified empirically,
    // not assumed: `[class~="px-2.5"]` came back out as
    // `[class~=px-2\.5]`. Falling back to the unminified (but always
    // backslash-free) input is a bigger stylesheet, never a broken one.
    if out.code.contains('\\') {
        return None;
    }
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

    /// A regression guard for a real bug: a class needing escaping in a
    /// `.class` selector (`px-2.5`, containing a literal dot) used to be
    /// emitted as `.px-2\.5{...}` -- valid CSS, but azul-layout's own
    /// parser (not lightningcss, which handles it fine) can't parse that
    /// backslash escape and silently drops every rule after it in the
    /// stylesheet, corrupting unrelated components' layout further down
    /// the same document. No selector this crate emits may contain a
    /// backslash; a class needing escaping goes through an attribute
    /// selector instead, which needs no escaping.
    #[test]
    fn a_class_needing_escaping_never_produces_a_backslash_selector() {
        let html = r#"<div class="px-2.5 py-0.5"></div>"#;
        let css = build_stylesheet(html);
        assert!(!css.contains('\\'), "{css}");
        assert!(css.contains(r#"[class~="px-2.5"]"#), "{css}");
        assert!(css.contains(r#"[class~="py-0.5"]"#), "{css}");
    }

    /// The actual failure mode this bug produced: with the broken escaped
    /// selector, `.w-full` (alphabetically after `px-2.5`) lost its
    /// declaration entirely once fed through azul-layout, even though a
    /// static string check here can't see that -- this at least locks
    /// down that both classes still get real declarations from this
    /// crate's own output.
    #[test]
    fn a_dotted_class_does_not_shadow_a_later_alphabetical_classs_rule() {
        let html = r#"<div class="px-2.5"></div><div class="w-full"></div>"#;
        let css = build_stylesheet(html);
        assert!(css.contains("width:100%"), "{css}");
    }

    #[test]
    fn print_variant_applies_unconditionally_and_screen_never() {
        let css = build_stylesheet(r#"<div class="print:hidden screen:block"></div>"#);
        assert!(css.contains("display:none"), "{css}");
        assert!(!css.contains("display:block"), "{css}");
    }

    #[test]
    fn structural_variants_emit_pseudo_class_rules() {
        let css = build_stylesheet(
            r#"<ul><li class="first:pt-0 last:border-b-0 odd:bg-slate-50 even:bg-white"></li></ul>"#,
        );
        assert!(css.contains(":first-child"), "{css}");
        assert!(css.contains(":last-child"), "{css}");
        assert!(
            css.contains("nth-child(odd)") || css.contains("nth-child(2n+1)"),
            "{css}"
        );
        assert!(
            css.contains("nth-child(even)") || css.contains("nth-child(2n)"),
            "{css}"
        );
        assert!(!css.contains('\\'), "{css}");
    }

    #[test]
    fn unresolved_classes_reports_typos_and_unsupported_variants() {
        let theme = Theme::light();
        let unresolved = unresolved_classes(
            r#"<div class="p-4 flexx hover:bg-red-500 print:hidden"></div>"#,
            &theme,
        );
        assert!(unresolved.contains("flexx"));
        assert!(unresolved.contains("hover:bg-red-500"));
        assert!(!unresolved.contains("p-4"));
        assert!(!unresolved.contains("print:hidden"));
    }
}
