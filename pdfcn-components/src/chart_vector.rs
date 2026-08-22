//! Charts v2 components (Ola 1.3), behind the crate's `vector` cargo
//! feature: `%LineChart`, `%StackedBarChart`, `%PieChart` and `%Sparkline`.
//!
//! Same layering as `%QRCode`: these modules produce only markup -- a
//! placeholder `<img src="pdfcn-chart:<hex spec>">` whose payload is a
//! small, self-describing JSON spec (kind, series, labels, size). The asset
//! pass in pdfcn-core decodes the spec, generates the SVG (axes, gridlines,
//! palette) and rasterizes it at print density. Specs are bounded and small,
//! so hex -- not a side channel -- is the right transport; the div-based
//! `%BarChart` in `chart.rs` remains the no-feature fallback.
//!
//! Invalid input renders an explicit invalid-component marker, never a
//! silently empty chart.

use maud::{html, Markup};
use pdfcn_template::ResolvedAttr;
use serde_json::{json, Value as JsonValue};

use crate::{attr, attr_or, hex_encode, invalid_component};

/// Parses a JSON array of numbers (stringified by the template layer).
pub(crate) fn parse_number_array(raw: Option<&str>) -> Option<Vec<f64>> {
    let raw = raw?;
    match serde_json::from_str::<JsonValue>(raw).ok()? {
        JsonValue::Array(items) => {
            let values: Vec<f64> = items
                .iter()
                .map(|v| v.as_f64())
                .collect::<Option<Vec<_>>>()?;
            if values.iter().any(|v| !v.is_finite()) {
                None
            } else {
                Some(values)
            }
        }
        _ => None,
    }
}

/// Parses an optional JSON array of display strings; any JSON type is
/// stringified, so both `"Jan"` and `1` work.
pub(crate) fn parse_string_array(raw: Option<&str>) -> Vec<String> {
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

fn placeholder(kind: &str, spec: JsonValue, attrs: &[ResolvedAttr], default_h: &str) -> Markup {
    let w = attr_or(attrs, "w", "420px");
    let h = attr_or(attrs, "h", default_h);
    // The canvas dims travel in the spec too: the rasterizer prefers the
    // layout box when it can see one, but needs a fallback for boxless use.
    let mut spec = spec;
    if let Some(obj) = spec.as_object_mut() {
        obj.insert(
            "w".into(),
            json!(w.trim_end_matches("px").parse::<f64>().unwrap_or(420.0)),
        );
        obj.insert(
            "h".into(),
            json!(h.trim_end_matches("px").parse::<f64>().unwrap_or(170.0)),
        );
    }
    html! {
        img class="pdfcn-chart inline-block max-w-full" src=(format!("pdfcn-chart:{}", hex_encode(spec.to_string().as_bytes()))) style={ "width:" (w) ";height:" (h) ";" } alt=(attr_or(attrs, "alt", kind));
    }
}

fn colors_attr(attrs: &[ResolvedAttr]) -> JsonValue {
    match attr(attrs, "colors") {
        Some(raw) => serde_json::from_str::<JsonValue>(raw)
            .ok()
            .filter(|v| v.is_array())
            .unwrap_or_else(|| json!([])),
        None => json!([]),
    }
}

/// Common shape of the cartesian specs (line / stacked): one or more series
/// plus optional x/series labels.
fn cartesian_spec(kind: &str, attrs: &[ResolvedAttr]) -> Result<JsonValue, &'static str> {
    // `values` (single flat series) or `series` (array of arrays).
    let values = parse_number_array(attr(attrs, "values"));
    let series = match attr(attrs, "series") {
        raw @ Some(_) => {
            let parsed = match serde_json::from_str::<JsonValue>(raw.unwrap_or("")) {
                Ok(v) => v,
                Err(_) => return Err("series must be a JSON array"),
            };
            match parsed {
                JsonValue::Array(items) if items.first().is_some_and(JsonValue::is_number) => {
                    json!([items])
                }
                JsonValue::Array(_) => parsed,
                _ => return Err("series must be a JSON array"),
            }
        }
        None => {
            let values = values
                .ok_or("\"values\" (or \"series\") must be a non-empty JSON array of numbers")?;
            if values.is_empty() {
                return Err("\"values\" (or \"series\") must be a non-empty JSON array of numbers");
            }
            json!([values])
        }
    };
    Ok(json!({
        "k": kind,
        "s": series,
        "xl": parse_string_array(attr(attrs, "xlabels")),
        "sl": parse_string_array(attr(attrs, "serieslabels")),
        "c": colors_attr(attrs),
    }))
}

