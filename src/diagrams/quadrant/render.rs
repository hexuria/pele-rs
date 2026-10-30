// Port of src/diagrams/quadrant/render.ts.

use super::db::{QuadrantDb, QuadrantText};
use crate::diagrams::common::fit_width::cap_width;
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc_text, label_svg, num};
use crate::svg::root::{svg_document, Accessible};
use crate::svg::theme::{class_names, resolve_style, series_color, ROUNDED};
use crate::text::label::{layout_label, Label};
use crate::text::measurer::style;
use crate::types::{RenderOptions, Rendered};
use crate::util::yaml::Value;

const GAP: f64 = 4.0;
const CAREFUL: usize = 300;

struct Dot {
    cx: f64,
    cy: f64,
    reach: f64,
    text: Label,
}

fn setting<'a>(config: &'a Config, key: &str) -> Option<&'a Value> {
    config.get("quadrantChart").and_then(|v| v.as_map()).and_then(|m| m.get(key))
}

fn length(config: &Config, key: &str, fallback: f64, max: f64) -> f64 {
    match setting(config, key) {
        Some(Value::Num(v)) if *v >= 0.0 => v.min(max),
        _ => fallback,
    }
}

fn hex(color: &str) -> String {
    if color.starts_with('#') {
        color.to_string()
    } else {
        format!("#{}", color)
    }
}

fn is_str(v: Option<&Value>, s: &str) -> bool {
    matches!(v, Some(Value::Str(x)) if x == s)
}

