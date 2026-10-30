// Port of src/diagrams/wardley/render.ts.

use super::{Flow, WardleyModel, WardleyNode};
use crate::diagrams::common::fit_width::fit_width;
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num, shared_style, span_style, text_style};
use crate::svg::edges::{marker, marker_trim};
use crate::svg::root::{svg_document, Accessible};
use crate::svg::theme::ROUNDED;
use crate::text::label::{layout_label, Label};
use crate::text::measurer::style;
use crate::types::{RenderOptions, Rendered};
use crate::util::yaml::{Map, Value};
use std::f64::consts::PI;

const DEFAULT_STAGES: [&str; 4] = ["Genesis", "Custom Built", "Product", "Commodity"];
// Mermaid's canvas has this much room around the plot on every side.
const CANVAS_MARGIN: f64 = 48.0;
const R: f64 = 6.0;
const SQUARE: f64 = 10.0;
const LABEL_OFFSET: f64 = 8.0;
const PIPELINE_PAD: f64 = 15.0;
const FORCE_WIDTH: f64 = 60.0;
const FORCE_HEIGHT: f64 = 30.0;
const FORCE_HEAD: f64 = 20.0;
const ACCENT: &str = "var(--_c)";
const LINE: &str = "var(--_l)";

struct Placed<'a> {
    x: f64,
    y: f64,
    node: &'a WardleyNode,
}

fn section(config: &Config) -> Option<&Map> {
    config.get("wardley-beta").and_then(|v| v.as_map())
}

fn setting(config: &Config, key: &str, fallback: f64, min: f64, max: f64) -> f64 {
    match section(config).and_then(|s| s.get(key)) {
        Some(Value::Num(v)) if *v >= min && *v <= max => *v,
        _ => fallback,
    }
}

fn clamp(value: f64, min: f64, max: f64) -> f64 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

// Writes a label with its lines starting at x and its first baseline at y.
fn start_label(label: &Label, x: f64, y: f64, attrs: &str) -> String {
    let shared = shared_style(label);
    let mut body = String::new();
    let mut baseline = y;
    for line in &label.lines {
        let mut first = true;
        for span in line {
            let st = span_style(span.style & !shared);
            let pos = if first { format!(" x=\"{}\" y=\"{}\"", num(x), num(baseline)) } else { String::new() };
            body.push_str(&format!("<tspan{}{}>{}</tspan>", pos, st, esc(&span.text)));
            first = false;
        }
        baseline += label.line_height;
    }
    if body.is_empty() {
        String::new()
    } else {
        format!("<text{} xml:space=\"preserve\">{}</text>", text_style(attrs, shared), body)
    }
}

fn dimension(config: &Config, given: Option<f64>, key: &str, fallback: f64) -> f64 {
    let value = match given {
        Some(g) if g.is_finite() => g,
        _ => setting(config, key, fallback, 1.0, 1e6),
    };
    clamp(value, 200.0, 10000.0) - 2.0 * CANVAS_MARGIN
}

pub fn render_wardley(model: &WardleyModel, config: &Config, options: &RenderOptions) -> Rendered {
    let natural = dimension(config, model.size.map(|s| s.0), "width", 900.0);
    fit_width(options, natural, 160.0, |plot_w, _tight| draw(model, config, options, plot_w, natural))
}

