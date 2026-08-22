//! shadcn/ui's default theme, resolved to literal CSS values.
//!
//! shadcn ships its theme as CSS custom properties (`--primary`, `--border`,
//! ...) consumed through `hsl(var(--primary))`. There is no custom-property
//! cascade at PDF-render time, so each token is resolved here to the literal
//! value shadcn's default (light) `:root` block would produce.
//!
//! Values are kept in shadcn's own `H S% L%` form rather than converted to
//! hex, so a token can be diffed against the upstream theme block it came
//! from. They are emitted as comma-separated `hsl()`, the form every CSS
//! parser in the pipeline accepts.

/// Which built-in token table a [`crate::theme::Theme`] resolves against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    /// shadcn's light `:root` block (the default).
    #[default]
    Light,
    /// shadcn's `.dark` block: dark surfaces, light ink.
    Dark,
}

/// shadcn's default light theme. Each entry is the token name as it appears
/// after the `--` in shadcn's `:root`, paired with that token's literal
/// value.
const THEME: &[(&str, &str)] = &[
    ("background", "hsl(0, 0%, 100%)"),
    ("foreground", "hsl(222.2, 84%, 4.9%)"),
    ("card", "hsl(0, 0%, 100%)"),
    ("card-foreground", "hsl(222.2, 84%, 4.9%)"),
    ("popover", "hsl(0, 0%, 100%)"),
    ("popover-foreground", "hsl(222.2, 84%, 4.9%)"),
    ("primary", "hsl(222.2, 47.4%, 11.2%)"),
    ("primary-foreground", "hsl(210, 40%, 98%)"),
    ("secondary", "hsl(210, 40%, 96.1%)"),
    ("secondary-foreground", "hsl(222.2, 47.4%, 11.2%)"),
    ("muted", "hsl(210, 40%, 96.1%)"),
    ("muted-foreground", "hsl(215.4, 16.3%, 46.9%)"),
    ("accent", "hsl(210, 40%, 96.1%)"),
    ("accent-foreground", "hsl(222.2, 47.4%, 11.2%)"),
    ("destructive", "hsl(0, 84.2%, 60.2%)"),
    ("destructive-foreground", "hsl(210, 40%, 98%)"),
    ("border", "hsl(214.3, 31.8%, 91.4%)"),
    ("input", "hsl(214.3, 31.8%, 91.4%)"),
    ("ring", "hsl(222.2, 84%, 4.9%)"),
];

/// shadcn's dark theme, straight from its `.dark` block -- same token
/// names as [`THEME`], dark surfaces and adjusted muted/accent/border
/// tokens. Same `H S% L%` form, so entries diff cleanly against upstream.
const DARK_THEME: &[(&str, &str)] = &[
    ("background", "hsl(222.2, 84%, 4.9%)"),
    ("foreground", "hsl(210, 40%, 98%)"),
    ("card", "hsl(222.2, 84%, 4.9%)"),
    ("card-foreground", "hsl(210, 40%, 98%)"),
    ("popover", "hsl(222.2, 84%, 4.9%)"),
    ("popover-foreground", "hsl(210, 40%, 98%)"),
    ("primary", "hsl(210, 40%, 98%)"),
    ("primary-foreground", "hsl(222.2, 47.4%, 11.2%)"),
    ("secondary", "hsl(217.2, 32.6%, 17.5%)"),
    ("secondary-foreground", "hsl(210, 40%, 98%)"),
    ("muted", "hsl(217.2, 32.6%, 17.5%)"),
    ("muted-foreground", "hsl(215, 20.2%, 65.1%)"),
    ("accent", "hsl(217.2, 32.6%, 17.5%)"),
    ("accent-foreground", "hsl(210, 40%, 98%)"),
    ("destructive", "hsl(0, 62.8%, 30.6%)"),
    ("destructive-foreground", "hsl(210, 40%, 98%)"),
    ("border", "hsl(217.2, 32.6%, 17.5%)"),
    ("input", "hsl(217.2, 32.6%, 17.5%)"),
    ("ring", "hsl(212.7, 26.8%, 83.9%)"),
];

fn table(mode: ThemeMode) -> &'static [(&'static str, &'static str)] {
    match mode {
        ThemeMode::Light => THEME,
        ThemeMode::Dark => DARK_THEME,
    }
}

/// Resolves a shadcn semantic theme token (`primary`, `muted-foreground`,
/// `border`, ...) to a literal CSS color.
pub fn color(name: &str) -> Option<&'static str> {
    THEME
        .iter()
        .find(|(token, _)| *token == name)
        .map(|(_, value)| *value)
}

/// Like [`color`], but resolved against a mode's table -- [`ThemeMode::Dark`]
/// yields shadcn's `.dark` values rather than the light `:root` ones.
pub fn color_in(mode: ThemeMode, name: &str) -> Option<&'static str> {
    table(mode)
        .iter()
        .find(|(token, _)| *token == name)
        .map(|(_, value)| *value)
}

/// One Tailwind/shadcn color scale: 11 shades, `50` through `950`.
type Scale = [(&'static str, &'static str); 11];

