//! A small, hand-rolled subset of Tailwind's utility classes, resolved
//! algorithmically (spacing scale, color palette) rather than via a lookup
//! table for every class name. Not a full Tailwind JIT — enough for
//! document layout: spacing, flex/grid, typography, color, borders.

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

/// Resolves a color-utility suffix (the part after `bg-`/`text-`/`border-`)
/// against, in order: the hand-picked palette, shadcn's semantic theme
/// tokens, then the full Tailwind/shadcn scales. The palette and theme
/// tokens keep winning where they overlap with a scale entry.
fn resolve_color(key: &str) -> Option<&'static str> {
    lookup(PALETTE, key)
        .or_else(|| tokens::color(key))
        .or_else(|| tokens::scale_color(key))
}

fn spacing_decls(props: &[&str], scale: &str) -> Option<String> {
    let value = lookup(SPACING, scale)?;
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
    let value = lookup(SPACING, key).or_else(|| lookup(INSET_FRACTIONS, key))?;
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
pub fn resolve(class: &str) -> Option<String> {
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
    if let Some(rest) = class.strip_prefix("m-") {
        return spacing_decls(&["margin"], rest);
    }
    if let Some(rest) = class.strip_prefix("mx-") {
        return spacing_decls(&["margin-left", "margin-right"], rest);
    }
    if let Some(rest) = class.strip_prefix("my-") {
        return spacing_decls(&["margin-top", "margin-bottom"], rest);
    }
    if let Some(rest) = class.strip_prefix("mt-") {
        return spacing_decls(&["margin-top"], rest);
    }
    if let Some(rest) = class.strip_prefix("mr-") {
        return spacing_decls(&["margin-right"], rest);
    }
    if let Some(rest) = class.strip_prefix("mb-") {
        return spacing_decls(&["margin-bottom"], rest);
    }
    if let Some(rest) = class.strip_prefix("ml-") {
        return spacing_decls(&["margin-left"], rest);
    }
    // Axis-specific gaps must be checked before the bare `gap-` prefix,
    // which would otherwise swallow them (`gap-x-4` -> scale "x-4") and
    // return None without ever reaching these branches.
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
        return spacing_decls(&["min-height"], rest);
    }
    if let Some(rest) = class.strip_prefix("h-") {
        return match rest {
            "full" => Some("height:100%".to_string()),
            "screen" => Some("height:100vh".to_string()),
            _ => spacing_decls(&["height"], rest),
        };
    }
    if let Some(rest) = class.strip_prefix("w-") {
        return match rest {
            "full" => Some("width:100%".to_string()),
            "screen" => Some("width:100vw".to_string()),
            _ => spacing_decls(&["width"], rest),
        };
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
    if let Some(rest) = class.strip_prefix("text-") {
        if let Some(size) = lookup(FONT_SIZE, rest) {
            return Some(format!("font-size:{size}"));
        }
        if let Some(color) = resolve_color(rest) {
            return Some(format!("color:{color}"));
        }
        return None;
    }
    if let Some(rest) = class.strip_prefix("bg-") {
        let color = resolve_color(rest)?;
        return Some(format!("background-color:{color}"));
    }
    if let Some(rest) = class.strip_prefix("border-") {
        if let Some(color) = resolve_color(rest) {
            return Some(format!("border-color:{color}"));
        }
        return None;
    }
    if let Some(rest) = class.strip_prefix("rounded-") {
        let value = tokens::radius(rest)?;
        return Some(format!("border-radius:{value}"));
    }
    if let Some(rest) = class.strip_prefix("shadow-") {
        let value = tokens::shadow(rest)?;
        return Some(format!("box-shadow:{value}"));
    }
    if let Some(rest) = class.strip_prefix("grid-cols-") {
        let n: u32 = rest.parse().ok()?;
        return Some(format!("grid-template-columns:repeat({n}, minmax(0, 1fr))"));
    }
    if let Some(rest) = class.strip_prefix("font-") {
        let weight = match rest {
            "thin" => "100",
            "light" => "300",
            "normal" => "400",
            "medium" => "500",
            "semibold" => "600",
            "bold" => "700",
            "extrabold" => "800",
            _ => return None,
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
        "text-left" => Some("text-align:left"),
        "text-center" => Some("text-align:center"),
        "text-right" => Some("text-align:right"),
        "italic" => Some("font-style:italic"),
        "uppercase" => Some("text-transform:uppercase"),
        "border" => Some("border-width:1px;border-style:solid"),
        // px, not the equivalent 0.25rem: see the comment on `tokens::RADIUS`.
        "rounded" => Some("border-radius:4px"),
        "shadow" => Some("box-shadow:0 1px 3px rgba(0,0,0,0.1)"),
        "break-inside-avoid" => Some("break-inside:avoid;page-break-inside:avoid"),
        "break-before-page" => Some("break-before:page"),
        "break-after-page" => Some("break-after:page"),
        "shrink-0" => Some("flex-shrink:0"),
        "flex-1" => Some("flex:1 1 0%"),
        "table-bordered" => Some("border-width:2px;border-style:solid"),
        "table-compact" => Some("font-size:0.75rem"),
        _ => None,
    };
    literal.map(str::to_string)
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
        assert_eq!(resolve("bg-red-600").as_deref(), Some("background-color:#dc2626"));
    }

    #[test]
    fn resolves_the_full_neutral_and_accent_scales() {
        assert_eq!(resolve("bg-slate-950").as_deref(), Some("background-color:#020617"));
        assert_eq!(resolve("bg-zinc-100").as_deref(), Some("background-color:#f4f4f5"));
        assert_eq!(resolve("bg-neutral-500").as_deref(), Some("background-color:#737373"));
        assert_eq!(resolve("bg-stone-800").as_deref(), Some("background-color:#292524"));
    }

    /// The new component modules (Alert/Avatar/form fields/nav) lean on
    /// height/width/min-height utilities that previously only existed as a
    /// couple of hardcoded literals (`w-full`, `h-full`).
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
        assert_eq!(resolve("object-contain").as_deref(), Some("object-fit:contain"));
        assert_eq!(resolve("object-top").as_deref(), Some("object-position:top"));
        assert_eq!(resolve("object-center").as_deref(), Some("object-position:center"));
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
        assert_eq!(resolve("rounded-2xl").as_deref(), Some("border-radius:16px"));
        assert_eq!(resolve("rounded-3xl").as_deref(), Some("border-radius:24px"));
    }
}
