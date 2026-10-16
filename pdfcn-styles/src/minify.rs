//! The stylesheet's last step: compact whitespace and write every colour the
//! way the layout engine reads it (docs/spikes/009).
//!
//! The stylesheet is pdfcn's own (one generated rule per utility class, plus
//! fixed print rules), so this is not a general CSS minifier: it only has to
//! leave what `azul-css` parses unchanged in meaning. Verified against
//! `azul-css`'s colour parser: it reads hex (3, 4, 6 and 8 digits), names, and
//! `rgb()`/`rgba()`/`hsl()`/`hsla()` with **comma-separated** arguments only --
//! the space-separated form with `/ alpha` that an arbitrary value
//! (`bg-[rgb(0_0_0/50%)]`) brings is a parse error, which used to be fixed by
//! lightningcss rewriting every colour to hex. So: every `rgb`/`rgba`/`hsl`/
//! `hsla` call becomes hex here, rounded the way lightningcss rounded it, and
//! a bare number where a length belongs gets `px`, as lightningcss did.
//! Anything this does not recognise is left exactly as written.

/// Properties whose value is a length (or several): a unitless non-zero
/// number there is read as px, as lightningcss wrote it out.
const LENGTH_PROPERTIES: &[&str] = &[
    "width",
    "height",
    "min-width",
    "min-height",
    "max-width",
    "max-height",
    "top",
    "right",
    "bottom",
    "left",
    "gap",
    "row-gap",
    "column-gap",
    "margin",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "padding",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "border-width",
    "border-top-width",
    "border-right-width",
    "border-bottom-width",
    "border-left-width",
    "border-radius",
    "font-size",
    "text-indent",
    "grid-template-columns",
    "grid-template-rows",
];

/// `css` with its whitespace compacted and its colour functions as hex.
pub(crate) fn minify(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    for rule in split_rules(css) {
        let Some((selector, body)) = rule.split_once('{') else {
            continue;
        };
        let selector = compact_selector(selector);
        if selector.is_empty() {
            continue;
        }
        let body = body.trim_end_matches('}');
        let declarations: Vec<String> = body
            .split(';')
            .filter_map(|decl| {
                let (property, value) = decl.split_once(':')?;
                let property = property.trim();
                let value = squeeze(value);
                if property.is_empty() || value.is_empty() {
                    return None;
                }
                let value = hex_colours(&value);
                let value = if LENGTH_PROPERTIES.contains(&property) {
                    px_for_bare_numbers(&value)
                } else {
                    value
                };
                Some(format!("{property}:{value}"))
            })
            .collect();
        if declarations.is_empty() {
            continue;
        }
        out.push_str(&selector);
        out.push('{');
        out.push_str(&declarations.join(";"));
        out.push('}');
    }
    out
}

/// Top-level `selector{...}` rules, braces balanced and quotes respected.
fn split_rules(css: &str) -> Vec<&str> {
    let mut rules = Vec::new();
    let (mut depth, mut quote, mut start) = (0usize, None::<char>, 0usize);
    for (i, c) in css.char_indices() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '{') => depth += 1,
            (None, '}') => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    rules.push(css[start..=i].trim());
                    start = i + 1;
                }
            }
            _ => {}
        }
    }
    rules
}

/// One space between selector parts, none around `,`; quoted attribute
/// values untouched.
fn compact_selector(selector: &str) -> String {
    let squeezed = squeeze(selector);
    let mut out = String::with_capacity(squeezed.len());
    let mut quote = None;
    let chars: Vec<char> = squeezed.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        match quote {
            Some(q) => {
                out.push(c);
                if c == q {
                    quote = None;
                }
            }
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                out.push(c);
            }
            None if c == ' ' && (chars.get(i + 1) == Some(&',') || out.ends_with(',')) => {}
            None => out.push(c),
        }
    }
    out
}

