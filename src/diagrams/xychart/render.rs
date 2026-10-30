// Port of src/diagrams/xychart/render.ts.

use super::db::{XAxis, XyChartDb};
use super::ticks::{tick_index, tick_step, tick_value, ticks_between};
use crate::diagrams::common::fit_width::cap_width;
use crate::errors::{PeleError, PeleErrorCode};
use crate::js::{self, num_str};
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num};
use crate::svg::root::{svg_document, Accessible};
use crate::svg::theme::series_color;
use crate::text::label::{layout_label, Label};
use crate::text::measurer::{style, width as measure};
use crate::types::{RenderOptions, Rendered};
use crate::util::yaml::{Map, Value};

const TICK: f64 = 4.0;
const SWATCH: f64 = 12.0;
const ROW: f64 = 26.0;
const LEGEND_GAP: f64 = 24.0;
const LIMIT: f64 = 1e300;

struct Mark {
    at: f64,
    label: Label,
}

fn record<'a>(value: Option<&'a Value>, empty: &'a Map) -> &'a Map {
    match value {
        Some(Value::Map(m)) => m,
        _ => empty,
    }
}

fn on(config: &Map, key: &str, fallback: bool) -> bool {
    match config.get(key) {
        Some(Value::Bool(b)) => *b,
        _ => fallback,
    }
}

fn dimension(value: Option<&Value>, fallback: f64) -> f64 {
    match value {
        Some(Value::Num(v)) if *v > 0.0 => js::round(v.max(100.0).min(4000.0)),
        _ => fallback,
    }
}

fn finite(value: Option<f64>) -> Option<f64> {
    value.filter(|v| v.is_finite()).map(|v| v.max(-LIMIT).min(LIMIT))
}

fn widest(marks: &[Mark]) -> f64 {
    let mut width: f64 = 0.0;
    for mark in marks {
        width = width.max(mark.label.width);
    }
    width
}

// Writes a label with its left edge, center or right edge at x.
fn aligned(label: &Label, x: f64, y: f64, anchor: &str) -> String {
    if label.lines.len() == 1 && label.lines[0].len() == 1 && label.lines[0][0].style == 0 {
        return format!("<text x=\"{}\" y=\"{}\" text-anchor=\"{}\">{}</text>", num(x), num(y + label.size * 0.35), anchor, esc(&label.lines[0][0].text));
    }
    let dx = match anchor {
        "start" => label.width / 2.0,
        "end" => -label.width / 2.0,
        _ => 0.0,
    };
    label_svg(label, x + dx, y, "")
}

fn text(raw: Option<&str>, font_size: f64, max: f64) -> Label {
    layout_label(raw, false, font_size, max, 0)
}

