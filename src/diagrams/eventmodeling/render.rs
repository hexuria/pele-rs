// Port of src/diagrams/eventmodeling/render.ts. With no `maxWidth` the model always runs across,
// so `verticalEventModel` is not ported (see common/fit_width.rs).

use super::{EntityKind, EventModelingModel};
use crate::diagrams::common::fit_width::turn_to_fit;
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num};
use crate::svg::edges::{marker, marker_trim};
use crate::svg::root::{svg_document, Accessible};
use crate::svg::theme::{series_color, FONT_MONO, ROUNDED};
use crate::text::entities::decode_entities;
use crate::text::label::{layout_label, Label};
use crate::text::measurer::{style, width as measure};
use crate::types::{RenderOptions, Rendered};

// Series slot for each kind of entity. A UI has none and is drawn plain.
fn series(kind: EntityKind) -> Option<i64> {
    match kind {
        EntityKind::Command => Some(0),
        EntityKind::Event => Some(1),
        EntityKind::ReadModel => Some(2),
        EntityKind::Processor => Some(6),
        EntityKind::Ui => None,
    }
}

const BOX_PAD_X: f64 = 12.0;
const BOX_PAD_Y: f64 = 10.0;
const BOX_MIN_WIDTH: f64 = 96.0;
const BOX_GAP: f64 = 16.0;
const STAGGER: f64 = 80.0;
const LANE_PAD: f64 = 12.0;
const LANE_GAP: f64 = 8.0;
const DATA_COLUMNS: usize = 48;
const NOTE_COLUMNS: usize = 80;
const CARD_GAP: f64 = 8.0;

