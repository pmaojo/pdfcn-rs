//! `%Progress`: shadcn's Progress, static — a filled track at a fixed
//! percentage (no live update, a PDF page has no state).

use maud::{html, Markup};
use pdfcn_template::ResolvedAttr;

use crate::attr;

pub fn progress(attrs: &[ResolvedAttr]) -> Markup {
    let value: u32 = attr(attrs, "value")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
        .min(100);
    html! {
        div class="progress relative h-2 w-full overflow-hidden rounded-full bg-secondary" {
            div class="progress-track h-full bg-primary" style={ "width:" (value) "%" } {}
        }
    }
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
    fn filled_track_width_matches_the_given_percentage() {
        let out = progress(&[a("value", "65")]).into_string();
        assert!(out.contains("width:65%"));
    }

    #[test]
    fn value_is_clamped_to_100() {
        let out = progress(&[a("value", "150")]).into_string();
        assert!(out.contains("width:100%"));
    }
}
