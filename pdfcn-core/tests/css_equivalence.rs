//! The stylesheet minifier that replaced lightningcss (docs/spikes/009) must
//! not change what the layout engine draws.
//!
//! For every utility in a broad corpus (`fixtures/utility-corpus.txt`: every
//! prefix and literal `pdfcn-styles` recognises, crossed with its scales,
//! colours and arbitrary values in every colour syntax), in the light and the
//! dark theme, the same document is rendered with the stylesheet as the
//! current minifier writes it and as lightningcss wrote it, and the pages'
//! drawing operations are compared. A block that differs is narrowed down to
//! the classes responsible, so a failure names them.
//!
//! The reference is exactly what the old code shipped: lightningcss's output,
//! or -- whenever that output contained a backslash, which it did for every
//! class needing an attribute selector, `w-1/2` and `p-2.5` included -- the
//! unminified stylesheet. On that second path the layout engine converted
//! `hsl()` itself, rounding a channel up to 1/255 differently from the hex
//! written now, so colours are compared to within 1/255; and it could not
//! read `rgb(0 0 0/50%)` at all, which now paints
//! (`arbitrary_colours_paint_what_they_ask_for`). Every other difference fails.

use std::collections::BTreeSet;

use lightningcss::stylesheet::{MinifyOptions, ParserOptions, PrinterOptions, StyleSheet};
use pdfcn_core::{render_html_with_theme, render_pdf, NoPartials, RenderOptions, Theme};
use printpdf::{parse_pdf_from_bytes, Op, PdfParseOptions};

/// The minifier as it was before docs/spikes/009, verbatim.
fn lightningcss_minify(css: &str) -> Option<String> {
    let mut sheet = StyleSheet::parse(css, ParserOptions::default()).ok()?;
    sheet.minify(MinifyOptions::default()).ok()?;
    let out = sheet
        .to_css(PrinterOptions {
            minify: true,
            ..PrinterOptions::default()
        })
        .ok()?;
    if out.code.contains('\\') {
        return None;
    }
    Some(out.code)
}

/// A colour function written with spaces (`bg-[rgb(0_0_0/50%)]`): what the
/// old unminified path could not read, and which broke the rules after it.
fn space_syntax_colour(class: &str) -> bool {
    ["rgb(", "rgba(", "hsl(", "hsla("]
        .iter()
        .any(|f| class.split(f).nth(1).is_some_and(|args| args.contains('_')))
}

fn corpus(theme: &Theme) -> Vec<String> {
    let text = include_str!("fixtures/utility-corpus.txt");
    text.lines()
        .filter(|class| pdfcn_styles::resolve_with(class, theme).is_some())
        .filter(|class| !space_syntax_colour(class))
        .map(str::to_string)
        .collect()
}

fn ops(html: &str) -> Vec<Vec<Op>> {
    let bytes = render_pdf(html, &RenderOptions::default()).expect("renders");
    let mut warnings = Vec::new();
    let doc = parse_pdf_from_bytes(&bytes, &PdfParseOptions::default(), &mut warnings)
        .expect("re-parses");
    doc.pages.into_iter().map(|p| p.ops).collect()
}

/// The document for `classes`, rendered with the current stylesheet and with
/// the one the old code shipped in its place.
fn both(classes: &[String], theme: &Theme) -> (Vec<Vec<Op>>, Vec<Vec<Op>>) {
    let html = document(classes, theme);
    let used: BTreeSet<String> = pdfcn_styles::extract_classes(&html);
    let current = pdfcn_styles::build_stylesheet_from_classes(&used, theme);
    let raw = pdfcn_styles::build_stylesheet_unminified(&used, theme);
    let old = lightningcss_minify(&raw).unwrap_or(raw);
    assert!(
        html.contains(&current),
        "the generated stylesheet is in the document"
    );
    let old_html = html.replacen(&current, &old, 1);
    (ops(&html), ops(&old_html))
}

/// Equal, except that colours may differ by 1/255 in a channel.
fn same(new: &[Vec<Op>], old: &[Vec<Op>]) -> bool {
    let close = |a: &printpdf::Color, b: &printpdf::Color| match (a, b) {
        (printpdf::Color::Rgb(a), printpdf::Color::Rgb(b)) => [(a.r, b.r), (a.g, b.g), (a.b, b.b)]
            .iter()
            .all(|(x, y)| (x - y).abs() <= 1.5 / 255.0),
        _ => a == b,
    };
    new.len() == old.len()
        && new.iter().zip(old).all(|(a, b)| {
            a.len() == b.len()
                && a.iter().zip(b).all(|(x, y)| match (x, y) {
                    (Op::SetFillColor { col: a }, Op::SetFillColor { col: b })
                    | (Op::SetOutlineColor { col: a }, Op::SetOutlineColor { col: b }) => {
                        close(a, b)
                    }
                    _ => x == y,
                })
        })
}

