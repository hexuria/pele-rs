// Port of src/diagrams/journey/render.ts. With no `maxWidth`, `turnToFit` never asks for the
// journey drawn down, so `verticalJourney` is not ported.

use super::db::{journey_actors, JourneyDb};
use crate::diagrams::common::fit::{fit_label, plain_label};
use crate::diagrams::common::fit_width::turn_to_fit;
use crate::js;
use crate::svg::builder::{esc_text, label_svg, num};
use crate::svg::root::{svg_document, Accessible};
use crate::svg::theme::{series_color, ROUNDED};
use crate::text::label::Label;
use crate::text::measurer::style;
use crate::types::{RenderOptions, Rendered};

const PAD_X: f64 = 10.0;
const PAD_Y: f64 = 8.0;
const MIN_TEXT: f64 = 124.0;
const MAX_TEXT: f64 = 200.0;
const GAP: f64 = 12.0;
const BAND_PAD: f64 = 6.0;
const GUTTER: f64 = 24.0;
const LEVEL: f64 = 26.0;
const FACE: f64 = 11.0;
const DOT: f64 = 5.0;
const DOT_STEP: f64 = 13.0;
const LEGEND_GAP: f64 = 20.0;

const EYES: &str = "<circle cx=\"-3.6\" cy=\"-2.8\" r=\"1.3\"/><circle cx=\"3.6\" cy=\"-2.8\" r=\"1.3\"/>";

fn face(x: f64, y: f64, mood: &str) -> String {
    let mouth = match mood {
        "happy" => "M-5,2A6,6 0 0 0 5,2",
        "neutral" => "M-4.5,4.5H4.5",
        _ => "M-5,6.5A6,6 0 0 1 5,6.5",
    };
    format!(
        "<g class=\"pele-face pele-face-{}\" transform=\"translate({},{})\"><circle r=\"{}\" fill=\"var(--_bg)\" stroke=\"var(--_l)\"/><g fill=\"var(--_l)\">{}</g><path d=\"{}\" fill=\"none\" stroke=\"var(--_l)\" stroke-linecap=\"round\"/></g>",
        mood,
        num(x),
        num(y),
        FACE,
        EYES,
        mouth
    )
}

fn mood(score: f64) -> &'static str {
    if score > 3.0 {
        "happy"
    } else if score < 3.0 {
        "sad"
    } else {
        "neutral"
    }
}

pub fn render_journey(model: &JourneyDb, options: &RenderOptions) -> Rendered {
    turn_to_fit(options, |_down| draw(model, options))
}

struct LegendItem {
    name: String,
    label: Label,
    x: f64,
    y: f64,
    h: f64,
}

