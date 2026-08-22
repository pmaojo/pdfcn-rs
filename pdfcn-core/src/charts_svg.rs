//! Charts v2 (Ola 1.3): SVG generators for the placeholder payloads that
//! `%LineChart` / `%StackedBarChart` / `%PieChart` / `%Sparkline` embed in
//! their `pdfcn-chart:` srcs. Sits on the Ola 1.2 substrate: a component
//! serializes a small self-describing JSON spec into the src, this module
//! turns it into SVG, and `raster::svg_to_png` rasterizes it into the image
//! map -- no new pipeline concepts, just another deterministic
//! bytes-under-a-src generator like the QR pass.
//!
//! All geometry is computed in the SVG's own px coordinate space; axis
//! labels use `<text font-family="Inter">`, which `raster::font_db` resolves
//! against the embedded Inter cuts. A spec that is malformed in any way
//! yields `None` -- the caller leaves the placeholder unresolved, which
//! degrades exactly like any other missing image. Nothing here panics.

use serde_json::Value as JsonValue;

/// shadcn-flavored default series palette. Charts are data ink, not brand
/// surfaces, so these are fixed scale colors rather than theme tokens -- an
/// explicit `colors` attribute on the component overrides them per series.
pub(crate) const PALETTE: [&str; 6] = [
    "#2563eb", "#16a34a", "#f59e0b", "#dc2626", "#8b5cf6", "#0891b2",
];

const INK: &str = "#52525b";
const GRID: &str = "#e4e4e7";
const AXIS: &str = "#d4d4d8";

/// Entry point: one chart spec -> one complete `<svg>` document.
pub(crate) fn chart_svg(spec: &JsonValue) -> Option<String> {
    match spec.get("k").and_then(JsonValue::as_str)? {
        "line" => line_svg(spec),
        "stack" => stacked_bar_svg(spec),
        "pie" => pie_svg(spec),
        "spark" => sparkline_svg(spec),
        _ => None,
    }
}

/// XML-escapes text destined for element content or a double-quoted
/// attribute. Chart labels come from caller data, so they are untrusted.
fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

