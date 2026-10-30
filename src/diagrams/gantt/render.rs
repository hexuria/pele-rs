// Port of src/diagrams/gantt/render.ts. The current time is read once, as upstream's `clock`
// does with no `now` option.

use super::axis::{auto_ticks, interval_ticks, time_format};
use super::clock::Date;
use super::dates::{add, add_days};
use super::db::{clock, GanttDb, GanttTask};
use crate::diagrams::common::fit_width::cap_width;
use crate::errors::PeleError;
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num};
use crate::svg::root::{svg_document, Accessible};
use crate::svg::theme::{resolve_style, CRITICAL, ROUNDED};
use crate::text::label::{layout_label, Label};
use crate::text::measurer::{style, width as measure};
use crate::types::{LinkInfo, RenderOptions, Rendered};
use crate::util::url::linked;
use crate::util::yaml::Value;
use std::collections::HashMap;

const WIDTH: f64 = 800.0;
const MIN_PLOT: f64 = 240.0;
const LABEL_GAP: f64 = 8.0;

fn get<'a>(config: &'a Config, key: &str) -> Option<&'a Value> {
    config.get("gantt").and_then(|v| v.as_map()).and_then(|m| m.get(key))
}

fn setting(config: &Config, key: &str, min: f64, max: f64) -> Option<f64> {
    match get(config, key) {
        Some(Value::Num(v)) if v.is_finite() => Some(max.min(min.max(*v))),
        _ => None,
    }
}

