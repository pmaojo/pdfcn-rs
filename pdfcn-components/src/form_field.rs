//! `%Input`/`%Textarea`/`%Select`/`%Checkbox`/`%RadioItem`/`%Label`: shadcn's
//! form controls, rendered as their static print reading. A live document
//! (invoice, filled contract) shows *filled-in* data, not an editable
//! control, so Input/Textarea/Select render as a labeled, boxed value
//! rather than an `<input>`; Checkbox/RadioItem render as a fixed glyph for
//! their checked state rather than a toggle.

use maud::{html, Markup};
use pdfcn_template::ResolvedAttr;

use crate::{attr, attr_or};

fn filled_field(is_textarea: bool, attrs: &[ResolvedAttr]) -> Markup {
    let label = attr(attrs, "label");
    let value = attr_or(attrs, "value", "");
    let box_class = if is_textarea {
        "form-field-box flex w-full min-h-16 rounded-md border border-input bg-background px-3 py-2 text-sm"
    } else {
        "form-field-box flex w-full rounded-md border border-input bg-background px-3 py-2 text-sm"
    };
    html! {
        div class="form-field mb-3" {
            @if let Some(l) = label {
                label class="form-field-label mb-1 block text-sm font-medium leading-none" { (l) }
            }
            div class=(box_class) { (value) }
        }
    }
}

pub fn input(attrs: &[ResolvedAttr]) -> Markup {
    filled_field(false, attrs)
}

pub fn textarea(attrs: &[ResolvedAttr]) -> Markup {
    filled_field(true, attrs)
}

pub fn select(attrs: &[ResolvedAttr]) -> Markup {
    filled_field(false, attrs)
}

pub fn label(attrs: &[ResolvedAttr], children: Markup) -> Markup {
    let text = attr(attrs, "text");
    html! {
        label class="text-sm font-medium leading-none" {
            @if let Some(t) = text { (t) }
            (children)
        }
    }
}

fn is_checked(attrs: &[ResolvedAttr]) -> bool {
    attr_or(attrs, "checked", "false") == "true"
}

pub fn checkbox(attrs: &[ResolvedAttr]) -> Markup {
    let checked = is_checked(attrs);
    let label = attr(attrs, "label");
    let glyph = if checked { "\u{2611}" } else { "\u{2610}" };
    let glyph_class = if checked {
        "checkbox-glyph checkbox-checked h-4 w-4 rounded-sm border border-primary bg-primary text-primary-foreground flex items-center justify-center text-xs"
    } else {
        "checkbox-glyph checkbox-unchecked h-4 w-4 rounded-sm border border-input bg-background flex items-center justify-center text-xs"
    };
    html! {
        div class="checkbox-field flex items-center gap-2" {
            span class=(glyph_class) { (glyph) }
            @if let Some(l) = label { span class="text-sm" { (l) } }
        }
    }
}

pub fn radio_item(attrs: &[ResolvedAttr]) -> Markup {
    let checked = is_checked(attrs);
    let label = attr(attrs, "label");
    let glyph = if checked { "\u{25CF}" } else { "\u{25CB}" };
    let glyph_class = if checked {
        "radio-glyph radio-checked h-4 w-4 rounded-full border border-primary text-primary flex items-center justify-center text-xs"
    } else {
        "radio-glyph radio-unchecked h-4 w-4 rounded-full border border-input flex items-center justify-center text-xs"
    };
    html! {
        div class="radio-field flex items-center gap-2" {
            span class=(glyph_class) { (glyph) }
            @if let Some(l) = label { span class="text-sm" { (l) } }
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
    fn input_renders_a_labeled_boxed_value() {
        let out = input(&[a("label", "Full name"), a("value", "Ada Lovelace")]).into_string();
        assert!(out.contains("Full name"));
        assert!(out.contains("Ada Lovelace"));
        assert!(out.contains("form-field-box"));
        assert!(out.contains("border-input"));
    }

    #[test]
    fn textarea_boxes_grow_taller_than_a_single_line() {
        let out = textarea(&[a("label", "Notes"), a("value", "Paid in full")]).into_string();
        assert!(out.contains("min-h-16"));
        assert!(out.contains("Paid in full"));
    }

    #[test]
    fn select_renders_the_chosen_value_as_a_filled_box() {
        let out = select(&[a("label", "Status"), a("value", "Approved")]).into_string();
        assert!(out.contains("Approved"));
        assert!(out.contains("form-field-box"));
    }

    #[test]
    fn checkbox_glyph_state_matches_checked_attribute() {
        let checked = checkbox(&[a("checked", "true"), a("label", "Terms agreed")]).into_string();
        assert!(checked.contains("checkbox-checked"));
        assert!(checked.contains("Terms agreed"));

        let unchecked = checkbox(&[a("checked", "false"), a("label", "Newsletter")]).into_string();
        assert!(unchecked.contains("checkbox-unchecked"));
        assert_ne!(checked, unchecked);
    }

    #[test]
    fn radio_item_glyph_state_matches_checked_attribute() {
        let checked = radio_item(&[a("checked", "true"), a("label", "Option A")]).into_string();
        assert!(checked.contains("radio-checked"));
        assert!(checked.contains("Option A"));
    }
}