/// Trims a float for display: no trailing `.0`, no scientific notation for
/// the magnitudes charts deal with.
fn fmt_num(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        let s = format!("{v:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn dims(spec: &JsonValue) -> (f64, f64) {
    let w = spec.get("w").and_then(JsonValue::as_f64).unwrap_or(420.0);
    let h = spec.get("h").and_then(JsonValue::as_f64).unwrap_or(170.0);
    (w.clamp(40.0, 2000.0), h.clamp(40.0, 2000.0))
}

/// The `s` field: one or more series of numbers. A single flat array is
/// accepted as a one-series chart, so callers don't have to nest.
fn series(spec: &JsonValue) -> Option<Vec<Vec<f64>>> {
    let outer = spec.get("s")?.as_array()?;
    if outer.is_empty() {
        return None;
    }
    // A flat array of numbers is a single-series chart.
    let rows: Vec<&Vec<JsonValue>> = match &outer[0] {
        JsonValue::Array(_) => {
            let mut nested = Vec::with_capacity(outer.len());
            for item in outer {
                nested.push(item.as_array()?);
            }
            nested
        }
        _ => vec![outer],
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let values: Vec<f64> = row.iter().map(|v| v.as_f64()).collect::<Option<Vec<_>>>()?;
        if values.iter().any(|v| !v.is_finite()) || values.iter().any(|v| *v < 0.0) {
            return None;
        }
        out.push(values);
    }
    let len = out[0].len();
    if len == 0 || out.iter().any(|s| s.len() != len) {
        return None;
    }
    Some(out)
}

fn string_list(spec: &JsonValue, key: &str) -> Vec<String> {
    spec.get(key)
        .and_then(JsonValue::as_array)
        .map(|items| {
            items
                .iter()
                .map(|v| match v {
                    JsonValue::String(s) => s.clone(),
                    JsonValue::Null => String::new(),
                    other => other.to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Series colors: an optional caller-supplied list of hex values first,
/// padded out to six entries with the default palette so every series
/// always has one.
fn colors(spec: &JsonValue) -> Vec<String> {
    let palette: Vec<String> = PALETTE.iter().map(|s| s.to_string()).collect();
    match spec.get("c").and_then(JsonValue::as_array) {
        Some(items) => {
            let custom: Vec<String> = items
                .iter()
                .filter_map(JsonValue::as_str)
                .filter(|c| c.starts_with('#') && c.len() <= 9)
                .map(str::to_string)
                .take(PALETTE.len())
                .collect();
            palette
                .into_iter()
                .enumerate()
                .map(|(i, fallback)| custom.get(i).cloned().unwrap_or(fallback))
                .collect()
        }
        None => palette,
    }
}

/// A "nice" axis step (1/2/5 x 10^n) for `~ticks` gridlines below `max`.
fn nice_step(max: f64, ticks: f64) -> f64 {
    let raw = (max / ticks).max(1e-9);
    let magnitude = 10f64.powf(raw.log10().floor());
    let normalized = raw / magnitude;
    let unit = if normalized <= 1.0 {
        1.0
    } else if normalized <= 2.0 {
        2.0
    } else if normalized <= 5.0 {
        5.0
    } else {
        10.0
    };
    unit * magnitude
}

fn open_svg(w: f64, h: f64) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\">"
    )
}

/// Shared y-axis scaffolding for the two cartesian kinds: returns the tick
/// values and appends gridlines + labels + axis lines to `out`.
fn y_axis(out: &mut String, max: f64, left: f64, top: f64, plot_w: f64, plot_h: f64) -> Vec<f64> {
    let step = nice_step(max, 4.0);
    let top_tick = (max / step).ceil();
    let ticks: Vec<f64> = (0..=top_tick as i64).map(|i| i as f64 * step).collect();
    for tick in &ticks {
        let y = top + plot_h - (tick / (top_tick * step)) * plot_h;
        out.push_str(&format!(
            "<line x1=\"{left}\" y1=\"{y:.1}\" x2=\"{:.1}\" y2=\"{y:.1}\" stroke=\"{GRID}\" stroke-width=\"1\"/>",
            left + plot_w
        ));
        out.push_str(&format!(
            "<text x=\"{}\" y=\"{:.1}\" text-anchor=\"end\" font-family=\"Inter\" font-size=\"9\" fill=\"{INK}\">{}</text>",
            left - 5.0,
            y + 3.0,
            esc(&fmt_num(*tick))
        ));
    }
    out.push_str(&format!(
        "<line x1=\"{left}\" y1=\"{top}\" x2=\"{left}\" y2=\"{:.1}\" stroke=\"{AXIS}\" stroke-width=\"1\"/>",
        top + plot_h
    ));
    out.push_str(&format!(
        "<line x1=\"{left}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{AXIS}\" stroke-width=\"1\"/>",
        top + plot_h,
        left + plot_w,
        top + plot_h
    ));
    ticks
}

fn x_labels(
    out: &mut String,
    labels: &[String],
    count: usize,
    left: f64,
    top: f64,
    plot_w: f64,
    plot_h: f64,
) {
    if labels.is_empty() {
        return;
    }
    // Sample to at most ~8 labels so long categories stay readable.
    let stride = labels.len().div_ceil(8);
    for (i, label) in labels.iter().enumerate() {
        if label.is_empty() || i % stride != 0 {
            continue;
        }
        let x =
            left + (count.max(2) as f64 - 1.0).min(i as f64) / (count - 1).max(1) as f64 * plot_w;
        let x = if count == 1 { left + plot_w / 2.0 } else { x };
        out.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" font-family=\"Inter\" font-size=\"9\" fill=\"{INK}\">{}</text>",
            x,
            top + plot_h + 13.0,
            esc(label)
        ));
    }
}

fn legend(out: &mut String, labels: &[String], colors: &[String], x: f64, y: f64) {
    let mut cursor = x;
    for (i, label) in labels.iter().enumerate() {
        let color = colors.get(i).map(String::as_str).unwrap_or(PALETTE[0]);
        out.push_str(&format!(
            "<rect x=\"{cursor:.1}\" y=\"{y:.1}\" width=\"9\" height=\"9\" rx=\"2\" fill=\"{color}\"/>"
        ));
        out.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"Inter\" font-size=\"9\" fill=\"{INK}\">{}</text>",
            cursor + 13.0,
            y + 8.0,
            esc(label)
        ));
        // Rough advance: swatch + gap + ~5.5px/char of Inter at 9px.
        cursor += 26.0 + label.chars().count() as f64 * 5.5;
    }
}

