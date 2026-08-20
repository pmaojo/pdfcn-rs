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
    if let Some(rest) = class.strip_prefix("gap-") {
        return spacing_decls(&["gap"], rest);
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
        "w-full" => Some("width:100%"),
        "h-full" => Some("height:100%"),
        "w-screen" => Some("width:100vw"),
        "text-left" => Some("text-align:left"),
        "text-center" => Some("text-align:center"),
        "text-right" => Some("text-align:right"),
        "italic" => Some("font-style:italic"),
        "uppercase" => Some("text-transform:uppercase"),
        "border" => Some("border-width:1px;border-style:solid"),
        "rounded" => Some("border-radius:0.25rem"),
        "shadow" => Some("box-shadow:0 1px 3px rgba(0,0,0,0.1)"),
        "break-inside-avoid" => Some("break-inside:avoid;page-break-inside:avoid"),
        "break-before-page" => Some("break-before:page"),
        "break-after-page" => Some("break-after:page"),
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

    #[test]
    fn resolves_the_shadow_and_radius_scales() {
        assert!(resolve("shadow-sm").is_some());
        assert!(resolve("shadow-md").is_some());
        assert!(resolve("shadow-lg").is_some());
        assert!(resolve("shadow-xl").is_some());
        assert_ne!(resolve("shadow-sm"), resolve("shadow-xl"));
        assert_eq!(resolve("rounded-2xl").as_deref(), Some("border-radius:1rem"));
        assert_eq!(resolve("rounded-3xl").as_deref(), Some("border-radius:1.5rem"));
    }
}