/// Whitespace runs collapsed to one space, outside quotes; trimmed.
fn squeeze(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut quote = None;
    let mut space = false;
    for c in text.trim().chars() {
        if quote.is_none() && c.is_whitespace() {
            space = true;
            continue;
        }
        if space {
            out.push(' ');
            space = false;
        }
        match quote {
            Some(q) if c == q => quote = None,
            None if c == '"' || c == '\'' => quote = Some(c),
            _ => {}
        }
        out.push(c);
    }
    out
}

/// Every `rgb(...)`, `rgba(...)`, `hsl(...)`, `hsla(...)` in `value` as hex;
/// a call that does not parse is kept as written.
fn hex_colours(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some((at, name)) = next_colour_call(rest) {
        let open = at + name.len();
        let Some(close_rel) = rest[open..].find(')') else {
            break;
        };
        let close = open + close_rel;
        out.push_str(&rest[..at]);
        let args = &rest[open + 1..close];
        let colour = if name.starts_with("hsl") {
            parse_hsl(args)
        } else {
            parse_rgb(args)
        };
        match colour {
            Some(rgba) => out.push_str(&hex(rgba)),
            None => out.push_str(&rest[at..=close]),
        }
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out
}

/// The first colour function in `text` that starts a word, and its name.
fn next_colour_call(text: &str) -> Option<(usize, &'static str)> {
    let lower = text.to_ascii_lowercase();
    ["rgba(", "rgb(", "hsla(", "hsl("]
        .iter()
        .filter_map(|name| {
            let mut from = 0;
            while let Some(rel) = lower[from..].find(name) {
                let at = from + rel;
                let starts_word = at == 0
                    || !lower.as_bytes()[at - 1].is_ascii_alphanumeric()
                        && lower.as_bytes()[at - 1] != b'-';
                if starts_word {
                    return Some((at, &name[..name.len() - 1]));
                }
                from = at + name.len();
            }
            None
        })
        .min_by_key(|(at, _)| *at)
}

/// The arguments of a colour call, comma- or space-separated, with an
/// optional `/ alpha`: up to three components and an alpha.
fn components(args: &str) -> Option<(Vec<&str>, Option<&str>)> {
    let (main, slash_alpha) = match args.split_once('/') {
        Some((m, a)) => (m, Some(a.trim())),
        None => (args, None),
    };
    let parts: Vec<&str> = if main.contains(',') {
        main.split(',').map(str::trim).collect()
    } else {
        main.split_whitespace().collect()
    };
    match (parts.len(), slash_alpha) {
        (3, alpha) => Some((parts, alpha)),
        (4, None) => Some((parts[..3].to_vec(), Some(parts[3]))),
        _ => None,
    }
}

/// An alpha (`0.5`, `50%`) as 0..=1.
fn alpha(text: Option<&str>) -> Option<f32> {
    let Some(text) = text else { return Some(1.0) };
    let value = match text.strip_suffix('%') {
        Some(p) => p.trim().parse::<f32>().ok()? / 100.0,
        None => text.trim().parse::<f32>().ok()?,
    };
    value.is_finite().then(|| value.clamp(0.0, 1.0))
}

fn parse_rgb(args: &str) -> Option<[f32; 4]> {
    let (parts, a) = components(args)?;
    let channel = |p: &str| -> Option<f32> {
        let v = match p.strip_suffix('%') {
            Some(pct) => pct.trim().parse::<f32>().ok()? * 2.55,
            None => p.parse::<f32>().ok()?,
        };
        v.is_finite().then(|| v.clamp(0.0, 255.0) / 255.0)
    };
    Some([
        channel(parts[0])?,
        channel(parts[1])?,
        channel(parts[2])?,
        alpha(a)?,
    ])
}

fn parse_hsl(args: &str) -> Option<[f32; 4]> {
    let (parts, a) = components(args)?;
    let hue = parts[0]
        .strip_suffix("deg")
        .unwrap_or(parts[0])
        .trim()
        .parse::<f32>()
        .ok()?;
    let pct = |p: &str| -> Option<f32> {
        let v = p
            .strip_suffix('%')
            .unwrap_or(p)
            .trim()
            .parse::<f32>()
            .ok()?;
        v.is_finite().then(|| v.clamp(0.0, 100.0) / 100.0)
    };
    let (s, l) = (pct(parts[1])?, pct(parts[2])?);
    if !hue.is_finite() {
        return None;
    }
    // CSS Color 4, "HSL to RGB".
    let h = hue.rem_euclid(360.0);
    let f = |n: f32| {
        let k = (n + h / 30.0) % 12.0;
        let a = s * l.min(1.0 - l);
        l - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
    };
    Some([f(0.0), f(8.0), f(4.0), alpha(a)?])
}

/// `#rrggbb`, or `#rrggbbaa` when not opaque; each channel rounded to 0..=255.
fn hex([r, g, b, a]: [f32; 4]) -> String {
    let byte = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
    let (r, g, b, a) = (byte(r), byte(g), byte(b), byte(a));
    if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }
}

