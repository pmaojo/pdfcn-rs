//! The Shadcn-style component registry (FR-2): `%InvoiceTable`, `%Badge`,
//! `%Card`, and friends expand into pre-styled `maud` markup fragments.
//! Attribute values arrive as plain strings (already resolved by
//! `pdfcn-template`); a component that needs structured data (e.g.
//! `InvoiceTable`'s `rows`) expects a JSON-encoded string, the same "data
//! island" convention as an HTML `data-*` attribute.

mod alert;
mod avatar;
mod form_field;
mod nav;
mod progress;

use std::fmt;

use maud::{html, Markup};
use pdfcn_template::ResolvedAttr;
use serde_json::Value as JsonValue;

pub(crate) fn attr<'a>(attrs: &'a [ResolvedAttr], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|a| a.name == name)
        .map(|a| a.value.as_str())
}

pub(crate) fn attr_or<'a>(attrs: &'a [ResolvedAttr], name: &str, default: &'a str) -> &'a str {
    attr(attrs, name).unwrap_or(default)
}

/// shadcn/ui components whose entire purpose is interaction (open/close,
/// hover, focus-trap, portal) and that therefore have no meaningful static
/// print rendering. Distinguished from "unknown component name" in
/// [`render`] so a caller sees a clear rejection instead of a generic
/// unknown-component error.
const INTERACTIVE_ONLY: &[&str] = &[
    "Dialog",
    "AlertDialog",
    "Sheet",
    "Drawer",
    "Popover",
    "Tooltip",
    "HoverCard",
    "DropdownMenu",
    "ContextMenu",
    "Menubar",
    "NavigationMenu",
    "Command",
    "Combobox",
    "Sonner",
    "Toast",
    "Resizable",
    "ScrollArea",
    "Sidebar",
    "Form",
];

/// A component name that is deliberately unsupported because its shadcn
/// meaning is inherently interactive and has no static-print equivalent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractiveOnlyComponent {
    pub component: String,
}

impl fmt::Display for InteractiveOnlyComponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: interactive-only, unsupported in static PDF output",
            self.component
        )
    }
}

impl std::error::Error for InteractiveOnlyComponent {}

/// Expands a component instance (`%InvoiceTable`, `%Badge`, ...) into its
/// underlying markup. `children` is the already-rendered markup of the
/// component's child nodes.
///
/// - `Ok(Some(markup))`: a known, supported component.
/// - `Ok(None)`: an unknown component name.
/// - `Err(_)`: a component that shadcn defines but that is deliberately
///   unsupported because it's interactive-only (see [`INTERACTIVE_ONLY`]) —
///   distinct from "unknown" so callers can surface a clear message instead
///   of silently falling through.
pub fn render(
    name: &str,
    attrs: &[ResolvedAttr],
    children: Markup,
) -> Result<Option<Markup>, InteractiveOnlyComponent> {
    if INTERACTIVE_ONLY.contains(&name) {
        return Err(InteractiveOnlyComponent {
            component: name.to_string(),
        });
    }
    Ok(match name {
        "DocumentLayout" => Some(document_layout(attrs, children)),
        "Header" => Some(header(attrs, children)),
        "Card" => Some(card(attrs, children)),
        "Table" => Some(table(attrs, children)),
        "Grid" => Some(grid(attrs, children)),
        "Badge" => Some(badge(attrs, children)),
        "Separator" => Some(separator(attrs)),
        "SignatureBlock" => Some(signature_block(attrs)),
        "InvoiceTable" => Some(invoice_table(attrs)),
        "Alert" => Some(alert::alert(attrs, children)),
        "Avatar" => Some(avatar::avatar(attrs)),
        "Input" => Some(form_field::input(attrs)),
        "Textarea" => Some(form_field::textarea(attrs)),
        "Select" => Some(form_field::select(attrs)),
        "Label" => Some(form_field::label(attrs, children)),
        "Checkbox" => Some(form_field::checkbox(attrs)),
        "RadioItem" => Some(form_field::radio_item(attrs)),
        "Progress" => Some(progress::progress(attrs)),
        "Breadcrumb" => Some(nav::breadcrumb(attrs)),
        "Pagination" => Some(nav::pagination(attrs)),
        _ => None,
    })
}

fn document_layout(attrs: &[ResolvedAttr], children: Markup) -> Markup {
    let size = attr_or(attrs, "size", "a4");
    html! {
        div class={ "document document-" (size) " p-8" } {
            (children)
        }
    }
}

fn header(attrs: &[ResolvedAttr], children: Markup) -> Markup {
    let title = attr(attrs, "title");
    let subtitle = attr(attrs, "subtitle");
    html! {
        header class="mb-6 pb-4 border-b" {
            @if let Some(t) = title {
                h1 class="text-2xl font-bold" { (t) }
            }
            @if let Some(s) = subtitle {
                p class="text-sm" { (s) }
            }
            (children)
        }
    }
}

fn card(attrs: &[ResolvedAttr], children: Markup) -> Markup {
    let title = attr(attrs, "title");
    html! {
        div class="card rounded-lg border bg-card text-card-foreground shadow-sm p-4 break-inside-avoid" {
            @if let Some(t) = title {
                h2 class="text-lg font-semibold mb-2" { (t) }
            }
            (children)
        }
    }
}