const SHADES: [&str; 11] = [
    "50", "100", "200", "300", "400", "500", "600", "700", "800", "900", "950",
];

macro_rules! scale {
    ($($hex:literal),+ $(,)?) => {{
        let hexes = [$($hex),+];
        let mut out: Scale = [("", ""); 11];
        let mut i = 0;
        while i < 11 {
            out[i] = (SHADES[i], hexes[i]);
            i += 1;
        }
        out
    }};
}

const SLATE: Scale = scale![
    "#f8fafc", "#f1f5f9", "#e2e8f0", "#cbd5e1", "#94a3b8", "#64748b", "#475569", "#334155",
    "#1e293b", "#0f172a", "#020617"
];
const GRAY: Scale = scale![
    "#f9fafb", "#f3f4f6", "#e5e7eb", "#d1d5db", "#9ca3af", "#6b7280", "#4b5563", "#374151",
    "#1f2937", "#111827", "#030712"
];
const ZINC: Scale = scale![
    "#fafafa", "#f4f4f5", "#e4e4e7", "#d4d4d8", "#a1a1aa", "#71717a", "#52525b", "#3f3f46",
    "#27272a", "#18181b", "#09090b"
];
const NEUTRAL: Scale = scale![
    "#fafafa", "#f5f5f5", "#e5e5e5", "#d4d4d4", "#a3a3a3", "#737373", "#525252", "#404040",
    "#262626", "#171717", "#0a0a0a"
];
const STONE: Scale = scale![
    "#fafaf9", "#f5f5f4", "#e7e5e4", "#d6d3d1", "#a8a29e", "#78716c", "#57534e", "#44403c",
    "#292524", "#1c1917", "#0c0a09"
];
const RED: Scale = scale![
    "#fef2f2", "#fee2e2", "#fecaca", "#fca5a5", "#f87171", "#ef4444", "#dc2626", "#b91c1c",
    "#991b1b", "#7f1d1d", "#450a0a"
];
const ORANGE: Scale = scale![
    "#fff7ed", "#ffedd5", "#fed7aa", "#fdba74", "#fb923c", "#f97316", "#ea580c", "#c2410c",
    "#9a3412", "#7c2d12", "#431407"
];
const AMBER: Scale = scale![
    "#fffbeb", "#fef3c7", "#fde68a", "#fcd34d", "#fbbf24", "#f59e0b", "#d97706", "#b45309",
    "#92400e", "#78350f", "#451a03"
];
const YELLOW: Scale = scale![
    "#fefce8", "#fef9c3", "#fef08a", "#fde047", "#facc15", "#eab308", "#ca8a04", "#a16207",
    "#854d0e", "#713f12", "#422006"
];
const LIME: Scale = scale![
    "#f7fee7", "#ecfccb", "#d9f99d", "#bef264", "#a3e635", "#84cc16", "#65a30d", "#4d7c0f",
    "#3f6212", "#365314", "#1a2e05"
];
const GREEN: Scale = scale![
    "#f0fdf4", "#dcfce7", "#bbf7d0", "#86efac", "#4ade80", "#22c55e", "#16a34a", "#15803d",
    "#166534", "#14532d", "#052e16"
];
const EMERALD: Scale = scale![
    "#ecfdf5", "#d1fae5", "#a7f3d0", "#6ee7b7", "#34d399", "#10b981", "#059669", "#047857",
    "#065f46", "#064e3b", "#022c22"
];
const TEAL: Scale = scale![
    "#f0fdfa", "#ccfbf1", "#99f6e4", "#5eead4", "#2dd4bf", "#14b8a6", "#0d9488", "#0f766e",
    "#115e59", "#134e4a", "#042f2e"
];
const CYAN: Scale = scale![
    "#ecfeff", "#cffafe", "#a5f3fc", "#67e8f9", "#22d3ee", "#06b6d4", "#0891b2", "#0e7490",
    "#155e75", "#164e63", "#083344"
];
const SKY: Scale = scale![
    "#f0f9ff", "#e0f2fe", "#bae6fd", "#7dd3fc", "#38bdf8", "#0ea5e9", "#0284c7", "#0369a1",
    "#075985", "#0c4a6e", "#082f49"
];
const BLUE: Scale = scale![
    "#eff6ff", "#dbeafe", "#bfdbfe", "#93c5fd", "#60a5fa", "#3b82f6", "#2563eb", "#1d4ed8",
    "#1e40af", "#1e3a8a", "#172554"
];
const INDIGO: Scale = scale![
    "#eef2ff", "#e0e7ff", "#c7d2fe", "#a5b4fc", "#818cf8", "#6366f1", "#4f46e5", "#4338ca",
    "#3730a3", "#312e81", "#1e1b4b"
];
const VIOLET: Scale = scale![
    "#f5f3ff", "#ede9fe", "#ddd6fe", "#c4b5fd", "#a78bfa", "#8b5cf6", "#7c3aed", "#6d28d9",
    "#5b21b6", "#4c1d95", "#2e1065"
];
const PURPLE: Scale = scale![
    "#faf5ff", "#f3e8ff", "#e9d5ff", "#d8b4fe", "#c084fc", "#a855f7", "#9333ea", "#7e22ce",
    "#6b21a8", "#581c87", "#3b0764"
];
const FUCHSIA: Scale = scale![
    "#fdf4ff", "#fae8ff", "#f5d0fe", "#f0abfc", "#e879f9", "#d946ef", "#c026d3", "#a21caf",
    "#86198f", "#701a75", "#4a044e"
];
const PINK: Scale = scale![
    "#fdf2f8", "#fce7f3", "#fbcfe8", "#f9a8d4", "#f472b6", "#ec4899", "#db2777", "#be185d",
    "#9d174d", "#831843", "#500724"
];
const ROSE: Scale = scale![
    "#fff1f2", "#ffe4e6", "#fecdd3", "#fda4af", "#fb7185", "#f43f5e", "#e11d48", "#be123c",
    "#9f1239", "#881337", "#4c0519"
];