fn draw(model: &JourneyDb, options: &RenderOptions) -> Rendered {
    let size = 16.0;
    let small = js::round(size * 0.875);
    let tiny = js::round(size * 0.75);
    let pad = 8.0;
    let tasks = &model.tasks;
    let n = tasks.len();

    // `Task: 5:` names an actor with no name; Mermaid gives it a dot and an empty legend row.
    let names: Vec<String> = journey_actors(tasks).into_iter().filter(|n| !n.is_empty()).collect();
    let actor = |name: &str| names.iter().position(|n| n == name).unwrap_or(0) as i64;

    // Cards widen, up to a point, for a word that would not fit; past that the word is cut.
    let mut text_w = MIN_TEXT;
    let mut labels: Vec<Label> = tasks.iter().map(|t| plain_label(Some(&t.task), small, text_w, 0)).collect();
    let mut widest: f64 = 0.0;
    for label in &labels {
        widest = widest.max(label.width);
    }
    if widest > text_w {
        text_w = widest.ceil().min(MAX_TEXT);
        labels = tasks.iter().map(|t| fit_label(Some(&t.task), small, text_w, 0)).collect();
    }

    let card_w = text_w + 2.0 * PAD_X;
    let pitch = card_w + GAP;
    let plot_w = if n > 0 { n as f64 * pitch - GAP } else { 0.0 };

    let line_h = js::round(small * 1.5);
    let limit = plot_w.max(240.0);
    let mut legend: Vec<LegendItem> = Vec::new();
    let mut legend_w: f64 = 0.0;
    let mut legend_h: f64 = 0.0;
    let mut lx = 0.0;
    for name in &names {
        let label = fit_label(Some(name), small, limit - 2.0 * DOT - 6.0, 0);
        let w = 2.0 * DOT + 6.0 + label.width;
        if lx > 0.0 && lx + w > limit {
            lx = 0.0;
            legend_h += 4.0;
        }
        let y = if lx > 0.0 { legend[legend.len() - 1].y } else { legend_h };
        let h = label.height.max(line_h);
        legend.push(LegendItem { name: name.clone(), label, x: lx, y, h });
        lx += w;
        legend_w = legend_w.max(lx);
        legend_h = legend_h.max(y + h);
        lx += LEGEND_GAP;
    }

    let body_w = if n > 0 { GUTTER + plot_w } else { 0.0 };
    let title = fit_label(model.title.as_deref(), size, body_w.max(320.0), style::BOLD);
    let title_h = if title.height > 0.0 { title.height + 16.0 } else { 0.0 };
    let width = body_w.max(title.width).max(if n > 0 { GUTTER + legend_w } else { 0.0 });
    let left = pad + (width - body_w) / 2.0 + GUTTER;
    let top = pad + title_h;

    // A band covers a run of tasks that share a section, as in Mermaid.
    let mut runs: Vec<(usize, usize, Option<Label>)> = Vec::new();
    let mut band_h: f64 = 0.0;
    let mut i = 0;
    while i < n {
        let mut end = i + 1;
        while end < n && tasks[end].section == tasks[i].section {
            end += 1;
        }
        let mut label = None;
        if !tasks[i].section.is_empty() {
            let l = fit_label(Some(&tasks[i].section), small, (end - i) as f64 * pitch - GAP - 2.0 * PAD_X, style::BOLD);
            band_h = band_h.max(l.height.max(line_h) + 2.0 * BAND_PAD);
            label = Some(l);
        }
        runs.push((i, end - i, label));
        i = end;
    }

    let mut text_h = line_h;
    for label in &labels {
        text_h = text_h.max(label.height);
    }
    let dots_h = if !names.is_empty() { 2.0 * DOT + 6.0 } else { 0.0 };
    let card_h = text_h + dots_h + 2.0 * PAD_Y;
    let cards_y = top + if band_h > 0.0 { band_h + BAND_PAD } else { 0.0 };
    let chart_top = cards_y + card_h + FACE + 16.0;
    let chart_bottom = chart_top + 4.0 * LEVEL;
    let right = left + plot_w;

    let mut body = String::new();
    let mut bottom = top;
    if n > 0 {
        let mut grid = String::new();
        let mut scale = String::new();
        for score in (1..=5).rev() {
            let y = chart_top + (5 - score) as f64 * LEVEL;
            grid.push_str(&format!("M{},{}H{}", num(left - 6.0), num(y), num(right)));
            scale.push_str(&format!("<text x=\"{}\" y=\"{}\">{}</text>", num(left - 12.0), num(y + tiny * 0.35), score));
        }
        body.push_str(&format!(
            "<path class=\"pele-grid\" d=\"{}\" fill=\"none\" stroke=\"var(--_a)\"/><g class=\"pele-scale\" font-size=\"{}\" text-anchor=\"end\" fill=\"var(--_m)\">{}</g>",
            grid, tiny, scale
        ));

        let mut line = String::new();
        let mut stems = String::new();
        let mut faces = String::new();
        let mut cards: Vec<String> = Vec::new();
        for (i, task) in tasks.iter().enumerate() {
            let x = left + i as f64 * pitch;
            let cx = x + card_w / 2.0;
            let score = task.score;
            let level = if score.is_nan() { 3.0 } else { score.max(1.0).min(5.0) };
            let y = chart_top + (5.0 - level) * LEVEL;
            line.push_str(&format!("{}{},{}", if i == 0 { "M" } else { "L" }, num(cx), num(y)));
            stems.push_str(&format!("M{},{}V{}", num(cx), num(cards_y + card_h), num(y - FACE)));
            faces.push_str(&face(cx, y, mood(score)));

            let mut dots = String::new();
            let people: Vec<&String> = task.people.iter().filter(|n| !n.is_empty()).collect();
            let step = DOT_STEP.min((card_w - 2.0 * PAD_X - 2.0 * DOT) / ((people.len() as f64 - 1.0).max(1.0)));
            for (k, name) in people.iter().enumerate() {
                dots.push_str(&format!(
                    "<circle class=\"pele-actor\" cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{}\" stroke=\"var(--_s)\"><title>{}</title></circle>",
                    num(cx + (k as f64 - (people.len() as f64 - 1.0) / 2.0) * step),
                    num(cards_y + card_h - PAD_Y - DOT),
                    DOT,
                    series_color(actor(name)),
                    esc_text(name)
                ));
            }
            cards.push(format!(
                "<g class=\"pele-node pele-task\" data-id=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_s)\" stroke=\"var(--_b)\"{}/>{}{}</g>",
                esc_text(js::trim(&task.task)),
                num(x),
                num(cards_y),
                num(card_w),
                num(card_h),
                ROUNDED,
                label_svg(&labels[i], cx, cards_y + PAD_Y + text_h / 2.0, &format!(" font-size=\"{}\"", small)),
                dots
            ));
        }
        body.push_str(&format!("<path class=\"pele-stems\" d=\"{}\" fill=\"none\" stroke=\"var(--_b)\" stroke-dasharray=\"2 3\"/>", stems));
        if n > 1 {
            body.push_str(&format!("<path class=\"pele-edge\" d=\"{}\" fill=\"none\" stroke=\"var(--_l)\" stroke-linejoin=\"round\"/>", line));
        }
        body.push_str(&faces);

        for (start, count, label) in &runs {
            let members = cards[*start..start + count].concat();
            let Some(label) = label else {
                body.push_str(&members);
                continue;
            };
            let x = left + *start as f64 * pitch;
            let w = *count as f64 * pitch - GAP;
            body.push_str(&format!(
                "<g class=\"pele-cluster pele-section\" data-id=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_a)\"{}/>{}{}</g>",
                esc_text(js::trim(&tasks[*start].section)),
                num(x),
                num(top),
                num(w),
                num(band_h),
                ROUNDED,
                label_svg(label, x + w / 2.0, top + band_h / 2.0, &format!(" font-size=\"{}\" font-weight=\"var(--_hw)\"", small)),
                members
            ));
        }
        bottom = chart_bottom + FACE;

        if !legend.is_empty() {
            let x0 = left + (plot_w - legend_w) / 2.0;
            let y0 = bottom + 16.0;
            let mut items = String::new();
            for item in &legend {
                let cy = y0 + item.y + item.h / 2.0;
                items.push_str(&format!(
                    "<g class=\"pele-legend-item\" data-id=\"{}\"><circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{}\"/>{}</g>",
                    esc_text(&item.name),
                    num(x0 + item.x + DOT),
                    num(cy),
                    DOT,
                    series_color(actor(&item.name)),
                    label_svg(&item.label, x0 + item.x + 2.0 * DOT + 6.0 + item.label.width / 2.0, cy, "")
                ));
            }
            body.push_str(&format!("<g class=\"pele-legend\" font-size=\"{}\">{}</g>", small, items));
            bottom = y0 + legend_h;
        }
    }

    let total_width = (width + 2.0 * pad).ceil();
    let total_height = (bottom.max(pad) + pad).ceil();
    let content = label_svg(&title, pad + width / 2.0, pad + title.height / 2.0, " class=\"pele-title\" font-weight=\"var(--_tw)\"") + &body;
    let acc = Accessible { acc_title: model.acc_title.as_deref(), acc_descr: model.acc_descr.as_deref() };
    Rendered { svg: svg_document("journey", total_width, total_height, size, options.responsive, &acc, &content), width: total_width, height: total_height, links: Vec::new() }
}
