//! `%Breadcrumb`/`%Pagination`: shadcn's navigation-trail components,
//! static — the last breadcrumb is the current page (not a link), and
//! Pagination is a fixed "Page X of Y" footer rather than a live control.

use maud::{html, Markup};
use pdfcn_template::ResolvedAttr;
use serde_json::Value as JsonValue;

use crate::{attr, attr_or};

/// `items` (required): a JSON array of crumb label strings, e.g.
/// `["Home","Invoices","INV-042"]`, the same "data island" convention as
/// `%InvoiceTable`'s `rows`.
pub fn breadcrumb(attrs: &[ResolvedAttr]) -> Markup {
    let items: Vec<String> = attr(attrs, "items")
        .and_then(|s| serde_json::from_str::<Vec<JsonValue>>(s).ok())
        .map(|values| {
            values
                .into_iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let last = items.len().saturating_sub(1);
    html! {
        nav class="breadcrumb text-sm text-muted-foreground" {
            ol class="flex items-center gap-1.5" {
                @for (i, crumb) in items.iter().enumerate() {
                    @if i == last {
                        li class="breadcrumb-current text-foreground font-medium" { (crumb) }
                    } @else {
                        li class="breadcrumb-item flex items-center gap-1.5" {
                            span { (crumb) }
                            span class="breadcrumb-separator" { "\u{203A}" }
                        }
                    }
                }
            }
        }
    }
}

pub fn pagination(attrs: &[ResolvedAttr]) -> Markup {
    let current = attr_or(attrs, "current", "1");
    let total = attr_or(attrs, "total", "1");
    html! {
        nav class="pagination flex items-center justify-center text-sm text-muted-foreground" {
            span class="pagination-status" { "Page " (current) " of " (total) }
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
    fn separates_crumbs_and_marks_the_last_one_current() {
        let out = breadcrumb(&[a("items", r#"["Home","Invoices","INV-042"]"#)]).into_string();
        assert_eq!(out.matches('\u{203A}').count(), 2);
        assert!(out.contains("breadcrumb-current"));
        let current_idx = out.find("breadcrumb-current").unwrap();
        assert!(out[current_idx..].contains("INV-042"));
    }

    #[test]
    fn pagination_shows_page_x_of_y() {
        let out = pagination(&[a("current", "3"), a("total", "12")]).into_string();
        assert!(out.contains("Page 3 of 12"));
    }
}