const SCALES: &[(&str, &Scale)] = &[
    ("slate", &SLATE),
    ("gray", &GRAY),
    ("zinc", &ZINC),
    ("neutral", &NEUTRAL),
    ("stone", &STONE),
    ("red", &RED),
    ("orange", &ORANGE),
    ("amber", &AMBER),
    ("yellow", &YELLOW),
    ("lime", &LIME),
    ("green", &GREEN),
    ("emerald", &EMERALD),
    ("teal", &TEAL),
    ("cyan", &CYAN),
    ("sky", &SKY),
    ("blue", &BLUE),
    ("indigo", &INDIGO),
    ("violet", &VIOLET),
    ("purple", &PURPLE),
    ("fuchsia", &FUCHSIA),
    ("pink", &PINK),
    ("rose", &ROSE),
];

/// Resolves a full Tailwind/shadcn scale entry such as `"slate-950"` or
/// `"zinc-100"` to its literal hex color.
pub fn scale_color(name: &str) -> Option<&'static str> {
    let (scale_name, shade) = name.rsplit_once('-')?;
    let scale = SCALES
        .iter()
        .find(|(n, _)| *n == scale_name)
        .map(|(_, s)| *s)?;
    scale
        .iter()
        .find(|(s, _)| *s == shade)
        .map(|(_, hex)| *hex)
}

/// shadcn's radius scale, keyed by the Tailwind `rounded-*` suffix (`sm`
/// through `3xl`). In `px`, not the equivalent `rem` (`0.5rem`, ...):
/// `printpdf`/`azul-layout`'s CSS parser silently treats a `rem`-unit
/// `border-radius` as `0` (verified: `8px` renders real rounded corners,
/// both `.5rem` and `0.5rem` render square ones) -- `px` is the one unit
/// this specific property renders correctly with, even though `rem` works
/// fine for every other property (padding, margin, font-size, ...) this
/// project uses it for.
const RADIUS: &[(&str, &str)] = &[
    ("sm", "2px"),
    ("md", "6px"),
    ("lg", "8px"),
    ("xl", "12px"),
    ("2xl", "16px"),
    ("3xl", "24px"),
    ("full", "9999px"),
];

/// Resolves a `rounded-*` suffix to its literal `border-radius` value.
pub fn radius(name: &str) -> Option<&'static str> {
    RADIUS.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
}

/// shadcn's elevation scale, keyed by the Tailwind `shadow-*` suffix.
const SHADOW: &[(&str, &str)] = &[
    ("sm", "0 1px 2px 0 rgba(0,0,0,0.05)"),
    ("md", "0 4px 6px -1px rgba(0,0,0,0.1), 0 2px 4px -2px rgba(0,0,0,0.1)"),
    ("lg", "0 10px 15px -3px rgba(0,0,0,0.1), 0 4px 6px -4px rgba(0,0,0,0.1)"),
    ("xl", "0 20px 25px -5px rgba(0,0,0,0.1), 0 8px 10px -6px rgba(0,0,0,0.1)"),
];

/// Resolves a `shadow-*` suffix to its literal `box-shadow` value.
pub fn shadow(name: &str) -> Option<&'static str> {
    SHADOW.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_shadcn_semantic_theme_tokens() {
        assert_eq!(color("primary"), Some("hsl(222.2, 47.4%, 11.2%)"));
        assert_eq!(color("primary-foreground"), Some("hsl(210, 40%, 98%)"));
        assert_eq!(color("input"), Some("hsl(214.3, 31.8%, 91.4%)"));
    }

    #[test]
    fn unknown_token_is_not_a_color() {
        assert_eq!(color("not-a-token"), None);
    }

    /// A foreground token must contrast with its own surface, or the pairing
    /// shadcn guarantees is silently broken in print.
    #[test]
    fn every_foreground_token_has_its_surface() {
        for (name, _) in THEME {
            if let Some(surface) = name.strip_suffix("-foreground") {
                assert!(
                    color(surface).is_some(),
                    "{name} has no matching surface token"
                );
            }
        }
    }
}