fn line_svg(spec: &JsonValue) -> Option<String> {
    let series = series(spec)?;
    let (w, h) = dims(spec);
    let names = string_list(spec, "sl");
    let labels = string_list(spec, "xl");
    let colors = colors(spec);
    let legend_h = if names.is_empty() { 0.0 } else { 18.0 };

    let left = 38.0;
    let top = 10.0 + legend_h;
    let plot_w = (w - left - 12.0).max(10.0);
    let plot_h = (h - top - 24.0).max(10.0);

    let max = series
        .iter()
        .flatten()
        .copied()
        .fold(0.0f64, f64::max)
        .max(1e-9);

    let mut out = open_svg(w, h);
    let ticks = y_axis(&mut out, max, left, top, plot_w, plot_h);
    let y_max = ticks[ticks.len() - 1];
    let count = series[0].len();

    for (i, values) in series.iter().enumerate() {
        let color = colors.get(i).map(String::as_str).unwrap_or(PALETTE[0]);
        let points: Vec<String> = values
            .iter()
            .enumerate()
            .map(|(j, v)| {
                let x = left
                    + if count == 1 {
                        plot_w / 2.0
                    } else {
                        j as f64 / (count - 1) as f64 * plot_w
                    };
                let y = top + plot_h - v / y_max * plot_h;
                format!("{x:.1},{y:.1}")
            })
            .collect();
        out.push_str(&format!(
            "<polyline fill=\"none\" stroke=\"{color}\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" points=\"{}\"/>",
            points.join(" ")
        ));
        if count <= 24 {
            for point in &points {
                let (x, y) = point.split_once(',').unwrap_or(("0", "0"));
                out.push_str(&format!(
                    "<circle cx=\"{x}\" cy=\"{y}\" r=\"2.4\" fill=\"{color}\"/>"
                ));
            }
        }
    }
    x_labels(&mut out, &labels, count, left, top, plot_w, plot_h);
    if !names.is_empty() {
        legend(&mut out, &names, &colors, left, 6.0);
    }
    out.push_str("</svg>");
    Some(out)
}

fn stacked_bar_svg(spec: &JsonValue) -> Option<String> {
    let series = series(spec)?;
    let (w, h) = dims(spec);
    let names = string_list(spec, "sl");
    let labels = string_list(spec, "xl");
    let colors = colors(spec);

    let left = 38.0;
    let top = 10.0;
    let plot_w = (w - left - 12.0).max(10.0);
    let plot_h = (h - top - 24.0).max(10.0);
    let count = series[0].len();

    let totals: Vec<f64> = (0..count)
        .map(|i| series.iter().map(|s| s[i]).sum())
        .collect();
    let max = totals.iter().copied().fold(0.0f64, f64::max).max(1e-9);

    let mut out = open_svg(w, h);
    let ticks = y_axis(&mut out, max, left, top, plot_w, plot_h);
    let y_max = ticks[ticks.len() - 1];

    let slot = plot_w / count as f64;
    let bar_w = (slot * 0.65).clamp(2.0, 48.0);
    for i in 0..count {
        let x = left + slot * i as f64 + (slot - bar_w) / 2.0;
        let mut cumulative = 0.0f64;
        for (s, values) in series.iter().enumerate() {
            let color = colors.get(s).map(String::as_str).unwrap_or(PALETTE[0]);
            let value = values[i];
            if value <= 0.0 {
                continue;
            }
            let y0 = top + plot_h - cumulative / y_max * plot_h;
            let y1 = top + plot_h - (cumulative + value) / y_max * plot_h;
            out.push_str(&format!(
                "<rect x=\"{x:.1}\" y=\"{y1:.1}\" width=\"{bar_w:.1}\" height=\"{:.1}\" fill=\"{color}\"/>",
                (y0 - y1).max(0.5)
            ));
            cumulative += value;
        }
    }
    x_labels(&mut out, &labels, count, left, top, plot_w, plot_h);
    if !names.is_empty() {
        legend(&mut out, &names, &colors, left, 6.0);
    }
    out.push_str("</svg>");
    Some(out)
}

/// One annular (or full) sector as an SVG path; `start`/`end` in degrees,
/// clockwise from 12 o'clock.
fn sector_path(cx: f64, cy: f64, r_outer: f64, r_inner: f64, start: f64, end: f64) -> String {
    let rad = |deg: f64| (deg - 90.0).to_radians();
    let (sx, sy) = (
        cx + r_outer * rad(start).cos(),
        cy + r_outer * rad(start).sin(),
    );
    let (ex, ey) = (cx + r_outer * rad(end).cos(), cy + r_outer * rad(end).sin());
    let large = (end - start) > 180.0;
    if r_inner <= 0.0 {
        format!(
            "M {cx:.2} {cy:.2} L {sx:.2} {sy:.2} A {r_outer:.2} {r_outer:.2} 0 {large} 1 {ex:.2} {ey:.2} Z"
        )
    } else {
        let (ix, iy) = (cx + r_inner * rad(end).cos(), cy + r_inner * rad(end).sin());
        let (isx, isy) = (
            cx + r_inner * rad(start).cos(),
            cy + r_inner * rad(start).sin(),
        );
        format!(
            "M {sx:.2} {sy:.2} A {r_outer:.2} {r_outer:.2} 0 {large} 1 {ex:.2} {ey:.2} L {ix:.2} {iy:.2} A {r_inner:.2} {r_inner:.2} 0 {large} 0 {isx:.2} {isy:.2} Z"
        )
    }
}

