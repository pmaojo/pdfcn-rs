//! `%Icon`: a vector icon by name, at any size, colour and stroke width.
//!
//! A curated set of [Tabler Icons](https://tabler.io/icons) (MIT) is built in
//! (`pdfcn icons` lists them). Tabler is the choice because its outline set is
//! the largest permissively-licensed one, drawn on one 24x24 grid with one
//! stroke weight, and because the stroke and colour are plain SVG attributes
//! that can be set per use. Anything outside the bundled subset, from Tabler's
//! other ~5,000 icons to Lucide, Heroicons or your own artwork, can be added as
//! an SVG file with `--icons-dir` (or [`register_icon`] from Rust) and used the
//! same way.
//!
//! ```haml
//! %Icon(name="receipt" size="20px" color="#0f766e" stroke="1.5")
//! ```
//!
//! The markup carries only `<img src="pdfcn-icon:<hex of name|color|stroke>">`;
//! `pdfcn-core`'s asset pass builds the SVG with [`icon_svg`] and rasterises it
//! at print density, like every other vector placeholder. Needs pdfcn-core's
//! `vector` feature (on by default).
//!
//! The bundled drawings are © Paweł Kuna, MIT; see `LICENSE-tabler-icons`.

use std::collections::BTreeMap;
use std::sync::{OnceLock, RwLock};

use maud::{html, Markup};
use pdfcn_template::ResolvedAttr;

use crate::{attr, invalid_component};

mod data;

/// What a registered SVG may weigh. Icons are tiny; this keeps a stray large
/// file from being copied into every rendered document.
const MAX_ICON_BYTES: usize = 64 * 1024;

/// Colour used when none is given: plain black, like an un-styled Tabler SVG.
const DEFAULT_COLOR: &str = "#000000";

type Registry = RwLock<BTreeMap<String, String>>;

/// Icons added at run time: full SVG documents, by name.
fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| RwLock::new(BTreeMap::new()))
}

/// Adds (or replaces) an icon from a complete SVG document. A registered icon
/// wins over a bundled one with the same name. `currentColor` in the SVG takes
/// the icon's colour, and a `stroke-width` on its root takes the `stroke`
/// attribute.
pub fn register_icon(name: &str, svg: &str) -> Result<(), String> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
    {
        return Err(format!(
            "icon name \"{name}\" must be lowercase letters, digits, \"-\" or \"_\""
        ));
    }
    if svg.len() > MAX_ICON_BYTES {
        return Err(format!(
            "icon \"{name}\" is larger than {MAX_ICON_BYTES} bytes"
        ));
    }
    let lower = svg.to_ascii_lowercase();
    if !lower.contains("<svg") {
        return Err(format!("icon \"{name}\" is not an SVG document"));
    }
    // Nothing here runs scripts or loads other files, but an icon has no use for them.
    for forbidden in [
        "<script",
        "<foreignobject",
        "<image",
        "<use",
        "xlink:href",
        "href=",
        "<!entity",
    ] {
        if lower.contains(forbidden) {
            return Err(format!(
                "icon \"{name}\" uses {forbidden}, which icons may not"
            ));
        }
    }
    registry()
        .write()
        .map_err(|_| "icon registry is poisoned".to_string())?
        .insert(name.to_string(), svg.to_string());
    Ok(())
}

/// Every available icon name: the bundled ones and those registered, sorted.
pub fn icon_names() -> Vec<String> {
    let mut names: Vec<String> = data::ICONS.iter().map(|(n, _)| n.to_string()).collect();
    if let Ok(r) = registry().read() {
        names.extend(r.keys().cloned());
    }
    names.sort();
    names.dedup();
    names
}

/// The Tabler Icons release the bundled drawings come from.
pub fn tabler_version() -> &'static str {
    data::TABLER_VERSION
}

fn bundled(name: &str) -> Option<&'static str> {
    data::ICONS
        .binary_search_by(|(n, _)| n.cmp(&name))
        .ok()
        .map(|i| data::ICONS[i].1)
}

/// A colour a SVG attribute can carry: `#rgb`, `#rrggbb`, a name, or
/// `rgb()`/`hsl()` and friends. Letters, digits and `# ( ) , . % /` and spaces
/// only: nothing that could close the attribute or open a tag.
fn valid_color(color: &str) -> bool {
    !color.is_empty()
        && color.len() <= 48
        && color.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '#' | '(' | ')' | ',' | '.' | '%' | '/' | ' ')
        })
}

/// A stroke width between 0.25 and 4 (Tabler draws at 2).
fn valid_stroke(stroke: &str) -> Option<f64> {
    stroke
        .parse::<f64>()
        .ok()
        .filter(|s| s.is_finite() && (0.25..=4.0).contains(s))
}

