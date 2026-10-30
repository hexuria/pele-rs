// Port of src/diagrams/railroad/render.ts. With no width limit a sequence never folds into rows
// (`measure` gets no room), so the `Folded` kind is not ported.

use super::common::{fold, RailroadNode};
use super::RailroadModel;
use crate::diagrams::common::fit_width::tighten;
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num};
use crate::svg::root::{svg_document, Accessible};
use crate::svg::theme::{FONT_MONO, ROUNDED};
use crate::text::entities::decode_entities;
use crate::text::label::layout_label;
use crate::text::measurer::{style, width as measure_text};
use crate::types::{RenderOptions, Rendered};
use crate::util::yaml::Value;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Terminal,
    NonTerminal,
    Special,
    Sequence,
    Choice,
    Bypass,
    Loop,
}

// A laid-out element. The track enters at the left edge and leaves at the right edge on one
// baseline, with `up` above it and `down` below it.
struct LBox {
    kind: Kind,
    width: f64,
    up: f64,
    down: f64,
    kids: Vec<usize>,
    // A leaf's text, or a loop's count.
    text: String,
    // The name a non-terminal refers to.
    id: String,
    // How far a bypass rises above the baseline, or a loop drops below it.
    reach: f64,
}

struct Metrics {
    size: f64,
    padding: f64,
    height: f64,
    // Space between elements in a sequence, and between stacked branches.
    gap: f64,
    sep: f64,
    arc: f64,
}

const LEAD: f64 = 12.0;
const HEAD_GAP: f64 = 6.0;
const RULE_GAP: f64 = 18.0;
const COUNT_SIZE: f64 = 12.0;
const ARROW: f64 = 7.0;

fn option(config: &Config, key: &str, fallback: f64, max: f64) -> f64 {
    match config.get("railroad").and_then(|v| v.as_map()).and_then(|m| m.get(key)) {
        Some(Value::Num(v)) if *v >= 0.0 => v.min(max),
        _ => fallback,
    }
}