pub fn render_xychart(model: &XyChartDb, config: &Config, options: &RenderOptions) -> Result<Rendered, PeleError> {
    if model.plots.is_empty() {
        return Err(PeleError::typed("No Plot to render, please provide a plot with some data", PeleErrorCode::Semantic, "xychart"));
    }
    let empty = Map::default();
    let chart = record(config.get("xyChart"), &empty);
    let size = 16.0;
    let small = js::round(size * 0.875);
    let tiny = js::round(size * 0.75);
    let pad = 8.0;
    let natural = dimension(chart.get("width"), 700.0);
    let width = cap_width(options, natural, 240.0, 0.0);
    // A narrower chart is not as much shorter, so that the plot keeps room for its marks.
    let height = js::round(dimension(chart.get("height"), 500.0) * 0.6f64.max(width / natural));
    let horizontal = match model.orientation {
        Some(o) => o == "horizontal",
        None => matches!(chart.get("chartOrientation"), Some(Value::Str(s)) if s == "horizontal"),
    };
    let show_values = on(chart, "showDataLabel", false);
    let x = &model.x_axis;
    let plots = &model.plots;
    let line_height = text(Some("0"), small, 4000.0).height;
    let tiny_height = text(Some("0"), tiny, 4000.0).height;

    let title = layout_label(if on(chart, "showTitle", true) { model.title.as_deref() } else { None }, false, size, width - 2.0 * pad, style::BOLD);
    let title_space = if title.height > 0.0 { title.height + 12.0 } else { 0.0 };

    let mut legend: Vec<(usize, Label)> = Vec::new();
    let mut legend_width: f64 = 0.0;
    if on(chart, "showLegend", true) {
        for (index, plot) in plots.iter().enumerate() {
            if plot.title.is_empty() {
                continue;
            }
            let label = text(Some(&plot.title), small, width * 0.3);
            legend_width = legend_width.max(label.width + SWATCH + 8.0);
            legend.push((index, label));
        }
    }
    let legend_space = if !legend.is_empty() { legend_width + LEGEND_GAP } else { 0.0 };

    let bars = plots.iter().filter(|p| p.bar).count();
    let mut lo = finite(Some(model.y_axis.min)).unwrap_or(f64::INFINITY);
    let mut hi = finite(Some(model.y_axis.max)).unwrap_or(f64::NEG_INFINITY);
    let exact = model.has_set_y_axis && lo.is_finite() && hi.is_finite() && lo != hi;
    if !exact {
        lo = f64::INFINITY;
        hi = f64::NEG_INFINITY;
        for plot in plots {
            for (_, raw) in &plot.data {
                let Some(value) = finite(*raw) else { continue };
                if value < lo {
                    lo = value;
                }
                if value > hi {
                    hi = value;
                }
            }
        }
        if lo > hi {
            lo = 0.0;
            hi = 0.0;
        }
        if bars > 0 {
            lo = lo.min(0.0);
            hi = hi.max(0.0);
        }
        if lo == hi {
            if lo == 0.0 {
                hi = 1.0;
            } else {
                lo -= lo.abs() / 2.0;
                hi += hi.abs() / 2.0;
            }
        }
    }

    let value_marks = |length: f64, spacing: f64, lo: &mut f64, hi: &mut f64| -> Vec<Mark> {
        let step = tick_step((*hi - *lo).abs(), 2f64.max(10f64.min((length / spacing).floor())));
        if !exact {
            *lo = tick_value(step, (tick_index(step, *lo) + 1e-9).floor());
            *hi = tick_value(step, (tick_index(step, *hi) - 1e-9).ceil());
        }
        let (l, h) = (*lo, *hi);
        ticks_between(l.min(h), l.max(h), step).into_iter().map(|value| Mark { at: (value - l) / (h - l), label: text(Some(&num_str(value)), small, 4000.0) }).collect()
    };

    let mut count = 1usize;
    match x {
        XAxis::Band { categories, .. } => count = categories.len().max(1),
        XAxis::Linear { .. } => {
            for plot in plots {
                count = count.max(plot.data.len());
            }
        }
    }
    let band = matches!(x, XAxis::Band { .. });
    let slot = 1.0 / count as f64;
    let inset = if band || bars > 0 { slot / 2.0 } else { 0.0 };
    let (x0, x1) = match x {
        XAxis::Linear { min, max, .. } => (finite(Some(*min)), finite(Some(*max))),
        _ => (None, None),
    };
    let ranged = x0.is_some() && x1.is_some() && x0 != x1;
    let place = |i: usize, n: usize| -> f64 {
        if band {
            (i as f64 + 0.5) * slot
        } else {
            inset
                + (if n > 1 {
                    i as f64 / (n as f64 - 1.0)
                } else if ranged {
                    0.0
                } else {
                    0.5
                }) * (1.0 - 2.0 * inset)
        }
    };

    let category_conf = record(chart.get("xAxis"), &empty);
    let value_conf = record(chart.get("yAxis"), &empty);
    let left_conf = if horizontal { category_conf } else { value_conf };
    let bottom_conf = if horizontal { value_conf } else { category_conf };
    let left_title = text(if on(left_conf, "showTitle", true) { Some(if horizontal { x.title() } else { &model.y_axis.title }) } else { None }, small, height * 0.8);
    let bottom_title = text(if on(bottom_conf, "showTitle", true) { Some(if horizontal { &model.y_axis.title } else { x.title() }) } else { None }, small, width * 0.8);
    let left_tick = if on(left_conf, "showTick", true) { TICK } else { 0.0 };
    let bottom_tick = if on(bottom_conf, "showTick", true) { TICK } else { 0.0 };
    let left_labels = on(left_conf, "showLabel", true);
    let bottom_labels = on(bottom_conf, "showLabel", true);

    let wrap_at = if horizontal { width * 0.3 } else { 48f64.max((width - 2.0 * pad - 64.0 - legend_space) * slot - 8.0) };
    let categories: Vec<Label> = match x {
        XAxis::Band { categories, .. } if on(category_conf, "showLabel", true) => categories.iter().map(|c| text(Some(c), small, wrap_at)).collect(),
        _ => Vec::new(),
    };
    let mut category_width: f64 = 0.0;
    let mut category_height: f64 = 0.0;
    for label in &categories {
        category_width = category_width.max(label.width);
        category_height = category_height.max(label.height);
    }

    let category_marks = |length: f64| -> Vec<Mark> {
        let mut marks = Vec::new();
        if let XAxis::Band { categories: cats, .. } = x {
            let need = if categories.is_empty() {
                8.0
            } else if horizontal {
                category_height
            } else {
                category_width + 12.0
            };
            let every = 1f64.max((need / (length * slot)).ceil()) as usize;
            let mut i = 0;
            while i < cats.len() {
                marks.push(Mark { at: (i as f64 + 0.5) * slot, label: categories.get(i).cloned().unwrap_or_else(|| text(None, small, 0.0)) });
                i += every;
            }
        } else if ranged {
            let (x0, x1) = (x0.unwrap(), x1.unwrap());
            let step = tick_step((x1 - x0).abs(), 2f64.max(10f64.min((length / if horizontal { 32.0 } else { 80.0 }).floor())));
            for value in ticks_between(x0.min(x1), x0.max(x1), step) {
                marks.push(Mark { at: inset + ((value - x0) / (x1 - x0)) * (1.0 - 2.0 * inset), label: text(Some(&num_str(value)), small, 4000.0) });
            }
        } else if let Some(x0) = x0 {
            marks.push(Mark { at: 0.5, label: text(Some(&num_str(x0)), small, 4000.0) });
        }
        marks
    };

    let mut point_label_width: f64 = 0.0;
    let mut value_width: f64 = 0.0;
    for plot in plots {
        for label in plot.point_labels.iter().flatten() {
            point_label_width = point_label_width.max(measure(label, tiny, 0));
        }
        if show_values && horizontal && plot.bar {
            for (_, value) in &plot.data {
                value_width = value_width.max(measure(&finite(*value).map(num_str).unwrap_or_default(), tiny, 0));
            }
        }
    }

    let bottom_label_height = if !bottom_labels {
        0.0
    } else if horizontal || !band {
        line_height
    } else {
        category_height
    };
    let bottom_space = bottom_tick + if bottom_label_height > 0.0 { bottom_label_height + 4.0 } else { 0.0 } + if bottom_title.height > 0.0 { bottom_title.height + 4.0 } else { 0.0 };
    let headroom = if horizontal {
        0.0
    } else if point_label_width > 0.0 || show_values {
        tiny_height + 4.0
    } else if left_labels {
        line_height / 2.0
    } else {
        0.0
    };
    let top = pad + title_space + headroom;
    let plot_height = 1f64.max(height - pad - bottom_space - top);
    let bottom = top + plot_height;

    let left_marks = if horizontal { category_marks(plot_height) } else { value_marks(plot_height, 56.0, &mut lo, &mut hi) };
    let left_label_width = if left_labels { widest(&left_marks) } else { 0.0 };
    let left = pad + if left_title.height > 0.0 { left_title.height + 6.0 } else { 0.0 } + if left_label_width > 0.0 { left_label_width + 6.0 } else { 0.0 } + left_tick;
    let beside = (if point_label_width > 0.0 { point_label_width + 12.0 } else { 0.0 }).max(if value_width > 0.0 { value_width + 8.0 } else { 0.0 });
    let reach = if horizontal { (width * 0.25).min(beside) } else { 0.0 };
    let mut plot_width = 1f64.max(width - pad - left - legend_space - reach);
    let bottom_marks = if horizontal { value_marks(plot_width, 80.0, &mut lo, &mut hi) } else { category_marks(plot_width) };
    if bottom_labels && legend_space + reach == 0.0 {
        let mut over: f64 = 0.0;
        for mark in &bottom_marks {
            over = over.max(mark.at * plot_width + mark.label.width / 2.0 - plot_width);
        }
        plot_width = 1f64.max(plot_width - over);
    }
    let right = left + plot_width;

    let px = |c: f64, v: f64| if horizontal { left + v * plot_width } else { left + c * plot_width };
    let py = |c: f64, v: f64| if horizontal { top + c * plot_height } else { bottom - v * plot_height };
    let scale = |value: f64| (value - lo) / (hi - lo);
    let clamp = |v: f64| v.max(0.0).min(1.0);
    let clamp = |v: f64| if v.is_nan() { v } else { clamp(v) };

    let rule = |v: f64| {
        if horizontal {
            format!("M{},{}V{}", num(px(0.0, v)), num(top), num(bottom))
        } else {
            format!("M{},{}H{}", num(left), num(py(0.0, v)), num(right))
        }
    };

    let mut grid = String::new();
    for mark in if horizontal { &bottom_marks } else { &left_marks } {
        grid.push_str(&rule(mark.at));
    }

    let mut axis = String::new();
    let mut labels = String::new();
    if on(left_conf, "showAxisLine", true) {
        axis.push_str(&format!("M{},{}V{}", num(left), num(top), num(bottom)));
    }
    if on(bottom_conf, "showAxisLine", true) {
        axis.push_str(&format!("M{},{}H{}", num(left), num(bottom), num(right)));
    }
    for mark in &left_marks {
        let y = if horizontal { top + mark.at * plot_height } else { bottom - mark.at * plot_height };
        if left_tick != 0.0 {
            axis.push_str(&format!("M{},{}h{}", num(left), num(y), num_str(-left_tick)));
        }
        if left_labels {
            labels.push_str(&aligned(&mark.label, left - left_tick - 6.0, y, "end"));
        }
    }
    for mark in &bottom_marks {
        let at = left + mark.at * plot_width;
        if bottom_tick != 0.0 {
            axis.push_str(&format!("M{},{}v{}", num(at), num(bottom), num_str(bottom_tick)));
        }
        if bottom_labels {
            labels.push_str(&aligned(&mark.label, at, bottom + bottom_tick + 4.0 + mark.label.height / 2.0, "middle"));
        }
    }

    let span = if horizontal { plot_height } else { plot_width } * slot;
    let group = span * 0.7;
    let thickness = group / (bars.max(1) as f64);
    let gap = if bars > 1 { 2f64.min(thickness * 0.2) } else { 0.0 };
    let base = clamp(scale(0.0));
    if bars > 0 && base > 0.0 && base < 1.0 {
        axis.push_str(&rule(base));
    }
    let mut series = String::new();
    let mut lines = String::new();
    let mut outside = String::new();
    let mut point_labels = String::new();
    let mut bar = 0usize;
    for (index, plot) in plots.iter().enumerate() {
        let color = series_color(index as i64);
        let id = if !plot.title.is_empty() { format!(" data-id=\"{}\"", esc_text(&plot.title)) } else { String::new() };
        let n = if band { plot.data.len().min(count) } else { plot.data.len() };
        if plot.bar {
            let offset = (bar as f64 - (bars as f64 - 1.0) / 2.0) * thickness - (thickness - gap) / 2.0;
            bar += 1;
            let mut rects = String::new();
            for i in 0..n {
                let Some(value) = finite(plot.data[i].1) else { continue };
                let c = place(i, n);
                let v = clamp(scale(value));
                let from = js_min(base, v);
                let to = js_max(base, v);
                let bx = if horizontal { px(c, from) } else { px(c, 0.0) + offset };
                let by = if horizontal { py(c, 0.0) + offset } else { py(c, to) };
                let bw = if horizontal { (to - from) * plot_width } else { thickness - gap };
                let bh = if horizontal { thickness - gap } else { (to - from) * plot_height };
                rects.push_str(&format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>", num(bx), num(by), num(bw), num(bh)));
                if !show_values {
                    continue;
                }
                let label = num_str(value);
                let w = measure(&label, tiny, 0);
                let out = if v >= base { 1.0 } else { -1.0 };
                let mut tx = bx + bw / 2.0;
                let mut ty = by + bh / 2.0;
                let mut anchor = "middle";
                if horizontal {
                    if bh < tiny {
                        continue;
                    }
                    tx = (if out > 0.0 { bx + bw } else { bx }) + out * 6.0;
                    anchor = if out > 0.0 { "start" } else { "end" };
                } else {
                    if w + 2.0 > span / bars as f64 {
                        continue;
                    }
                    ty = (if out > 0.0 { by } else { by + bh }) - out * (tiny_height / 2.0 + 2.0);
                }
                outside.push_str(&format!("<text x=\"{}\" y=\"{}\" text-anchor=\"{}\">{}</text>", num(tx), num(ty + tiny * 0.35), anchor, label));
            }
            series.push_str(&format!("<g class=\"pele-series pele-bars\"{} fill=\"{}\">{}</g>", id, color, rects));
            continue;
        }

        // A line is cut where it leaves the value range, so it stays inside the plot.
        let mut d = String::new();
        let mut dots = String::new();
        let mut pen = false;
        let mut pc = 0.0;
        let mut pv = f64::NAN;
        for i in 0..n {
            let Some(value) = finite(plot.data[i].1) else {
                pv = f64::NAN;
                continue;
            };
            let c = place(i, n);
            let v = scale(value);
            let visible = v >= 0.0 && v <= 1.0;
            if pv.is_nan() {
                if visible {
                    d.push_str(&format!("M{},{}", num(px(c, v)), num(py(c, v))));
                }
                pen = visible;
            } else {
                let dv = v - pv;
                let mut t0: f64 = 0.0;
                let mut t1: f64 = 1.0;
                if dv != 0.0 {
                    let a = -pv / dv;
                    let b = (1.0 - pv) / dv;
                    t0 = js_max(t0, js_min(a, b));
                    t1 = js_min(t1, js_max(a, b));
                } else if !visible {
                    t0 = 2.0;
                }
                if t0 <= t1 {
                    let c0 = pc + (c - pc) * t0;
                    let v0 = pv + dv * t0;
                    let c1 = pc + (c - pc) * t1;
                    let v1 = pv + dv * t1;
                    if !pen || t0 > 0.0 {
                        d.push_str(&format!("M{},{}", num(px(c0, v0)), num(py(c0, v0))));
                    }
                    d.push_str(&format!("L{},{}", num(px(c1, v1)), num(py(c1, v1))));
                }
                pen = visible;
            }
            pc = c;
            pv = v;
            if !visible {
                continue;
            }
            let cx = px(c, v);
            let cy = py(c, v);
            if span >= 12.0 {
                dots.push_str(&format!("<circle cx=\"{}\" cy=\"{}\" r=\"3\"/>", num(cx), num(cy)));
            }
            let label = text(plot.point_labels.as_ref().and_then(|l| l.get(i)).map(String::as_str), tiny, 4000.0);
            if label.height > 0.0 {
                let half = label.width / 2.0;
                point_labels.push_str(&if horizontal {
                    aligned(&label, cx + 10.0, cy, "start")
                } else {
                    aligned(&label, cx.min(right - half).max(left + half), cy - 6.0 - label.height / 2.0, "middle")
                });
            }
        }
        lines.push_str(&format!("<g class=\"pele-series pele-line\"{} fill=\"{}\">", id, color));
        if !d.is_empty() {
            lines.push_str(&format!("<path d=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>", d, color));
        }
        lines.push_str(&dots);
        lines.push_str("</g>");
    }

    let mut titles = String::new();
    if left_title.height > 0.0 {
        titles.push_str(&format!("<g transform=\"translate({},{}) rotate(-90)\">{}</g>", num(pad + left_title.height / 2.0), num(top + plot_height / 2.0), label_svg(&left_title, 0.0, 0.0, "")));
    }
    titles.push_str(&label_svg(&bottom_title, left + plot_width / 2.0, height - pad - bottom_title.height / 2.0, ""));

    let mut legend_out = String::new();
    let mut legend_height = 0.0;
    for (_, label) in &legend {
        legend_height += ROW.max(label.height + 6.0);
    }
    let mut ly = top + 0f64.max((plot_height - legend_height) / 2.0);
    let lx = right + reach + LEGEND_GAP;
    for (index, label) in &legend {
        let row = ROW.max(label.height + 6.0);
        if ly + row > height {
            break;
        }
        let y = ly + row / 2.0;
        let color = series_color(*index as i64);
        legend_out.push_str(&format!("<g class=\"pele-legend-item\" data-id=\"{}\">", esc_text(&plots[*index].title)));
        if plots[*index].bar {
            legend_out.push_str(&format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"2\" fill=\"{}\"/>", num(lx), num(y - SWATCH / 2.0), SWATCH, SWATCH, color));
        } else {
            legend_out.push_str(&format!("<path d=\"M{},{}h{}\" stroke=\"{}\" stroke-width=\"2\" stroke-linecap=\"round\"/>", num(lx), num(y), SWATCH, color));
        }
        legend_out.push_str(&label_svg(label, lx + SWATCH + 8.0 + label.width / 2.0, y, ""));
        legend_out.push_str("</g>");
        ly += row;
    }

    let mut content = label_svg(&title, width / 2.0, pad + title.height / 2.0, " class=\"pele-title\" font-weight=\"var(--_tw)\"");
    if !grid.is_empty() {
        content.push_str(&format!("<path class=\"pele-grid\" d=\"{}\" stroke=\"var(--_a)\"/>", grid));
    }
    content.push_str(&format!("<g class=\"pele-plot\">{}{}</g>", series, lines));
    if !axis.is_empty() {
        content.push_str(&format!("<path class=\"pele-axis\" d=\"{}\" fill=\"none\" stroke=\"var(--_b)\"/>", axis));
    }
    if !labels.is_empty() {
        content.push_str(&format!("<g class=\"pele-axis-labels\" font-size=\"{}\" fill=\"var(--_m)\">{}</g>", small, labels));
    }
    if !titles.is_empty() {
        content.push_str(&format!("<g class=\"pele-axis-titles\" font-size=\"{}\">{}</g>", small, titles));
    }
    if !outside.is_empty() {
        content.push_str(&format!("<g class=\"pele-data-labels\" font-size=\"{}\">{}</g>", tiny, outside));
    }
    if !point_labels.is_empty() {
        content.push_str(&format!("<g class=\"pele-point-labels\" font-size=\"{}\">{}</g>", tiny, point_labels));
    }
    if !legend_out.is_empty() {
        content.push_str(&format!("<g class=\"pele-legend\" font-size=\"{}\">{}</g>", small, legend_out));
    }
    let acc = Accessible { acc_title: model.acc_title.as_deref(), acc_descr: model.acc_descr.as_deref() };
    Ok(Rendered { svg: svg_document("xychart", width, height, size, options.responsive, &acc, &content), width, height, links: Vec::new() })
}

// Math.min / Math.max: NaN wins.
fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}

fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}
