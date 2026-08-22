//! The Shadcn-style component registry (FR-2): `%InvoiceTable`, `%Badge`,
//! `%Card`, and friends expand into pre-styled `maud` markup fragments.
//! Attribute values arrive as plain strings (already resolved by
//! `pdfcn-template`); a component that needs structured data (e.g.
//! `InvoiceTable`'s `rows`) expects a JSON-encoded string, the same "data
//! island" convention as an HTML `data-*` attribute.

mod alert;
mod avatar;
mod chart;
mod form_field;
mod nav;
mod progress;
mod qrcode;

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
        "QRCode" => Some(qrcode::qrcode(attrs)),
        "BarChart" => Some(chart::bar_chart(attrs)),
        "PageFooter" => Some(page_footer(attrs)),
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
///
/// Audited against shadcn/ui's real `components/ui/card.tsx`: the outer
/// wrapper's `rounded-lg border border-border bg-card text-card-foreground
/// shadow-sm` matches shadcn's own base classes exactly (`border-border`
/// spelled out is the same Wave 0 token the bare `border` would resolve to
/// -- kept explicit for the same reason documented on `table`). What
/// doesn't match 1:1 is deliberate: shadcn splits `CardHeader` (title,
/// `text-2xl font-semibold leading-none tracking-tight`) from `CardContent`
/// (`p-6 pt-0`) as separate padded regions, while pdfcn flattens both into
/// one `card-body p-4` with a `text-lg font-semibold` title -- there's no
/// `%CardHeader`/`%CardContent`/`%CardFooter` split here. Matching
/// shadcn's `text-2xl` literally would make the title compete visually
/// with card body content that's often itself `text-2xl` (e.g. a stat
/// card's value), which is a real regression for pdfcn's denser
/// print/invoice layouts, not a defect. See task 15 (revisit flattened
/// static components) for whether the header/content split is worth
/// adding.
fn card(attrs: &[ResolvedAttr], children: Markup) -> Markup {
    let title = attr(attrs, "title");
    let image = attr(attrs, "image");
    let extra = attr_or(attrs, "class", "");
    html! {
        div class={ "card relative overflow-hidden rounded-lg border border-border bg-card text-card-foreground shadow-sm break-inside-avoid " (extra) } {
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

/// Audited against shadcn/ui's real `components/ui/table.tsx`: it wraps
/// `<table>` in a `relative w-full overflow-auto` scroll container and
/// gives the table itself `w-full caption-bottom text-sm` -- pdfcn keeps
/// `border border-border` on top of that (already a Wave 0 token, not a
/// hand-picked color) since there's no separate `TableRow` component here
/// to carry shadcn's own per-row `border-b`, and a document table with no
/// visible grid lines at all would be a regression for pdfcn's print/
/// invoice use case.
fn table(attrs: &[ResolvedAttr], children: Markup) -> Markup {
    let variant = attr_or(attrs, "variant", "default");
    let variant_class = match variant {
        "striped" => " table-striped",
        "bordered" => " table-bordered",
        "compact" => " table-compact",
        _ => "",
    };
    html! {
        div class="relative w-full overflow-auto" {
            table class={ "table w-full caption-bottom text-sm border border-border" (variant_class) } {
                (children)
            }
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

/// Audited against shadcn/ui's real `components/ui/badge.tsx`: `outline`
/// there is just `text-foreground` -- the surrounding border color comes
/// from the base class's bare `border` (which already resolves to the
/// `border-border` token), not from a color explicitly picked for badges.
/// pdfcn previously added `border-input` on top, which is both redundant
/// (duplicates the base class's `border`) and the wrong semantic token --
/// `input` is shadcn's form-control border color, not a general-purpose
/// one, and a badge is not a form control.
///
/// `success` has no shadcn upstream equivalent (real shadcn ships only
/// `default`/`secondary`/`destructive`/`outline`); pdfcn adds it because
/// invoice/document status badges ("Paid", "OK") are a real print use case
/// shadcn's own component set doesn't anticipate. It's sourced from the
/// Wave 0 `green` accent scale (`tokens::scale_color`), not a hand-picked
/// hex, so it stays consistent with the rest of the token system even
/// though the variant itself is a pdfcn-only extension.
fn badge_classes(variant: &str) -> &'static str {
    match variant {
        "outline" => "text-foreground",
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

/// Audited against shadcn/ui's real `components/ui/separator.tsx`: it
/// supports `orientation` (`horizontal`'s `h-px w-full` vs `vertical`'s
/// `h-full w-px`) and forwards a caller `className` -- pdfcn's version
/// previously ignored every attribute, hardcoding horizontal-only with no
/// way to extend or override it, unlike `%Card`/`%Badge` which both
/// support `class`. The `my-4` default vertical rhythm has no shadcn
/// upstream equivalent (real Separator carries no margin of its own,
/// leaving spacing entirely to the caller's `className`); pdfcn keeps it
/// as the default here since every existing `%Separator` use relies on it
/// for section spacing in a print document, but a caller can now override
/// or extend it via `class`.
fn separator(attrs: &[ResolvedAttr]) -> Markup {
    let vertical = attr_or(attrs, "orientation", "horizontal") == "vertical";
    let dim = if vertical { "h-full w-px" } else { "h-px w-full my-4" };
    let extra = attr_or(attrs, "class", "");
    html! {
        div role="separator" class={ "separator shrink-0 bg-border " (dim) " " (extra) };
    }
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
        table class="invoice-table w-full border border-border" {
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

/// `%PageFooter`: document-chrome footer strip -- a hairline rule over a
/// left/center/right three-slot row, the classic letterhead footer.
/// All three slots are optional; an empty slot renders nothing, and a
/// single populated slot sits at its own side (left by default).
///
/// This is *flow-level* chrome: it renders once, at the end of the
/// document, wherever the content ends. It does not repeat on every page
/// -- the layout engine has no verified support for `position:fixed`
/// page repetition or `@page` margin boxes, so claiming per-page repetition
/// would be a lie. For a per-document "Page X of Y" label, `%Pagination`
/// takes data-driven values; for a letterhead footer line, this is it.
///
/// `left` / `center` / `right` (optional): slot text, interpolated by the
/// template engine like any attribute (`left="{{ company.name }}"`).
/// `class` (optional): extra utility classes on the footer's root.
fn page_footer(attrs: &[ResolvedAttr]) -> Markup {
    let left = attr(attrs, "left");
    let center = attr(attrs, "center");
    let right = attr(attrs, "right");
    let extra = attr_or(attrs, "class", "");
    html! {
        footer class={ "page-footer mt-8 pt-3 border-t border-border w-full text-xs text-muted-foreground flex justify-between items-center gap-4 break-inside-avoid " (extra) } {
            @if let Some(l) = left {
                span class="page-footer-left" { (l) }
            }
            @if let Some(c) = center {
                span class="page-footer-center" { (c) }
            }
            @if let Some(r) = right {
                span class="page-footer-right" { (r) }
            }
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
    fn badge_uses_variant_classes() {
        let out = render("Badge", &[a("variant", "destructive")], html! {})
            .unwrap()
            .unwrap();
        assert!(out.into_string().contains("bg-destructive"));
    }

    #[test]
    fn badge_outline_variant_matches_shadcns_real_classes() {
        // shadcn's own outline variant is just `text-foreground` -- the
        // border comes from the base class's bare `border`, not a
        // form-control `border-input` token pdfcn previously added.
        let out = render("Badge", &[a("variant", "outline")], html! {})
            .unwrap()
            .unwrap()
            .into_string();
        assert!(out.contains("text-foreground"));
        assert!(!out.contains("border-input"));
    }

    #[test]
    fn separator_defaults_to_horizontal() {
        let out = render("Separator", &[], html! {}).unwrap().unwrap().into_string();
        assert!(out.contains("h-px w-full"));
        assert!(!out.contains("h-full w-px"));
    }

    #[test]
    fn separator_supports_vertical_orientation() {
        let out = render("Separator", &[a("orientation", "vertical")], html! {})
            .unwrap()
            .unwrap()
            .into_string();
        assert!(out.contains("h-full w-px"));
        assert!(!out.contains("h-px w-full"));
    }

    #[test]
    fn separator_class_attribute_appends_to_its_own_root_element() {
        let out = render("Separator", &[a("class", "mt-8")], html! {})
            .unwrap()
            .unwrap()
            .into_string();
        assert!(out.contains("mt-8"));
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
    fn table_wrapper_matches_shadcns_real_anatomy() {
        // Real shadcn Table wraps in a scrollable `relative w-full
        // overflow-auto` div, and the `<table>` itself carries `w-full
        // caption-bottom text-sm` -- audited against shadcn/ui's actual
        // `components/ui/table.tsx`, not just pdfcn's hand-picked classes.
        let out = render("Table", &[], html! { "rows" })
            .unwrap()
            .unwrap()
            .into_string();
        assert!(out.starts_with(r#"<div class="relative w-full overflow-auto">"#));
        assert!(out.contains("caption-bottom"));
        assert!(out.contains("text-sm"));
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

    #[test]
    fn page_footer_renders_populated_slots_and_skips_empty_ones() {
        let out = render(
            "PageFooter",
            &[a("left", "Acme Inc."), a("right", "hello@acme.com")],
            html! {},
        )
        .unwrap()
        .unwrap()
        .into_string();
        assert!(out.contains("Acme Inc."));
        assert!(out.contains("hello@acme.com"));
        assert!(out.contains("page-footer-left"));
        assert!(out.contains("page-footer-right"));
        assert!(!out.contains("page-footer-center"));
        // Letterhead chrome: hairline rule, small muted ink.
        assert!(out.contains("border-t"));
        assert!(out.contains("text-xs"));
    }

    #[test]
    fn page_footer_with_no_slots_still_renders_its_rule() {
        let out = render("PageFooter", &[], html! {})
            .unwrap()
            .unwrap()
            .into_string();
        assert!(out.contains("border-t"));
        assert!(!out.contains("<span"));
    }

    #[test]
    fn page_footer_class_appends_to_its_root() {
        let out = render("PageFooter", &[a("class", "mt-12")], html! {})
            .unwrap()
            .unwrap()
            .into_string();
        assert!(out.contains("mt-12"));
    }
}
