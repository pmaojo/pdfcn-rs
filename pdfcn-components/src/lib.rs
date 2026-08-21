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

/// `image` (optional): a full-bleed cover image above the card body — the
/// shadcn "product card" composition. `class` (optional): extra utility
/// classes appended to the card's own root element -- use this rather than
/// wrapping `%Card` in another div for spacing (e.g. `class="m-2"` in a
/// grid of cards), since an extra wrapper level between the grid and the
/// card can throw off absolute-positioning math in the underlying
/// `printpdf`/`azul-layout` renderer (see the `.absolute` note below).
///
/// The outer wrapper carries `relative` + `overflow-hidden`, so a child
/// marked `.absolute` composes on top of the image -- positioned against
/// the whole card, not just the padded body -- while staying clipped to
/// the card's rounded corners. That composed child must be a plain
/// element carrying its own styling directly (a `div` with
/// `bg-destructive`/`rounded-full`/etc., holding its text straight away);
/// it can't be a wrapper `div` around a further component, nor a
/// `display:flex`/`inline-flex` component like `%Badge` (directly or
/// nested) -- both trigger a renderer bug where the composed element keeps
/// its pre-absolute static position instead of moving to the corner.
fn card(attrs: &[ResolvedAttr], children: Markup) -> Markup {
    let title = attr(attrs, "title");
    let image = attr(attrs, "image");
    let extra = attr_or(attrs, "class", "");
    html! {
        div class={ "card relative overflow-hidden rounded-lg border bg-card text-card-foreground shadow-sm break-inside-avoid " (extra) } {
            @if let Some(src) = image {
                img class="card-image w-full h-48 object-cover" src=(src) alt=(attr_or(attrs, "image-alt", ""));
            }
            div class="card-body p-4" {
                @if let Some(t) = title {
                    h2 class="text-lg font-semibold mb-2" { (t) }
                }
                (children)
            }
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

/// `class` (optional): extra utility classes appended to the badge's own
/// root element -- e.g. spacing (`class="mt-2"`) or a color override for
/// one-off use. Not a way to make a positioned badge: `%Badge` renders as
/// `inline-flex`, which the underlying `printpdf`/`azul-layout` renderer
/// currently mispositions whenever it (or an ancestor) is
/// `position:absolute` -- it keeps its pre-absolute static position
/// instead of moving to the given offset. For a badge-like overlay (a
/// discount ribbon pinned to a `%Card` image), use a plain `div` with the
/// badge-style utility classes directly instead of `%Badge`.
fn badge(attrs: &[ResolvedAttr], children: Markup) -> Markup {
    let variant = attr_or(attrs, "variant", "default");
    let label = attr(attrs, "label");
    let extra = attr_or(attrs, "class", "");
    html! {
        span class={ "badge inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-semibold " (badge_classes(variant)) " " (extra) } {
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
    fn card_with_image_renders_a_full_bleed_cover_photo() {
        let out = render(
            "Card",
            &[a("title", "Trail Runner"), a("image", "sneaker.png"), a("image-alt", "Trail Runner shoe")],
            html! {},
        )
        .unwrap()
        .unwrap()
        .into_string();
        assert!(out.contains("<img"));
        assert!(out.contains(r#"src="sneaker.png""#));
        assert!(out.contains(r#"alt="Trail Runner shoe""#));
        assert!(out.contains("object-cover"));
        // The card wrapper must be a positioning root so an absolutely
        // positioned child (a price badge) overlays the image correctly.
        assert!(out.contains("relative"));
        assert!(out.contains("overflow-hidden"));
    }

    #[test]
    fn card_without_image_omits_the_img_tag() {
        let out = render("Card", &[a("title", "Overview")], html! {})
            .unwrap()
            .unwrap()
            .into_string();
        assert!(!out.contains("<img"));
    }

    #[test]
    fn card_composes_an_absolutely_positioned_overlay_over_its_image() {
        // A discount ribbon: a plain element carrying `.absolute` (and its
        // own badge-style utility classes) directly, holding its text
        // straight away -- the pattern verified (by inspecting actual
        // rendered PDF coordinates, not just this HTML) to position
        // correctly against the card. Deliberately NOT `%Badge` wrapped in
        // a `.absolute` div, and not `%Badge` given `.absolute` directly
        // either: `%Badge` renders `display:inline-flex`, which the
        // underlying `printpdf`/`azul-layout` renderer currently
        // mispositions whenever it (or a sibling) sits under
        // `position:absolute` -- it keeps its pre-absolute static
        // position instead of moving to the given offset, landing on top
        // of the title text instead of the image. See `card`'s doc
        // comment for the full pattern.
        let overlay = html! {
            div class="absolute top-2 right-2 z-10 rounded-full bg-destructive text-white text-xs font-semibold" { "-20%" }
        };
        let out = render("Card", &[a("image", "sneaker.png")], overlay)
            .unwrap()
            .unwrap()
            .into_string();
        assert!(out.contains("card-image"));
        assert!(out.contains("absolute top-2 right-2 z-10"));
        assert!(!out.contains("inline-flex"));
    }

    #[test]
    fn card_class_attribute_appends_to_its_own_root_element() {
        // Spacing between cards in a grid must land on `%Card`'s own root
        // element via `class`, not a wrapper `div` around it -- an extra
        // wrapper level between the grid and the card breaks the same
        // absolute-positioning math referenced above.
        let out = render("Card", &[a("class", "m-2")], html! {})
            .unwrap()
            .unwrap()
            .into_string();
        assert!(out.contains("card relative overflow-hidden"));
        assert!(out.contains("m-2"));
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