fn pie_svg(spec: &JsonValue) -> Option<String> {
    let raw = spec.get("v")?.as_array()?;
    let values: Vec<f64> = raw
        .iter()
        .map(JsonValue::as_f64)
        .collect::<Option<Vec<_>>>()?;
    if values.is_empty() || values.iter().any(|v| !v.is_finite() || *v < 0.0) {
        return None;
    }
    let total: f64 = values.iter().sum();
    if total <= 1e-9 {
        return None;
    }
    let (w, h) = dims(spec);
    let names = string_list(spec, "lb");
    let donut = spec.get("d").and_then(JsonValue::as_bool).unwrap_or(false);
    let colors = colors(spec);

    let legend_w = if names.is_empty() {
        0.0
    } else {
        (names.iter().map(|l| l.chars().count()).max().unwrap_or(0) as f64 * 5.5 + 30.0)
            .min(w * 0.45)
    };
    let chart_cx = (w - legend_w) / 2.0;
    let cy = h / 2.0;
    let r_outer = ((w - legend_w).min(h) / 2.0 - 6.0).max(8.0);
    let r_inner = if donut { r_outer * 0.6 } else { 0.0 };

    let mut out = open_svg(w, h);
    let mut start = 0.0f64;
    for (i, value) in values.iter().enumerate() {
        let color = colors.get(i).map(String::as_str).unwrap_or(PALETTE[0]);
        let sweep = value / total * 360.0;
        let end = start + sweep;
        if sweep >= 359.99 {
            // A full circle: two half arcs (an A command cannot do 360 deg).
            for (from, to) in [(0.0, 180.0), (180.0, 359.999)] {
                out.push_str(&format!(
                    "<path d=\"{}\" fill=\"{color}\"/>",
                    sector_path(chart_cx, cy, r_outer, r_inner, from, to)
                ));
            }
        } else if sweep > 0.01 {
            out.push_str(&format!(
                "<path d=\"{}\" fill=\"{color}\"/>",
                sector_path(chart_cx, cy, r_outer, r_inner, start, end)
            ));
        }
        start = end;
    }
    if !names.is_empty() {
        let row_h = 16.0;
        let rows = names.len().min(8);
        let y0 = cy - rows as f64 * row_h / 2.0;
        let lx = chart_cx + r_outer + 14.0;
        for (i, label) in names.iter().take(8).enumerate() {
            let color = colors.get(i).map(String::as_str).unwrap_or(PALETTE[0]);
            let y = y0 + i as f64 * row_h;
            out.push_str(&format!(
                "<rect x=\"{lx:.1}\" y=\"{:.1}\" width=\"9\" height=\"9\" rx=\"2\" fill=\"{color}\"/>",
                y + 1.0
            ));
            out.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"Inter\" font-size=\"9\" fill=\"{INK}\">{}</text>",
                lx + 13.0,
                y + 9.0,
                esc(label)
            ));
        }
    }
    out.push_str("</svg>");
    Some(out)
}