/// Replaces the value of `stroke-width` on the root `<svg ...>` tag.
fn with_root_stroke_width(svg: &str, width: &str) -> String {
    let Some(tag_end) = svg.find('>') else {
        return svg.to_string();
    };
    let (tag, rest) = svg.split_at(tag_end);
    let Some(at) = tag.find("stroke-width=\"") else {
        return svg.to_string();
    };
    let value_start = at + "stroke-width=\"".len();
    let Some(len) = tag[value_start..].find('"') else {
        return svg.to_string();
    };
    format!(
        "{}{}{}{}",
        &tag[..value_start],
        width,
        &tag[value_start + len..],
        rest
    )
}

/// The complete SVG for `name`, or `None` if there is no such icon or the
/// colour or stroke is not acceptable.
pub fn icon_svg(name: &str, color: &str, stroke: Option<&str>) -> Option<String> {
    if !valid_color(color) {
        return None;
    }
    let stroke_width = match stroke {
        Some(s) => Some(valid_stroke(s)?.to_string()),
        None => None,
    };
    if let Some(svg) = registry().read().ok().and_then(|r| r.get(name).cloned()) {
        let svg = svg.replace("currentColor", color);
        return Some(match &stroke_width {
            Some(w) => with_root_stroke_width(&svg, w),
            None => svg,
        });
    }
    let inner = bundled(name)?.replace("currentColor", color);
    Some(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"24\" height=\"24\" viewBox=\"0 0 24 24\" \
         fill=\"none\" stroke=\"{color}\" stroke-width=\"{}\" stroke-linecap=\"round\" \
         stroke-linejoin=\"round\">{inner}</svg>",
        stroke_width.as_deref().unwrap_or("2")
    ))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn error(message: &str) -> Markup {
    html! {
        div class=(invalid_component()) { "Icon: " (message) }
    }
}

pub fn icon(attrs: &[ResolvedAttr]) -> Markup {
    let Some(name) = attr(attrs, "name").filter(|n| !n.is_empty()) else {
        return error("a \"name\" is required (see `pdfcn icons`)");
    };
    let color = attr(attrs, "color").unwrap_or(DEFAULT_COLOR);
    let stroke = attr(attrs, "stroke");
    // Check now, not at rasterisation, so a mistake shows up in the document.
    if icon_svg(name, color, stroke).is_none() {
        return if !valid_color(color) {
            error(&format!("colour \"{color}\" is not a plain colour value"))
        } else if stroke.is_some_and(|s| valid_stroke(s).is_none()) {
            error(&format!(
                "stroke \"{}\" must be a number from 0.25 to 4",
                stroke.unwrap_or("")
            ))
        } else {
            error(&format!(
                "no icon named \"{name}\" (see `pdfcn icons`, or add it with --icons-dir)"
            ))
        };
    }
    let size = attr(attrs, "size").unwrap_or("24px");
    let alt = attr(attrs, "alt").unwrap_or("");
    let payload = format!("{name}|{color}|{}", stroke.unwrap_or(""));
    html! {
        img class="icon inline-block align-middle" src=(format!("pdfcn-icon:{}", hex(payload.as_bytes()))) style={ "width:" (size) ";height:" (size) ";" } alt=(alt);
    }
}

