//! Document themes: a mode (light/dark) plus per-token brand overrides,
//! resolved at stylesheet-build time. There is no custom-property cascade
//! in the layout engine (see `tokens.rs`), so a "theme" here means the
//! same thing it means everywhere else in this crate: every semantic token
//! is resolved to a literal CSS value before the CSS ever reaches the
//! renderer.
//!
//! Resolution order for one token name, first match wins:
//!
//! 1. an explicit override (`overrides["primary"] == "#2563eb"`) -- how a
//!    caller rebrands `bg-primary`/`text-primary`/`border-primary` and
//!    every component built on them without touching any template;
//! 2. the mode's built-in table (shadcn's own light `:root` or `.dark`
//!    block, see `tokens.rs`);
//! 3. nothing -- the utility falls through to the literal palette/scale
//!    colors exactly as before.
//!
//! Overrides are keyed by the bare token name (`primary`, not
//! `bg-primary`): one entry rebrands every utility and component variant
//! derived from that token.

use std::collections::BTreeMap;

use crate::tokens;
pub use crate::tokens::ThemeMode;

/// A resolved document theme: shadcn's light or dark token set with
/// optional per-token overrides layered on top.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Theme {
    /// Which built-in token table underpins the theme.
    pub mode: ThemeMode,
    /// Token name -> literal CSS color, winning over the mode's table.
    /// Keys are bare semantic token names (`primary`, `muted-foreground`),
    /// values anything a CSS `color` accepts (hex, `hsl(..)`, ..).
    pub overrides: BTreeMap<String, String>,
}

impl Theme {
    /// shadcn's default light theme, no overrides.
    pub fn light() -> Self {
        Self {
            mode: ThemeMode::Light,
            overrides: BTreeMap::new(),
        }
    }

    /// shadcn's dark theme (its `.dark` block), no overrides -- dark
    /// surfaces, light ink, adjusted muted/accent/border tokens.
    pub fn dark() -> Self {
        Self {
            mode: ThemeMode::Dark,
            overrides: BTreeMap::new(),
        }
    }

    /// Resolves one semantic token (`primary`, `destructive`,
    /// `muted-foreground`, ...) through overrides first, then the mode's
    /// table. Returns `None` for names that aren't semantic tokens at all,
    /// leaving palette/scale lookups untouched downstream.
    pub fn token(&self, name: &str) -> Option<&str> {
        if let Some(value) = self.overrides.get(name) {
            return Some(value.as_str());
        }
        tokens::color_in(self.mode, name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_mode_matches_shadcn_light_tokens() {
        assert_eq!(
            Theme::light().token("primary"),
            Some("hsl(222.2, 47.4%, 11.2%)")
        );
    }

    #[test]
    fn dark_mode_flips_surface_and_ink() {
        let theme = Theme::dark();
        assert_eq!(theme.token("background"), Some("hsl(222.2, 84%, 4.9%)"));
        assert_eq!(theme.token("foreground"), Some("hsl(210, 40%, 98%)"));
        // Dark surfaces pair with light primary ink, like shadcn's .dark.
        assert_eq!(theme.token("primary"), Some("hsl(210, 40%, 98%)"));
    }

    #[test]
    fn an_override_wins_over_both_modes() {
        let mut theme = Theme::dark();
        theme
            .overrides
            .insert("primary".to_string(), "#2563eb".to_string());
        assert_eq!(theme.token("primary"), Some("#2563eb"));
        assert_eq!(
            Theme::light().token("primary"),
            Some("hsl(222.2, 47.4%, 11.2%)")
        );
    }

    #[test]
    fn non_token_names_resolve_to_none() {
        assert_eq!(Theme::light().token("slate-500"), None);
        assert_eq!(Theme::light().token("not-a-token"), None);
    }

    #[test]
    fn every_foreground_token_has_a_surface_in_both_modes() {
        for theme in [Theme::light(), Theme::dark()] {
            // Bare `foreground` is skipped: its surface counterpart is
            // `background`, which doesn't fit the strip-suffix pairing below.
            for name in ["primary-foreground", "card-foreground", "muted-foreground"] {
                let surface = name.strip_suffix("-foreground").unwrap();
                assert!(
                    theme.token(surface).is_some(),
                    "{surface} missing in {:?} mode",
                    theme.mode
                );
            }
        }
    }
}
