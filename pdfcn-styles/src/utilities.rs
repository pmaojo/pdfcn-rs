//! A small, hand-rolled subset of Tailwind's utility classes, resolved
//! algorithmically (spacing scale, color palette) rather than via a lookup
//! table for every class name. Not a full Tailwind JIT — enough for
//! document layout: spacing, flex/grid, typography, color, borders.

use crate::theme::Theme;
use crate::tokens;

const SPACING: &[(&str, &str)] = &[
    ("0", "0"),
    ("px", "1px"),
    ("0.5", "0.125rem"),
    ("1", "0.25rem"),
    ("1.5", "0.375rem"),
    ("2", "0.5rem"),
    ("2.5", "0.625rem"),
    ("3", "0.75rem"),
    ("4", "1rem"),
    ("5", "1.25rem"),
    ("6", "1.5rem"),
    ("8", "2rem"),
    ("10", "2.5rem"),
    ("12", "3rem"),
    ("16", "4rem"),
    ("20", "5rem"),
    ("24", "6rem"),
    // The scale used to stop here, so every one of Tailwind's larger steps
    // (`h-48`, `w-64`, `p-96`, ...) silently resolved to nothing -- not a
    // smaller box, no CSS rule at all. `%Card`'s own cover image uses `h-48`
    // (pdfcn-components/src/lib.rs) and was hitting exactly this gap.
    ("28", "7rem"),
    ("32", "8rem"),
    ("36", "9rem"),
    ("40", "10rem"),
    ("44", "11rem"),
    ("48", "12rem"),
    ("52", "13rem"),
    ("56", "14rem"),
    ("60", "15rem"),
    ("64", "16rem"),
    ("72", "18rem"),
    ("80", "20rem"),
    ("96", "24rem"),
];

const FONT_SIZE: &[(&str, &str)] = &[
    ("xs", "0.75rem"),
    ("sm", "0.875rem"),
    ("base", "1rem"),
    ("lg", "1.125rem"),
    ("xl", "1.25rem"),
    ("2xl", "1.5rem"),
    ("3xl", "1.875rem"),
    ("4xl", "2.25rem"),
    ("5xl", "3rem"),
    ("6xl", "3.75rem"),
    ("7xl", "4.5rem"),
    ("8xl", "6rem"),
    ("9xl", "8rem"),
];

/// Tailwind's own max-width container scale (not the spacing scale --
/// `max-w-xl` is 36rem there, while `p-xl` isn't a thing).
const MAX_WIDTH: &[(&str, &str)] = &[
    ("xs", "20rem"),
    ("sm", "24rem"),
    ("md", "28rem"),
    ("lg", "32rem"),
    ("xl", "36rem"),
    ("2xl", "42rem"),
    ("3xl", "48rem"),
    ("4xl", "56rem"),
    ("5xl", "64rem"),
    ("6xl", "72rem"),
    ("7xl", "80rem"),
];

const PALETTE: &[(&str, &str)] = &[
    ("slate-50", "#f8fafc"),
    ("slate-100", "#f1f5f9"),
    ("slate-200", "#e2e8f0"),
    ("slate-300", "#cbd5e1"),
    ("slate-500", "#64748b"),
    ("slate-700", "#334155"),
    ("slate-900", "#0f172a"),
    ("gray-100", "#f3f4f6"),
    ("gray-200", "#e5e7eb"),
    ("gray-500", "#6b7280"),
    ("gray-900", "#111827"),
    ("red-500", "#ef4444"),
    ("red-600", "#dc2626"),
    ("green-500", "#22c55e"),
    ("green-600", "#16a34a"),
    ("blue-500", "#3b82f6"),
    ("blue-600", "#2563eb"),
    ("amber-500", "#f59e0b"),
    ("white", "#ffffff"),
    ("black", "#000000"),
    ("transparent", "transparent"),
];

/// Fractional/edge-to-edge values `top-*`/`inset-*`/etc. accept in addition
/// to the spacing scale (Tailwind's inset scale mixes both).
const INSET_FRACTIONS: &[(&str, &str)] = &[
    ("1/2", "50%"),
    ("1/3", "33.333333%"),
    ("2/3", "66.666667%"),
    ("1/4", "25%"),
    ("2/4", "50%"),
    ("3/4", "75%"),
    ("full", "100%"),
];

fn lookup<'a>(table: &'a [(&str, &str)], key: &str) -> Option<&'a str> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// Parses a Tailwind-style arbitrary value -- the `[...]` suffix of
/// `w-[220px]`, `p-[13px]`, `bg-[#0ea5e9]` -- into the literal CSS value,
/// so callers aren't stuck on the fixed spacing/size/color scales. Only
/// two shapes are accepted, matching how the utilities below use it:
/// a CSS length (`px`/`rem`/`em`/`%`, optional `-` sign) or a hex color
/// (`#rgb`/`#rrggbb`). Anything else returns `None` -- an unknown shape
/// degrades to "class not recognized", never to emitting unvalidated CSS.
fn arbitrary_value(bracketed: &str) -> Option<String> {
    let inner = bracketed.strip_prefix('[')?.strip_suffix(']')?;
    let inner = inner.trim();
    if inner.is_empty() {
        return None;
    }
    // `rgb(10,20,30)`, `hsl(210_40%_98%)`, `rgb(0_0_0/50%)`.
    if let Some(color) = color_function(inner) {
        return Some(color);
    }
    // `calc(100%-2rem)`: lengths combined with + - * /.
    if let Some(calc) = calc_expression(inner) {
        return Some(calc);
    }
    // Hex color: #rgb or #rrggbb.
    if let Some(hex) = inner.strip_prefix('#') {
        return (matches!(hex.len(), 3 | 6) && hex.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| format!("#{hex}"));
    }
    // CSS length: optional '-', decimal digits, one optional unit
    // (px/rem/em/%; a bare number is accepted too, as the engine treats
    // unitless lengths as px).
    let magnitude = inner.strip_prefix('-').unwrap_or(inner);
    let (number, unit) = magnitude
        .find(|c: char| c.is_ascii_alphabetic() || c == '%')
        .map_or((magnitude, ""), |split| {
            (&magnitude[..split], &magnitude[split..])
        });
    let valid_number = !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit() || c == '.')
        && number.chars().filter(|c| *c == '.').count() <= 1
        && number.parse::<f64>().is_ok();
    let valid_unit = matches!(unit, "" | "px" | "rem" | "em" | "%");
    (valid_number && valid_unit).then(|| {
        let sign = if inner.starts_with('-') { "-" } else { "" };
        format!("{sign}{number}{unit}")
    })
}

/// `rgb()`/`rgba()`/`hsl()`/`hsla()` with only the characters a colour needs.
/// Tailwind writes spaces as `_` inside a class name; both are accepted.
fn color_function(inner: &str) -> Option<String> {
    let name_end = inner.find('(')?;
    if !matches!(&inner[..name_end], "rgb" | "rgba" | "hsl" | "hsla") || !inner.ends_with(')') {
        return None;
    }
    let args = &inner[name_end + 1..inner.len() - 1];
    if args.is_empty()
        || !args
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '%' | '/' | '_' | ' ' | '-'))
    {
        return None;
    }
    Some(format!(
        "{}({})",
        &inner[..name_end],
        args.replace('_', " ")
    ))
}

/// `grid-cols-[1fr_2fr]`, `grid-cols-[200px_1fr]`, `grid-cols-[repeat(3,1fr)]`:
/// a track list, with `_` standing for a space. Only the characters a track
/// list needs are accepted.
fn arbitrary_tracks(bracketed: &str) -> Option<String> {
    let inner = bracketed.strip_prefix('[')?.strip_suffix(']')?.trim();
    if inner.is_empty()
        || !inner.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '.' | '%' | ',' | '(' | ')' | '_' | '-')
        })
    {
        return None;
    }
    Some(inner.replace('_', " "))
}

/// Whether a resolved arbitrary value is a colour rather than a length.
fn is_color_literal(value: &str) -> bool {
    value.starts_with('#') || value.starts_with("rgb") || value.starts_with("hsl")
}