struct Bounds {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl Bounds {
    fn grow(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) {
        if x0 < self.min_x {
            self.min_x = x0;
        }
        if y0 < self.min_y {
            self.min_y = y0;
        }
        if x1 > self.max_x {
            self.max_x = x1;
        }
        if y1 > self.max_y {
            self.max_y = y1;
        }
    }
    fn centered(&mut self, label: &Label, cx: f64, cy: f64, attrs: &str) -> String {
        self.grow(cx - label.width / 2.0, cy - label.height / 2.0, cx + label.width / 2.0, cy + label.height / 2.0);
        label_svg(label, cx, cy, attrs)
    }
    fn starting(&mut self, label: &Label, x: f64, y: f64, attrs: &str) -> String {
        self.grow(x, y - label.size, x + label.width, y - label.size + label.height + 4.0);
        start_label(label, x, y, attrs)
    }
}

fn text(raw: &str, font_size: f64, max_width: f64, st: u32) -> Label {
    layout_label(Some(raw), false, font_size, max_width, st)
}

fn draw(model: &WardleyModel, config: &Config, options: &RenderOptions, plot_w: f64, natural_w: f64) -> Rendered {
    let size = 16.0;
    let small = js::round(size * 0.8125);
    let tiny = js::round(size * 0.6875);
    let pad = 8.0;

    // A narrower map is not as much shorter, so that components keep some room between them.
    let plot_h = dimension(config, model.size.map(|s| s.1), "height", 600.0) * 0.75f64.max(plot_w / natural_w);
    let show_grid = matches!(section(config).and_then(|s| s.get("showGrid")), Some(Value::Bool(true)));

    let mut b = Bounds { min_x: 0.0, min_y: 0.0, max_x: plot_w, max_y: plot_h };
    let project_x = |value: f64| (value / 100.0) * plot_w;
    let project_y = |value: f64| plot_h - (value / 100.0) * plot_h;

    let stages: Vec<&str> = if !model.stages.is_empty() { model.stages.iter().map(|s| s.as_str()).collect() } else { DEFAULT_STAGES.to_vec() };
    let boundaries = model.stage_boundaries.as_ref();
    let custom = boundaries.is_some_and(|b| b.len() == stages.len());
    let mut frame = String::new();
    let mut stage_labels = String::new();
    let mut stage_bottom = plot_h;
    let mut previous = 0.0;
    for (index, stage) in stages.iter().enumerate() {
        let n = stages.len() as f64;
        let start = if custom { previous } else { index as f64 / n };
        let end = if custom { clamp(boundaries.unwrap()[index], 0.0, 1.0) } else { (index as f64 + 1.0) / n };
        previous = end;
        if index > 0 {
            frame.push_str(&format!("<path class=\"pele-wardley-stage\" d=\"M{},0V{}\" stroke=\"var(--_b)\" stroke-dasharray=\"4 4\"/>", num(start * plot_w), num(plot_h)));
        }
        let label = text(stage, small, 48f64.max((end - start).abs() * plot_w - 12.0), 0);
        stage_labels.push_str(&b.centered(
            &label,
            ((start + end) / 2.0) * plot_w,
            plot_h + 8.0 + label.height / 2.0,
            &format!(" class=\"pele-wardley-stage-label\" font-size=\"{}\" fill=\"var(--_m)\"", js::num_str(small)),
        ));
        stage_bottom = stage_bottom.max(plot_h + 8.0 + label.height);
    }
    if show_grid {
        for i in 1..4 {
            let i = i as f64;
            frame.push_str(&format!(
                "<path class=\"pele-wardley-grid\" d=\"M{},0V{}M0,{}H{}\" stroke=\"var(--_b)\" stroke-dasharray=\"2 6\"/>",
                num((plot_w * i) / 4.0),
                num(plot_h),
                num((plot_h * i) / 4.0),
                num(plot_w)
            ));
        }
    }
    frame.push_str(&format!("<path class=\"pele-wardley-axis\" d=\"M0,0V{}H{}\" stroke=\"{}\"/>", num(plot_h), num(plot_w), LINE));
    let x_title = text("Evolution", small, 4000.0, 0);
    let y_title = text("Visibility", small, 4000.0, 0);
    let axis_attrs = format!(" class=\"pele-wardley-axis-label\" font-size=\"{}\" font-weight=\"var(--_hw)\"", js::num_str(small));
    let axis_labels = b.centered(&x_title, plot_w / 2.0, stage_bottom + 6.0 + x_title.height / 2.0, &axis_attrs)
        + &label_svg(&y_title, -16.0, plot_h / 2.0, &format!("{} transform=\"rotate(-90 -16 {})\"", axis_attrs, num(plot_h / 2.0)));
    b.grow(-16.0 - y_title.height / 2.0, 0.0, 0.0, 0.0);
    let legend_top = stage_bottom + 6.0 + x_title.height + 10.0;

    let mut positions: Vec<Placed> = Vec::new();
    for node in &model.nodes {
        positions.push(Placed { x: project_x(node.x), y: project_y(node.y), node });
    }
    let find = |positions: &[Placed], id: &str| positions.iter().position(|p| p.node.id == id);

    // Pipelines: a box around the components, with the parent as a square on its top edge.
    let mut pipelines = String::new();
    for pipeline in &model.pipelines {
        let mut parts: Vec<usize> = Vec::new();
        for id in &pipeline.component_ids {
            if let Some(k) = find(&positions, id) {
                parts.push(k);
            }
        }
        if parts.is_empty() {
            continue;
        }
        parts.sort_by(|&a, &c| js::ord(positions[a].x - positions[c].x));
        let left = positions[parts[0]].x;
        let right = positions[*parts.last().unwrap()].x;
        let y = positions[parts[0]].y;
        let top = y - 2.0 * R;
        if let Some(k) = find(&positions, &pipeline.node_id) {
            positions[k].x = (left + right) / 2.0;
            positions[k].y = top - SQUARE / 6.0;
        }
        b.grow(left - PIPELINE_PAD, top, right + PIPELINE_PAD, top + 4.0 * R);
        pipelines.push_str(&format!(
            "<g class=\"pele-wardley-pipeline\" data-id=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_s)\" stroke=\"var(--_b)\"{}/>{}</g>",
            esc_text(&pipeline.node_id),
            num(left - PIPELINE_PAD),
            num(top),
            num(right - left + 2.0 * PIPELINE_PAD),
            num(4.0 * R),
            ROUNDED,
            if parts.len() > 1 { format!("<path d=\"M{},{}H{}\" stroke=\"{}\" stroke-dasharray=\"4 4\"/>", num(left), num(y), num(right), LINE) } else { String::new() }
        ));
    }

    let anchor_label = |node: &WardleyNode| text(&node.label, small, 4000.0, style::BOLD);
    // How far from a node's center a line to it stops.
    let reach = |at: &Placed, ux: f64, uy: f64| -> f64 {
        let node = at.node;
        if node.class_name == Some("anchor") {
            let label = anchor_label(node);
            return 1.0 / js::hypot(ux / (label.width / 2.0 + 5.0), uy / (small * 0.8));
        }
        if node.is_pipeline_parent {
            return SQUARE / std::f64::consts::SQRT_2;
        }
        if node.source_strategy.as_deref().is_some_and(|s| !s.is_empty()) {
            2.0 * R
        } else {
            R
        }
    };

    // Quadrants around a node that a link runs through: 1 up and right, 2 down and right, 4 up and left, 8 down and left.
    let mut taken = vec![0u32; positions.len()];
    let occupy = |taken: &mut Vec<u32>, k: usize, ux: f64, uy: f64| {
        if ux.abs() < 0.25 || uy.abs() < 0.25 {
            return;
        }
        taken[k] |= if ux > 0.0 {
            if uy < 0.0 {
                1
            } else {
                2
            }
        } else if uy < 0.0 {
            4
        } else {
            8
        };
    };

    let mut links = String::new();
    for link in &model.links {
        let (Some(fk), Some(tk)) = (find(&positions, &link.source), find(&positions, &link.target)) else { continue };
        if fk == tk {
            continue;
        }
        // A pipeline component belongs to its parent already.
        if model.pipelines.iter().rev().find(|p| p.node_id == link.target).is_some_and(|p| p.component_ids.contains(&link.source)) {
            continue;
        }
        let (from, to) = (&positions[fk], &positions[tk]);
        let dx = to.x - from.x;
        let dy = to.y - from.y;
        let distance = js::hypot(dx, dy);
        if distance < 0.01 {
            continue;
        }
        let ux = dx / distance;
        let uy = dy / distance;
        let start = reach(from, ux, uy);
        let end = reach(to, ux, uy);
        if start + end >= distance {
            continue;
        }
        occupy(&mut taken, fk, ux, uy);
        occupy(&mut taken, tk, -ux, -uy);
        let mut x1 = from.x + ux * start;
        let mut y1 = from.y + uy * start;
        let mut x2 = to.x - ux * end;
        let mut y2 = to.y - uy * end;
        let mut heads = String::new();
        if matches!(link.flow, Some(Flow::Forward | Flow::Bidirectional)) {
            heads.push_str(&marker("arrow_point", x2, y2, ux, uy, LINE, "none"));
            x2 -= ux * marker_trim("arrow_point");
            y2 -= uy * marker_trim("arrow_point");
        }
        if matches!(link.flow, Some(Flow::Backward | Flow::Bidirectional)) {
            heads.push_str(&marker("arrow_point", x1, y1, -ux, -uy, LINE, "none"));
            x1 += ux * marker_trim("arrow_point");
            y1 += uy * marker_trim("arrow_point");
        }
        let mut caption = String::new();
        if let Some(l) = link.label.as_deref().filter(|l| !l.is_empty()) {
            let label = text(l, tiny + 1.0, 200.0, 0);
            // Beside the middle of the line, on its upper side.
            let side = if ux >= 0.0 { 1.0 } else { -1.0 };
            let px = uy * side;
            let py = -ux * side;
            let away = 5.0 + (px.abs() * label.width) / 2.0 + (py.abs() * label.height) / 2.0;
            caption = b.centered(
                &label,
                (from.x + to.x) / 2.0 + px * away,
                (from.y + to.y) / 2.0 + py * away,
                &format!(" class=\"pele-edge-label\" font-size=\"{}\" fill=\"var(--_m)\"", js::num_str(tiny + 1.0)),
            );
        }
        links.push_str(&format!(
            "<g class=\"pele-edge{}\" data-id=\"{}-{}\"><path d=\"M{},{}L{},{}\" stroke=\"{}\"{}/>{}{}</g>",
            if link.dashed { " pele-wardley-dashed" } else { "" },
            esc_text(&link.source),
            esc_text(&link.target),
            num(x1),
            num(y1),
            num(x2),
            num(y2),
            LINE,
            if link.dashed { " stroke-dasharray=\"6 6\"" } else { "" },
            heads,
            caption
        ));
    }

    // Evolution: a dashed arrow to where the component is heading, drawn as an open circle.
    let mut trends = String::new();
    for trend in &model.trends {
        let Some(k) = find(&positions, &trend.node_id) else { continue };
        let origin = &positions[k];
        let tx = project_x(trend.target_x);
        let ty = project_y(trend.target_y);
        let dx = tx - origin.x;
        let dy = ty - origin.y;
        let distance = js::hypot(dx, dy);
        let mut arrow = String::new();
        let start = if distance > 0.0 { reach(origin, dx / distance, dy / distance) } else { 0.0 };
        if distance > start + R + 4.0 {
            let ux = dx / distance;
            let uy = dy / distance;
            let ex = tx - ux * (R + 2.0);
            let ey = ty - uy * (R + 2.0);
            arrow = format!(
                "<path d=\"M{},{}L{},{}\" stroke=\"{}\" stroke-dasharray=\"4 4\"/>{}",
                num(origin.x + ux * start),
                num(origin.y + uy * start),
                num(ex - ux * marker_trim("arrow_point")),
                num(ey - uy * marker_trim("arrow_point")),
                ACCENT,
                marker("arrow_point", ex, ey, ux, uy, ACCENT, "none")
            );
        }
        b.grow(tx - R, ty - R, tx + R, ty + R);
        trends.push_str(&format!(
            "<g class=\"pele-wardley-trend\" data-id=\"{}\">{}<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"var(--_bg)\" stroke=\"{}\"/></g>",
            esc_text(&trend.node_id),
            arrow,
            num(tx),
            num(ty),
            js::num_str(R),
            ACCENT
        ));
    }

    let mut nodes = String::new();
    let tri = 1.2 * R;
    let dot = 0.7 * R;
    for (k, at) in positions.iter().enumerate() {
        let node = at.node;
        let (x, y) = (at.x, at.y);
        let anchor = node.class_name == Some("anchor");
        let strategy = node.source_strategy.as_deref().filter(|s| !s.is_empty());
        let mut shape = String::new();
        if let Some(strategy) = strategy {
            let fill = match strategy {
                "outsource" => "var(--_m)",
                "buy" => "var(--_a)",
                "build" => "var(--_s)",
                _ => "var(--_bg)",
            };
            shape.push_str(&format!("<circle class=\"pele-wardley-{}\" cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{}\" stroke=\"{}\"/>", strategy, num(x), num(y), js::num_str(2.0 * R), fill, LINE));
        }
        if strategy == Some("market") {
            let bx = tri * (PI / 6.0).cos();
            let by = y + tri * (PI / 6.0).sin();
            shape.push_str(&format!("<path d=\"M{},{}L{},{}H{}Z\" fill=\"none\" stroke=\"{}\"/>", num(x), num(y - tri), num(x - bx), num(by), num(x + bx), LINE));
            for (cx, cy) in [(x, y - tri), (x - bx, by), (x + bx, by)] {
                shape.push_str(&format!("<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"var(--_bg)\" stroke=\"{}\"/>", num(cx), num(cy), num(dot), LINE));
            }
        } else if node.is_pipeline_parent {
            shape.push_str(&format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_bg)\" stroke=\"{}\"/>",
                num(x - SQUARE / 2.0),
                num(y - SQUARE / 2.0),
                js::num_str(SQUARE),
                js::num_str(SQUARE),
                LINE
            ));
        } else if !anchor {
            shape.push_str(&format!("<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"var(--_bg)\" stroke=\"{}\"/>", num(x), num(y), js::num_str(R), LINE));
        }
        if node.inertia == Some(true) {
            let offset = (if node.is_pipeline_parent { SQUARE / 2.0 } else { R }) + 10.0 + if strategy.is_some() { R + 4.0 } else { 0.0 };
            shape.push_str(&format!("<rect class=\"pele-wardley-inertia\" x=\"{}\" y=\"{}\" width=\"4\" height=\"{}\" fill=\"{}\"/>", num(x + offset), num(y - R), js::num_str(2.0 * R), LINE));
        }
        let ox = node.label_offset_x.map(|v| clamp(v, -5000.0, 5000.0));
        let oy = node.label_offset_y.map(|v| clamp(v, -5000.0, 5000.0));
        let caption;
        if anchor {
            let label = anchor_label(node);
            caption = b.centered(&label, x + ox.unwrap_or(0.0), y + oy.unwrap_or(-3.0), &format!(" class=\"pele-label\" font-size=\"{}\" font-weight=\"var(--_hw)\"", js::num_str(small)));
        } else {
            let label = text(&node.label, small, 4000.0, 0);
            let attrs = format!(" class=\"pele-label\" font-size=\"{}\"", js::num_str(small));
            let lift = LABEL_OFFSET + if strategy.is_some() { 10.0 } else { 0.0 };
            if ox.is_some() || oy.is_some() {
                caption = b.starting(&label, x + ox.unwrap_or(lift), y + oy.unwrap_or(-lift), &attrs);
            } else if node.in_pipeline {
                caption = b.centered(&label, x, y + 2.0 * R + 4.0 + label.height / 2.0, &attrs);
            } else {
                // The label goes up and to the right unless a link leaves that way; then the next free corner.
                let busy = taken[k];
                let corner = if busy & 1 == 0 {
                    1
                } else if busy & 2 == 0 {
                    2
                } else if busy & 4 == 0 {
                    4
                } else if busy & 8 == 0 {
                    8
                } else {
                    1
                };
                let right = corner == 1 || corner == 2;
                let up = corner == 1 || corner == 4;
                caption = b.starting(&label, if right { x + lift } else { x - lift - label.width }, if up { y - lift } else { y + lift + small * 0.75 }, &attrs);
            }
        }
        b.grow(x - 2.0 * R, y - 2.0 * R, x + 2.0 * R + 20.0, y + 2.0 * R);
        let class = if anchor {
            "anchor"
        } else if node.is_pipeline_parent {
            "pipeline-parent"
        } else if node.in_pipeline {
            "pipeline-component"
        } else {
            "component"
        };
        nodes.push_str(&format!("<g class=\"pele-node pele-wardley-{}\" data-id=\"{}\">{}{}</g>", class, esc_text(&node.id), shape, caption));
    }

    let mut annotations = String::new();
    for annotation in &model.annotations {
        let number = js::num_str(annotation.number);
        let label = text(&number, tiny, 4000.0, style::BOLD);
        let mut marks = String::new();
        for &(cx, cy) in &annotation.coordinates {
            let x = project_x(cx);
            let y = project_y(cy);
            let r = 10f64.max(label.width / 2.0 + 4.0);
            b.grow(x - r, y - r, x + r, y + r);
            marks.push_str(&format!("<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"var(--_bg)\" stroke=\"{}\"/>", num(x), num(y), num(r), LINE));
            marks.push_str(&label_svg(&label, x, y, &format!(" font-size=\"{}\" font-weight=\"var(--_hw)\"", js::num_str(tiny))));
        }
        annotations.push_str(&format!("<g class=\"pele-wardley-annotation\" data-id=\"{}\">{}</g>", esc_text(&number), marks));
    }
    let mut listed: Vec<_> = model.annotations.iter().filter(|a| !a.text.is_empty()).collect();
    listed.sort_by(|a, c| js::ord(a.number - c.number));
    if !listed.is_empty() {
        let lines: Vec<Label> = listed.iter().map(|a| text(&format!("{}. {}", js::num_str(a.number), a.text), small, 320.0, 0)).collect();
        let mut width: f64 = 0.0;
        let mut height = 0.0;
        for line in &lines {
            width = width.max(line.width);
            height += line.height;
        }
        width += 20.0;
        height += 14.0;
        // Without a position the list goes under the map, where it covers nothing.
        let (bx, by) = match model.annotations_box {
            Some((x, y)) => (clamp(project_x(x), 0.0, 0f64.max(plot_w - width)), clamp(project_y(y), 0.0, 0f64.max(plot_h - height))),
            None => (0.0, legend_top),
        };
        b.grow(bx, by, bx + width, by + height);
        let mut rows = String::new();
        let mut y = by + 7.0;
        for line in &lines {
            rows.push_str(&start_label(line, bx + 10.0, y + line.line_height / 2.0 + small * 0.35, &format!(" font-size=\"{}\"", js::num_str(small))));
            y += line.height;
        }
        annotations.push_str(&format!(
            "<g class=\"pele-wardley-annotations\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_bg)\" stroke=\"var(--_b)\"{}/>{}</g>",
            num(bx),
            num(by),
            num(width),
            num(height),
            ROUNDED,
            rows
        ));
    }

    let mut notes = String::new();
    for note in &model.notes {
        let label = text(&note.text, small, 360f64.min(plot_w * 0.6), style::BOLD);
        notes.push_str(&b.starting(&label, project_x(note.x), project_y(note.y), &format!(" class=\"pele-wardley-note\" font-size=\"{}\" font-weight=\"var(--_hw)\"", js::num_str(small))));
    }

    // Accelerators point right, deaccelerators left.
    let mut forces = String::new();
    let mut force = |b: &mut Bounds, name: &str, px: f64, py: f64, forward: bool| {
        let x = project_x(px);
        let y = project_y(py);
        let half = FORCE_HEIGHT / 2.0;
        let tip = if forward { x + FORCE_WIDTH } else { x };
        let tail = if forward { x } else { x + FORCE_WIDTH };
        let neck = if forward { x + FORCE_WIDTH - FORCE_HEAD } else { x + FORCE_HEAD };
        let d = format!("M{},{}H{}V{}L{},{}L{},{}V{}H{}Z", num(tail), num(y - half), num(neck), num(y - half - 8.0), num(tip), num(y), num(neck), num(y + half + 8.0), num(y + half), num(tail));
        b.grow(x, y - half - 8.0, x + FORCE_WIDTH, y + half + 8.0);
        let caption =
            b.centered(&text(name, tiny + 1.0, 160.0, style::BOLD), x + FORCE_WIDTH / 2.0, y + half + 8.0 + 12.0, &format!(" font-size=\"{}\" font-weight=\"var(--_hw)\"", js::num_str(tiny + 1.0)));
        forces.push_str(&format!(
            "<g class=\"pele-wardley-{}\" data-id=\"{}\"><path d=\"{}\" fill=\"var(--_s)\" stroke=\"{}\"/>{}</g>",
            if forward { "accelerator" } else { "deaccelerator" },
            esc_text(name),
            d,
            LINE,
            caption
        ));
    };
    for a in &model.accelerators {
        force(&mut b, &a.name, a.x, a.y, true);
    }
    for d in &model.deaccelerators {
        force(&mut b, &d.name, d.x, d.y, false);
    }

    let title = layout_label(model.title.as_deref(), false, size, 4000.0, style::BOLD);
    let title_height = if title.height > 0.0 { title.height + 12.0 } else { 0.0 };
    let left = b.min_x.min(plot_w / 2.0 - title.width / 2.0);
    let right = b.max_x.max(plot_w / 2.0 + title.width / 2.0);
    let tx = pad - left;
    let ty = pad + title_height - b.min_y;
    let total_width = (right - left + 2.0 * pad).ceil();
    let total_height = (b.max_y - b.min_y + title_height + 2.0 * pad).ceil();
    let mut content = label_svg(&title, tx + plot_w / 2.0, pad + title.height / 2.0, " class=\"pele-title\" font-weight=\"var(--_tw)\"");
    content.push_str(&format!("<g transform=\"translate({},{})\">", num(tx), num(ty)));
    content.push_str(&format!("<g class=\"pele-wardley-frame\" fill=\"none\">{}</g>", frame));
    content.push_str(&stage_labels);
    content.push_str(&axis_labels);
    content.push_str(&pipelines);
    if !links.is_empty() {
        content.push_str(&format!("<g class=\"pele-edges\" fill=\"none\">{}</g>", links));
    }
    if !trends.is_empty() {
        content.push_str(&format!("<g class=\"pele-wardley-trends\" fill=\"none\">{}</g>", trends));
    }
    content.push_str(&nodes);
    content.push_str(&annotations);
    content.push_str(&notes);
    content.push_str(&forces);
    content.push_str("</g>");
    let acc = Accessible { acc_title: model.acc_title.as_deref(), acc_descr: model.acc_descr.as_deref() };
    Rendered { svg: svg_document("wardley", total_width, total_height, size, options.responsive, &acc, &content), width: total_width, height: total_height, links: Vec::new() }
}