fn sparkline_svg(spec: &JsonValue) -> Option<String> {
    let series = series(spec)?;
    let values = &series[0];
    let (w, h) = dims(spec);
    let color_owned = colors(spec)
        .into_iter()
        .next()
        .unwrap_or_else(|| PALETTE[0].to_string());
    let color = color_owned.as_str();

    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let (min, max) = if (max - min).abs() < 1e-9 {
        (min - 1.0, max + 1.0)
    } else {
        (min, max)
    };
    let count = values.len();
    let pad = 2.0;
    let x = |i: usize| pad + i as f64 / (count - 1).max(1) as f64 * (w - 2.0 * pad);
    let y = |v: f64| pad + (1.0 - (v - min) / (max - min)) * (h - 2.0 * pad);

    let points: Vec<String> = values
        .iter()
        .enumerate()
        .map(|(i, v)| format!("{:.1},{:.1}", x(i), y(*v)))
        .collect();
    let mut out = open_svg(w, h);
    let first_x = points[0]
        .split_once(',')
        .map(|(x, _)| x.to_string())
        .unwrap_or_else(|| "0".to_string());
    let last_x = w - pad;
    out.push_str(&format!(
        "<polygon fill=\"{color}\" fill-opacity=\"0.12\" stroke=\"none\" points=\"{},{:.1} {} {last_x:.1},{:.1}\"/>",
        first_x,
        h - pad,
        points.join(" "),
        h - pad
    ));
    out.push_str(&format!(
        "<polyline fill=\"none\" stroke=\"{color}\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" points=\"{}\"/>",
        points.join(" ")
    ));
    out.push_str("</svg>");
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn svg_of(spec: JsonValue) -> String {
        chart_svg(&spec).expect("spec should render")
    }

    #[test]
    fn line_chart_draws_gridlines_axis_labels_and_one_polyline_per_series() {
        let svg = svg_of(json!({
            "k": "line",
            "s": [[10, 20, 15], [5, 8, 12]],
            "xl": ["Ene", "Feb", "Mar"],
            "sl": ["2025", "2026"],
        }));
        assert!(svg.starts_with("<svg"));
        assert_eq!(svg.matches("<polyline").count(), 2, "{svg}");
        assert!(svg.contains("<line"), "{svg}");
        assert!(svg.contains("Ene") && svg.contains("Mar"), "{svg}");
        assert!(svg.contains("2025") && svg.contains("2026"), "{svg}");
        // Nice ticks: a max of 20 with default ticks lands on a 5-step axis.
        assert!(svg.contains(">20<"), "{svg}");
    }

    #[test]
    fn stacked_bars_stack_to_their_total() {
        let svg = svg_of(json!({
            "k": "stack",
            "s": [[10, 20], [5, 5]],
            "xl": ["Q1", "Q2"],
        }));
        assert_eq!(svg.matches("<rect").count(), 4, "{svg}");
        assert!(svg.contains(">Q1<"), "{svg}");
    }

    #[test]
    fn pie_and_donut_differ_in_inner_radius() {
        let values = json!({ "k": "pie", "v": [40, 30, 30], "lb": ["a", "b", "c"] });
        let pie = svg_of(values.clone());
        let donut = svg_of(json!({
            "k": "pie", "v": [40, 30, 30], "lb": ["a", "b", "c"], "d": true,
        }));
        assert!(pie.contains("A "), "{pie}");
        // Donut sectors carry two arcs (outer + inner); pie sectors one.
        assert!(
            donut.matches("A ").count() > pie.matches("A ").count(),
            "{donut}"
        );
        assert!(pie.contains(">a<"), "{pie}");
    }

    #[test]
    fn a_single_full_slice_still_renders_without_nan() {
        let svg = svg_of(json!({ "k": "pie", "v": [100] }));
        assert!(!svg.to_lowercase().contains("nan"), "{svg}");
        assert_eq!(svg.matches("<path").count(), 2, "{svg}");
    }

    #[test]
    fn sparkline_is_a_single_stroke_with_an_area_fill() {
        let svg = svg_of(json!({ "k": "spark", "s": [3, 1, 4, 1, 5, 9, 2, 6] }));
        assert_eq!(svg.matches("<polyline").count(), 1, "{svg}");
        assert!(svg.contains("<polygon"), "{svg}");
    }

    #[test]
    fn labels_are_xml_escaped() {
        let svg = svg_of(json!({
            "k": "line", "s": [[1, 2]], "xl": ["<b>&raw</b>"],
        }));
        assert!(!svg.contains("<b>"), "{svg}");
        assert!(svg.contains("&lt;b&gt;&amp;raw&lt;/b&gt;"), "{svg}");
    }

    #[test]
    fn malformed_specs_degrade_to_none() {
        assert!(chart_svg(&json!({})).is_none());
        assert!(chart_svg(&json!({ "k": "unknown" })).is_none());
        assert!(chart_svg(&json!({ "k": "line" })).is_none());
        assert!(chart_svg(&json!({ "k": "line", "s": [] })).is_none());
        assert!(chart_svg(&json!({ "k": "line", "s": [[1, 2], [3]] })).is_none());
        assert!(chart_svg(&json!({ "k": "line", "s": [[1, -2]] })).is_none());
        assert!(chart_svg(&json!({ "k": "line", "s": [[1, "x"]] })).is_none());
        assert!(chart_svg(&json!({ "k": "pie", "v": [0, 0] })).is_none());
    }

    #[test]
    fn flat_single_series_is_accepted_as_one_series() {
        let svg = chart_svg(&json!({ "k": "line", "s": [1, 2, 3] })).expect("flat array works");
        assert_eq!(svg.matches("<polyline").count(), 1);
    }
}