// Text is literal here: a terminal such as "<br>" or "*" is itself, not markup. Mermaid's `#35;`
// codes still stand for characters, and escapes that would be invisible are written out.
fn shown(text: &str) -> String {
    let s = if text.contains("¶ß") { decode_entities(&text.replace('&', "&amp;")) } else { text.to_string() };
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

fn even(value: f64) -> f64 {
    (value / 2.0).ceil() * 2.0
}

fn leaf(kind: Kind, text: String, id: String, st: u32, m: &Metrics) -> LBox {
    let half = m.height / 2.0;
    let width = even(measure_text(&text, m.size, st) + 2.0 * m.padding).max(m.height);
    LBox { kind, width, up: half, down: half, kids: Vec::new(), text, id, reach: 0.0 }
}

fn count(min: f64, max: f64) -> String {
    if max == f64::INFINITY && min <= 1.0 {
        return String::new();
    }
    let text = |v: f64| if v.is_finite() { js::num_str(v) } else { "∞".to_string() };
    if min == max {
        return text(min) + "×";
    }
    if max == f64::INFINITY {
        return format!("≥{}×", text(min));
    }
    if min == 0.0 {
        format!("≤{}×", text(max))
    } else {
        format!("{}–{}×", text(min), text(max))
    }
}

// Where the next branch of a choice sits below the baseline, given the one above it.
fn below(offset: f64, above: &LBox, branch: &LBox, m: &Metrics) -> f64 {
    (offset + above.down + m.sep + branch.up).max(2.0 * m.arc)
}

fn bypass(kid: usize, boxes: &[LBox], m: &Metrics) -> LBox {
    let k = &boxes[kid];
    let reach = (k.up + m.sep).max(2.0 * m.arc);
    LBox { kind: Kind::Bypass, width: k.width + 4.0 * m.arc, up: reach, down: k.down, kids: vec![kid], text: String::new(), id: String::new(), reach }
}

fn add(boxes: &mut Vec<LBox>, b: LBox) -> usize {
    boxes.push(b);
    boxes.len() - 1
}

fn measure(model: &RailroadModel, root: usize, m: &Metrics, boxes: &mut Vec<LBox>) -> usize {
    let nodes = &model.nodes.0;
    fold(
        root,
        |k| nodes[k].parts().to_vec(),
        |k, kids: Vec<usize>| match &nodes[k] {
            RailroadNode::Terminal(value) => {
                let text = shown(value);
                let text = if !text.is_empty() && text.chars().all(|c| c == ' ') { "␣".repeat(text.len()) } else { text };
                add(boxes, leaf(Kind::Terminal, text, String::new(), style::MONO, m))
            }
            RailroadNode::NonTerminal(name) => add(boxes, leaf(Kind::NonTerminal, shown(name), name.clone(), 0, m)),
            RailroadNode::Special(text) => add(boxes, leaf(Kind::Special, format!("? {} ?", shown(text)), String::new(), style::ITALIC, m)),
            RailroadNode::Sequence(_) => {
                let mut width = (kids.len() as f64 - 1.0).max(0.0) * m.gap;
                let mut up: f64 = 0.0;
                let mut down: f64 = 0.0;
                for &kid in &kids {
                    width += boxes[kid].width;
                    up = up.max(boxes[kid].up);
                    down = down.max(boxes[kid].down);
                }
                add(boxes, LBox { kind: Kind::Sequence, width, up, down, kids, text: String::new(), id: String::new(), reach: 0.0 })
            }
            RailroadNode::Choice(_) => {
                let mut inner: f64 = 0.0;
                let mut offset = 0.0;
                for i in 0..kids.len() {
                    inner = inner.max(boxes[kids[i]].width);
                    if i > 0 {
                        offset = below(offset, &boxes[kids[i - 1]], &boxes[kids[i]], m);
                    }
                }
                let up = kids.first().map_or(0.0, |&k| boxes[k].up);
                let down = kids.last().map_or(0.0, |&k| offset + boxes[k].down);
                add(boxes, LBox { kind: Kind::Choice, width: inner + 4.0 * m.arc, up, down, kids, text: String::new(), id: String::new(), reach: 0.0 })
            }
            RailroadNode::Optional(_) => {
                let b = bypass(kids[0], boxes, m);
                add(boxes, b)
            }
            RailroadNode::Repetition(_, min, max) => {
                let (min, max) = (*min, *max);
                let kid = kids[0];
                if min == 1.0 && max == 1.0 {
                    return kid;
                }
                let text = count(min, max);
                let k = &boxes[kid];
                let reach = (k.down + m.sep).max(2.0 * m.arc);
                let label = if text.is_empty() { 0.0 } else { even(measure_text(&text, COUNT_SIZE, 0)) };
                let lp = LBox {
                    kind: Kind::Loop,
                    width: k.width.max(label) + 2.0 * m.arc,
                    up: k.up,
                    down: reach + if text.is_empty() { ARROW / 2.0 + 1.0 } else { COUNT_SIZE + 6.0 },
                    kids: vec![kid],
                    text,
                    id: String::new(),
                    reach,
                };
                let lp = add(boxes, lp);
                if min == 0.0 {
                    let b = bypass(lp, boxes, m);
                    add(boxes, b)
                } else {
                    lp
                }
            }
        },
    )
}

pub fn render_railroad(model: &RailroadModel, config: &Config, options: &RenderOptions) -> Rendered {
    tighten(options, |tight| draw(model, config, options, tight))
}

fn draw(model: &RailroadModel, config: &Config, options: &RenderOptions, tight: bool) -> Rendered {
    let base = 16.0;
    let font_size = option(config, "fontSize", 0.0, 200.0);
    let size = js::or(font_size, || js::round(base * 0.875));
    let head_size = js::or(font_size, || base);
    let padding = option(config, "padding", -1.0, 200.0);
    let line_height = js::round(size * 1.5);
    let m = Metrics {
        size,
        padding: if padding < 0.0 { 12.0 } else { padding },
        height: even(line_height + if padding < 0.0 { 9.0 } else { 2.0 * padding }),
        gap: option(config, "horizontalSeparation", 16.0, 500.0).min(if tight { 12.0 } else { f64::INFINITY }),
        sep: option(config, "verticalSeparation", 10.0, 500.0),
        arc: option(config, "arcRadius", 10.0, 200.0).min(if tight { 6.0 } else { f64::INFINITY }),
    };
    let show_markers = !matches!(config.get("railroad").and_then(|v| v.as_map()).and_then(|m| m.get("showMarkers")), Some(Value::Bool(false)));
    let radius = if show_markers { option(config, "markerRadius", 4.0, 50.0) } else { 0.0 };
    let stroke_width = option(config, "strokeWidth", 0.0, 20.0);
    let stroke = if stroke_width > 0.0 { format!(" stroke-width=\"{}\"", num(stroke_width)) } else { String::new() };
    let pad = 8.0;
    let arc = m.arc;

    let a = num(arc);
    let turn = |sweep: u8, dx: &str, dy: &str| format!("a{},{} 0 0 {} {}{},{}{}", a, a, sweep, dx, a, dy, a);
    let east_to_south = turn(1, "", "");
    let south_to_east = turn(0, "", "");
    let east_to_north = turn(0, "", "-");
    let north_to_east = turn(1, "", "-");
    let loop_down = turn(0, "-", "");
    let loop_up = turn(0, "-", "-");

    let title = layout_label(model.title.as_deref(), false, base, 4000.0, style::BOLD);
    let title_height = if title.height > 0.0 { title.height + 16.0 } else { 0.0 };
    let head_height = js::round(head_size * 1.5);
    let text_shift = size * 0.35;
    let box_top = m.height / 2.0;

    let mut rules = String::new();
    let mut width = title.width;
    let mut top = pad + title_height;
    let mut bottom = pad + title.height;
    for (rule_name, definition) in &model.rules {
        let mut boxes: Vec<LBox> = Vec::new();
        let root = measure(model, *definition, &m, &mut boxes);
        let name = shown(rule_name);
        let root_box = &boxes[root];
        let baseline = top + head_height + HEAD_GAP + root_box.up;
        let start = pad + 2.0 * radius;
        let left = start + LEAD;
        let end = left + root_box.width + LEAD;
        let mut track = String::new();
        let mut arrows = String::new();
        let mut nodes = String::new();
        let line = |track: &mut String, x1: f64, y: f64, x2: f64| {
            if x2 > x1 {
                track.push_str(&format!("M{},{}H{}", num(x1), num(y), num(x2)));
            }
        };
        line(&mut track, start, baseline, left);
        line(&mut track, left + root_box.width, baseline, end);

        // place(): boxes come off a stack, each with the left end of its baseline.
        let mut stack: Vec<(usize, f64, f64)> = vec![(root, left, baseline)];
        while let Some((bi, x, y)) = stack.pop() {
            let b = &boxes[bi];
            let kids = &b.kids;
            let right = x + b.width;
            match b.kind {
                Kind::Sequence => {
                    let mut kx = right;
                    for i in (0..kids.len()).rev() {
                        kx -= boxes[kids[i]].width;
                        stack.push((kids[i], kx, y));
                        if i > 0 {
                            line(&mut track, kx - m.gap, y, kx);
                        }
                        kx -= m.gap;
                    }
                }
                Kind::Choice => {
                    let inner = b.width - 4.0 * arc;
                    let rail_in = x + arc;
                    let rail_out = right - arc;
                    let from = stack.len();
                    let mut offset = 0.0;
                    for i in 0..kids.len() {
                        let kid = &boxes[kids[i]];
                        if i > 0 {
                            offset = below(offset, &boxes[kids[i - 1]], kid, &m);
                        }
                        let ky = y + offset;
                        let kx = x + 2.0 * arc + (inner - kid.width) / 2.0;
                        let last = i == kids.len() - 1;
                        if i == 0 {
                            line(&mut track, x, y, kx);
                            line(&mut track, kx + kid.width, y, right);
                        } else {
                            if last {
                                track.push_str(&format!("M{},{}{}V{}", num(x), num(y), east_to_south, num(ky - arc)));
                            } else {
                                track.push_str(&format!("M{},{}", num(rail_in), num(ky - arc)));
                            }
                            track.push_str(&south_to_east);
                            if kx > rail_in + arc {
                                track.push_str(&format!("H{}", num(kx)));
                            }
                            track.push_str(&format!("M{},{}", num(kx + kid.width), num(ky)));
                            if kx > rail_in + arc {
                                track.push_str(&format!("H{}", num(rail_out - arc)));
                            }
                            track.push_str(&east_to_north);
                            if last {
                                track.push_str(&format!("V{}{}", num(y + arc), north_to_east));
                            }
                        }
                        stack.push((kids[i], kx, ky));
                    }
                    // Reversed, so that the branches come off the stack top to bottom.
                    stack[from..].reverse();
                }
                Kind::Bypass => {
                    let kid = kids[0];
                    let kx = x + 2.0 * arc;
                    line(&mut track, x, y, kx);
                    line(&mut track, kx + boxes[kid].width, y, right);
                    track.push_str(&format!(
                        "M{},{}{}V{}{}H{}{}V{}{}",
                        num(x),
                        num(y),
                        east_to_north,
                        num(y - b.reach + arc),
                        north_to_east,
                        num(right - 2.0 * arc),
                        east_to_south,
                        num(y - arc),
                        south_to_east
                    ));
                    stack.push((kid, kx, y));
                }
                Kind::Loop => {
                    let kid = kids[0];
                    let kx = x + (b.width - boxes[kid].width) / 2.0;
                    let loop_y = y + b.reach;
                    let middle = x + b.width / 2.0;
                    line(&mut track, x, y, kx);
                    line(&mut track, kx + boxes[kid].width, y, right);
                    track.push_str(&format!("M{},{}{}V{}{}H{}{}V{}{}", num(x + arc), num(y), loop_down, num(loop_y - arc), south_to_east, num(right - arc), east_to_north, num(y + arc), loop_up));
                    arrows.push_str(&format!("M{},{}l{},{}v{}z", num(middle - ARROW / 2.0), num(loop_y), js::num_str(ARROW), js::num_str(-ARROW / 2.0), js::num_str(ARROW)));
                    if !b.text.is_empty() {
                        nodes.push_str(&format!(
                            "<text class=\"pele-edge-label pele-count\" x=\"{}\" y=\"{}\" font-size=\"{}\" fill=\"var(--_m)\">{}</text>",
                            num(middle),
                            num(loop_y + COUNT_SIZE + 3.0),
                            js::num_str(COUNT_SIZE),
                            esc(&b.text)
                        ));
                    }
                    stack.push((kid, kx, y));
                }
                _ => {
                    let text = &b.text;
                    let space = if text.starts_with(' ') || text.ends_with(' ') || text.contains("  ") { " xml:space=\"preserve\"" } else { "" };
                    let shape = format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"", num(x), num(y - box_top), num(b.width), num(m.height));
                    let label = format!("<text x=\"{}\" y=\"{}\"{}", num(x + b.width / 2.0), num(y + text_shift), space);
                    match b.kind {
                        Kind::Terminal => nodes.push_str(&format!(
                            "<g class=\"pele-node pele-terminal\">{} rx=\"{}\" fill=\"var(--_s)\" stroke=\"var(--_b)\"{}/>{} font-family=\"{}\">{}</text></g>",
                            shape,
                            num(box_top),
                            stroke,
                            label,
                            FONT_MONO,
                            esc(text)
                        )),
                        Kind::NonTerminal => nodes.push_str(&format!(
                            "<g class=\"pele-node pele-nonterminal\" data-id=\"{}\">{} fill=\"var(--_bg)\" stroke=\"var(--_b)\"{}{}/>{}>{}</text></g>",
                            esc_text(&b.id),
                            shape,
                            stroke,
                            ROUNDED,
                            label,
                            esc(text)
                        )),
                        _ => nodes.push_str(&format!(
                            "<g class=\"pele-node pele-special\">{} fill=\"var(--_bg)\" stroke=\"var(--_b)\" stroke-dasharray=\"4 3\"{}{}/>{} font-style=\"italic\"><tspan fill=\"var(--_m)\">? </tspan>{}<tspan fill=\"var(--_m)\"> ?</tspan></text></g>",
                            shape,
                            stroke,
                            ROUNDED,
                            label,
                            esc(&text[2..text.len() - 2])
                        )),
                    }
                }
            }
        }
        let markers = if radius > 0.0 {
            format!(
                "<circle class=\"pele-marker pele-start\" cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"var(--_l)\"/><circle class=\"pele-marker pele-end\" cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"var(--_l)\"/>",
                num(pad + radius),
                num(baseline),
                num(radius),
                num(end + radius),
                num(baseline),
                num(radius)
            )
        } else {
            String::new()
        };
        rules.push_str(&format!(
            "<g class=\"pele-rule\" data-id=\"{}\"><text class=\"pele-rule-name\" x=\"{}\" y=\"{}\" font-size=\"{}\" font-weight=\"var(--_hw)\" text-anchor=\"start\">{}</text><path class=\"pele-edge pele-track\" d=\"{}\" fill=\"none\" stroke=\"var(--_l)\"{}/>{}{}{}</g>",
            esc_text(rule_name),
            num(pad),
            num(top + head_height / 2.0 + head_size * 0.35),
            num(head_size),
            esc(&name),
            track,
            stroke,
            if arrows.is_empty() { String::new() } else { format!("<path class=\"pele-marker\" d=\"{}\" fill=\"var(--_l)\"/>", arrows) },
            markers,
            nodes
        ));
        width = width.max(end + 2.0 * radius - pad).max(measure_text(&name, head_size, style::BOLD));
        bottom = baseline + root_box.down;
        top = bottom + m.sep + RULE_GAP;
    }

    let total_width = (width + 2.0 * pad).ceil();
    let total_height = (bottom + pad).ceil();
    let mut content = label_svg(&title, total_width / 2.0, pad + title.height / 2.0, " class=\"pele-title\" font-weight=\"var(--_tw)\"");
    if !rules.is_empty() {
        content.push_str(&format!("<g class=\"pele-rules\" transform=\"translate(0.5,0.5)\" font-size=\"{}\" text-anchor=\"middle\">{}</g>", num(size), rules));
    }
    let acc = Accessible { acc_title: model.acc_title.as_deref(), acc_descr: model.acc_descr.as_deref() };
    Rendered { svg: svg_document("railroad", total_width, total_height, base, options.responsive, &acc, &content), width: total_width, height: total_height, links: Vec::new() }
}