/// `calc(...)` over lengths. Operators are put between spaces the way CSS
/// requires (`calc(100%-2rem)` is written without them in a class name), and
/// only digits, units and the four operators are allowed.
fn calc_expression(inner: &str) -> Option<String> {
    let body = inner.strip_prefix("calc(")?.strip_suffix(')')?;
    if body.is_empty()
        || !body.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '.' | '%' | '+' | '-' | '*' | '/' | '(' | ')' | '_' | ' ')
        })
    {
        return None;
    }
    // Letters are only ever units (or a nested `calc`): no `url(`, `var(`, ...
    let mut word = String::new();
    for c in body.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_alphabetic() {
            word.push(c);
        } else {
            if !word.is_empty()
                && !matches!(word.as_str(), "px" | "rem" | "em" | "vw" | "vh" | "calc")
            {
                return None;
            }
            word.clear();
        }
    }
    let chars: Vec<char> = body.replace('_', " ").chars().collect();
    let mut out = String::new();
    let mut previous: Option<char> = None; // last significant (non-space) char
    for (i, c) in chars.iter().enumerate() {
        match c {
            ' ' => {}
            '+' | '-' | '*' | '/' => {
                // A sign is binary only after an operand; `calc(-2rem + 4px)` starts with a unary minus.
                let after_operand = previous
                    .is_some_and(|p| p.is_ascii_alphanumeric() || matches!(p, '%' | ')' | '.'));
                let next_is_operand = chars[i + 1..].iter().find(|c| **c != ' ').is_some();
                if after_operand && next_is_operand {
                    out.push(' ');
                    out.push(*c);
                    out.push(' ');
                } else {
                    out.push(*c);
                }
                previous = Some(*c);
                continue;
            }
            _ => {
                if previous.is_some_and(|p| p.is_ascii_alphanumeric() || matches!(p, '%' | ')'))
                    && chars[..i].last() == Some(&' ')
                    && c.is_ascii_alphanumeric()
                {
                    // Two operands separated only by whitespace is not a valid expression.
                    return None;
                }
                out.push(*c);
                previous = Some(*c);
            }
        }
    }
    let open = out.matches('(').count();
    let close = out.matches(')').count();
    (open == close).then(|| format!("calc({out})"))
}

/// The value for a size-style utility suffix: the spacing scale first,
/// then an arbitrary `[...]` length. (`px`/`rem`/`em`/`%`/unitless.)
fn scale_or_arbitrary(key: &str) -> Option<String> {
    lookup(SPACING, key)
        .map(str::to_string)
        .or_else(|| arbitrary_value(key))
}

/// Converts a `Nrem` length to the equivalent px at CSS's 16px root size;
/// anything else passes through unchanged. Used where the engine's
/// rem handling is known-broken (border-radius), not as a general
/// normalization.
fn rem_to_px(value: &str) -> String {
    if let Some(magnitude) = value.strip_suffix("rem") {
        if let Ok(n) = magnitude.parse::<f64>() {
            return format!("{}px", n * 16.0);
        }
    }
    value.to_string()
}

/// Resolves a color-utility suffix (the part after `bg-`/`text-`/`border-`)
/// against, in order: the hand-picked palette, the theme (per-token brand
/// overrides first, then the light/dark token table -- see
/// [`crate::theme::Theme`]), then the full Tailwind/shadcn scales. The
/// palette keeps winning where it overlaps with a token, as before; an
/// explicit override wins over both, since overriding is exactly how a
/// caller rebrands a semantic token.
fn resolve_color(key: &str, theme: &Theme) -> Option<String> {
    lookup(PALETTE, key)
        .map(str::to_string)
        .or_else(|| theme.token(key).map(str::to_string))
        .or_else(|| tokens::scale_color(key).map(str::to_string))
        .or_else(|| arbitrary_value(key).filter(|v| is_color_literal(v)))
}

/// Margin utilities, including `auto` (`mx-auto` centering) and the leading
/// `-` negative form (`-mt-2`, `-mx-4`).
fn margin_utility(class: &str) -> Option<String> {
    let (negative, body) = match class.strip_prefix('-') {
        Some(rest) if rest.starts_with('m') => (true, rest),
        _ => (false, class),
    };
    let (props, rest): (&[&str], &str) = if let Some(r) = body.strip_prefix("mx-") {
        (&["margin-left", "margin-right"], r)
    } else if let Some(r) = body.strip_prefix("my-") {
        (&["margin-top", "margin-bottom"], r)
    } else if let Some(r) = body.strip_prefix("mt-") {
        (&["margin-top"], r)
    } else if let Some(r) = body.strip_prefix("mr-") {
        (&["margin-right"], r)
    } else if let Some(r) = body.strip_prefix("mb-") {
        (&["margin-bottom"], r)
    } else if let Some(r) = body.strip_prefix("ml-") {
        (&["margin-left"], r)
    } else {
        (&["margin"], body.strip_prefix("m-")?)
    };
    let value = if rest == "auto" {
        if negative {
            return None;
        }
        "auto".to_string()
    } else {
        let v = scale_or_arbitrary(rest)?;
        if negative && v != "0" && !v.starts_with('-') {
            format!("-{v}")
        } else {
            v
        }
    };
    Some(
        props
            .iter()
            .map(|p| format!("{p}:{value}"))
            .collect::<Vec<_>>()
            .join(";"),
    )
}