/// Decodes a `pdfcn-icon:` payload back into `(name, colour, stroke)`.
pub fn decode_payload(hex_payload: &str) -> Option<(String, String, Option<String>)> {
    if hex_payload.len() % 2 != 0 {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..hex_payload.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(hex_payload.get(i..i + 2)?, 16).ok())
        .collect();
    let text = String::from_utf8(bytes?).ok()?;
    let mut parts = text.splitn(3, '|');
    let name = parts.next()?.to_string();
    let color = parts.next()?.to_string();
    let stroke = parts.next().filter(|s| !s.is_empty()).map(str::to_string);
    Some((name, color, stroke))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(name: &str, value: &str) -> ResolvedAttr {
        ResolvedAttr {
            name: name.into(),
            value: value.into(),
        }
    }

    #[test]
    fn the_bundled_table_is_sorted_so_lookups_work() {
        let names: Vec<&str> = data::ICONS.iter().map(|(n, _)| *n).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
        for (name, _) in data::ICONS {
            assert!(bundled(name).is_some(), "{name}");
        }
        assert!(names.len() >= 150, "{}", names.len());
    }

    #[test]
    fn every_bundled_icon_is_a_wellformed_drawing() {
        for (name, inner) in data::ICONS {
            assert!(inner.contains('<'), "{name} is empty");
            assert!(
                !inner.contains("<script") && !inner.contains("href"),
                "{name}"
            );
            let svg = icon_svg(name, "#123456", None).unwrap_or_default();
            assert!(svg.starts_with("<svg") && svg.ends_with("</svg>"), "{name}");
            assert_eq!(svg.matches('<').count(), svg.matches('>').count(), "{name}");
        }
    }

    #[test]
    fn colour_and_stroke_are_applied_to_the_svg() {
        let svg = icon_svg("receipt", "#0f766e", Some("1.5")).unwrap();
        assert!(svg.contains(r##"stroke="#0f766e""##), "{svg}");
        assert!(svg.contains(r#"stroke-width="1.5""#), "{svg}");
        assert!(!svg.contains("currentColor"));
        let plain = icon_svg("receipt", "black", None).unwrap();
        assert!(plain.contains(r#"stroke-width="2""#));
    }

    #[test]
    fn a_hostile_colour_or_stroke_never_reaches_the_svg() {
        for color in [
            "red\" onload=\"x",
            "red\"><script>",
            "url(#a)\"",
            "#fff;stroke:red",
            "red<",
            "",
            &"a".repeat(60),
        ] {
            assert!(icon_svg("receipt", color, None).is_none(), "{color}");
        }
        for stroke in ["0", "5", "NaN", "1;2", "two", "-1", "inf"] {
            assert!(
                icon_svg("receipt", "red", Some(stroke)).is_none(),
                "{stroke}"
            );
        }
    }

    #[test]
    fn unknown_icons_are_a_visible_error_not_a_blank() {
        let out = icon(&[a("name", "no-such-icon")]).into_string();
        assert!(
            out.contains("pdfcn-invalid-component") && out.contains("no-such-icon"),
            "{out}"
        );
        assert!(!out.contains("pdfcn-icon:"));
        assert!(icon(&[]).into_string().contains("name"));
        assert!(icon(&[a("name", "receipt"), a("color", "red\"x")])
            .into_string()
            .contains("colour"));
        assert!(icon(&[a("name", "receipt"), a("stroke", "9")])
            .into_string()
            .contains("stroke"));
    }

    #[test]
    fn the_component_emits_a_placeholder_with_its_size() {
        let out = icon(&[
            a("name", "receipt"),
            a("size", "32px"),
            a("color", "#0f766e"),
        ])
        .into_string();
        assert!(out.contains("src=\"pdfcn-icon:"), "{out}");
        assert!(out.contains("width:32px;height:32px;"), "{out}");
        // The SVG itself never travels in the markup.
        assert!(!out.contains("<svg") && !out.contains("<path"), "{out}");
    }

    #[test]
    fn the_payload_round_trips() {
        let out = icon(&[
            a("name", "receipt"),
            a("color", "rgb(1, 2, 3)"),
            a("stroke", "1.25"),
        ])
        .into_string();
        let hex = out
            .split("pdfcn-icon:")
            .nth(1)
            .and_then(|r| r.split('"').next())
            .unwrap();
        assert_eq!(
            decode_payload(hex),
            Some(("receipt".into(), "rgb(1, 2, 3)".into(), Some("1.25".into())))
        );
        assert_eq!(decode_payload("zz"), None);
        assert_eq!(decode_payload("abc"), None);
    }

    #[test]
    fn registered_icons_take_the_colour_and_stroke_and_win_over_bundled_ones() {
        register_icon(
            "xx-test-custom",
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10" stroke="currentColor" stroke-width="3"><path d="M0 0L10 10"/></svg>"#,
        )
        .unwrap();
        let svg = icon_svg("xx-test-custom", "#ff0000", Some("1")).unwrap();
        assert!(
            svg.contains(r##"stroke="#ff0000""##) && svg.contains(r#"stroke-width="1""#),
            "{svg}"
        );
        assert!(icon_names().contains(&"xx-test-custom".to_string()));
        assert!(icon_names().contains(&"receipt".to_string()));
    }

    #[test]
    fn registration_refuses_unsafe_or_malformed_icons() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0"/></svg>"#;
        assert!(register_icon("Bad Name", svg).is_err());
        assert!(register_icon("", svg).is_err());
        assert!(register_icon("ok-name", "<p>not svg</p>").is_err());
        for evil in [
            r#"<svg><script>alert(1)</script></svg>"#,
            r#"<svg><foreignObject/></svg>"#,
            r#"<svg><image href="http://x/y.png"/></svg>"#,
            r##"<svg><use xlink:href="#a"/></svg>"##,
        ] {
            assert!(register_icon("ok-name", evil).is_err(), "{evil}");
        }
        assert!(
            register_icon("big", &format!("<svg>{}</svg>", "x".repeat(MAX_ICON_BYTES))).is_err()
        );
    }
}
