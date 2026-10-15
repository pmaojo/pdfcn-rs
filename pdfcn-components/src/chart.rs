//! `%BarChart`: a simple vertical bar chart rendered entirely with the
//! layout primitives the engine already handles — one flex row of columns,
//! each column stacking a bar (pixel height in proportion to the chart's
//! tallest value) and an optional label underneath.
//!
//! No SVG, no canvas: every visual property maps to utilities and inline
//! styles `azul-layout` resolves, and the spacing between columns comes
//! from the same gap→margin rewrite the rest of the pipeline uses.
//!
//! ```haml
//! %BarChart(values="{{ stats.monthly }}" labels="{{ stats.months }}" height="180px")
//! ```

use maud::{html, Markup};
use pdfcn_template::ResolvedAttr;
use serde_json::Value as JsonValue;

use crate::attr;

/// Parses the `values` attribute (a JSON array of numbers, stringified by
/// the template layer) into f64s.
fn parse_values(raw: Option<&str>) -> Option<Vec<f64>> {
    let raw = raw?;
    match serde_json::from_str::<JsonValue>(raw).ok()? {
        JsonValue::Array(items) => Some(items.iter().map(|v| v.as_f64().unwrap_or(0.0)).collect()),
        _ => None,
    }
}

/// Parses the optional `labels` attribute into display strings; a label of
/// any JSON type is stringified, so both `"Jan"` and `1` work.
fn parse_labels(raw: Option<&str>) -> Vec<String> {
    raw.and_then(|r| serde_json::from_str::<JsonValue>(r).ok())
        .and_then(|v| v.as_array().cloned())
        .map(|items| {
            items
                .iter()
                .map(|item| match item {
                    JsonValue::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn bar_chart(attrs: &[ResolvedAttr]) -> Markup {
    let values = parse_values(attr(attrs, "values")).unwrap_or_default();
    if values.is_empty() {
        return html! {
            div class="pdfcn-invalid-component bg-destructive text-white text-xs font-semibold rounded px-2 py-1" {
                "BarChart: \"values\" must be a non-empty JSON array of numbers"
            }
        };
    }
    let height = attr(attrs, "height").unwrap_or("160px");
    let labels = parse_labels(attr(attrs, "labels"));
    // A tallest value of zero would divide by zero; clamp so an all-zero
    // chart still renders (as flat, zero-height bars) instead of NaN.
    let max = values.iter().copied().fold(0.0, f64::max).max(1e-9);
    // Bars get explicit pixel heights. A percentage would be taken against
    // the whole column -- label row included -- so bars would not be in
    // proportion to each other. The frame carries padding and a border
    // (content-box), so its content height is the requested height minus
    // those; the frame's width is left `auto` for the same reason (an
    // explicit `w-full` plus padding overflows the parent).
    let total_h = height
        .trim_end_matches("px")
        .trim()
        .parse::<f64>()
        .unwrap_or(160.0);
    let label_h = if labels.is_empty() { 0.0 } else { LABEL_ROW_PX };
    let plot_h = (total_h - FRAME_PAD_PX - label_h).max(8.0);
    html! {
        div class="barchart rounded-md border border-border bg-card p-3" {
            div class="flex items-end justify-between gap-2" style={ "height:" (plot_h + label_h) "px" } {
                @for (i, value) in values.iter().enumerate() {
                    div class="flex flex-1 flex-col items-center justify-end" style={ "height:" (plot_h + label_h) "px" } {
                        div class="w-full rounded-t bg-primary" style={ "height:" (format!("{:.1}", value / max * plot_h)) "px" } title=(value.to_string()) {}
                        @if let Some(label) = labels.get(i) {
                            span class="mt-1 text-xs text-muted-foreground" style={ "height:" (LABEL_ROW_PX) "px" } { (label) }
                        }
                    }
                }
            }
        }
    }
}

/// Vertical space the frame's `p-3` padding (2 x 12px) and 1px borders take.
const FRAME_PAD_PX: f64 = 26.0;
/// Height reserved for the label row under the bars.
const LABEL_ROW_PX: f64 = 18.0;

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
    fn bars_scale_against_the_tallest_value() {
        // 160px frame, no labels: plot = 160 - 26 = 134px for the tallest bar.
        let out = bar_chart(&[a("values", "[10, 20, 40]")]).into_string();
        assert!(out.contains("height:33.5px"), "{out}");
        assert!(out.contains("height:67.0px"), "{out}");
        assert!(out.contains("height:134.0px"), "{out}");
    }

    #[test]
    fn labels_render_under_their_bars() {
        let out =
            bar_chart(&[a("values", "[1, 2]"), a("labels", r#"["Jan","Feb"]"#)]).into_string();
        assert!(out.contains("Jan"), "{out}");
        assert!(out.contains("Feb"), "{out}");
        assert_eq!(out.matches("text-xs").count(), 2, "{out}");
    }

    #[test]
    fn missing_or_invalid_values_renders_a_visible_marker() {
        assert!(bar_chart(&[]).into_string().contains("non-empty"));
        assert!(bar_chart(&[a("values", "not json")])
            .into_string()
            .contains("non-empty"));
    }

    #[test]
    fn all_zero_chart_renders_without_nan() {
        let out = bar_chart(&[a("values", "[0, 0]")]).into_string();
        assert!(out.contains("barchart"), "{out}");
        assert!(!out.contains("NaN"), "{out}");
    }

    #[test]
    fn custom_height_is_applied_to_the_frame() {
        // 220px total = 26 frame + 194 plot, so the lone bar fills 194px.
        let out = bar_chart(&[a("values", "[5]"), a("height", "220px")]).into_string();
        assert!(out.contains("height:194.0px"), "{out}");
        assert!(
            !out.contains("w-full barchart") && !out.contains("barchart w-full"),
            "{out}"
        );
    }
}