pub fn render_quadrant(model: &QuadrantDb, config: &Config, options: &RenderOptions) -> Rendered {
    let size = 16.0;
    let axis_size = js::round(size * 0.875);
    let point_size = js::round(size * 0.75);
    let natural = js::round(100f64.max(length(config, "chartWidth", 500.0, 4000.0)));
    let width = js::round(cap_width(options, natural, 280.0, 0.0));
    let height = js::round((100f64.max(length(config, "chartHeight", 500.0, 4000.0)) * width) / natural);
    let outer = length(config, "quadrantPadding", 8.0, 200.0);
    let x_pad = length(config, "xAxisLabelPadding", 5.0, 200.0);
    let y_pad = length(config, "yAxisLabelPadding", 5.0, 200.0);
    let has_points = !model.points.is_empty();

    let label = |text: Option<&QuadrantText>, font_size: f64, max: f64| -> Label { layout_label(text.map(|t| t.text.as_str()), text.is_some_and(|t| t.markdown), font_size, max.max(40.0), 0) };

    let title = layout_label(model.title.as_deref(), false, size, (width - 2.0 * outer).max(40.0), style::BOLD);
    let title_pad = length(config, "titlePadding", 10.0, 200.0);
    let title_space = if title.height > 0.0 { title.height + title_pad } else { 0.0 };

    let nonempty = |t: &Option<QuadrantText>| t.as_ref().is_some_and(|t| !t.text.is_empty());
    let has_x = nonempty(&model.x_axis_left) || nonempty(&model.x_axis_right);
    let has_y = nonempty(&model.y_axis_bottom) || nonempty(&model.y_axis_top);
    // Mermaid moves the x axis labels below the chart as soon as there are points.
    let x_below = has_points || is_str(setting(config, "xAxisPosition"), "bottom");
    let y_right = is_str(setting(config, "yAxisPosition"), "right");

    let side = width - 2.0 * outer;
    let x_left = label(model.x_axis_left.as_ref(), axis_size, if model.x_axis_right.is_some() { side / 2.0 - 16.0 } else { side });
    let x_right = label(model.x_axis_right.as_ref(), axis_size, side / 2.0 - 16.0);
    let x_height = x_left.height.max(x_right.height);
    let x_space = if has_x { x_height + x_pad } else { 0.0 };
    let tall = height - 2.0 * outer - title_space - x_space;
    let y_bottom = label(model.y_axis_bottom.as_ref(), axis_size, if model.y_axis_top.is_some() { tall / 2.0 - 16.0 } else { tall });
    let y_top = label(model.y_axis_top.as_ref(), axis_size, tall / 2.0 - 16.0);
    let y_height = y_bottom.height.max(y_top.height);
    let y_space = if has_y { y_height + y_pad } else { 0.0 };

    let left = outer + if y_right { 0.0 } else { y_space };
    let top = outer + title_space + if x_below { 0.0 } else { x_space };
    let w = 0f64.max(side - y_space);
    let h = 0f64.max(tall);
    let tile_w = 0f64.max((w - GAP) / 2.0);
    let tile_h = 0f64.max((h - GAP) / 2.0);

    // Boxes a point label keeps clear of: the gaps between quadrants, quadrant labels, points,
    // and the labels placed before it.
    let mut taken: Vec<f64> = vec![left + tile_w, top, left + w - tile_w, top + h, left, top + tile_h, left + w, top + h - tile_h];
    let is_clear = |taken: &[f64], from: usize, x0: f64, y0: f64, x1: f64, y1: f64| -> bool {
        let mut k = from;
        while k < taken.len() {
            // Empty quadrant labels must not block point labels.
            if !(taken[k] >= taken[k + 2] || taken[k + 1] >= taken[k + 3]) && x0 < taken[k + 2] && x1 > taken[k] && y0 < taken[k + 3] && y1 > taken[k + 1] {
                return false;
            }
            k += 4;
        }
        true
    };

    let text_top = length(config, "quadrantTextTopPadding", 5.0, 200.0);
    let mut quadrants = String::new();
    for k in 0..4 {
        let x = if k == 0 || k == 3 { left + w - tile_w } else { left };
        let y = if k < 2 { top } else { top + h - tile_h };
        let text = label(model.quadrants[k].as_ref(), size, tile_w - 16.0);
        let cx = x + tile_w / 2.0;
        let cy = if has_points { y + text_top + text.height / 2.0 } else { y + tile_h / 2.0 };
        taken.extend_from_slice(&[cx - text.width / 2.0, cy - text.height / 2.0, cx + text.width / 2.0, cy + text.height / 2.0]);
        quadrants.push_str(&format!(
            "<g class=\"pele-quadrant\" data-id=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}/>{}</g>",
            k + 1,
            num(x),
            num(y),
            num(tile_w),
            num(tile_h),
            ROUNDED,
            label_svg(&text, cx, cy, " class=\"pele-label\" fill=\"var(--_fg)\" stroke=\"none\"")
        ));
    }

    let mut axes = String::new();
    if has_x {
        let y = if x_below { top + h + x_pad + x_height / 2.0 } else { top - x_pad - x_height / 2.0 };
        axes.push_str(&label_svg(&x_left, if x_right.height > 0.0 { left + w / 4.0 } else { left + x_left.width / 2.0 }, y, ""));
        axes.push_str(&label_svg(&x_right, left + (w * 3.0) / 4.0, y, ""));
    }
    if has_y {
        let x = if y_right { left + w + y_pad + y_height / 2.0 } else { left - y_pad - y_height / 2.0 };
        let turned = |text: &Label, y: f64| {
            if text.height > 0.0 {
                format!("<g transform=\"translate({},{}) rotate(-90)\">{}</g>", num(x), num(y), label_svg(text, 0.0, 0.0, ""))
            } else {
                String::new()
            }
        };
        axes.push_str(&turned(&y_bottom, if y_top.height > 0.0 { top + (h * 3.0) / 4.0 } else { top + h - y_bottom.width / 2.0 }));
        axes.push_str(&turned(&y_top, top + h / 4.0));
    }

    let radius = length(config, "pointRadius", 5.0, 200.0);
    let text_pad = length(config, "pointTextPadding", 5.0, 200.0);
    let fill = series_color(0);
    let mut dots: Vec<Dot> = Vec::new();
    let mut circles: Vec<String> = Vec::new();
    for point in &model.points {
        if !(point.x >= 0.0 && point.x <= 1.0 && point.y >= 0.0 && point.y <= 1.0) {
            continue;
        }
        let shared = model.class(&point.class_name);
        let r = point.style.radius.or(shared.and_then(|s| s.radius)).unwrap_or(radius).min(2000.0);
        let color = point.style.color.clone().or(shared.and_then(|s| s.color.clone()));
        let stroke_color = point.style.stroke_color.clone().or(shared.and_then(|s| s.stroke_color.clone()));
        let stroke_width = point.style.stroke_width.clone().or(shared.and_then(|s| s.stroke_width.clone()));
        let mut styles: Vec<String> = Vec::new();
        if let Some(c) = color.as_deref().filter(|c| !c.is_empty()) {
            styles.push(format!("fill:{}", hex(c)));
        }
        let sw = stroke_width.as_deref().filter(|s| !s.is_empty());
        let sc = stroke_color.as_deref().filter(|s| !s.is_empty());
        if let Some(sw) = sw {
            if let Some(sc) = sc {
                styles.push(format!("stroke:{}", hex(sc)));
            }
            styles.push(format!("stroke-width:{}", sw));
        }
        let cx = left + point.x * w;
        let cy = top + (1.0 - point.y) * h;
        dots.push(Dot { cx, cy, reach: r + sw.map_or(0.0, |s| js::parse_int(s, 10).min(400.0) / 2.0), text: label(Some(&point.text), point_size, 200.0) });
        circles.push(format!(
            "<circle class=\"pele-point{}\" data-id=\"{}\" cx=\"{}\" cy=\"{}\" r=\"{}\"{}{}/>",
            class_names(&point.class_name),
            esc_text(&point.text.text),
            num(cx),
            num(cy),
            num(r),
            if sw.is_some() && sc.is_none() { format!(" stroke=\"{}\"", fill) } else { String::new() },
            resolve_style(&styles).shape
        ));
    }

    // A label goes below its point, or above, right or left of it, or below its quadrant's
    // title, when that is the first place that is inside the chart and free.
    let careful = dots.len() <= CAREFUL;
    if careful {
        for d in &dots {
            taken.extend_from_slice(&[d.cx - d.reach, d.cy - d.reach, d.cx + d.reach, d.cy + d.reach]);
        }
    }
    let mut labels = String::new();
    for d in &dots {
        let (cx, cy, reach, text) = (d.cx, d.cy, d.reach, &d.text);
        let half_w = text.width / 2.0;
        let half_h = text.size * 0.6 + (text.height - text.line_height) / 2.0;
        let away = reach + text_pad;
        let inset = away.min(0f64.max((tile_w - text.width) / 2.0));
        let heading = 8 + 4 * if cx < left + w / 2.0 {
            if cy < top + h / 2.0 {
                1
            } else {
                2
            }
        } else if cy < top + h / 2.0 {
            0
        } else {
            3
        };
        let below = (cy + away).max(taken[heading + 3] + text_pad) + half_h;
        let spots = [cx, cy + away + half_h, cx, cy - away - half_h, cx + away + half_w, cy, cx - away - half_w, cy, cx, below];
        let mut tx = cx;
        let mut ty = spots[1];
        let mut found = false;
        let mut k = 0;
        while k < 20 {
            let spot = k % 10;
            let margin = if spot == 8 { inset } else { 0.0 };
            let x = if (spot < 4 || spot == 8) && text.width < w { spots[spot].max(left + half_w + margin).min(left + w - half_w - margin) } else { spots[spot] };
            let y = spots[spot + 1];
            if x - half_w < left || x + half_w > left + w || y - half_h < top || y + half_h > top + h {
                k += 2;
                continue;
            }
            let free = !careful || is_clear(&taken, if k < 10 { 0 } else { 8 }, x - half_w, y - half_h, x + half_w, y + half_h);
            if !found || free {
                tx = x;
                ty = y;
                found = true;
            }
            if free {
                break;
            }
            k += 2;
        }
        if careful {
            taken.extend_from_slice(&[tx - half_w, ty - half_h, tx + half_w, ty + half_h]);
        }
        labels.push_str(&label_svg(text, tx, ty, " class=\"pele-label\""));
    }

    // Mermaid puts each new point under the ones before it.
    let mut content = label_svg(&title, width / 2.0, title_pad / 2.0 + title.height / 2.0, " class=\"pele-title\" font-weight=\"var(--_tw)\"");
    content.push_str(&format!("<g class=\"pele-quadrants\" fill=\"var(--_s)\" stroke=\"var(--_b)\">{}</g>", quadrants));
    if !axes.is_empty() {
        content.push_str(&format!("<g class=\"pele-axis-labels\" font-size=\"{}\" fill=\"var(--_m)\">{}</g>", axis_size, axes));
    }
    if !dots.is_empty() {
        circles.reverse();
        content.push_str(&format!("<g class=\"pele-points\" fill=\"{}\">{}</g><g class=\"pele-point-labels\" font-size=\"{}\">{}</g>", fill, circles.concat(), point_size, labels));
    }
    let acc = Accessible { acc_title: model.acc_title.as_deref(), acc_descr: model.acc_descr.as_deref() };
    Rendered { svg: svg_document("quadrantChart", width, height, size, options.responsive, &acc, &content), width, height, links: Vec::new() }
}
