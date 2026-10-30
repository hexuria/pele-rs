// Port of src/diagrams/cynefin/render.ts.

use super::boundaries::{backward, cliff, curve_path, ellipse_path, forward, resolve_seed, wave, WaveOptions, FOLD, HORIZON};
use super::CynefinModel;
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc_text, label_svg, num};
use crate::svg::edges::{marker, marker_trim};
use crate::svg::root::{svg_document, Accessible};
use crate::svg::theme::{series_color, ROUNDED};
use crate::text::label::{layout_label, Label};
use crate::text::measurer::style;
use crate::types::{RenderOptions, Rendered};
use crate::util::yaml::Value;

// [domain, column, row, series color]
const QUADRANTS: [(&str, f64, f64, i64); 4] = [("complex", 0.0, 0.0, 0), ("complicated", 1.0, 0.0, 1), ("chaotic", 0.0, 1.0, 3), ("clear", 1.0, 1.0, 2)];
const CONFUSION_COLOR: i64 = 4;

// [name, decision model, kind of practice]
fn meta(name: &str) -> [&'static str; 3] {
    match name {
        "complex" => ["Complex", "Probe → Sense → Respond", "Emergent Practices"],
        "complicated" => ["Complicated", "Sense → Analyse → Respond", "Good Practices"],
        "clear" => ["Clear", "Sense → Categorise → Respond", "Best Practices"],
        "chaotic" => ["Chaotic", "Act → Sense → Respond", "Novel Practices"],
        _ => ["Confusion", "", "Disorder"],
    }
}

const MAX_CONFUSION_ITEMS: usize = 3;
const ITEM_MAX_WIDTH: f64 = 200.0;
const ITEM_PAD_X: f64 = 10.0;
const ITEM_PAD_Y: f64 = 4.0;
const ITEM_GAP: f64 = 6.0;
const HEAD_GAP: f64 = 10.0;
const MARGIN: f64 = 20.0;
const CLEARANCE: f64 = 10.0;

struct Badge {
    text: String,
    label: Label,
    w: f64,
    h: f64,
    overflow: bool,
}

struct Block {
    name: &'static str,
    title: Label,
    subtitles: Vec<Label>,
    badges: Vec<Badge>,
    w: f64,
    h: f64,
    cx: f64,
    cy: f64,
}

fn section(config: &Config) -> Option<&crate::util::yaml::Map> {
    config.get("cynefin").and_then(|v| v.as_map())
}

fn setting(config: &Config, key: &str, fallback: f64, min: f64, max: f64) -> f64 {
    match section(config).and_then(|s| s.get(key)) {
        Some(Value::Num(v)) if *v >= min && *v <= max => *v,
        _ => fallback,
    }
}

