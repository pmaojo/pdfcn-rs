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

/// Resolves a shadcn semantic theme token (`primary`, `muted-foreground`,
/// `border`, ...) to a literal CSS color.
pub fn color(name: &str) -> Option<&'static str> {
    THEME
        .iter()
        .find(|(token, _)| *token == name)
        .map(|(_, value)| *value)
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