fn table(attrs: &[ResolvedAttr], children: Markup) -> Markup {
    let variant = attr_or(attrs, "variant", "default");
    let variant_class = match variant {
        "striped" => " table-striped",
        "bordered" => " table-bordered",
        "compact" => " table-compact",
        _ => "",
    };
    html! {
        table class={ "table w-full border" (variant_class) } {
            (children)
        }
    }
}

fn grid(attrs: &[ResolvedAttr], children: Markup) -> Markup {
    let cols = attr_or(attrs, "cols", "2");
    html! {
        div class={ "grid grid-cols-" (cols) " gap-4" } {
            (children)
        }
    }
}

fn badge_classes(variant: &str) -> &'static str {
    match variant {
        "outline" => "border border-input text-foreground",
        "destructive" => "border-transparent bg-destructive text-destructive-foreground",
        "success" => "border-transparent bg-green-600 text-white",
        "secondary" => "border-transparent bg-secondary text-secondary-foreground",
        _ => "border-transparent bg-primary text-primary-foreground",
    }
}

fn badge(attrs: &[ResolvedAttr], children: Markup) -> Markup {
    let variant = attr_or(attrs, "variant", "default");
    let label = attr(attrs, "label");
    html! {
        span class={ "badge inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-semibold " (badge_classes(variant)) } {
            @if let Some(l) = label {
                (l)
            }
            (children)
        }
    }
}

fn separator(_attrs: &[ResolvedAttr]) -> Markup {
    html! { div role="separator" class="separator shrink-0 bg-border h-px w-full my-4"; }
}

fn signature_block(attrs: &[ResolvedAttr]) -> Markup {
    let name = attr_or(attrs, "name", "");
    let label = attr_or(attrs, "label", "Signature");
    html! {
        div class="signature-block mt-8 break-inside-avoid" {
            div class="border-t border-slate-500 pt-1 w-full" {
                @if !name.is_empty() {
                    p class="text-sm font-medium" { (name) }
                }
                p class="text-xs text-slate-500" { (label) }
            }
        }
    }
}

/// `rows` (required): a JSON array of flat objects, e.g.
/// `[{"description":"Widget","qty":2,"price":"$9.00"}]`.
/// `columns` (optional): a JSON array of `{"key":"...","label":"..."}` to
/// control column order/labels; defaults to the first row's keys.
fn invoice_table(attrs: &[ResolvedAttr]) -> Markup {
    let rows: Vec<JsonValue> = attr(attrs, "rows")
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();

    let columns: Vec<(String, String)> = attr(attrs, "columns")
        .and_then(|s| serde_json::from_str::<Vec<JsonValue>>(s).ok())
        .map(|cols| {
            cols.into_iter()
                .filter_map(|c| {
                    let key = c.get("key")?.as_str()?.to_string();
                    let label = c
                        .get("label")
                        .and_then(|l| l.as_str())
                        .unwrap_or(&key)
                        .to_string();
                    Some((key, label))
                })
                .collect()
        })
        .unwrap_or_else(|| {
            rows.first()
                .and_then(|r| r.as_object())
                .map(|obj| obj.keys().map(|k| (k.clone(), k.clone())).collect())
                .unwrap_or_default()
        });

    html! {
        table class="invoice-table w-full border" {
            thead {
                tr {
                    @for (_, label) in &columns {
                        th class="text-left p-2 border-b font-semibold" { (label) }
                    }
                }
            }
            tbody {
                @for row in &rows {
                    tr class="break-inside-avoid" {
                        @for (key, _) in &columns {
                            td class="p-2 border-b" {
                                (row.get(key).map(cell_text).unwrap_or_default())
                            }
                        }
                    }
                }
            }
        }
    }
}

fn cell_text(v: &JsonValue) -> String {
    match v {
        JsonValue::String(s) => s.clone(),
        JsonValue::Null => String::new(),
        other => other.to_string(),
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
    fn badge_uses_variant_classes() {
        let out = render("Badge", &[a("variant", "destructive")], html! {})
            .unwrap()
            .unwrap();
        assert!(out.into_string().contains("bg-destructive"));
    }

    #[test]
    fn invoice_table_renders_rows_from_json() {
        let rows = a("rows", r#"[{"item":"Widget","qty":"2"}]"#);
        let out = render("InvoiceTable", &[rows], html! {})
            .unwrap()
            .unwrap()
            .into_string();
        assert!(out.contains("Widget"));
        assert!(out.contains("break-inside-avoid"));
    }

    #[test]
    fn unknown_component_returns_ok_none() {
        assert!(render("NotAComponent", &[], html! {}).unwrap().is_none());
    }

    #[test]
    fn table_variants_extend_beyond_default_and_striped() {
        let bordered = render("Table", &[a("variant", "bordered")], html! {})
            .unwrap()
            .unwrap()
            .into_string();
        assert!(bordered.contains("table-bordered"));

        let compact = render("Table", &[a("variant", "compact")], html! {})
            .unwrap()
            .unwrap()
            .into_string();
        assert!(compact.contains("table-compact"));
    }

    #[test]
    fn interactive_only_components_are_rejected_explicitly() {
        for name in ["Dialog", "Tooltip", "DropdownMenu", "Popover", "Sonner"] {
            let err = render(name, &[], html! {}).unwrap_err();
            let message = err.to_string();
            assert!(
                message.contains("interactive-only, unsupported in static PDF output"),
                "unexpected message for {name}: {message}"
            );
        }
    }
}