pub fn line_chart(attrs: &[ResolvedAttr]) -> Markup {
    match cartesian_spec("line", attrs) {
        Ok(spec) => placeholder("line chart", spec, attrs, "170px"),
        Err(message) => marker(message),
    }
}

pub fn stacked_bar_chart(attrs: &[ResolvedAttr]) -> Markup {
    match cartesian_spec("stack", attrs) {
        Ok(spec) => placeholder("stacked bar chart", spec, attrs, "170px"),
        Err(message) => marker(message),
    }
}

pub fn pie_chart(attrs: &[ResolvedAttr]) -> Markup {
    let Some(values) = parse_number_array(attr(attrs, "values")).filter(|v| !v.is_empty()) else {
        return marker("\"values\" must be a non-empty JSON array of numbers");
    };
    let donut = attr(attrs, "donut")
        .map(|d| d == "true" || d == "1")
        .unwrap_or(false);
    let spec = json!({
        "k": "pie",
        "v": values,
        "lb": parse_string_array(attr(attrs, "labels")),
        "d": donut,
        "c": colors_attr(attrs),
    });
    placeholder("pie chart", spec, attrs, "160px")
}

pub fn sparkline(attrs: &[ResolvedAttr]) -> Markup {
    let Some(values) = parse_number_array(attr(attrs, "values")).filter(|v| !v.is_empty()) else {
        return marker("\"values\" must be a non-empty JSON array of numbers");
    };
    let spec = json!({ "k": "spark", "s": [values] });
    placeholder("sparkline", spec, attrs, "40px")
}

fn marker(message: &str) -> Markup {
    html! {
        div class=(invalid_component()) { "Chart: " (message) }
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

    fn payload(out: &str) -> String {
        let hexed = out
            .split(r#"src="pdfcn-chart:"#)
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .unwrap();
        (0..hexed.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hexed[i..i + 2], 16).unwrap())
            .map(|b| b as char)
            .collect()
    }

    #[test]
    fn line_chart_encodes_a_self_describing_spec() {
        let out = line_chart(&[
            a("values", "[10, 20, 15]"),
            a("xlabels", r#"["Ene","Feb","Mar"]"#),
            a("serieslabels", r#"["2026"]"#),
        ])
        .into_string();
        assert!(out.contains("src=\"pdfcn-chart:"), "{out}");
        let spec: JsonValue = serde_json::from_str(&payload(&out)).unwrap();
        assert_eq!(spec["k"], "line");
        assert_eq!(spec["s"], json!([[10.0, 20.0, 15.0]]));
        assert_eq!(spec["xl"], json!(["Ene", "Feb", "Mar"]));
    }

    #[test]
    fn multi_series_is_accepted_via_the_series_attribute() {
        let out = line_chart(&[a("series", r#"[[10, 20], [5, 8]]"#)]).into_string();
        let spec: JsonValue = serde_json::from_str(&payload(&out)).unwrap();
        // serde_json distinguishes Number(10) from Number(10.0), so compare
        // through as_f64 rather than raw Value equality.
        assert_eq!(spec["s"][0][0].as_f64(), Some(10.0));
        assert_eq!(spec["s"][0][1].as_f64(), Some(20.0));
        assert_eq!(spec["s"][1][0].as_f64(), Some(5.0));
        assert_eq!(spec["s"][1][1].as_f64(), Some(8.0));
    }

    #[test]
    fn stacked_and_pie_and_spark_kinds_are_distinct() {
        let stacked = stacked_bar_chart(&[a("values", "[1, 2]")]).into_string();
        let pie = pie_chart(&[a("values", "[1, 2, 3]"), a("donut", "true")]).into_string();
        let spark = sparkline(&[a("values", "[3, 1, 4]")]).into_string();
        assert!(payload(&stacked).contains(r#""k":"stack""#), "{stacked}");
        assert!(payload(&pie).contains(r#""k":"pie""#) && payload(&pie).contains(r#""d":true"#));
        assert!(payload(&spark).contains(r#""k":"spark""#));
        assert!(spark.contains("height:40px"), "{spark}");
    }

    #[test]
    fn junk_input_renders_an_explicit_marker() {
        assert!(line_chart(&[]).into_string().contains("Chart:"));
        assert!(line_chart(&[a("values", "nope")])
            .into_string()
            .contains("Chart:"));
        assert!(pie_chart(&[a("values", "[1]")])
            .into_string()
            .contains("pdfcn-chart:"));
        assert!(sparkline(&[]).into_string().contains("Chart:"));
    }
}
