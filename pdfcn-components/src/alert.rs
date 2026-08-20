//! `%Alert`: shadcn's Alert, static (no dismiss action — a PDF page has no
//! interaction). Anatomy: an optional icon slot, a title, and a description
//! body, varying by severity.

use maud::{html, Markup};
use pdfcn_template::ResolvedAttr;

use crate::{attr, attr_or};

fn variant_classes(variant: &str) -> &'static str {
    match variant {
        "destructive" => "border-destructive text-destructive bg-background",
        "warning" => "border-amber-500 text-amber-700 bg-amber-50",
        "success" => "border-green-500 text-green-700 bg-green-50",
        _ => "border-border bg-background text-foreground",
    }
}

pub fn alert(attrs: &[ResolvedAttr], children: Markup) -> Markup {
    let variant = attr_or(attrs, "variant", "default");
    let title = attr(attrs, "title");
    html! {
        div role="alert" class={ "alert relative w-full rounded-lg border p-4 " (variant_classes(variant)) } {
            span class="alert-icon" {}
            @if let Some(t) = title {
                h5 class="alert-title mb-1 font-medium leading-none tracking-tight" { (t) }
            }
            div class="alert-description text-sm" { (children) }
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
    fn default_variant_uses_shadcn_default_classes() {
        let out = alert(&[a("title", "Heads up")], html! {}).into_string();
        assert!(out.contains("border-border"));
        assert!(out.contains("bg-background"));
        assert!(out.contains("Heads up"));
    }

    #[test]
    fn destructive_variant_uses_destructive_classes() {
        let out = alert(
            &[a("variant", "destructive"), a("title", "Payment due")],
            html! {},
        )
        .into_string();
        assert!(out.contains("border-destructive"));
        assert!(out.contains("text-destructive"));
    }

    #[test]
    fn exposes_an_icon_slot() {
        let out = alert(&[], html! {}).into_string();
        assert!(out.contains("alert-icon"));
    }
}