fn document(classes: &[String], theme: &Theme) -> String {
    let mut template = String::from("%div\n");
    for class in classes {
        let escaped = class.replace('"', "&quot;");
        template.push_str(&format!("  %div(class=\"{escaped}\") Ag {escaped}\n"));
    }
    render_html_with_theme(&template, &serde_json::json!({}), &NoPartials, theme).expect("html")
}

/// The classes whose drawing differs.
fn differing(classes: &[String], theme: &Theme) -> Vec<String> {
    let (new, old) = both(classes, theme);
    if same(&new, &old) {
        return Vec::new();
    }
    if classes.len() == 1 {
        return classes.to_vec();
    }
    let (a, b) = classes.split_at(classes.len() / 2);
    let mut out = differing(a, theme);
    out.extend(differing(b, theme));
    if out.is_empty() {
        // Only together: report the block.
        out.push(format!(
            "(the block {:?} .. {:?})",
            classes.first(),
            classes.last()
        ));
    }
    out
}

#[test]
fn every_utility_draws_the_same_as_before() {
    let mut failures = Vec::new();
    let mut checked = 0;
    for theme in [Theme::light(), Theme::dark()] {
        let classes = corpus(&theme);
        checked += classes.len();
        for block in classes.chunks(64) {
            for class in differing(block, &theme) {
                failures.push(format!("{:?}: {class}", theme.mode));
            }
        }
    }
    assert!(checked > 1000, "the corpus resolved only {checked} classes");
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Fill colours a document paints, as 0..=255 RGB.
fn fills(html: &str) -> Vec<[u8; 3]> {
    ops(html)
        .into_iter()
        .flatten()
        .filter_map(|op| match op {
            Op::SetFillColor {
                col: printpdf::Color::Rgb(c),
            } => {
                let b = |v: f32| (v * 255.0).round() as u8;
                Some([b(c.r), b(c.g), b(c.b)])
            }
            _ => None,
        })
        .collect()
}

/// Where lightningcss gave no reference: the colour is painted, in every
/// syntax, which the old unminified fallback could not do for the
/// space-separated forms.
#[test]
fn arbitrary_colours_paint_what_they_ask_for() {
    let theme = Theme::light();
    for (class, rgb) in [
        ("bg-[#2563eb]", [0x25, 0x63, 0xeb]),
        ("bg-[rgb(255,0,0)]", [255, 0, 0]),
        ("bg-[rgb(10_20_30)]", [10, 20, 30]),
        ("bg-[hsl(210_40%_96%)]", [0xf1, 0xf5, 0xf9]),
        ("bg-[hsl(210,40%,96%)]", [0xf1, 0xf5, 0xf9]),
    ] {
        let html = document(&[class.to_string()], &theme);
        assert!(fills(&html).contains(&rgb), "{class} did not paint {rgb:?}");
    }
}

/// The bug the old path had and this one does not: once one class sent the
/// stylesheet out unminified (`bg-[#2563eb]` here), a space-separated colour
/// was unreadable to the layout engine, and the fills of the rules after it
/// were lost with it.
#[test]
fn a_space_separated_colour_no_longer_costs_the_rules_after_it() {
    let theme = Theme::light();
    let classes: Vec<String> = [
        "bg-[#2563eb]",
        "bg-[rgb(0_0_0/50%)]",
        "bg-red-500",
        "bg-sky-500",
    ]
    .iter()
    .map(|c| c.to_string())
    .collect();
    let (new, old) = both(&classes, &theme);
    let count = |pages: &[Vec<Op>]| {
        pages
            .iter()
            .flatten()
            .filter(|op| matches!(op, Op::SetFillColor { .. }))
            .count()
    };
    let html = document(&classes, &theme);
    for rgb in [[0x25, 0x63, 0xeb], [0xef, 0x44, 0x44], [0x0e, 0xa5, 0xe9]] {
        assert!(fills(&html).contains(&rgb), "{rgb:?} not painted");
    }
    assert!(
        count(&new) > count(&old),
        "the old stylesheet drew as much: {} vs {}",
        count(&old),
        count(&new)
    );
}

/// The other bug of the old unminified path, found by the golden snapshots:
/// with a class that forced it (`w-1/2` here) the layout engine got
/// `border-color: hsl(214.3, 31.8%, 91.4%)`, which its `border-color` parser
/// (four space-separated sides) cannot read -- so shadcn's border token came
/// out black, or not at all. As hex it is the slate border it should be.
#[test]
fn a_border_takes_its_token_colour_whatever_else_the_document_uses() {
    let theme = Theme::light();
    let classes = vec!["w-1/2 border border-border".to_string()];
    let (new, old) = both(&classes, &theme);
    let border = [226, 232, 240];
    let paints = |pages: &[Vec<Op>]| {
        pages.iter().flatten().any(|op| {
            matches!(op, Op::SetFillColor { col: printpdf::Color::Rgb(c) } | Op::SetOutlineColor { col: printpdf::Color::Rgb(c) }
                if [c.r, c.g, c.b].map(|v| (v * 255.0).round() as u8) == border)
        })
    };
    assert!(paints(&new), "the border token is not painted");
    assert!(
        !paints(&old),
        "the old path painted it after all: the comment above is wrong"
    );
}