/// A Tailwind fraction (`1/2`, `2/3`, `5/12`) as a CSS percentage.
fn fraction_percent(key: &str) -> Option<String> {
    let (a, b) = key.split_once('/')?;
    let (a, b): (u32, u32) = (a.parse().ok()?, b.parse().ok()?);
    if !(2..=12).contains(&b) || a == 0 || a >= b {
        return None;
    }
    let pct = f64::from(a) / f64::from(b) * 100.0;
    let text = format!("{pct:.6}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    Some(format!("{text}%"))
}

/// Shared sizing suffixes for `w-`/`h-`/`min-w-`/...: keywords and
/// fractions on top of the spacing scale. `axis_viewport` is the viewport
/// unit for `screen` (`vw`/`vh`).
fn size_value(key: &str, axis_viewport: &str) -> Option<String> {
    match key {
        "full" => Some("100%".into()),
        "screen" => Some(format!("100{axis_viewport}")),
        "auto" => Some("auto".into()),
        "fit" => Some("fit-content".into()),
        "min" => Some("min-content".into()),
        "max" => Some("max-content".into()),
        _ => fraction_percent(key).or_else(|| scale_or_arbitrary(key)),
    }
}

/// Crate-visible color resolver (palette, theme tokens, scales, arbitrary
/// hex, `/NN` opacity) for passes that compose several classes into one
/// declaration (`composite.rs`).
pub(crate) fn color_value(key: &str, theme: &Theme) -> Option<String> {
    resolve_color_alpha(key, theme)
}

/// Resolves a color suffix with an optional `/NN` opacity (`black/50`).
/// Hex and `hsl(...)` colors gain an alpha channel; anything else (named
/// `transparent`) is returned unchanged.
fn resolve_color_alpha(key: &str, theme: &Theme) -> Option<String> {
    // A `/` inside brackets belongs to the value (`[rgb(0_0_0/50%)]`); the
    // opacity suffix comes after them (`[#0f766e]/50`).
    if key.ends_with(']') {
        return resolve_color(key, theme);
    }
    let Some((name, alpha)) = key.rsplit_once('/') else {
        return resolve_color(key, theme);
    };
    let alpha: u8 = alpha.parse().ok().filter(|a| *a <= 100)?;
    let base = resolve_color(name, theme)?;
    let a = f64::from(alpha) / 100.0;
    if let Some(hex) = base.strip_prefix('#') {
        let full: String = match hex.len() {
            3 => hex.chars().flat_map(|c| [c, c]).collect(),
            6 => hex.to_string(),
            _ => return None,
        };
        let channel = |i: usize| u8::from_str_radix(&full[i..i + 2], 16).ok();
        return Some(format!(
            "rgba({},{},{},{a})",
            channel(0)?,
            channel(2)?,
            channel(4)?
        ));
    }
    if let Some(inner) = base.strip_prefix("hsl(").and_then(|r| r.strip_suffix(')')) {
        return Some(format!("hsla({inner}, {a})"));
    }
    if let Some(inner) = base.strip_prefix("rgb(").and_then(|r| r.strip_suffix(')')) {
        // Only the comma form takes a fourth value; a modern `rgb(r g b / a)`
        // already carries its own alpha and is left alone.
        if inner.matches(',').count() == 2 {
            return Some(format!("rgba({inner},{a})"));
        }
    }
    Some(base)
}

/// A size-like value for transform utilities: Tailwind's spacing scale, a
/// fraction of the box (`1/2` -> `50%`) or an arbitrary `[...]` length.
pub(crate) fn offset_value(key: &str) -> Option<String> {
    if key == "full" {
        return Some("100%".to_string());
    }
    fraction_percent(key).or_else(|| scale_or_arbitrary(key))
}

fn spacing_decls(props: &[&str], scale: &str) -> Option<String> {
    let value = scale_or_arbitrary(scale)?;
    Some(
        props
            .iter()
            .map(|p| format!("{p}:{value}"))
            .collect::<Vec<_>>()
            .join(";"),
    )
}

/// Whether `class` (with any leading `-` already stripped) names an offset
/// utility -- used to decide whether a leading `-` means "negative offset"
/// versus being part of some other, unrelated class name.
fn is_offset_class(class: &str) -> bool {
    class.starts_with("top-")
        || class.starts_with("right-")
        || class.starts_with("bottom-")
        || class.starts_with("left-")
        || class.starts_with("inset-")
}

/// Resolves an offset (`top-*`, `inset-*`, ...) suffix to the spacing
/// scale, plus the fractional/`full` values Tailwind's inset scale adds on
/// top of it. `negative` pulls an absolutely-positioned overlay -- a badge,
/// a price tag -- half off an edge (`-top-2`).
fn inset_decls(props: &[&str], key: &str, negative: bool) -> Option<String> {
    let value = lookup(SPACING, key)
        .map(str::to_string)
        .or_else(|| lookup(INSET_FRACTIONS, key).map(str::to_string))
        .or_else(|| arbitrary_value(key))?;
    let value = if negative && value != "0" {
        format!("-{value}")
    } else {
        value.to_string()
    };
    Some(
        props
            .iter()
            .map(|p| format!("{p}:{value}"))
            .collect::<Vec<_>>()
            .join(";"),
    )
}

/// Resolves one utility class name to its CSS declaration block (without
/// braces), e.g. `"p-4"` -> `"padding:1rem"`. Returns `None` for classes
/// this subset doesn't recognize (they're silently skipped, matching how
/// an unknown Tailwind class would just do nothing without a build step).
///
/// Theme-aware: semantic token utilities (`bg-primary`, ...) resolve
/// through `theme` -- see [`crate::theme::Theme`]. Most callers want
/// [`resolve_with`]; this light-mode default stays for callers without a
/// document theme.
pub fn resolve(class: &str) -> Option<String> {
    resolve_with(class, &Theme::light())
}

/// Like [`resolve`], but semantic tokens resolve through `theme`'s mode
/// and overrides.
pub fn resolve_with(class: &str, theme: &Theme) -> Option<String> {
    if let Some(rest) = class.strip_prefix("p-") {
        return spacing_decls(&["padding"], rest);
    }
    if let Some(rest) = class.strip_prefix("px-") {
        return spacing_decls(&["padding-left", "padding-right"], rest);
    }
    if let Some(rest) = class.strip_prefix("py-") {
        return spacing_decls(&["padding-top", "padding-bottom"], rest);
    }
    if let Some(rest) = class.strip_prefix("pt-") {
        return spacing_decls(&["padding-top"], rest);
    }
    if let Some(rest) = class.strip_prefix("pr-") {
        return spacing_decls(&["padding-right"], rest);
    }
    if let Some(rest) = class.strip_prefix("pb-") {
        return spacing_decls(&["padding-bottom"], rest);
    }
    if let Some(rest) = class.strip_prefix("pl-") {
        return spacing_decls(&["padding-left"], rest);
    }
    if let Some(decls) = margin_utility(class) {
        return Some(decls);
    }
    // Axis-specific gaps must be checked before the bare `gap-` prefix,
    // which would otherwise swallow them (`gap-x-4` -> scale "x-4") and
    // return None without ever reaching these branches.
    // Multi-column text: `columns-2` -> `column-count: 2`.
    if let Some(rest) = class.strip_prefix("columns-") {
        let n: u32 = rest.parse().ok().filter(|n| (1..=12).contains(n))?;
        return Some(format!("column-count:{n}"));
    }
    // First-line indent, on the spacing scale: `indent-4`.
    if let Some(rest) = class.strip_prefix("indent-") {
        return spacing_decls(&["text-indent"], rest);
    }
    if let Some(rest) = class.strip_prefix("gap-x-") {
        return spacing_decls(&["column-gap"], rest);
    }
    if let Some(rest) = class.strip_prefix("gap-y-") {
        return spacing_decls(&["row-gap"], rest);
    }
    if let Some(rest) = class.strip_prefix("gap-") {
        return spacing_decls(&["gap"], rest);
    }
    if let Some(rest) = class.strip_prefix("min-h-") {
        return size_value(rest, "vh").map(|v| format!("min-height:{v}"));
    }
    if let Some(rest) = class.strip_prefix("min-w-") {
        return size_value(rest, "vw").map(|v| format!("min-width:{v}"));
    }
    if let Some(rest) = class.strip_prefix("size-") {
        return size_value(rest, "vw").map(|v| format!("width:{v};height:{v}"));
    }
    if let Some(rest) = class.strip_prefix("h-") {
        return size_value(rest, "vh").map(|v| format!("height:{v}"));
    }
    if let Some(rest) = class.strip_prefix("w-") {
        return size_value(rest, "vw").map(|v| format!("width:{v}"));
    }
    // --- Ola 1.4 fidelity additions: purely static, high-use utilities.
    // Named leading steps first, then the numeric scale (`leading-6` is
    // Tailwind's spacing scale applied to line-height).
    if let Some(rest) = class.strip_prefix("leading-") {
        if let Some(named) = match rest {
            "none" => Some("1"),
            "tight" => Some("1.25"),
            "snug" => Some("1.375"),
            "normal" => Some("1.5"),
            "relaxed" => Some("1.625"),
            "loose" => Some("2"),
            _ => None,
        } {
            return Some(format!("line-height:{named}"));
        }
        return spacing_decls(&["line-height"], rest);
    }
    if let Some(rest) = class.strip_prefix("tracking-") {
        let value = match rest {
            "tighter" => "-0.05em",
            "tight" => "-0.025em",
            "normal" => "0em",
            "wide" => "0.025em",
            "wider" => "0.05em",
            "widest" => "0.1em",
            // `tracking-[0.05em]`, `tracking-[1px]`.
            _ => return arbitrary_value(rest).map(|v| format!("letter-spacing:{v}")),
        };
        return Some(format!("letter-spacing:{value}"));
    }
    if let Some(rest) = class.strip_prefix("max-w-") {
        if let Some(value) = lookup(MAX_WIDTH, rest) {
            return Some(format!("max-width:{value}"));
        }
        if let Some(literal) = match rest {
            "none" => Some("none"),
            "full" => Some("100%"),
            "prose" => Some("65ch"),
            "fit" => Some("fit-content"),
            "min" => Some("min-content"),
            "max" => Some("max-content"),
            _ => None,
        } {
            return Some(format!("max-width:{literal}"));
        }
        return spacing_decls(&["max-width"], rest);
    }
    if let Some(rest) = class.strip_prefix("max-h-") {
        if rest == "none" {
            return Some("max-height:none".to_string());
        }
        return spacing_decls(&["max-height"], rest);
    }
    if let Some(rest) = class.strip_prefix("opacity-") {
        // Whole percentages 0-100 only (Tailwind's scale steps by 5); an
        // out-of-range number is an unrecognized class, not a clamped one.
        return rest
            .parse::<u8>()
            .ok()
            .filter(|n| *n <= 100)
            .map(|n| format!("opacity:{n}%"));
    }
    if let Some(rest) = class.strip_prefix("col-span-") {
        if rest == "full" {
            return Some("grid-column:1/-1".to_string());
        }
        return rest
            .parse::<u32>()
            .ok()
            .filter(|n| (1..=12).contains(n))
            .map(|n| format!("grid-column:span {n}/span {n}"));
    }
    if let Some(rest) = class.strip_prefix("aspect-") {
        let ratio = match rest {
            "auto" => "auto",
            "square" => "1/1",
            "video" => "16/9",
            _ => return None,
        };
        return Some(format!("aspect-ratio:{ratio}"));
    }
    match class {
        "border-collapse" => return Some("border-collapse:collapse".into()),
        "border-separate" => return Some("border-collapse:separate".into()),
        "text-left" => return Some("text-align:left".into()),
        "text-center" => return Some("text-align:center".into()),
        "text-right" => return Some("text-align:right".into()),
        "text-justify" => return Some("text-align:justify".into()),
        "text-ellipsis" => return Some("text-overflow:ellipsis".into()),
        "border-solid" => return Some("border-style:solid".into()),
        "border-dashed" => return Some("border-style:dashed".into()),
        "border-dotted" => return Some("border-style:dotted".into()),
        "border-double" => return Some("border-style:double".into()),
        "border-none" => return Some("border-style:none".into()),
        "border-x" => {
            return Some("border-left-width:1px;border-right-width:1px;border-style:solid".into())
        }
        "border-y" => {
            return Some("border-top-width:1px;border-bottom-width:1px;border-style:solid".into())
        }
        _ => {}
    }
    for (prefix, props) in [
        (
            "rounded-t",
            &["border-top-left-radius", "border-top-right-radius"][..],
        ),
        (
            "rounded-b",
            &["border-bottom-left-radius", "border-bottom-right-radius"][..],
        ),
        (
            "rounded-l",
            &["border-top-left-radius", "border-bottom-left-radius"][..],
        ),
        (
            "rounded-r",
            &["border-top-right-radius", "border-bottom-right-radius"][..],
        ),
        ("rounded-tl", &["border-top-left-radius"][..]),
        ("rounded-tr", &["border-top-right-radius"][..]),
        ("rounded-bl", &["border-bottom-left-radius"][..]),
        ("rounded-br", &["border-bottom-right-radius"][..]),
    ] {
        // Bare `rounded-t` is Tailwind's `0.25rem`, same as bare `rounded`.
        let value = if class == prefix {
            Some("4px".to_string())
        } else {
            class
                .strip_prefix(prefix)
                .and_then(|r| r.strip_prefix('-'))
                .and_then(|size| {
                    tokens::radius(size)
                        .map(str::to_string)
                        .or_else(|| arbitrary_value(size))
                        .map(|v| rem_to_px(&v))
                })
        };
        if class == prefix || class.starts_with(&format!("{prefix}-")) {
            let value = value?;
            return Some(
                props
                    .iter()
                    .map(|p| format!("{p}:{value}"))
                    .collect::<Vec<_>>()
                    .join(";"),
            );
        }
    }
    // Directional border widths and colors (`border-t`, `border-b-2`,
    // `border-l-border`). Must sit before the generic `border-` branch,
    // whose prefix would otherwise swallow the side letter.
    for (prefix, prop) in [
        ("border-t-", "border-top"),
        ("border-r-", "border-right"),
        ("border-b-", "border-bottom"),
        ("border-l-", "border-left"),
    ] {
        if let Some(rest) = class.strip_prefix(prefix) {
            if rest == "0" {
                return Some(format!("{prop}-width:0;border-style:solid"));
            }
            if let Ok(n) = rest.parse::<u32>() {
                return Some(format!("{prop}-width:{n}px;border-style:solid"));
            }
            if let Some(color) = resolve_color(rest, theme) {
                return Some(format!("{prop}-color:{color}"));
            }
            return None;
        }
    }
    // A negative offset (`-top-2`, pulling an overlay half off an edge) puts
    // the `-` before the property name, not after it like every other
    // negative Tailwind utility would -- strip it once here so the prefix
    // checks below see the same `<prop>-<scale>` shape either way.
    let (offset_negative, offset_class) = match class.strip_prefix('-') {
        Some(rest) if is_offset_class(rest) => (true, rest),
        _ => (false, class),
    };
    if let Some(rest) = offset_class.strip_prefix("inset-x-") {
        return inset_decls(&["left", "right"], rest, offset_negative);
    }
    if let Some(rest) = offset_class.strip_prefix("inset-y-") {
        return inset_decls(&["top", "bottom"], rest, offset_negative);
    }
    if let Some(rest) = offset_class.strip_prefix("inset-") {
        return inset_decls(&["top", "right", "bottom", "left"], rest, offset_negative);
    }
    if let Some(rest) = offset_class.strip_prefix("top-") {
        return inset_decls(&["top"], rest, offset_negative);
    }
    if let Some(rest) = offset_class.strip_prefix("right-") {
        return inset_decls(&["right"], rest, offset_negative);
    }
    if let Some(rest) = offset_class.strip_prefix("bottom-") {
        return inset_decls(&["bottom"], rest, offset_negative);
    }
    if let Some(rest) = offset_class.strip_prefix("left-") {
        return inset_decls(&["left"], rest, offset_negative);
    }
    if let Some(rest) = class.strip_prefix("z-") {
        return match rest {
            "auto" => Some("z-index:auto".to_string()),
            n => n.parse::<i32>().ok().map(|v| format!("z-index:{v}")),
        };
    }
    if let Some(rest) = class.strip_prefix("object-") {
        let literal = match rest {
            "contain" => "object-fit:contain",
            "cover" => "object-fit:cover",
            "fill" => "object-fit:fill",
            "none" => "object-fit:none",
            "scale-down" => "object-fit:scale-down",
            "top" => "object-position:top",
            "bottom" => "object-position:bottom",
            "center" => "object-position:center",
            "left" => "object-position:left",
            "right" => "object-position:right",
            _ => return None,
        };
        return Some(literal.to_string());
    }
    // `text-shadow` is not a Tailwind v3 utility; v4 spells it like this.
    if let Some(rest) = class.strip_prefix("text-shadow") {
        let value = match rest {
            "" | "-sm" => "0 1px 2px rgba(0,0,0,0.20)",
            "-md" => "0 2px 4px rgba(0,0,0,0.22)",
            "-lg" => "0 3px 6px rgba(0,0,0,0.25)",
            "-none" => "none",
            _ => return None,
        };
        return Some(format!("text-shadow:{value}"));
    }
    if let Some(rest) = class.strip_prefix("text-") {
        if let Some(size) = lookup(FONT_SIZE, rest) {
            return Some(format!("font-size:{size}"));
        }
        // Arbitrary values: a hex (`text-[#334155]`) is a color, anything
        // else (`text-[15px]`) is a font size.
        if let Some(value) = arbitrary_value(rest) {
            return if is_color_literal(&value) {
                Some(format!("color:{value}"))
            } else {
                Some(format!("font-size:{value}"))
            };
        }
        if let Some(color) = resolve_color_alpha(rest, theme) {
            return Some(format!("color:{color}"));
        }
        return None;
    }
    if let Some(rest) = class.strip_prefix("bg-") {
        let color = resolve_color_alpha(rest, theme)?;
        return Some(format!("background-color:{color}"));
    }
    if let Some(rest) = class.strip_prefix("border-") {
        // A bare side border (`border-t`) is a 1px width on that side,
        // exactly like bare `border` is on all four.
        if let Some(prop) = match rest {
            "t" => Some("border-top"),
            "r" => Some("border-right"),
            "b" => Some("border-bottom"),
            "l" => Some("border-left"),
            _ => None,
        } {
            return Some(format!("{prop}-width:1px;border-style:solid"));
        }
        // `border-2`, `border-4`, ...: Tailwind's all-sides width scale.
        if let Some(n) = rest
            .parse::<u32>()
            .ok()
            .filter(|n| matches!(n, 0 | 1 | 2 | 4 | 8))
        {
            return Some(if n == 0 {
                "border-width:0".to_string()
            } else {
                format!("border-width:{n}px;border-style:solid")
            });
        }
        if let Some(color) = resolve_color_alpha(rest, theme) {
            return Some(format!("border-color:{color}"));
        }
        // `border-[2px]`: an arbitrary length is a border width.
        if let Some(value) =
            arbitrary_value(rest).filter(|v| !is_color_literal(v) && !v.starts_with("calc"))
        {
            return Some(format!("border-width:{value};border-style:solid"));
        }
        return None;
    }
    if let Some(rest) = class.strip_prefix("rounded-") {
        let value = tokens::radius(rest)
            .map(str::to_string)
            .or_else(|| arbitrary_value(rest))?;
        // px, never rem: the engine treats a rem-unit border-radius as 0
        // (see the comment on `tokens::RADIUS`). Convert, don't rename.
        let value = rem_to_px(&value);
        return Some(format!("border-radius:{value}"));
    }
    if let Some(rest) = class.strip_prefix("shadow-") {
        let value = tokens::shadow(rest)?;
        return Some(format!("box-shadow:{value}"));
    }
    if let Some(rest) = class.strip_prefix("grid-rows-") {
        if let Some(tracks) = arbitrary_tracks(rest) {
            return Some(format!("grid-template-rows:{tracks}"));
        }
        let n: u32 = rest.parse().ok().filter(|n| (1..=12).contains(n))?;
        return Some(format!("grid-template-rows:repeat({n}, minmax(0, 1fr))"));
    }
    if let Some(rest) = class.strip_prefix("row-span-") {
        if rest == "full" {
            return Some("grid-row:1/-1".to_string());
        }
        return rest
            .parse::<u32>()
            .ok()
            .filter(|n| (1..=12).contains(n))
            .map(|n| format!("grid-row:span {n}/span {n}"));
    }
    if let Some(rest) = class.strip_prefix("grid-cols-") {
        if let Some(tracks) = arbitrary_tracks(rest) {
            return Some(format!("grid-template-columns:{tracks}"));
        }
        let n: u32 = rest.parse().ok()?;
        return Some(format!("grid-template-columns:repeat({n}, minmax(0, 1fr))"));
    }
    if let Some(rest) = class.strip_prefix("font-") {
        let weight = match rest {
            "thin" => "100",
            "extralight" => "200",
            "light" => "300",
            "normal" => "400",
            "medium" => "500",
            "semibold" => "600",
            "bold" => "700",
            "extrabold" => "800",
            "black" => "900",
            // The generic keywords resolve to no embedded face (the engine falls
            // back to whatever it registered first), so name the bundled
            // families instead: Inter, Source Serif 4, JetBrains Mono.
            "sans" => return Some("font-family:'Inter',sans-serif".to_string()),
            "serif" => return Some("font-family:'Source Serif 4',serif".to_string()),
            "mono" => return Some("font-family:'JetBrains Mono',monospace".to_string()),
            // `font-[600]`: any weight from 1 to 1000.
            _ => {
                let n: u32 = rest
                    .strip_prefix('[')?
                    .strip_suffix(']')?
                    .parse()
                    .ok()
                    .filter(|n| (1..=1000).contains(n))?;
                return Some(format!("font-weight:{n}"));
            }
        };
        return Some(format!("font-weight:{weight}"));
    }
    let literal: Option<&str> = match class {
        "flex" => Some("display:flex"),
        "grid" => Some("display:grid"),
        "block" => Some("display:block"),
        "inline-block" => Some("display:inline-block"),
        "hidden" => Some("display:none"),
        "flex-row" => Some("flex-direction:row"),
        "flex-col" => Some("flex-direction:column"),
        "flex-wrap" => Some("flex-wrap:wrap"),
        "items-start" => Some("align-items:flex-start"),
        "items-center" => Some("align-items:center"),
        "items-end" => Some("align-items:flex-end"),
        "justify-start" => Some("justify-content:flex-start"),
        "justify-center" => Some("justify-content:center"),
        "justify-between" => Some("justify-content:space-between"),
        "justify-end" => Some("justify-content:flex-end"),
        "inline-flex" => Some("display:inline-flex"),
        "relative" => Some("position:relative"),
        "absolute" => Some("position:absolute"),
        "fixed" => Some("position:fixed"),
        "sticky" => Some("position:sticky"),
        "static" => Some("position:static"),
        "overflow-hidden" => Some("overflow:hidden"),
        "overflow-auto" => Some("overflow:auto"),
        "caption-bottom" => Some("caption-side:bottom"),
        "italic" => Some("font-style:italic"),
        "uppercase" => Some("text-transform:uppercase"),
        "lowercase" => Some("text-transform:lowercase"),
        "capitalize" => Some("text-transform:capitalize"),
        "normal-case" => Some("text-transform:none"),
        "not-italic" => Some("font-style:normal"),
        "underline" => Some("text-decoration:underline"),
        "line-through" => Some("text-decoration:line-through"),
        "no-underline" => Some("text-decoration:none"),
        "truncate" => Some("overflow:hidden;text-overflow:ellipsis;white-space:nowrap"),
        "whitespace-normal" => Some("white-space:normal"),
        "whitespace-nowrap" => Some("white-space:nowrap"),
        "whitespace-pre" => Some("white-space:pre"),
        "whitespace-pre-line" => Some("white-space:pre-line"),
        "whitespace-pre-wrap" => Some("white-space:pre-wrap"),
        "break-words" => Some("overflow-wrap:break-word"),
        "break-all" => Some("word-break:break-all"),
        "align-top" => Some("vertical-align:top"),
        "align-middle" => Some("vertical-align:middle"),
        "align-bottom" => Some("vertical-align:bottom"),
        "align-baseline" => Some("vertical-align:baseline"),
        "align-sub" => Some("vertical-align:sub"),
        "align-super" => Some("vertical-align:super"),
        "align-text-top" => Some("vertical-align:text-top"),
        "align-text-bottom" => Some("vertical-align:text-bottom"),
        "inline" => Some("display:inline"),
        "table" => Some("display:table"),
        "table-row" => Some("display:table-row"),
        "table-cell" => Some("display:table-cell"),
        "invisible" => Some("visibility:hidden"),
        "visible" => Some("visibility:visible"),
        "overflow-visible" => Some("overflow:visible"),
        "overflow-x-hidden" => Some("overflow-x:hidden"),
        "overflow-y-hidden" => Some("overflow-y:hidden"),
        "overflow-x-auto" => Some("overflow-x:auto"),
        "overflow-y-auto" => Some("overflow-y:auto"),
        "flex-row-reverse" => Some("flex-direction:row-reverse"),
        "flex-col-reverse" => Some("flex-direction:column-reverse"),
        "flex-nowrap" => Some("flex-wrap:nowrap"),
        "flex-wrap-reverse" => Some("flex-wrap:wrap-reverse"),
        "flex-auto" => Some("flex:1 1 auto"),
        "flex-initial" => Some("flex:0 1 auto"),
        "flex-none" => Some("flex:none"),
        "grow" => Some("flex-grow:1"),
        "grow-0" => Some("flex-grow:0"),
        "shrink" => Some("flex-shrink:1"),
        "shrink-0" => Some("flex-shrink:0"),
        "justify-around" => Some("justify-content:space-around"),
        "justify-evenly" => Some("justify-content:space-evenly"),
        "items-stretch" => Some("align-items:stretch"),
        "items-baseline" => Some("align-items:baseline"),
        "self-auto" => Some("align-self:auto"),
        "self-start" => Some("align-self:flex-start"),
        "self-center" => Some("align-self:center"),
        "self-end" => Some("align-self:flex-end"),
        "self-stretch" => Some("align-self:stretch"),
        "border" => Some("border-width:1px;border-style:solid"),
        // px, not the equivalent 0.25rem: see the comment on `tokens::RADIUS`.
        "rounded" => Some("border-radius:4px"),
        "shadow" => Some("box-shadow:0 1px 3px rgba(0,0,0,0.1)"),
        "break-inside-avoid" => Some("break-inside:avoid;page-break-inside:avoid"),
        "break-before-page" => Some("break-before:page"),
        "break-after-page" => Some("break-after:page"),
        "flex-1" => Some("flex:1 1 0%"),
        "table-bordered" => Some("border-width:2px;border-style:solid"),
        "float-left" => Some("float:left"),
        "float-right" => Some("float:right"),
        "float-none" => Some("float:none"),
        "box-border" => Some("box-sizing:border-box"),
        "box-content" => Some("box-sizing:content-box"),
        "table-fixed" => Some("table-layout:fixed"),
        "table-auto" => Some("table-layout:auto"),
        // `list-none` also drops the browser indent, like Tailwind's reset
        // does; the markers themselves are removed by `gap::rewrite_gaps`,
        // because the engine ignores `list-style-type: none`.
        "list-none" => Some("list-style-type:none;padding-left:0"),
        "list-disc" => Some("list-style-type:disc"),
        "list-decimal" => Some("list-style-type:decimal"),
        "list-inside" => Some("list-style-position:inside"),
        "list-outside" => Some("list-style-position:outside"),
        "content-normal" => Some("align-content:normal"),
        "content-start" => Some("align-content:flex-start"),
        "content-center" => Some("align-content:center"),
        "content-end" => Some("align-content:flex-end"),
        "content-between" => Some("align-content:space-between"),
        "content-around" => Some("align-content:space-around"),
        "content-evenly" => Some("align-content:space-evenly"),
        "content-stretch" => Some("align-content:stretch"),
        "justify-items-start" => Some("justify-items:start"),
        "justify-items-center" => Some("justify-items:center"),
        "justify-items-end" => Some("justify-items:end"),
        "justify-items-stretch" => Some("justify-items:stretch"),
        // The engine ignores the `place-items` shorthand, so spell out both.
        "place-items-start" => Some("align-items:start;justify-items:start"),
        "place-items-center" => Some("align-items:center;justify-items:center"),
        "place-items-end" => Some("align-items:end;justify-items:end"),
        "place-items-stretch" => Some("align-items:stretch;justify-items:stretch"),
        "table-compact" => Some("font-size:0.75rem"),
        _ => None,
    };
    literal.map(str::to_string)
}

/// Classes that only affect how a page behaves on screen (pointer, selection,
/// transitions, animation). They have no effect in a PDF, and nothing is lost
/// by ignoring them, so they are accepted silently instead of being reported
/// as unrecognised -- real Tailwind markup carries plenty of them and
/// `--strict` should not fail on it.
pub(crate) fn is_screen_only(class: &str) -> bool {
    const EXACT: &[&str] = &[
        "transition",
        "resize",
        "outline-none",
        "appearance-none",
        "will-change-auto",
        "group",
        "peer",
    ];
    const PREFIXES: &[&str] = &[
        "cursor-",
        "pointer-events-",
        "select-",
        "resize-",
        "transition-",
        "duration-",
        "delay-",
        "ease-",
        "animate-",
        "will-change-",
        "appearance-",
        "touch-",
        "scroll-",
        "snap-",
        "caret-",
        "accent-",
    ];
    EXACT.contains(&class) || PREFIXES.iter().any(|p| class.starts_with(p))
}

/// `space-x-4` / `space-y-2`: spacing between siblings. Not a stylesheet rule:
/// `gap::rewrite_gaps` turns them into margins on the children, so they must
/// not be reported as unresolved. The scale key has to be valid, though; a typo
/// (`space-x-banana`) still is.
pub(crate) fn is_sibling_spacing(class: &str) -> bool {
    class
        .strip_prefix("space-x-")
        .or_else(|| class.strip_prefix("space-y-"))
        .is_some_and(|key| scale_or_arbitrary(key).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_shadcn_theme_tokens_through_color_utilities() {
        assert_eq!(
            resolve("bg-primary").as_deref(),
            Some("background-color:hsl(222.2, 47.4%, 11.2%)")
        );
        assert_eq!(
            resolve("text-primary-foreground").as_deref(),
            Some("color:hsl(210, 40%, 98%)")
        );
        assert_eq!(
            resolve("border-input").as_deref(),
            Some("border-color:hsl(214.3, 31.8%, 91.4%)")
        );
    }

    /// The literal palette must keep winning: a theme token is a fallback for
    /// names the palette has no entry for, never an override of one.
    #[test]
    fn palette_colors_still_win_over_tokens() {
        assert_eq!(
            resolve("bg-red-600").as_deref(),
            Some("background-color:#dc2626")
        );
    }

    /// The spacing scale used to stop at "24" (6rem); anything past it
    /// (`h-48`, used by `%Card`'s cover image, among others) resolved to
    /// `None` -- an unrecognized class, not a smaller box.
    #[test]
    fn resolves_the_extended_spacing_scale_past_24() {
        assert_eq!(resolve("h-48").as_deref(), Some("height:12rem"));
        assert_eq!(resolve("h-96").as_deref(), Some("height:24rem"));
        assert_eq!(resolve("w-64").as_deref(), Some("width:16rem"));
        assert_eq!(resolve("p-32").as_deref(), Some("padding:8rem"));
    }

    #[test]
    fn resolves_the_full_neutral_and_accent_scales() {
        assert_eq!(
            resolve("bg-slate-950").as_deref(),
            Some("background-color:#020617")
        );
        assert_eq!(
            resolve("bg-zinc-100").as_deref(),
            Some("background-color:#f4f4f5")
        );
        assert_eq!(
            resolve("bg-neutral-500").as_deref(),
            Some("background-color:#737373")
        );
        assert_eq!(
            resolve("bg-stone-800").as_deref(),
            Some("background-color:#292524")
        );
    }

    /// The new component modules (Alert/Avatar/form fields/nav) lean on
    /// height/width/min-height utilities that previously only existed as a
    /// couple of hardcoded literals (`w-full`, `h-full`).
    // --- Ola 1.4 fidelity additions.
    #[test]
    fn resolves_typography_leading_and_tracking() {
        assert_eq!(resolve("leading-none").as_deref(), Some("line-height:1"));
        assert_eq!(
            resolve("leading-relaxed").as_deref(),
            Some("line-height:1.625")
        );
        assert_eq!(resolve("leading-6").as_deref(), Some("line-height:1.5rem"));
        assert_eq!(
            resolve("tracking-wide").as_deref(),
            Some("letter-spacing:0.025em")
        );
        assert_eq!(
            resolve("tracking-widest").as_deref(),
            Some("letter-spacing:0.1em")
        );
        assert_eq!(resolve("tracking-nonsense"), None);
    }

    #[test]
    fn resolves_max_size_constraints() {
        assert_eq!(resolve("max-w-xl").as_deref(), Some("max-width:36rem"));
        assert_eq!(resolve("max-w-prose").as_deref(), Some("max-width:65ch"));
        assert_eq!(resolve("max-w-full").as_deref(), Some("max-width:100%"));
        assert_eq!(resolve("max-w-[220px]").as_deref(), Some("max-width:220px"));
        assert_eq!(resolve("max-h-16").as_deref(), Some("max-height:4rem"));
        assert_eq!(resolve("max-h-none").as_deref(), Some("max-height:none"));
    }

    #[test]
    fn resolves_directional_borders_widths_and_colors() {
        assert_eq!(
            resolve("border-t").as_deref(),
            Some("border-top-width:1px;border-style:solid")
        );
        assert_eq!(
            resolve("border-b-2").as_deref(),
            Some("border-bottom-width:2px;border-style:solid")
        );
        assert_eq!(
            resolve("border-l-0").as_deref(),
            Some("border-left-width:0;border-style:solid")
        );
        assert_eq!(
            resolve("border-r-red-500").as_deref(),
            Some("border-right-color:#ef4444")
        );
        // The generic border-color utility still works alongside them.
        assert_eq!(
            resolve("border-input").as_deref(),
            Some("border-color:hsl(214.3, 31.8%, 91.4%)")
        );
    }

    #[test]
    fn resolves_opacity_grid_span_and_aspect_ratio() {
        assert_eq!(resolve("opacity-50").as_deref(), Some("opacity:50%"));
        assert_eq!(resolve("opacity-100").as_deref(), Some("opacity:100%"));
        assert_eq!(resolve("opacity-101"), None);
        assert_eq!(
            resolve("col-span-3").as_deref(),
            Some("grid-column:span 3/span 3")
        );
        assert_eq!(
            resolve("col-span-full").as_deref(),
            Some("grid-column:1/-1")
        );
        assert_eq!(resolve("col-span-13"), None);
        assert_eq!(
            resolve("aspect-square").as_deref(),
            Some("aspect-ratio:1/1")
        );
        assert_eq!(
            resolve("aspect-video").as_deref(),
            Some("aspect-ratio:16/9")
        );
    }

    #[test]
    fn resolves_height_and_width_utilities() {
        assert_eq!(resolve("h-10").as_deref(), Some("height:2.5rem"));
        assert_eq!(resolve("w-10").as_deref(), Some("width:2.5rem"));
        assert_eq!(resolve("h-px").as_deref(), Some("height:1px"));
        assert_eq!(resolve("min-h-16").as_deref(), Some("min-height:4rem"));
        assert_eq!(resolve("w-full").as_deref(), Some("width:100%"));
        assert_eq!(resolve("h-full").as_deref(), Some("height:100%"));
    }

    /// Composing an image overlay (a badge/price tag pinned to a corner of a
    /// picture, or to anywhere on the page) needs absolute/fixed positioning
    /// plus offsets and stacking order — not just `relative`.
    #[test]
    fn resolves_absolute_positioning_and_offsets() {
        assert_eq!(resolve("absolute").as_deref(), Some("position:absolute"));
        assert_eq!(resolve("fixed").as_deref(), Some("position:fixed"));
        assert_eq!(resolve("top-4").as_deref(), Some("top:1rem"));
        assert_eq!(resolve("right-2").as_deref(), Some("right:0.5rem"));
        assert_eq!(resolve("left-0").as_deref(), Some("left:0"));
        assert_eq!(resolve("bottom-full").as_deref(), Some("bottom:100%"));
        assert_eq!(resolve("-top-2").as_deref(), Some("top:-0.5rem"));
        assert_eq!(
            resolve("inset-0").as_deref(),
            Some("top:0;right:0;bottom:0;left:0")
        );
        assert_eq!(
            resolve("inset-x-4").as_deref(),
            Some("left:1rem;right:1rem")
        );
    }

    #[test]
    fn resolves_z_index_stacking() {
        assert_eq!(resolve("z-10").as_deref(), Some("z-index:10"));
        assert_eq!(resolve("z-50").as_deref(), Some("z-index:50"));
        assert_eq!(resolve("z-auto").as_deref(), Some("z-index:auto"));
        assert_eq!(resolve("z-nope"), None);
    }

    #[test]
    fn resolves_object_fit_and_position_for_composed_images() {
        assert_eq!(resolve("object-cover").as_deref(), Some("object-fit:cover"));
        assert_eq!(
            resolve("object-contain").as_deref(),
            Some("object-fit:contain")
        );
        assert_eq!(
            resolve("object-top").as_deref(),
            Some("object-position:top")
        );
        assert_eq!(
            resolve("object-center").as_deref(),
            Some("object-position:center")
        );
    }

    /// Axis-specific gaps previously fell into the bare `gap-` prefix branch,
    /// which swallowed `gap-x-4` (scale "x-4") and returned None — silently
    /// resolving to nothing at all.
    #[test]
    fn resolves_axis_specific_gap_utilities() {
        assert_eq!(resolve("gap-x-4").as_deref(), Some("column-gap:1rem"));
        assert_eq!(resolve("gap-y-2").as_deref(), Some("row-gap:0.5rem"));
        assert_eq!(resolve("gap-4").as_deref(), Some("gap:1rem"));
    }

    #[test]
    fn resolves_the_shadow_and_radius_scales() {
        assert!(resolve("shadow-sm").is_some());
        assert!(resolve("shadow-md").is_some());
        assert!(resolve("shadow-lg").is_some());
        assert!(resolve("shadow-xl").is_some());
        assert_ne!(resolve("shadow-sm"), resolve("shadow-xl"));
        assert_eq!(
            resolve("rounded-2xl").as_deref(),
            Some("border-radius:16px")
        );
        assert_eq!(
            resolve("rounded-3xl").as_deref(),
            Some("border-radius:24px")
        );
    }

    /// Arbitrary values (`w-[220px]`, `text-[15px]`, `bg-[#0ea5e9]`, ...)
    /// escape the fixed scales without emitting unvalidated CSS.
    #[test]
    fn resolves_arbitrary_lengths_and_colors() {
        assert_eq!(resolve("w-[220px]").as_deref(), Some("width:220px"));
        assert_eq!(resolve("p-[13px]").as_deref(), Some("padding:13px"));
        assert_eq!(resolve("mt-[3.5rem]").as_deref(), Some("margin-top:3.5rem"));
        assert_eq!(resolve("h-[10%]").as_deref(), Some("height:10%"));
        assert_eq!(resolve("gap-[8px]").as_deref(), Some("gap:8px"));
        assert_eq!(resolve("top-[12px]").as_deref(), Some("top:12px"));
        assert_eq!(resolve("-top-[6px]").as_deref(), Some("top:-6px"));
        assert_eq!(resolve("text-[15px]").as_deref(), Some("font-size:15px"));
        assert_eq!(resolve("text-[#334155]").as_deref(), Some("color:#334155"));
        assert_eq!(
            resolve("bg-[#0ea5e9]").as_deref(),
            Some("background-color:#0ea5e9")
        );
        assert_eq!(
            resolve("rounded-[10px]").as_deref(),
            Some("border-radius:10px")
        );
    }

    /// Malformed or unsupported arbitrary shapes must be rejected, not
    /// passed through as CSS -- the class name is attacker-influenced input.
    #[test]
    fn rejects_malformed_arbitrary_values() {
        for class in [
            "w-[]",
            "w-[ ]",
            "w-[abc]",
            "w-[10pt]",
            "w-[..px]",
            "bg-[url(http://x)]",
            "bg-[#12345]",
            "text-[var(--x)]",
            // calc() is accepted (the engine evaluates it), but not malformed
            // or non-length contents.
            "p-[calc(1px+;color:red)]",
            "p-[calc(url(x))]",
        ] {
            assert_eq!(resolve(class), None, "{class} must not resolve");
        }
    }

    /// The engine treats a rem-unit border-radius as 0 (see tokens::RADIUS),
    /// so an arbitrary rem radius must be converted, not renamed (0.5rem is
    /// 8px, never "0.5px").
    #[test]
    fn converts_arbitrary_rem_radii_to_px() {
        assert_eq!(
            resolve("rounded-[0.5rem]").as_deref(),
            Some("border-radius:8px")
        );
    }

    #[test]
    fn an_arbitrary_border_length_is_a_width_not_a_color() {
        assert_eq!(
            resolve("border-[2px]").as_deref(),
            Some("border-width:2px;border-style:solid")
        );
        assert_eq!(
            resolve("border-[#94a3b8]").as_deref(),
            Some("border-color:#94a3b8")
        );
    }

    /// Semantic tokens resolve through the theme: dark mode flips surfaces,
    /// and an explicit override rebrands every utility built on that token.
    #[test]
    fn semantic_tokens_follow_the_theme() {
        let mut theme = crate::theme::Theme::dark();
        theme
            .overrides
            .insert("primary".to_string(), "#2563eb".to_string());
        assert_eq!(
            resolve_with("bg-background", &theme).as_deref(),
            Some("background-color:hsl(222.2, 84%, 4.9%)")
        );
        assert_eq!(
            resolve_with("bg-primary", &theme).as_deref(),
            Some("background-color:#2563eb")
        );
        // Light default stays untouched for other callers.
        assert_eq!(
            resolve("bg-primary").as_deref(),
            Some("background-color:hsl(222.2, 47.4%, 11.2%)")
        );
    }

    /// Regression: the generic `text-` branch (size / color) returns `None`
    /// for an unknown suffix, so the `text-align` literals further down the
    /// function were unreachable and `text-left`/`text-center`/`text-right`
    /// silently produced no CSS.
    #[test]
    fn resolves_text_alignment() {
        assert_eq!(resolve("text-left").as_deref(), Some("text-align:left"));
        assert_eq!(resolve("text-center").as_deref(), Some("text-align:center"));
        assert_eq!(resolve("text-right").as_deref(), Some("text-align:right"));
    }

    #[test]
    fn resolves_auto_and_negative_margins() {
        assert_eq!(
            resolve("mx-auto").as_deref(),
            Some("margin-left:auto;margin-right:auto")
        );
        assert_eq!(resolve("-mt-2").as_deref(), Some("margin-top:-0.5rem"));
        assert_eq!(resolve("-m-4").as_deref(), Some("margin:-1rem"));
        assert_eq!(resolve("-mt-0").as_deref(), Some("margin-top:0"));
        assert_eq!(resolve("-mx-auto"), None);
        assert_eq!(resolve("mt-4").as_deref(), Some("margin-top:1rem"));
    }

    #[test]
    fn resolves_fractional_and_keyword_sizes() {
        assert_eq!(resolve("w-1/2").as_deref(), Some("width:50%"));
        assert_eq!(resolve("w-1/3").as_deref(), Some("width:33.333333%"));
        assert_eq!(resolve("w-5/12").as_deref(), Some("width:41.666667%"));
        assert_eq!(resolve("w-auto").as_deref(), Some("width:auto"));
        assert_eq!(resolve("h-fit").as_deref(), Some("height:fit-content"));
        assert_eq!(resolve("min-w-0").as_deref(), Some("min-width:0"));
        assert_eq!(resolve("min-w-full").as_deref(), Some("min-width:100%"));
        assert_eq!(resolve("size-8").as_deref(), Some("width:2rem;height:2rem"));
        assert_eq!(resolve("w-3/2"), None);
        assert_eq!(resolve("w-screen").as_deref(), Some("width:100vw"));
        assert_eq!(resolve("h-screen").as_deref(), Some("height:100vh"));
    }

    #[test]
    fn resolves_color_alpha() {
        assert_eq!(
            resolve("bg-black/50").as_deref(),
            Some("background-color:rgba(0,0,0,0.5)")
        );
        assert_eq!(
            resolve("text-white/80").as_deref(),
            Some("color:rgba(255,255,255,0.8)")
        );
        assert_eq!(
            resolve("bg-primary/10").as_deref(),
            Some("background-color:hsla(222.2, 47.4%, 11.2%, 0.1)")
        );
        assert_eq!(resolve("bg-black/101"), None);
        assert_eq!(resolve("bg-nonsense/50"), None);
    }

    #[test]
    fn resolves_text_decoration_transform_and_wrapping() {
        assert_eq!(
            resolve("underline").as_deref(),
            Some("text-decoration:underline")
        );
        assert_eq!(
            resolve("line-through").as_deref(),
            Some("text-decoration:line-through")
        );
        assert_eq!(
            resolve("capitalize").as_deref(),
            Some("text-transform:capitalize")
        );
        assert_eq!(
            resolve("truncate").as_deref(),
            Some("overflow:hidden;text-overflow:ellipsis;white-space:nowrap")
        );
        assert_eq!(
            resolve("whitespace-pre-wrap").as_deref(),
            Some("white-space:pre-wrap")
        );
        assert_eq!(
            resolve("text-justify").as_deref(),
            Some("text-align:justify")
        );
        assert_eq!(resolve("text-6xl").as_deref(), Some("font-size:3.75rem"));
        assert_eq!(resolve("font-black").as_deref(), Some("font-weight:900"));
        assert_eq!(
            resolve("font-mono").as_deref(),
            Some("font-family:'JetBrains Mono',monospace")
        );
    }

    #[test]
    fn resolves_flex_alignment_literals() {
        assert_eq!(
            resolve("justify-around").as_deref(),
            Some("justify-content:space-around")
        );
        assert_eq!(resolve("self-center").as_deref(), Some("align-self:center"));
        assert_eq!(resolve("grow").as_deref(), Some("flex-grow:1"));
        assert_eq!(resolve("flex-none").as_deref(), Some("flex:none"));
        assert_eq!(resolve("shrink-0").as_deref(), Some("flex-shrink:0"));
        assert_eq!(
            resolve("flex-row-reverse").as_deref(),
            Some("flex-direction:row-reverse")
        );
    }

    #[test]
    fn resolves_border_styles_axes_and_corner_radii() {
        assert_eq!(
            resolve("border-dashed").as_deref(),
            Some("border-style:dashed")
        );
        assert_eq!(
            resolve("border-collapse").as_deref(),
            Some("border-collapse:collapse")
        );
        assert_eq!(
            resolve("border-y").as_deref(),
            Some("border-top-width:1px;border-bottom-width:1px;border-style:solid")
        );
        assert_eq!(
            resolve("rounded-t-lg").as_deref(),
            Some("border-top-left-radius:8px;border-top-right-radius:8px")
        );
        assert_eq!(
            resolve("rounded-bl").as_deref(),
            Some("border-bottom-left-radius:4px")
        );
        assert_eq!(
            resolve("rounded-tr-xl").as_deref(),
            Some("border-top-right-radius:12px")
        );
        // The all-corners utility and the unrelated colors still resolve.
        assert_eq!(resolve("rounded-lg").as_deref(), Some("border-radius:8px"));
        assert_eq!(resolve("rounded-t-nonsense"), None);
    }

    #[test]
    fn resolves_all_sides_border_widths() {
        assert_eq!(
            resolve("border-2").as_deref(),
            Some("border-width:2px;border-style:solid")
        );
        assert_eq!(resolve("border-0").as_deref(), Some("border-width:0"));
        assert_eq!(resolve("border-3"), None);
        // Colors and the directional forms are unaffected.
        assert_eq!(
            resolve("border-red-500").as_deref(),
            Some("border-color:#ef4444")
        );
        assert_eq!(
            resolve("border-b-2").as_deref(),
            Some("border-bottom-width:2px;border-style:solid")
        );
    }

    #[test]
    fn resolves_text_shadow_and_vertical_align_sub_super() {
        assert_eq!(
            resolve("text-shadow").as_deref(),
            Some("text-shadow:0 1px 2px rgba(0,0,0,0.20)")
        );
        assert_eq!(
            resolve("text-shadow-none").as_deref(),
            Some("text-shadow:none")
        );
        assert_eq!(resolve("text-shadow-xl"), None);
        assert_eq!(
            resolve("align-super").as_deref(),
            Some("vertical-align:super")
        );
        assert_eq!(resolve("align-sub").as_deref(), Some("vertical-align:sub"));
        // The text size/color branch is unaffected.
        assert_eq!(resolve("text-lg").as_deref(), Some("font-size:1.125rem"));
    }

    // --- Utilities added after measuring Tailwind coverage against the engine.
    #[test]
    fn rounded_none_and_shadow_none_undo_a_decoration() {
        assert_eq!(resolve("rounded-none").as_deref(), Some("border-radius:0"));
        assert_eq!(
            resolve("rounded-t-none").as_deref(),
            Some("border-top-left-radius:0;border-top-right-radius:0")
        );
        assert_eq!(resolve("shadow-none").as_deref(), Some("box-shadow:none"));
    }

    #[test]
    fn resolves_float_box_sizing_and_table_layout() {
        assert_eq!(resolve("float-left").as_deref(), Some("float:left"));
        assert_eq!(resolve("float-right").as_deref(), Some("float:right"));
        assert_eq!(
            resolve("box-border").as_deref(),
            Some("box-sizing:border-box")
        );
        assert_eq!(
            resolve("table-fixed").as_deref(),
            Some("table-layout:fixed")
        );
    }

    #[test]
    fn resolves_list_styles() {
        assert_eq!(
            resolve("list-decimal").as_deref(),
            Some("list-style-type:decimal")
        );
        assert_eq!(
            resolve("list-disc").as_deref(),
            Some("list-style-type:disc")
        );
        assert_eq!(
            resolve("list-inside").as_deref(),
            Some("list-style-position:inside")
        );
        // The indent goes too; the markers are removed by the gap pass.
        assert_eq!(
            resolve("list-none").as_deref(),
            Some("list-style-type:none;padding-left:0")
        );
    }

    #[test]
    fn resolves_content_and_item_alignment() {
        assert_eq!(
            resolve("content-between").as_deref(),
            Some("align-content:space-between")
        );
        assert_eq!(
            resolve("justify-items-center").as_deref(),
            Some("justify-items:center")
        );
        assert_eq!(
            resolve("place-items-center").as_deref(),
            Some("align-items:center;justify-items:center")
        );
    }

    #[test]
    fn resolves_columns_and_indent() {
        assert_eq!(resolve("columns-2").as_deref(), Some("column-count:2"));
        assert_eq!(resolve("columns-13"), None);
        assert_eq!(resolve("indent-4").as_deref(), Some("text-indent:1rem"));
        assert_eq!(resolve("indent-px").as_deref(), Some("text-indent:1px"));
    }

    #[test]
    fn resolves_arbitrary_weights_tracking_and_tracks() {
        assert_eq!(resolve("font-[600]").as_deref(), Some("font-weight:600"));
        assert_eq!(resolve("font-[0]"), None);
        assert_eq!(resolve("font-[bold]"), None);
        assert_eq!(
            resolve("tracking-[0.05em]").as_deref(),
            Some("letter-spacing:0.05em")
        );
        assert_eq!(
            resolve("grid-cols-[1fr_2fr]").as_deref(),
            Some("grid-template-columns:1fr 2fr")
        );
        assert_eq!(
            resolve("grid-cols-[repeat(3,1fr)]").as_deref(),
            Some("grid-template-columns:repeat(3,1fr)")
        );
        assert_eq!(
            resolve("grid-rows-[auto_1fr]").as_deref(),
            Some("grid-template-rows:auto 1fr")
        );
        // A track list may not smuggle in a second declaration.
        assert_eq!(resolve("grid-cols-[1fr;color:red]"), None);
    }

    #[test]
    fn resolves_arbitrary_color_functions() {
        assert_eq!(
            resolve("bg-[rgb(10,20,30)]").as_deref(),
            Some("background-color:rgb(10,20,30)")
        );
        assert_eq!(
            resolve("text-[hsl(210_40%_98%)]").as_deref(),
            Some("color:hsl(210 40% 98%)")
        );
        assert_eq!(
            resolve("bg-[rgb(0_0_0/50%)]").as_deref(),
            Some("background-color:rgb(0 0 0/50%)")
        );
        assert_eq!(
            resolve("border-[rgb(1,2,3)]").as_deref(),
            Some("border-color:rgb(1,2,3)")
        );
        // Not a colour function, and no way to close the declaration early.
        assert_eq!(resolve("bg-[rgb(1,2,3);color:red]"), None);
        assert_eq!(resolve("bg-[url(x)]"), None);
    }

    #[test]
    fn resolves_calc_in_arbitrary_lengths() {
        assert_eq!(
            resolve("w-[calc(100%-2rem)]").as_deref(),
            Some("width:calc(100% - 2rem)")
        );
        assert_eq!(
            resolve("w-[calc(100%_-_2rem)]").as_deref(),
            Some("width:calc(100% - 2rem)")
        );
        assert_eq!(
            resolve("h-[calc(-2rem+4px)]").as_deref(),
            Some("height:calc(-2rem + 4px)")
        );
        assert_eq!(
            resolve("w-[calc(2*3rem)]").as_deref(),
            Some("width:calc(2 * 3rem)")
        );
        assert_eq!(
            resolve("p-[calc(1rem+2px)]").as_deref(),
            Some("padding:calc(1rem + 2px)")
        );
        // Nothing but a calc expression gets through.
        assert_eq!(resolve("w-[calc(1px;color:red)]"), None);
        assert_eq!(resolve("w-[calc(1px)2]"), None);
        assert_eq!(resolve("w-[calc(1px 2px)]"), None);
        assert_eq!(resolve("w-[calc((1px)]"), None);
        assert_eq!(resolve("w-[calc()]"), None);
    }

    #[test]
    fn screen_only_classes_are_accepted_and_real_gaps_are_not() {
        for class in [
            "cursor-pointer",
            "select-none",
            "pointer-events-none",
            "transition",
            "transition-colors",
            "duration-200",
            "animate-spin",
            "outline-none",
        ] {
            assert!(is_screen_only(class), "{class}");
            assert_eq!(resolve(class), None, "{class} must not emit CSS");
        }
        for class in [
            "blur",
            "mix-blend-multiply",
            "order-1",
            "overline",
            "line-clamp-2",
        ] {
            assert!(!is_screen_only(class), "{class} still deserves a warning");
        }
    }

    #[test]
    fn sibling_spacing_needs_a_valid_scale_key() {
        assert!(is_sibling_spacing("space-x-2"));
        assert!(is_sibling_spacing("space-y-[10px]"));
        assert!(!is_sibling_spacing("space-x-banana"));
        assert!(!is_sibling_spacing("space-x-reverse"));
        assert!(!is_sibling_spacing("space-4"));
    }
}