fn text(config: &Config, key: &str) -> Option<String> {
    match get(config, key) {
        Some(Value::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn flag(config: &Config, key: &str) -> bool {
    matches!(get(config, key), Some(Value::Bool(true)))
}

// `.replace(/<br\s*\/?>/gi, ' ')`
fn no_breaks(raw: &str) -> String {
    let c: Vec<char> = raw.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        if c[i] == '<' && i + 2 < c.len() && c[i + 1].eq_ignore_ascii_case(&'b') && c[i + 2].eq_ignore_ascii_case(&'r') {
            let mut p = i + 3;
            while p < c.len() && js::is_space(c[p]) {
                p += 1;
            }
            if p < c.len() && c[p] == '/' {
                p += 1;
            }
            if p < c.len() && c[p] == '>' {
                out.push(' ');
                i = p + 1;
                continue;
            }
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

// A task label sits on one row, so only its first line is kept.
fn one_line(raw: &str, size: f64) -> Label {
    let label = layout_label(Some(&no_breaks(raw)), false, size, 4000.0, 0);
    if label.lines.len() <= 1 {
        return label;
    }
    Label { lines: vec![label.lines[0].clone()], widths: vec![label.widths[0]], width: label.widths[0], height: label.line_height, ..label }
}

fn time(d: Option<Date>) -> f64 {
    d.map_or(f64::NAN, |d| d.0)
}

// Whether scheduling gave the task a start and an end.
fn scheduled(task: &GanttTask) -> bool {
    time(task.start_time).is_finite() && time(task.end_time).is_finite()
}

// Mermaid's compact mode: within a section, a task goes on the first row that is free when it starts.
fn pack_rows(tasks: &[usize], all: &[GanttTask], offset: usize, row_of: &mut HashMap<usize, usize>) -> usize {
    let mut sorted: Vec<usize> = tasks.iter().copied().filter(|&t| scheduled(&all[t])).collect();
    sorted.sort_by(|&a, &b| js::ord(js::or(time(all[a].start_time) - time(all[b].start_time), || (all[a].order - all[b].order) as f64)));
    let mut free: Vec<f64> = Vec::new();
    for &t in &sorted {
        let start = time(all[t].start_time);
        let mut row = 0;
        while row < free.len() && start < free[row] {
            row += 1;
        }
        if row == free.len() {
            free.push(0.0);
        }
        free[row] = time(all[t].end_time);
        row_of.insert(t, offset + row);
    }
    let mut rows = free.len();
    for &t in tasks {
        if let std::collections::hash_map::Entry::Vacant(e) = row_of.entry(t) {
            e.insert(offset + rows);
            rows += 1;
        }
    }
    rows.max(1)
}

struct Bar {
    task: usize,
    label: Label,
    row: usize,
    x0: f64,
    x1: f64,
    label_x: f64,
    inside: bool,
}

pub fn render_gantt(model: &mut GanttDb, config: &Config, options: &RenderOptions) -> Result<Rendered, PeleError> {
    let size = 16.0;
    let pad = 8.0;
    let now = clock();

    let label_size = setting(config, "fontSize", 4.0, 72.0).unwrap_or(js::round(size * 0.8125));
    let section_size = setting(config, "sectionFontSize", 4.0, 72.0).unwrap_or(label_size);
    let tick_size = js::round(size * 0.75);
    let bar_height = setting(config, "barHeight", 4.0, 400.0).unwrap_or(js::round(label_size * 1.85));
    let bar_gap = setting(config, "barGap", 0.0, 400.0).unwrap_or(js::round(bar_height / 4.0));
    let row_height = bar_height + bar_gap;
    let section_styles = js::round(setting(config, "numberSectionStyles", 1.0, 1000.0).unwrap_or(4.0)) as usize;
    let compact = model.display_mode == "compact";
    let top_axis = model.top_axis || flag(config, "topAxis");

    model.get_tasks(now)?;
    let tasks = &model.tasks;
    let row_tasks: Vec<usize> = (0..tasks.len()).filter(|&t| !tasks[t].vert).collect();
    let verts: Vec<usize> = (0..tasks.len()).filter(|&t| tasks[t].vert && scheduled(&tasks[t])).collect();

    let mut categories: Vec<String> = Vec::new();
    for &t in &row_tasks {
        if !categories.contains(&tasks[t].section) {
            categories.push(tasks[t].section.clone());
        }
    }
    let category = |s: &str| categories.iter().position(|c| c == s).unwrap();

    let mut row_of: HashMap<usize, usize> = HashMap::new();
    let mut row_section: Vec<String> = Vec::new();
    if compact {
        let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
        for &t in &row_tasks {
            match groups.iter_mut().find(|(s, _)| *s == tasks[t].section) {
                Some(g) => g.1.push(t),
                None => groups.push((tasks[t].section.clone(), vec![t])),
            }
        }
        for (section, group) in &groups {
            let rows = pack_rows(group, tasks, row_section.len(), &mut row_of);
            for _ in 0..rows {
                row_section.push(section.clone());
            }
        }
    } else {
        for &t in &row_tasks {
            row_of.insert(t, row_section.len());
            row_section.push(tasks[t].section.clone());
        }
    }
    let row_count = row_section.len();

    let mut start = f64::INFINITY;
    let mut stop = f64::NEG_INFINITY;
    for task in tasks {
        if !scheduled(task) {
            continue;
        }
        start = start.min(time(task.start_time));
        stop = stop.max(time(task.end_time));
    }
    let has_plot = start <= stop;

    let title = layout_label(model.title.as_deref(), false, size, 4000.0, style::BOLD);
    let title_height = if title.height > 0.0 { title.height + 16.0 } else { 0.0 };
    let acc_title = model.acc_title.clone();
    let acc_descr = model.acc_descr.clone();
    let document = |width: f64, height: f64, content: &str| {
        svg_document("gantt", width, height, size, options.responsive, &Accessible { acc_title: acc_title.as_deref(), acc_descr: acc_descr.as_deref() }, content)
    };

    if !has_plot {
        let width = (title.width.max(1.0) + 2.0 * pad).ceil();
        let height = (title.height.max(1.0) + 2.0 * pad).ceil();
        let content = label_svg(&title, width / 2.0, pad + title.height / 2.0, " class=\"pele-title\" font-weight=\"var(--_tw)\"");
        return Ok(Rendered { svg: document(width, height, &content), width, height, links: Vec::new() });
    }

    let mut section_labels: Vec<Label> = Vec::new();
    let mut section_width: f64 = 0.0;
    for section in &categories {
        let label = layout_label(Some(section), false, section_size, 180.0, 0);
        section_width = section_width.max(label.width);
        section_labels.push(label);
    }

    let axis_format = if !model.axis_format.is_empty() {
        model.axis_format.clone()
    } else if model.date_format == "D" {
        "%d".to_string()
    } else {
        text(config, "axisFormat").unwrap_or_else(|| "%Y-%m-%d".into())
    };
    let tick_label = |t: f64| time_format(&axis_format, Date(t));
    let sample_width = measure(&tick_label(start), tick_size, 0).max(measure(&tick_label(stop), tick_size, 0));

    let left = pad + setting(config, "leftPadding", 0.0, 2000.0).unwrap_or_else(|| (if section_width > 0.0 { section_width + 24.0 } else { 0.0 }).max(sample_width / 2.0));
    let right_padding = setting(config, "rightPadding", 0.0, 2000.0).unwrap_or_else(|| 16f64.max(sample_width / 2.0));
    let target = cap_width(options, setting(config, "useWidth", 100.0, 20000.0).unwrap_or(WIDTH), 280.0, 0.0);
    let plot_width = MIN_PLOT.max(target - left - right_padding - pad);
    let right = left + plot_width;
    let scale = if stop > start { plot_width / (stop - start) } else { 0.0 };
    let x = |t: f64| if scale == 0.0 { left + plot_width / 2.0 } else { left + (t - start) * scale };

    let tick_height = js::round(tick_size * 1.75);
    let top_padding = setting(config, "topPadding", 0.0, 2000.0);
    let grid_start = setting(config, "gridLineStartPadding", 0.0, 2000.0);
    let head_height = pad + title_height + if top_axis { tick_height } else { 0.0 };
    let plot_top = head_height.max(top_padding.unwrap_or(0.0));
    let grid_lead = match (top_padding, grid_start) {
        (Some(tp), Some(gs)) => 0f64.max((tp - gs).min(plot_top - head_height)),
        _ => 0.0,
    };
    let plot_height = (row_count.max(1) as f64) * row_height;
    let plot_bottom = plot_top + plot_height;

    // Bars and where their labels go: inside when they fit, otherwise beside the bar on the side with room.
    let mut bars: Vec<Bar> = Vec::new();
    let mut extent = right + right_padding;
    for &t in &row_tasks {
        let task = &tasks[t];
        let label = one_line(&task.task, label_size);
        let row = row_of[&t];
        if !scheduled(task) {
            let label_x = left + LABEL_GAP + label.width / 2.0;
            extent = extent.max(label_x + label.width / 2.0);
            bars.push(Bar { task: t, label, row, x0: f64::NAN, x1: f64::NAN, label_x, inside: false });
            continue;
        }
        let mut x0 = x(time(task.start_time));
        let mut x1 = x(time(task.render_end_time.or(task.end_time))).max(x0 + if task.milestone { 0.0 } else { 2.0 });
        if task.milestone {
            let mid = x0 + (x(time(task.end_time)) - x0) / 2.0;
            x0 = mid - bar_height / 2.0;
            x1 = mid + bar_height / 2.0;
        }
        let inside = !task.milestone && label.width + 2.0 * LABEL_GAP <= x1 - x0;
        let mut label_x = (x0 + x1) / 2.0;
        if !inside {
            let fits_right = x1 + LABEL_GAP + label.width <= right + right_padding;
            let fits_left = x0 - LABEL_GAP - label.width >= left;
            label_x = if fits_right || !fits_left { x1 + LABEL_GAP + label.width / 2.0 } else { x0 - LABEL_GAP - label.width / 2.0 };
            extent = extent.max(label_x + label.width / 2.0);
        }
        bars.push(Bar { task: t, label, row, x0, x1, label_x, inside });
    }

    let interval = model.tick_interval.clone().filter(|s| !s.is_empty()).or_else(|| text(config, "tickInterval").filter(|s| !s.is_empty())).unwrap_or_default();
    let weekday = model.weekday.clone().or_else(|| text(config, "weekday")).unwrap_or_else(|| "sunday".into());
    let count = 2f64.max(10f64.min((plot_width / (sample_width + 24.0)).floor()));
    let ticks = interval_ticks(&interval, &weekday, start, stop, (plot_width / 3.0).ceil()).unwrap_or_else(|| auto_ticks(start, stop, count));

    let mut grid = String::new();
    let mut axis_bottom = String::new();
    let mut axis_top = String::new();
    let mut last_right = f64::NEG_INFINITY;
    let mut last_label = String::new();
    let bottom_y = plot_bottom + tick_height / 2.0 + 2.0;
    let top_y = plot_top - grid_lead - tick_height / 2.0 - 2.0;
    for &t in &ticks {
        let tx = x(t);
        grid.push_str(&format!("M{},{}V{}", num(tx), num(plot_top - grid_lead), num(plot_bottom)));
        let label = tick_label(t);
        let width = measure(&label, tick_size, 0);
        // A label is skipped when it would run into the one before it or only repeat it; its grid line stays.
        if tx - width / 2.0 < last_right + 8.0 || label == last_label {
            continue;
        }
        last_right = tx + width / 2.0;
        extent = extent.max(last_right);
        let body = format!("text-anchor=\"middle\">{}</text>", esc(&label));
        last_label = label;
        axis_bottom.push_str(&format!("<text x=\"{}\" y=\"{}\" {}", num(tx), num(bottom_y + tick_size * 0.35), body));
        if top_axis {
            axis_top.push_str(&format!("<text x=\"{}\" y=\"{}\" {}", num(tx), num(top_y + tick_size * 0.35), body));
        }
    }

    // Vertical markers take no row. Their labels go under the axis, on as many lines as it takes.
    let mut vert_lines = String::new();
    let mut vert_labels = String::new();
    let mut vert_ends: Vec<f64> = Vec::new();
    let vert_top = plot_bottom + tick_height + 4.0;
    let vert_line = js::round(label_size * 1.5);
    let mut sorted_verts = verts.clone();
    sorted_verts.sort_by(|&a, &b| js::ord(time(tasks[a].start_time) - time(tasks[b].start_time)));
    for &t in &sorted_verts {
        let task = &tasks[t];
        let vx = x(time(task.start_time));
        let label = one_line(&task.task, label_size);
        let cx = (pad + label.width / 2.0).max(vx);
        let mut line = 0;
        while line < vert_ends.len() && cx - label.width / 2.0 < vert_ends[line] + 8.0 {
            line += 1;
        }
        if line == vert_ends.len() {
            vert_ends.push(0.0);
        }
        vert_ends[line] = cx + label.width / 2.0;
        extent = extent.max(vert_ends[line]);
        vert_lines.push_str(&format!("<path class=\"pele-vert\" data-id=\"{}\" d=\"M{},{}V{}\"/>", esc_text(&task.id), num(vx), num(plot_top - grid_lead), num(plot_bottom)));
        vert_labels.push_str(&label_svg(&label, cx, vert_top + line as f64 * vert_line + vert_line / 2.0, &format!(" class=\"pele-vert-label\" data-id=\"{}\"", esc_text(&task.id))));
    }

    let width = (extent.max(pad + title.width) + pad).ceil();
    let height = (plot_bottom + tick_height + if !vert_ends.is_empty() { 4.0 + vert_ends.len() as f64 * vert_line } else { 0.0 } + pad).ceil();

    // Section bands, one per run of rows that share a section, shaded on alternate sections.
    let mut bands = String::new();
    let mut section_out = String::new();
    let mut row = 0;
    while row < row_count {
        let section = &row_section[row];
        let mut end = row + 1;
        while end < row_count && row_section[end] == *section {
            end += 1;
        }
        let index = category(section) % section_styles;
        let y = plot_top + row as f64 * row_height;
        let h = (end - row) as f64 * row_height;
        if categories.len() > 1 || !section.is_empty() {
            bands.push_str(&format!(
                "<rect class=\"pele-section pele-section-{}\" x=\"0\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>",
                index,
                num(y),
                js::num_str(width),
                num(h),
                if index % 2 == 0 { "var(--_s)" } else { "none" }
            ));
        }
        let label = &section_labels[category(section)];
        section_out.push_str(&label_svg(label, pad + 8.0 + label.width / 2.0, y + h / 2.0, &format!(" class=\"pele-section-label\" data-id=\"{}\"", esc_text(section))));
        row = end;
    }

    // Excluded days, as Mermaid marks them: a day at a time from the first task's start, and not
    // at all when the chart spans more than five years.
    let mut excluded = String::new();
    if (!model.excludes.is_empty() || !model.includes.is_empty()) && add(Date(start), 6.0, "y").0 > stop {
        let mut from: Option<Date> = None;
        let mut to: Option<Date> = None;
        let flush = |from: &mut Option<Date>, to: &mut Option<Date>, excluded: &mut String| {
            let (Some(f), Some(t)) = (*from, *to) else { return };
            let mut day_start = f;
            day_start.set_hours(0.0, 0.0, 0.0, 0.0);
            let mut day_end = t;
            day_end.set_hours(23.0, 59.0, 59.0, 999.0);
            let ex0 = left.max(x(day_start.0));
            let ex1 = right.min(x(day_end.0));
            if ex1 > ex0 {
                excluded.push_str(&format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>", num(ex0), num(plot_top), num(ex1 - ex0), num(plot_height)));
            }
            *from = None;
            *to = None;
        };
        let mut day = Date(start);
        while day.0 <= stop {
            if model.is_invalid_date(day)? {
                if from.is_none() {
                    from = Some(day);
                }
                to = Some(day);
            } else {
                flush(&mut from, &mut to, &mut excluded);
            }
            day = add_days(day, 1.0);
        }
        flush(&mut from, &mut to, &mut excluded);
    }
    let tasks = &model.tasks;

    let mut links: Vec<LinkInfo> = Vec::new();
    let mut tasks_out = String::new();
    for bar in &bars {
        let task = &tasks[bar.task];
        let cy = plot_top + bar.row as f64 * row_height + row_height / 2.0;
        let state = if task.active {
            "active"
        } else if task.done {
            "done"
        } else {
            ""
        };
        let mut classes = "pele-task".to_string();
        if !state.is_empty() {
            classes.push_str(" pele-");
            classes.push_str(state);
        }
        if task.crit {
            classes.push_str(" pele-crit");
        }
        if task.milestone {
            classes.push_str(" pele-milestone");
        }
        if task.classes.iter().any(|c| c == "clickable") {
            classes.push_str(" pele-clickable");
        }
        let mut shape = String::new();
        let mut text_fill = "";
        if bar.x0.is_nan() {
            classes.push_str(" pele-unscheduled");
            text_fill = " fill=\"var(--_m)\"";
        } else {
            // A critical task takes the critical color in place of the accent.
            let color = if task.crit { CRITICAL } else { "var(--_c)" };
            let paint = match state {
                "done" => "fill=\"var(--_a)\"".to_string(),
                "active" => format!("fill=\"{}\" fill-opacity=\"0.25\"", color),
                _ => format!("fill=\"{}\"", color),
            };
            let stroke = format!("stroke=\"{}\"", if state == "done" && !task.crit { "var(--_b)" } else { color });
            if task.milestone {
                let cx = (bar.x0 + bar.x1) / 2.0;
                let r = bar_height / 2.0;
                shape = format!(
                    "<path d=\"M{},{}L{},{}L{},{}L{},{}Z\" {} {} stroke-linejoin=\"round\"/>",
                    num(cx),
                    num(cy - r),
                    num(cx + r),
                    num(cy),
                    num(cx),
                    num(cy + r),
                    num(cx - r),
                    num(cy),
                    paint,
                    stroke
                );
            } else {
                shape = format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" {} {}{}/>", num(bar.x0), num(cy - bar_height / 2.0), num(bar.x1 - bar.x0), num(bar_height), paint, stroke, ROUNDED);
            }
            if bar.inside {
                text_fill = if state.is_empty() { " fill=\"var(--_bg)\"" } else { "" };
            } else if state == "done" {
                text_fill = " fill=\"var(--_m)\"";
            }
        }
        let mut body = shape + &label_svg(&bar.label, bar.label_x, cy, &format!(" class=\"pele-label\"{}", text_fill));
        if let Some(url) = model.links.get(&task.id) {
            body = linked(&body, url, &task.id, &mut links, "");
        }
        tasks_out.push_str(&format!("<g class=\"{}\" data-id=\"{}\">{}</g>", classes, esc_text(&task.id), body));
    }

    let mut today = String::new();
    if model.today_marker != "off" && now >= start && now <= stop && stop > start {
        let tx = x(now);
        let top = plot_top - grid_lead;
        let style = if !model.today_marker.is_empty() { resolve_style(&model.today_marker.split(',').map(str::to_string).collect::<Vec<_>>()).line } else { String::new() };
        today = format!("<g class=\"pele-today\"><path d=\"M{},{}V{}\" fill=\"none\" stroke=\"var(--_c)\" stroke-width=\"2\"{}/>", num(tx), num(top), num(plot_bottom), style);
        if style.is_empty() {
            today.push_str(&format!("<path d=\"M{},{}H{}L{},{}Z\" fill=\"var(--_c)\"/>", num(tx - 4.0), num(top - 5.0), num(tx + 4.0), num(tx), num(top + 1.0)));
        }
        today.push_str("</g>");
    }

    let mut axis_line = format!("M{},{}H{}", num(left), num(plot_bottom), num(right));
    if top_axis {
        axis_line.push_str(&format!("M{},{}H{}", num(left), num(plot_top - grid_lead), num(right)));
    }
    let mut content = label_svg(&title, width / 2.0, pad + title.height / 2.0, " class=\"pele-title\" font-weight=\"var(--_tw)\"");
    if !bands.is_empty() {
        content.push_str(&format!("<g class=\"pele-sections\">{}</g>", bands));
    }
    if !excluded.is_empty() {
        content.push_str(&format!("<g class=\"pele-excluded\" fill=\"var(--_a)\">{}</g>", excluded));
    }
    content.push_str(&format!("<g class=\"pele-grid\" fill=\"none\"><path d=\"{}\" stroke=\"var(--_b)\" stroke-dasharray=\"1 3\"/><path d=\"{}\" stroke=\"var(--_b)\"/></g>", grid, axis_line));
    if !vert_lines.is_empty() {
        content.push_str(&format!("<g class=\"pele-verts\" fill=\"none\" stroke=\"var(--_l)\" stroke-width=\"1.5\" stroke-dasharray=\"4 3\">{}</g>", vert_lines));
    }
    content.push_str(&today);
    content.push_str(&format!("<g class=\"pele-tasks\" font-size=\"{}\">{}</g>", num(label_size), tasks_out));
    content.push_str(&format!("<g class=\"pele-section-labels\" font-size=\"{}\">{}</g>", num(section_size), section_out));
    content.push_str(&format!("<g class=\"pele-axis\" font-size=\"{}\" fill=\"var(--_m)\">{}{}</g>", tick_size, axis_bottom, axis_top));
    if !vert_labels.is_empty() {
        content.push_str(&format!("<g class=\"pele-vert-labels\" font-size=\"{}\">{}</g>", num(label_size), vert_labels));
    }
    Ok(Rendered { svg: document(width, height, &content), width, height, links })
}