pub fn render_cynefin(model: &CynefinModel, config: &Config, options: &RenderOptions) -> Rendered {
    let size = 16.0;
    let small = js::round(size * 0.8125);
    let item_size = js::round(size * 0.875);
    let pad = 8.0;
    let show_descriptions = !matches!(section(config).and_then(|s| s.get("showDomainDescriptions")), Some(Value::Bool(false)));
    let amplitude = setting(config, "boundaryAmplitude", 8.0, 0.0, 50.0);
    let seed = resolve_seed(setting(config, "seed", 0.0, -1e15, 1e15), "pele");
    let item_line = js::round(item_size * 1.5);

    let badge = |text: String, overflow: bool| -> Badge {
        let label = layout_label(Some(&text), false, item_size, ITEM_MAX_WIDTH, 0);
        Badge { w: label.width + 2.0 * ITEM_PAD_X, h: label.height.max(item_line) + 2.0 * ITEM_PAD_Y, text, label, overflow }
    };

    let block = |name: &'static str| -> Block {
        let m = meta(name);
        let title = layout_label(Some(m[0]), false, size, 4000.0, style::BOLD);
        let mut subtitles = Vec::new();
        if show_descriptions {
            for text in [m[1], m[2]] {
                if !text.is_empty() {
                    subtitles.push(layout_label(Some(text), false, small, 4000.0, style::ITALIC));
                }
            }
        }
        let items: &[super::CynefinItem] = model.domain(name).map_or(&[], |d| &d.items);
        let shown = if name == "confusion" && items.len() > MAX_CONFUSION_ITEMS { MAX_CONFUSION_ITEMS } else { items.len() };
        let mut badges = Vec::new();
        for item in &items[..shown] {
            badges.push(badge(item.label.clone(), false));
        }
        if shown < items.len() {
            badges.push(badge(format!("+{} more", items.len() - shown), true));
        }
        let mut w = title.width;
        let mut h = title.height;
        for s in &subtitles {
            w = w.max(s.width);
            h += s.height;
        }
        if !badges.is_empty() {
            h += HEAD_GAP - ITEM_GAP;
        }
        for b in &badges {
            w = w.max(b.w);
            h += ITEM_GAP + b.h;
        }
        Block { name, title, subtitles, badges, w, h, cx: 0.0, cy: 0.0 }
    };

    let mut quadrants: Vec<Block> = QUADRANTS.iter().map(|q| block(q.0)).collect();
    let mut confusion = block("confusion");
    let rx = 88f64.max(confusion.w * 0.71 + 8.0);
    let ry = 64f64.max(confusion.h * 0.71 + 6.0);

    let mut quad_w = setting(config, "width", 640.0, 100.0, 10000.0) / 2.0;
    let mut quad_h = setting(config, "height", 440.0, 100.0, 10000.0) / 2.0;
    for b in &quadrants {
        quad_w = quad_w.max(b.w + 2.0 * MARGIN);
        quad_h = quad_h.max(b.h + 2.0 * MARGIN);
    }
    // A block sits in the middle of its quadrant, so its inner corner must stay clear of the ellipse.
    for b in &quadrants {
        let dx = (quad_w - b.w) / 2.0;
        let dy = (quad_h - b.h) / 2.0;
        if (dx / (rx + CLEARANCE)).powi(2) + (dy / (ry + CLEARANCE)).powi(2) >= 1.0 {
            continue;
        }
        let wider = b.w + 2.0 * (rx + CLEARANCE);
        let taller = b.h + 2.0 * (ry + CLEARANCE);
        if wider * quad_h <= quad_w * taller {
            quad_w = quad_w.max(wider);
        } else {
            quad_h = quad_h.max(taller);
        }
    }

    let title = layout_label(model.title.as_deref(), false, size, 4000.0, style::BOLD);
    let title_height = if title.height > 0.0 { title.height + 12.0 } else { 0.0 };
    let width = 2.0 * quad_w;
    let height = 2.0 * quad_h;
    let x0 = pad;
    let y0 = pad + title_height;
    let cx = x0 + quad_w;
    let cy = y0 + quad_h;
    let right = x0 + width;
    let bottom = y0 + height;

    let fold = wave(y0, cy, cx, seed, amplitude, WaveOptions { segments: 4, pin_end: true, ..FOLD });
    let west = wave(x0, cx, cy, seed + 100.0, amplitude, WaveOptions { segments: 4, pin_end: true, ..HORIZON });
    let east = wave(cx, right, cy, seed + 200.0, amplitude, WaveOptions { segments: 4, pin_start: true, ..HORIZON });
    let drop = cliff(cx, cy, bottom, width * 0.03);
    let at = |curve: &[f64], end: bool| -> String {
        if end {
            format!("{},{}", num(curve[curve.len() - 2]), num(curve[curve.len() - 1]))
        } else {
            format!("{},{}", num(curve[0]), num(curve[1]))
        }
    };

    let region = |name: &str| -> String {
        match name {
            "complex" => format!("M{},{}L{}{}{}Z", num(x0), num(y0), at(&fold, false), forward(&fold), backward(&west)),
            "complicated" => format!("M{}L{},{}L{}{}{}Z", at(&fold, false), num(right), num(y0), at(&east, true), backward(&east), backward(&fold)),
            "chaotic" => format!("M{}{}{}L{},{}Z", at(&west, false), forward(&west), forward(&drop), num(x0), num(bottom)),
            _ => format!("M{}{}L{},{}L{}{}Z", at(&east, false), forward(&east), num(right), num(bottom), at(&drop, true), backward(&drop)),
        }
    };

    let mut fills = String::new();
    for (name, _, _, color) in QUADRANTS {
        fills.push_str(&format!("<path class=\"pele-cynefin-region\" data-id=\"{}\" d=\"{}\" fill=\"{}\" fill-opacity=\"0.14\"/>", name, region(name), series_color(color)));
    }
    let boundaries = format!(
        "<g class=\"pele-cynefin-boundaries\" fill=\"none\" stroke=\"var(--_b)\" stroke-dasharray=\"6 3\"><path d=\"{}\"/><path d=\"{}\"/><path d=\"{}\"/><path class=\"pele-cynefin-cliff\" d=\"{}\" stroke=\"var(--_l)\" stroke-width=\"2\" stroke-dasharray=\"none\" stroke-linecap=\"round\"/></g>",
        curve_path(&fold),
        curve_path(&west),
        curve_path(&east),
        curve_path(&drop)
    );
    let oval = ellipse_path(cx, cy, rx, ry);
    let ellipse = format!(
        "<path d=\"{}\" fill=\"var(--_bg)\"/><path class=\"pele-cynefin-region\" data-id=\"confusion\" d=\"{}\" fill=\"{}\" fill-opacity=\"0.18\" stroke=\"var(--_b)\" stroke-dasharray=\"4 2\"/>",
        oval,
        oval,
        series_color(CONFUSION_COLOR)
    );

    let draw_block = |b: &Block| -> String {
        let mut y = b.cy - b.h / 2.0;
        let mut out = label_svg(&b.title, b.cx, y + b.title.height / 2.0, " class=\"pele-label\" font-weight=\"var(--_hw)\"");
        y += b.title.height;
        for s in &b.subtitles {
            out.push_str(&label_svg(s, b.cx, y + s.height / 2.0, &format!(" class=\"pele-cynefin-subtitle\" font-size=\"{}\" font-style=\"italic\" fill=\"var(--_m)\"", js::num_str(small))));
            y += s.height;
        }
        y += HEAD_GAP - ITEM_GAP;
        for item in &b.badges {
            y += ITEM_GAP;
            out.push_str(&format!(
                "<g class=\"pele-node{}\"{}><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_bg)\" stroke=\"var(--_b)\"{}{}/>{}</g>",
                if item.overflow { " pele-cynefin-overflow" } else { "" },
                if item.overflow { String::new() } else { format!(" data-id=\"{}\"", esc_text(&item.text)) },
                num(b.cx - item.w / 2.0),
                num(y),
                num(item.w),
                num(item.h),
                if item.overflow { " stroke-dasharray=\"3 2\"" } else { "" },
                ROUNDED,
                label_svg(&item.label, b.cx, y + item.h / 2.0, &format!(" class=\"pele-label\" font-size=\"{}\"{}", js::num_str(item_size), if item.overflow { " fill=\"var(--_m)\"" } else { "" }))
            ));
            y += item.h;
        }
        format!("<g class=\"pele-cynefin-domain\" data-id=\"{}\">{}</g>", b.name, out)
    };

    let mut domains = String::new();
    for (index, (_, col, row, _)) in QUADRANTS.iter().enumerate() {
        let b = &mut quadrants[index];
        b.cx = x0 + quad_w * (col + 0.5);
        b.cy = y0 + quad_h * (row + 0.5);
        domains.push_str(&draw_block(b));
    }
    confusion.cx = cx;
    confusion.cy = cy;
    domains.push_str(&draw_block(&confusion));

    // Blocks by domain name; index 4 is confusion.
    let block_of = |name: &str| -> Option<usize> {
        if name == "confusion" {
            return Some(4);
        }
        QUADRANTS.iter().position(|q| q.0 == name)
    };
    let get = |i: usize| -> &Block {
        if i == 4 {
            &confusion
        } else {
            &quadrants[i]
        }
    };
    let inside = |b: usize, x: f64, y: f64| -> bool {
        if b == 4 {
            ((x - cx) / (rx + 4.0)).powi(2) + ((y - cy) / (ry + 4.0)).powi(2) <= 1.0
        } else {
            let b = get(b);
            (x - b.cx).abs() <= b.w / 2.0 + 8.0 && (y - b.cy).abs() <= b.h / 2.0 + 8.0
        }
    };

    let mut edges = String::new();
    let mut seen: Vec<(String, usize)> = Vec::new();
    for transition in &model.transitions {
        let (Some(fi), Some(ti)) = (block_of(&transition.from), block_of(&transition.to)) else { continue };
        if fi == ti {
            continue;
        }
        let (from, to) = (get(fi), get(ti));
        let key = if transition.from < transition.to { format!("{} {}", transition.from, transition.to) } else { format!("{} {}", transition.to, transition.from) };
        let repeat = match seen.iter_mut().find(|(k, _)| *k == key) {
            Some((_, n)) => {
                *n += 1;
                *n - 1
            }
            None => {
                seen.push((key, 1));
                0
            }
        };

        let dx = to.cx - from.cx;
        let dy = to.cy - from.cy;
        let len = js::or(js::hypot(dx, dy), || 1.0);
        let mx = (from.cx + to.cx) / 2.0;
        let my = (from.cy + to.cy) / 2.0;
        // Curves bow away from the middle of the diagram, which keeps them off the ellipse.
        let outward = if -dy * (mx - cx) + dx * (my - cy) >= 0.0 { 1.0 } else { -1.0 };
        let nx = (-dy / len) * outward;
        let ny = (dx / len) * outward;
        // A diagonal runs through the middle, so it bows far enough to pass the ellipse.
        let diagonal = fi != 4 && ti != 4 && dx.abs() > 1.0 && dy.abs() > 1.0;
        let bow = (if diagonal { 2.0 * (js::hypot(rx * nx, ry * ny) + 16.0) } else { len * 0.15 }) + repeat as f64 * 56.0;
        let px = mx + nx * bow;
        let py = my + ny * bow;
        let point = |t: f64| -> (f64, f64) {
            let u = 1.0 - t;
            (u * u * from.cx + 2.0 * u * t * px + t * t * to.cx, u * u * from.cy + 2.0 * u * t * py + t * t * to.cy)
        };
        let crossing = |b: usize, mut lo: f64, mut hi: f64, leaving: bool| -> f64 {
            for _ in 0..16 {
                let mid = (lo + hi) / 2.0;
                let (x, y) = point(mid);
                if inside(b, x, y) == leaving {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            (lo + hi) / 2.0
        };
        const STEPS: usize = 48;
        let s = STEPS as f64;
        let ins = |b: usize, t: f64| {
            let (x, y) = point(t);
            inside(b, x, y)
        };
        let mut i = 1;
        while i <= STEPS && ins(fi, i as f64 / s) {
            i += 1;
        }
        if i > STEPS {
            continue;
        }
        let t0 = crossing(fi, (i - 1) as f64 / s, i as f64 / s, true);
        while i <= STEPS && !ins(ti, i as f64 / s) {
            i += 1;
        }
        let t1 = if i > STEPS { 1.0 } else { crossing(ti, (i - 1) as f64 / s, i as f64 / s, false) };

        let (sx, sy) = point(t0);
        let (mut ex, mut ey) = point(t1);
        let blossom = |a: f64, b: f64, c: f64| (1.0 - t0) * (1.0 - t1) * a + ((1.0 - t0) * t1 + t0 * (1.0 - t1)) * b + t0 * t1 * c;
        let qx = blossom(from.cx, px, to.cx);
        let qy = blossom(from.cy, py, to.cy);
        let el = js::or(js::hypot(ex - qx, ey - qy), || 1.0);
        let ux = (ex - qx) / el;
        let uy = (ey - qy) / el;
        let head = marker("arrow_point", ex, ey, ux, uy, "var(--_l)", "none");
        ex -= ux * marker_trim("arrow_point");
        ey -= uy * marker_trim("arrow_point");

        let mut text = String::new();
        if let Some(l) = &transition.label {
            let label = layout_label(Some(l), false, small, ITEM_MAX_WIDTH, 0);
            let (lx, ly) = point((t0 + t1) / 2.0);
            let away = (nx.abs() * label.width) / 2.0 + (ny.abs() * label.height) / 2.0 + 6.0;
            let half = label.width / 2.0 + 4.0;
            let tx = (lx + nx * away).max(x0 + half).min((right - half).max(x0 + half));
            text = label_svg(&label, tx, ly + ny * away, &format!(" class=\"pele-edge-label\" font-size=\"{}\"", js::num_str(small)));
        }
        edges.push_str(&format!(
            "<g class=\"pele-edge\" data-id=\"{}-{}\"><path d=\"M{},{}Q{},{} {},{}\" fill=\"none\" stroke=\"var(--_l)\"/>{}{}</g>",
            transition.from,
            transition.to,
            num(sx),
            num(sy),
            num(qx),
            num(qy),
            num(ex),
            num(ey),
            head,
            text
        ));
    }

    let total_width = (width.max(title.width) + 2.0 * pad).ceil();
    let total_height = (height + title_height + 2.0 * pad).ceil();
    let mut content = label_svg(&title, x0 + width / 2.0, pad + title.height / 2.0, " class=\"pele-title\" font-weight=\"var(--_tw)\"");
    content.push_str(&format!("<g class=\"pele-cynefin-regions\">{}</g>", fills));
    content.push_str(&boundaries);
    content.push_str(&ellipse);
    content.push_str(&domains);
    if !edges.is_empty() {
        content.push_str(&format!("<g class=\"pele-edges\">{}</g>", edges));
    }
    let acc = Accessible { acc_title: model.acc_title.as_deref(), acc_descr: model.acc_descr.as_deref() };
    Rendered { svg: svg_document("cynefin", total_width, total_height, size, options.responsive, &acc, &content), width: total_width, height: total_height, links: Vec::new() }
}
