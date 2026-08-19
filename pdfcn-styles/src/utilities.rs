//! A small, hand-rolled subset of Tailwind's utility classes, resolved
//! algorithmically (spacing scale, color palette) rather than via a lookup
//! table for every class name. Not a full Tailwind JIT — enough for
//! document layout: spacing, flex/grid, typography, color, borders.

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
        if let Some(color) = lookup(PALETTE, rest) {
            return Some(format!("color:{color}"));
        }
        return None;
    }
    if let Some(rest) = class.strip_prefix("bg-") {
        let color = lookup(PALETTE, rest)?;
        return Some(format!("background-color:{color}"));
    }
    if let Some(rest) = class.strip_prefix("border-") {
        if let Some(color) = lookup(PALETTE, rest) {
            return Some(format!("border-color:{color}"));
        }
        return None;
    }
    if let Some(rest) = class.strip_prefix("rounded-") {
        let value = match rest {
            "sm" => "0.125rem",
            "md" => "0.375rem",
            "lg" => "0.5rem",
            "xl" => "0.75rem",
            "full" => "9999px",
            _ => return None,
        };
        return Some(format!("border-radius:{value}"));
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