struct Placed {
    name: Label,
    data: Vec<String>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

struct Band {
    label: Label,
    right: f64,
    y: f64,
    h: f64,
}

// Splits text into lines of at most `columns` characters, keeping its own line breaks.
fn hard_wrap(text: &str, columns: usize) -> Vec<String> {
    let mut out = Vec::new();
    for line in decode_entities(text).split('\n') {
        if js::len16(line) <= columns {
            out.push(line.to_string());
            continue;
        }
        let chars: Vec<char> = line.chars().collect();
        if chars.len() <= columns {
            out.push(line.to_string());
            continue;
        }
        for piece in chars.chunks(columns) {
            out.push(piece.iter().collect());
        }
    }
    out
}

fn lines(rows: &[String], x: f64, y: f64, step: f64, attrs: &str) -> String {
    let mut body = String::new();
    for (i, row) in rows.iter().enumerate() {
        if !row.is_empty() {
            body.push_str(&format!("<tspan x=\"{}\" y=\"{}\">{}</tspan>", num(x), num(y + i as f64 * step), esc(row)));
        }
    }
    if body.is_empty() {
        String::new()
    } else {
        format!("<text{} xml:space=\"preserve\">{}</text>", attrs, body)
    }
}

pub fn render_event_model(model: &EventModelingModel, config: &Config, options: &RenderOptions) -> Rendered {
    turn_to_fit(options, |_down| draw(model, config, options))
}

fn draw(model: &EventModelingModel, _config: &Config, options: &RenderOptions) -> Rendered {
    let size = 16.0;
    let name_size = js::round(size * 0.875);
    let small = js::round(size * 0.8125);
    let mono = js::round(size * 0.75);
    let mono_step = js::round(mono * 1.4);
    let small_step = js::round(small * 1.5);
    let pad = 8.0;
    let widest = |rows: &[String], font_size: f64, st: u32| -> f64 {
        let mut width: f64 = 0.0;
        for row in rows {
            width = width.max(measure(row, font_size, st));
        }
        width
    };

    let mut bands: Vec<Band> = Vec::new();
    let mut label_column: f64 = 0.0;
    for lane in &model.lanes {
        let label = layout_label(Some(&lane.label), false, small, 4000.0, style::BOLD);
        label_column = label_column.max(label.width);
        bands.push(Band { label, right: f64::NEG_INFINITY, y: 0.0, h: 0.0 });
    }
    label_column += 2.0 * LANE_PAD;

    // Time runs left to right. A frame in a new lane starts under the end of the one before it,
    // so the arrow between them can drop straight down; a frame in the same lane follows on.
    let mut placed: Vec<Placed> = Vec::new();
    for (k, b) in model.boxes.iter().enumerate() {
        let name = layout_label(Some(&b.name), false, name_size, 4000.0, style::BOLD);
        let data = match &b.data {
            Some(d) if !d.is_empty() => hard_wrap(d, DATA_COLUMNS),
            _ => Vec::new(),
        };
        let w = BOX_MIN_WIDTH.max(name.width.max(widest(&data, mono, style::MONO)) + 2.0 * BOX_PAD_X);
        let h = 2.0 * BOX_PAD_Y + name.height + if !data.is_empty() { 6.0 + data.len() as f64 * mono_step } else { 0.0 };
        let mut x = 0.0;
        if k > 0 {
            let previous = &placed[k - 1];
            let stagger = previous.x + (previous.w / 2.0).max(previous.w - STAGGER);
            let band = &bands[b.lane];
            x = if model.boxes[k - 1].lane == b.lane { band.right + BOX_GAP } else { stagger.max(band.right + BOX_GAP) };
        }
        let band = &mut bands[b.lane];
        band.right = x + w;
        band.h = band.h.max(h);
        placed.push(Placed { name, data, x, y: 0.0, w, h });
    }

    let mut content_right: f64 = 0.0;
    for item in &placed {
        content_right = content_right.max(item.x + item.w);
    }
    let lane_width = label_column + content_right + LANE_PAD;

    let title = layout_label(model.title.as_deref(), false, size, 4000.0, style::BOLD);
    let title_height = if title.height > 0.0 { title.height + 12.0 } else { 0.0 };
    let left = pad;
    let top = pad + title_height;

    let mut y = top;
    let mut lanes = String::new();
    for (k, band) in bands.iter_mut().enumerate() {
        band.y = y;
        band.h += 2.0 * LANE_PAD;
        lanes.push_str(&format!(
            "<g class=\"pele-cluster pele-em-lane\" data-id=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_s)\"{}/>{}</g>",
            esc_text(&model.lanes[k].label),
            num(left),
            num(y),
            num(lane_width),
            num(band.h),
            ROUNDED,
            label_svg(
                &band.label,
                left + LANE_PAD + band.label.width / 2.0,
                y + band.h / 2.0,
                &format!(" class=\"pele-cluster-label\" font-size=\"{}\" font-weight=\"var(--_hw)\" fill=\"var(--_m)\"", js::num_str(small))
            )
        ));
        y += band.h + LANE_GAP;
    }
    let mut bottom = if !bands.is_empty() { y - LANE_GAP } else { top };

    let mut nodes = String::new();
    for (k, item) in placed.iter_mut().enumerate() {
        let b = &model.boxes[k];
        let band = &bands[b.lane];
        item.x += left + label_column;
        item.y = band.y + (band.h - item.h) / 2.0;
        let frame = format!("x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"", num(item.x), num(item.y), num(item.w), num(item.h));
        let shape = match series(b.kind) {
            None => format!("<rect {} fill=\"var(--_bg)\" stroke=\"var(--_b)\"{}/>", frame, ROUNDED),
            Some(s) => format!("<rect {} fill=\"var(--_bg)\"{}/><rect {} fill=\"{}\" fill-opacity=\"0.22\" stroke=\"{}\"{}/>", frame, ROUNDED, frame, series_color(s), series_color(s), ROUNDED),
        };
        let name_y = item.y + BOX_PAD_Y + item.name.height / 2.0;
        let text = label_svg(&item.name, item.x + item.w / 2.0, name_y, &format!(" class=\"pele-label\" font-size=\"{}\" font-weight=\"var(--_hw)\"", js::num_str(name_size)))
            + &lines(
                &item.data,
                item.x + BOX_PAD_X,
                item.y + BOX_PAD_Y + item.name.height + 6.0 + mono_step / 2.0 + mono * 0.35,
                mono_step,
                &format!(" class=\"pele-em-data\" font-family=\"{}\" font-size=\"{}\"", FONT_MONO, js::num_str(mono)),
            );
        nodes.push_str(&format!("<g class=\"pele-node pele-em-{}{}\" data-id=\"{}\">{}{}</g>", b.kind.name(), if b.reset { " pele-em-reset" } else { "" }, esc_text(&b.id), shape, text));
    }

    let mut edges = String::new();
    let trim = marker_trim("arrow_point");
    for relation in &model.relations {
        if relation.source == relation.target {
            continue;
        }
        let (s, t) = (&placed[relation.source], &placed[relation.target]);
        let d;
        let head;
        if model.boxes[relation.source].lane == model.boxes[relation.target].lane {
            // Along the lane, from the side of one box to the side of the other.
            let forward = t.x >= s.x;
            let sx = if forward { s.x + s.w } else { s.x };
            let tx = if forward { t.x } else { t.x + t.w };
            let sy = s.y + s.h / 2.0;
            let ty = t.y + t.h / 2.0;
            let dir = if tx >= sx { 1.0 } else { -1.0 };
            d = format!("M{},{}L{},{}", num(sx), num(sy), num(tx - dir * trim), num(ty));
            head = marker("arrow_point", tx, ty, dir, 0.0, "var(--_l)", "none");
        } else {
            // Between lanes, straight down or up where the boxes overlap, else in an S.
            let down = t.y > s.y;
            let from = s.x.max(t.x);
            let to = (s.x + s.w).min(t.x + t.w);
            let inset = |w: f64| 24f64.min(w / 4.0);
            let sx = if to - from >= 16.0 {
                (from + to) / 2.0
            } else if t.x > s.x {
                s.x + s.w - inset(s.w)
            } else {
                s.x + inset(s.w)
            };
            let tx = if to - from >= 16.0 {
                sx
            } else if t.x > s.x {
                t.x + inset(t.w)
            } else {
                t.x + t.w - inset(t.w)
            };
            let sy = if down { s.y + s.h } else { s.y };
            let ty = if down { t.y } else { t.y + t.h };
            let dir = if down { 1.0 } else { -1.0 };
            let end = ty - dir * trim;
            let mid = (sy + end) / 2.0;
            d = if sx == tx {
                format!("M{},{}V{}", num(sx), num(sy), num(end))
            } else {
                format!("M{},{}C{},{} {},{} {},{}", num(sx), num(sy), num(sx), num(mid), num(tx), num(mid), num(tx), num(end))
            };
            head = marker("arrow_point", tx, ty, 0.0, dir, "var(--_l)", "none");
        }
        edges.push_str(&format!(
            "<g class=\"pele-edge\" data-id=\"{}-{}\"><path d=\"{}\" fill=\"none\" stroke=\"var(--_l)\"/>{}</g>",
            esc_text(&model.boxes[relation.source].id),
            esc_text(&model.boxes[relation.target].id),
            d,
            head
        ));
    }

    // Notes and given/when/then specifications, listed under the lanes with the frame they belong to.
    let mut names: Vec<(&str, &str)> = Vec::new();
    for b in &model.boxes {
        match names.iter_mut().find(|(k, _)| *k == b.id) {
            Some(slot) => slot.1 = &b.name,
            None => names.push((&b.id, &b.name)),
        }
    }
    let mut cards = String::new();
    let mut cards_right: f64 = 0.0;
    let mut card = |kind: &str, heading: &str, frame: &str, rows: Vec<String>, cards: &mut String, bottom: &mut f64| {
        let suffix = names.iter().find(|(k, _)| *k == frame).map_or(String::new(), |(_, n)| format!(" {}", n));
        let head = layout_label(Some(&format!("{} · {}{}", heading, frame, suffix)), false, small, 4000.0, style::BOLD);
        let w = head.width.max(widest(&rows, small, 0)) + 2.0 * BOX_PAD_X;
        let h = 2.0 * BOX_PAD_Y + head.height + rows.len() as f64 * small_step;
        let cy = *bottom + if cards.is_empty() && bands.is_empty() { 0.0 } else { CARD_GAP };
        cards.push_str(&format!(
            "<g class=\"pele-em-{}\" data-id=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_bg)\" stroke=\"var(--_b)\"{}/>{}{}</g>",
            kind,
            esc_text(frame),
            num(left),
            num(cy),
            num(w),
            num(h),
            ROUNDED,
            label_svg(&head, left + BOX_PAD_X + head.width / 2.0, cy + BOX_PAD_Y + head.height / 2.0, &format!(" font-size=\"{}\" font-weight=\"var(--_hw)\"", js::num_str(small))),
            lines(&rows, left + BOX_PAD_X, cy + BOX_PAD_Y + head.height + small_step / 2.0 + small * 0.35, small_step, &format!(" font-size=\"{}\"", js::num_str(small)))
        ));
        *bottom = cy + h;
        cards_right = cards_right.max(w);
    };
    for note in &model.notes {
        card("note", "Note", &note.frame, hard_wrap(&note.text, NOTE_COLUMNS), &mut cards, &mut bottom);
    }
    for spec in &model.specifications {
        let mut rows = vec![format!("Given  {}", spec.given.join(", "))];
        if !spec.when.is_empty() {
            rows.push(format!("When  {}", spec.when.join(", ")));
        }
        rows.push(format!("Then  {}", spec.then.join(", ")));
        let rows: Vec<String> = rows.iter().flat_map(|row| hard_wrap(row, NOTE_COLUMNS)).collect();
        card("specification", "Given / When / Then", &spec.frame, rows, &mut cards, &mut bottom);
    }

    let width = (if !bands.is_empty() { lane_width } else { 0.0 }).max(cards_right).max(title.width);
    let total_width = (width + 2.0 * pad).ceil();
    let total_height = (bottom + pad).ceil();
    let content = label_svg(&title, total_width / 2.0, pad + title.height / 2.0, " class=\"pele-title\" font-weight=\"var(--_tw)\"") + &lanes + &edges + &nodes + &cards;
    let acc = Accessible { acc_title: model.acc_title.as_deref(), acc_descr: model.acc_descr.as_deref() };
    Rendered { svg: svg_document("eventmodeling", total_width, total_height, size, options.responsive, &acc, &content), width: total_width, height: total_height, links: Vec::new() }
}