/// A space-separated value with every bare non-zero number given `px`.
fn px_for_bare_numbers(value: &str) -> String {
    value
        .split(' ')
        .map(|token| {
            let bare = token.parse::<f64>().is_ok_and(|n| n != 0.0);
            if bare {
                format!("{token}px")
            } else {
                token.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_become_hex_in_every_syntax() {
        assert_eq!(hex_colours("hsl(222.2, 84%, 4.9%)"), "#020817");
        assert_eq!(hex_colours("hsl(210 40% 96%)"), "#f1f5f9");
        assert_eq!(hex_colours("hsl(210 40% 96%/0.5)"), "#f1f5f980");
        assert_eq!(hex_colours("rgb(0 0 0/50%)"), "#00000080");
        assert_eq!(hex_colours("rgba(0,0,0,0.5)"), "#00000080");
        assert_eq!(hex_colours("rgb(255,0,0)"), "#ff0000");
        assert_eq!(
            hex_colours("0 1px 3px rgba(0,0,0,0.1)"),
            "0 1px 3px #0000001a"
        );
        // Not colours, or not parseable: left alone.
        assert_eq!(hex_colours("oklch(0.7 0.1 200)"), "oklch(0.7 0.1 200)");
        assert_eq!(hex_colours("rgb(1,2)"), "rgb(1,2)");
        assert_eq!(hex_colours("url(hsl(.png)"), "url(hsl(.png)");
    }

    #[test]
    fn whitespace_is_compacted_but_quoted_selectors_survive() {
        let css = "\n@page { margin: 0; }\nhtml, body { color : red ; }\n\
                   [class~=\"px-2.5\"]{padding-left:0.625rem;padding-right:0.625rem}\n\
                   .table-striped tr:nth-child(even){background-color:hsl(210, 40%, 96.1%)}\n";
        assert_eq!(
            minify(css),
            "@page{margin:0}html,body{color:red}\
             [class~=\"px-2.5\"]{padding-left:0.625rem;padding-right:0.625rem}\
             .table-striped tr:nth-child(even){background-color:#f1f5f9}"
        );
    }

    #[test]
    fn a_bare_length_gets_px_and_nothing_else_does() {
        assert_eq!(minify(".a{border-radius:3}"), ".a{border-radius:3px}");
        assert_eq!(minify(".a{line-height:3}"), ".a{line-height:3}");
        assert_eq!(minify(".a{margin:0 auto}"), ".a{margin:0 auto}");
    }

    #[test]
    fn malformed_input_degrades_never_panics() {
        for css in [
            "",
            "{",
            "}",
            "a{",
            "a{b}",
            "a{:;}",
            ".x{color:rgb(}",
            "\"{",
            "a{b:c}}}",
        ] {
            let _ = minify(css);
        }
    }
}
