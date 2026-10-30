// Port of src/diagrams/sequence/render.ts, for the default render options the host uses: no
// width limit (so a diagram is never tightened), 16px text, no monospace messages, no icon or
// math resolver.

use super::actors::{actor_kind, actor_size, draw_actor, is_figure, ActorSize};
use super::db::SeqDb;
use super::types::{linetype as lt, placement, MessageText, SeqActor, SeqMessage};
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num};
use crate::svg::edges::{marker, ARROW};
use crate::svg::ink::ink;
use crate::svg::root::{svg_document, Accessible};
use crate::svg::theme::{class_names, resolve_style, rounded, ROUNDED};
use crate::svg::title::title_label;
use crate::text::label::{layout_label, Label};
use crate::text::measurer::{style as st, width as measure};
use crate::types::{RenderOptions, Rendered};
use crate::util::yaml::{Map, Value};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Head {
    None,
    Arrow,
    Cross,
    Open,
    HalfSolid,
    HalfStick,
}

#[derive(Clone, Copy, Debug)]
struct LineStyle {
    dotted: bool,
    head: Head,
    // Where the heads are: at the receiver, at the sender only (the reverse arrows), or at both.
    at_start: bool,
    at_end: bool,
    bottom: bool,
}

fn line_style(ty: u32) -> Option<LineStyle> {
    let s = |dotted: bool, head: Head, ends: u8, bottom: bool| Some(LineStyle { dotted, head, at_start: ends != 1, at_end: ends != 0, bottom });
    match ty {
        lt::SOLID => s(false, Head::Arrow, 1, false),
        lt::DOTTED => s(true, Head::Arrow, 1, false),
        lt::SOLID_CROSS => s(false, Head::Cross, 1, false),
        lt::DOTTED_CROSS => s(true, Head::Cross, 1, false),
        lt::SOLID_OPEN => s(false, Head::None, 1, false),
        lt::DOTTED_OPEN => s(true, Head::None, 1, false),
        lt::SOLID_POINT => s(false, Head::Open, 1, false),
        lt::DOTTED_POINT => s(true, Head::Open, 1, false),
        lt::BIDIRECTIONAL_SOLID => s(false, Head::Arrow, 2, false),
        lt::BIDIRECTIONAL_DOTTED => s(true, Head::Arrow, 2, false),
        41..=48 | 51..=58 => {
            let dotted = ty > 50;
            match ty - if dotted { 10 } else { 0 } {
                lt::SOLID_TOP => s(dotted, Head::HalfSolid, 1, false),
                lt::SOLID_BOTTOM => s(dotted, Head::HalfSolid, 1, true),
                lt::STICK_TOP => s(dotted, Head::HalfStick, 1, false),
                lt::STICK_BOTTOM => s(dotted, Head::HalfStick, 1, true),
                lt::SOLID_ARROW_TOP_REVERSE => s(dotted, Head::HalfSolid, 0, false),
                lt::SOLID_ARROW_BOTTOM_REVERSE => s(dotted, Head::HalfSolid, 0, true),
                lt::STICK_ARROW_TOP_REVERSE => s(dotted, Head::HalfStick, 0, false),
                _ => s(dotted, Head::HalfStick, 0, true),
            }
        }
        _ => None,
    }
}

fn frame_kind(ty: u32) -> Option<&'static str> {
    Some(match ty {
        lt::LOOP_START => "loop",
        lt::ALT_START => "alt",
        lt::OPT_START => "opt",
        lt::PAR_START | lt::PAR_OVER_START => "par",
        lt::CRITICAL_START => "critical",
        lt::BREAK_START => "break",
        lt::RECT_START => "rect",
        _ => return None,
    })
}

const MSG_PAD: f64 = 12.0;
const SELF_W: f64 = 28.0;
const SELF_H: f64 = 22.0;
const ACT_W: f64 = 10.0;
const ACT_STEP: f64 = 4.0;
const ACT_MIN: f64 = 14.0;
const NOTE_GAP: f64 = 10.0;
const NOTE_OVER: f64 = 16.0;
const FRAME_PAD: f64 = 20.0;
const FRAME_NEST: f64 = 10.0;
const TAB_H: f64 = 20.0;
const FRAME_TEXT: f64 = 240.0;
const BADGE_H: f64 = 16.0;
const CENTRAL_R: f64 = 4.0;
const DASH: &str = " stroke-dasharray=\"4 4\"";

pub struct Numbering {
    /// The number each message line carries, whether or not it is shown. NaN for everything else.
    pub index: Vec<f64>,
    pub shown: Vec<bool>,
    /// Whether numbers are on after the last statement.
    pub visible: bool,
}

pub fn is_line_message(ty: Option<u32>) -> bool {
    ty.is_some_and(|t| line_style(t).is_some())
}

/// Counts message lines the way Mermaid's renderer does while it draws.
pub fn number_messages(messages: &[SeqMessage], always: bool) -> Numbering {
    let mut index = vec![f64::NAN; messages.len()];
    let mut shown = vec![false; messages.len()];
    let mut current = 1.0;
    let mut step = 1.0;
    let mut visible = false;
    for (i, m) in messages.iter().enumerate() {
        if m.type_ == Some(lt::AUTONUMBER) {
            if let MessageText::Numbering(spec) = &m.message {
                current = spec.start.filter(|v| *v != 0.0 && !v.is_nan()).unwrap_or(current);
                step = spec.step.filter(|v| *v != 0.0 && !v.is_nan()).unwrap_or(step);
                visible = spec.visible;
            }
        } else if is_line_message(m.type_) {
            index[i] = current;
            shown[i] = visible || always;
            current = js::round((current + step) * 100.0) / 100.0;
        }
    }
    Numbering { index, shown, visible }
}

struct Column<'a> {
    id: String,
    actor: &'a SeqActor,
    kind: &'static str,
    label: Label,
    size: ActorSize,
    x: f64,
    create_at: i64,
    destroy_at: i64,
    top: f64,
    life_start: f64,
    life_end: f64,
    activations: Vec<f64>,
}

#[derive(Clone, Copy, PartialEq)]
enum V {
    Line,
    SelfMsg,
    Note,
}

struct View {
    kind: V,
    a: usize,
    b: usize,
    label: Label,
    x: f64,
    w: f64,
}

#[derive(Clone)]
struct Frame {
    kind: &'static str,
    x0: f64,
    x1: f64,
    top: f64,
    title: Label,
    tab_w: f64,
    fill: String,
    dividers: Vec<usize>,
    divider_y: Vec<f64>,
    divider_labels: Vec<Label>,
}

struct BoxRun {
    box_: usize,
    first: usize,
    last: usize,
    title: Label,
    grow: f64,
}

fn option(conf: Option<&Map>, key: &str, fallback: f64) -> f64 {
    match conf.and_then(|c| c.get(key)) {
        Some(Value::Num(v)) if *v >= 0.0 && *v <= 2000.0 => *v,
        _ => fallback,
    }
}

fn half(x: f64, y: f64, dir: f64, up: f64, stick: bool, color: &str) -> String {
    let bx = num(x - dir * ARROW);
    if stick {
        return format!("<path class=\"pele-marker\" d=\"M{},{}L{},{}\"/>", num(x), num(y), bx, num(y + up * 5.0));
    }
    format!("<path class=\"pele-marker\" d=\"M{},{}L{},{}V{}Z\" fill=\"{}\" stroke=\"none\"/>", num(x), num(y), bx, num(y + up * 4.5), num(y), color)
}

// Draws the head whose tip is at (x, y) and points along dir. `up` is the side a half head sits on.
fn head(ty: Head, x: f64, y: f64, dir: f64, up: f64, color: &str) -> String {
    match ty {
        Head::Arrow => marker("arrow_point", x, y, dir, 0.0, color, "none"),
        Head::Cross => marker("arrow_cross", x, y, dir, 0.0, color, "none"),
        Head::Open => {
            format!("<path class=\"pele-marker\" d=\"M{},{}L{},{}L{},{}\" stroke-linejoin=\"round\"/>", num(x - dir * ARROW), num(y - 4.5), num(x), num(y), num(x - dir * ARROW), num(y + 4.5))
        }
        Head::HalfSolid => half(x, y, dir, up, false, color),
        Head::HalfStick => half(x, y, dir, up, true, color),
        Head::None => String::new(),
    }
}

fn trim(ty: Head) -> f64 {
    match ty {
        Head::Arrow => ARROW - 1.0,
        Head::Cross => 4.0,
        _ => 0.0,
    }
}

pub fn render_sequence(db: &SeqDb, config: &Config, options: &RenderOptions) -> Rendered {
    // With no width limit, tighten draws once, loose.
    draw(db, config, options)
}

fn draw(db: &SeqDb, config: &Config, options: &RenderOptions) -> Rendered {
    let conf = config.get("sequence").and_then(|v| v.as_map());
    let flag = |key: &str| conf.and_then(|c| c.get(key)).cloned();
    let size = 16.0;
    let small = js::round(size * 0.875);
    let tiny = js::round(size * 0.75);
    let pad = 8.0;
    let mirror = !matches!(flag("mirrorActors"), Some(Value::Bool(false)));
    let right_angles = matches!(flag("rightAngles"), Some(Value::Bool(true)));
    let actor_margin = option(conf, "actorMargin", 40.0);
    let min_w = option(conf, "width", 64.0);
    let min_h = option(conf, "height", js::round(size * 1.5) + 20.0);
    let wrap_w = option(conf, "width", 160.0);
    // Mermaid's default of 35 corresponds to Pele's default gap. Other values move it by the difference.
    let msg_gap = (14.0 + option(conf, "messageMargin", 35.0) - 35.0).max(4.0);
    let note_pad = option(conf, "noteMargin", 10.0);
    let box_pad = option(conf, "boxMargin", 10.0);
    let align: i32 = match flag("messageAlign") {
        Some(Value::Str(s)) if s == "left" => -1,
        Some(Value::Str(s)) if s == "right" => 1,
        _ => 0,
    };

    let msg_pad = MSG_PAD;
    let note_over = NOTE_OVER;
    let frame_pad = FRAME_PAD;
    let frame_nest = FRAME_NEST;
    let messages = &db.messages;
    let count = messages.len();
    let numbering = number_messages(messages, matches!(flag("showSequenceNumbers"), Some(Value::Bool(true))));
    let text = |raw: Option<&str>, font_size: f64, wrap: bool, width: f64| layout_label(raw, false, font_size, if wrap { width } else { f64::INFINITY }, 0);
    let message_text = |raw: &str, wrap: bool, width: f64| text(Some(raw), small, wrap, width);
    let msg_str = |m: &SeqMessage| -> Option<String> {
        match &m.message {
            MessageText::Text(s) => Some(s.clone()),
            _ => None,
        }
    };

    let used: Option<Vec<Option<String>>> = if matches!(flag("hideUnusedParticipants"), Some(Value::Bool(true))) {
        let mut u = Vec::new();
        for m in messages {
            u.push(m.from.clone());
            u.push(m.to.clone());
        }
        Some(u)
    } else {
        None
    };

    let mut next_line = vec![-1i64; count + 1];
    for i in (0..count).rev() {
        next_line[i] = if is_line_message(messages[i].type_) { i as i64 } else { next_line[i + 1] };
    }

    let mut cols: Vec<Column> = Vec::new();
    let mut col_of: HashMap<String, usize> = HashMap::new();
    for (id, actor) in &db.actors {
        if let Some(u) = &used {
            if !u.iter().any(|x| x.as_deref() == Some(id.as_str())) {
                continue;
            }
        }
        let kind = actor_kind(&actor.type_);
        let label = text(Some(&actor.description), size, actor.wrap, wrap_w);
        let mut create_at = db.created_actors.get(id).map_or(-1, |&c| next_line[c.min(count)]);
        if create_at >= 0 && messages[create_at as usize].to.as_deref() != Some(id.as_str()) {
            create_at = -1;
        }
        let mut destroy_at = db.destroyed_actors.get(id).map_or(-1, |&d| next_line[d.min(count)]);
        if destroy_at >= 0 && messages[destroy_at as usize].to.as_deref() != Some(id.as_str()) && messages[destroy_at as usize].from.as_deref() != Some(id.as_str()) {
            destroy_at = -1;
        }
        col_of.insert(id.clone(), cols.len());
        let sz = actor_size(kind, &label, min_w, min_h);
        cols.push(Column { id: id.clone(), actor, kind, label, size: sz, x: 0.0, create_at, destroy_at, top: 0.0, life_start: 0.0, life_end: -1.0, activations: Vec::new() });
    }
    let n_cols = cols.len();
    let last: i64 = n_cols as i64 - 1;

    // Widths between lifelines. Every message and note says how far apart the lifelines it touches must be.
    // Map<number, number>, in insertion order.
    let mut needs: Vec<(i64, f64)> = Vec::new();
    let mut need = |lo: i64, hi: i64, width: f64| {
        if lo < 0 || hi > last {
            return;
        }
        let key = lo * n_cols as i64 + hi;
        match needs.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => {
                if slot.1 < width {
                    slot.1 = width;
                }
            }
            None => needs.push((key, width)),
        }
    };
    let badge_width = |i: usize| if numbering.shown[i] { BADGE_H.max(measure(&js::num_str(numbering.index[i]), tiny, 0) + 8.0) } else { 0.0 };

    let mut views: Vec<Option<View>> = (0..count).map(|_| None).collect();
    let mut sized: Vec<Option<Vec<(f64, Label)>>> = (0..count).map(|_| None).collect();
    for i in 0..count {
        let m = &messages[i];
        let a = m.from.as_ref().and_then(|f| col_of.get(f).copied());
        let b = m.to.as_ref().and_then(|t| col_of.get(t).copied());
        let (Some(a), Some(b), Some(msg)) = (a, b, msg_str(m)) else { continue };
        let (ai, bi) = (a as i64, b as i64);
        if m.type_ == Some(lt::NOTE) {
            let label = text(Some(&msg), small, m.wrap, wrap_w);
            let w = label.width + 2.0 * note_pad;
            if m.placement == Some(placement::LEFTOF) {
                need(ai - 1, ai, w + 2.0 * NOTE_GAP + ACT_W);
            } else if m.placement == Some(placement::RIGHTOF) {
                need(ai, ai + 1, w + 2.0 * NOTE_GAP + ACT_W);
            } else if a == b {
                need(ai - 1, ai, w / 2.0 + NOTE_GAP);
                need(ai, ai + 1, w / 2.0 + NOTE_GAP);
            } else {
                need(ai.min(bi), ai.max(bi), w - 2.0 * note_over);
            }
            views[i] = Some(View { kind: V::Note, a, b, label, x: 0.0, w });
        } else if is_line_message(m.type_) {
            let label = message_text(&msg, m.wrap, wrap_w);
            let badge = badge_width(i);
            if a == b {
                need(ai, ai + 1, SELF_W + 8.0 + label.width + 16.0 + ACT_W + badge);
                views[i] = Some(View { kind: V::SelfMsg, a, b, label, x: 0.0, w: 0.0 });
            } else {
                let created = if cols[b].create_at == i as i64 { cols[b].size.w / 2.0 } else { 0.0 };
                let central = if m.central_connection != 0 { 4.0 * CENTRAL_R } else { 0.0 };
                need(ai.min(bi), ai.max(bi), label.width + 2.0 * msg_pad + ACT_W + badge + created + central);
                views[i] = Some(View { kind: V::Line, a, b, label, x: 0.0, w: 0.0 });
            }
        }
    }

    let mut runs: Vec<BoxRun> = Vec::new();
    for i in 0..n_cols {
        let Some(bx) = cols[i].actor.box_ else { continue };
        match runs.last_mut() {
            Some(run) if run.box_ == bx && run.last + 1 == i => run.last = i,
            _ => {
                let b = &db.boxes[bx];
                runs.push(BoxRun { box_: bx, first: i, last: i, title: text(b.name.as_deref(), small, b.wrap, wrap_w), grow: 0.0 });
            }
        }
    }

    let mut gaps: Vec<f64> = vec![0.0; n_cols.saturating_sub(1)];
    let needs_get = |needs: &[(i64, f64)], key: i64| needs.iter().find(|(k, _)| *k == key).map_or(0.0, |s| s.1);
    for i in 0..gaps.len() {
        let (left, right) = (&cols[i], &cols[i + 1]);
        let boxed = if left.actor.box_ != right.actor.box_ { (if left.actor.box_.is_some() { box_pad } else { 0.0 }) + (if right.actor.box_.is_some() { box_pad } else { 0.0 }) } else { 0.0 };
        gaps[i] = (left.size.w / 2.0 + right.size.w / 2.0 + actor_margin + boxed).max(needs_get(&needs, i as i64 * n_cols as i64 + i as i64 + 1));
    }
    // A message across several lifelines widens each gap it crosses by an equal share of what is missing.
    let mut spans: Vec<(usize, usize, f64)> = Vec::new();
    for &(key, width) in &needs {
        let lo = (key / n_cols as i64) as usize;
        let hi = (key % n_cols as i64) as usize;
        if hi > lo + 1 {
            spans.push((lo, hi, width));
        }
    }
    spans.sort_by(|p, q| js::ord(js::or((p.1 - p.0) as f64 - (q.1 - q.0) as f64, || p.0 as f64 - q.0 as f64)));
    for &(lo, hi, width) in &spans {
        let total: f64 = gaps[lo..hi].iter().sum();
        if total >= width {
            continue;
        }
        let share = (width - total) / (hi - lo) as f64;
        for g in gaps[lo..hi].iter_mut() {
            *g += share;
        }
    }
    let place = |cols: &mut Vec<Column>, gaps: &[f64]| {
        for i in 1..cols.len() {
            cols[i].x = cols[i - 1].x + gaps[i - 1];
        }
    };
    place(&mut cols, &gaps);
    for run in runs.iter_mut() {
        let inner = cols[run.last].x + cols[run.last].size.w / 2.0 - (cols[run.first].x - cols[run.first].size.w / 2.0);
        run.grow = ((run.title.width - inner) / 2.0).max(0.0);
        if run.grow == 0.0 {
            continue;
        }
        if run.first > 0 {
            gaps[run.first - 1] += run.grow;
        }
        if (run.last as i64) < last {
            gaps[run.last] += run.grow;
        }
    }

    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    fn extend(min_x: &mut f64, max_x: &mut f64, x0: f64, x1: f64) {
        if x0 < *min_x {
            *min_x = x0;
        }
        if x1 > *max_x {
            *max_x = x1;
        }
    }
    let box_edges =
        |cols: &[Column], run: &BoxRun| -> (f64, f64) { (cols[run.first].x - cols[run.first].size.w / 2.0 - box_pad - run.grow, cols[run.last].x + cols[run.last].size.w / 2.0 + box_pad + run.grow) };
    let mut frames: Vec<Option<usize>> = vec![None; count];
    let mut frame_store: Vec<Frame> = Vec::new();

    // layout()
    place(&mut cols, &gaps);
    for col in &cols {
        extend(&mut min_x, &mut max_x, col.x - col.size.w / 2.0, col.x + col.size.w / 2.0);
    }
    for run in &runs {
        let (a, b) = box_edges(&cols, run);
        extend(&mut min_x, &mut max_x, a, b);
    }
    {
        // Horizontal extents of every block, from what it holds.
        let mut open: Vec<usize> = Vec::new();
        let span_all = |cols: &[Column]| if last < 0 { (0.0, 120.0) } else { (cols[0].x - frame_pad, cols[last as usize].x + frame_pad) };
        for i in 0..count {
            let m = &messages[i];
            let ty = m.type_;
            let fk = ty.and_then(frame_kind);
            if let Some(kind) = fk {
                frame_store.push(Frame {
                    kind,
                    x0: f64::INFINITY,
                    x1: f64::NEG_INFINITY,
                    top: 0.0,
                    title: Label::default(),
                    tab_w: 0.0,
                    fill: if kind == "rect" { msg_str(m).unwrap_or_default() } else { String::new() },
                    dividers: vec![i],
                    divider_y: Vec::new(),
                    divider_labels: Vec::new(),
                });
                frames[i] = Some(frame_store.len() - 1);
                open.push(frame_store.len() - 1);
            } else if matches!(ty, Some(lt::ALT_ELSE) | Some(lt::PAR_AND) | Some(lt::CRITICAL_OPTION)) {
                if let Some(&f) = open.last() {
                    frame_store[f].dividers.push(i);
                }
            } else if matches!(ty, Some(lt::LOOP_END) | Some(lt::ALT_END) | Some(lt::OPT_END) | Some(lt::PAR_END) | Some(lt::CRITICAL_END) | Some(lt::BREAK_END) | Some(lt::RECT_END)) {
                let Some(f) = open.pop() else { continue };
                frames[i] = Some(f);
                let frame = &mut frame_store[f];
                if frame.x0 > frame.x1 {
                    let (a, b) = span_all(&cols);
                    frame.x0 = a;
                    frame.x1 = b;
                }
                if frame.kind != "rect" {
                    frame.tab_w = measure(frame.kind, tiny, st::BOLD) + 14.0;
                    // A block grows to the right to give its texts up to FRAME_TEXT of width before they wrap.
                    for k in 0..frame.dividers.len() {
                        let raw = msg_str(&messages[frame.dividers[k]]).unwrap_or_default();
                        let tab = if k == 0 { frame.tab_w + 8.0 } else { 0.0 };
                        let room = frame.x1 - frame.x0 - 16.0 - tab;
                        let label = if !raw.is_empty() { text(Some(&format!("[{}]", raw)), small, true, room.max(FRAME_TEXT)) } else { Label::default() };
                        if label.width > room {
                            frame.x1 += label.width - room;
                        }
                        if k == 0 {
                            frame.title = label;
                        } else {
                            frame.divider_labels.push(label);
                        }
                    }
                }
                let (fx0, fx1) = (frame.x0, frame.x1);
                extend(&mut min_x, &mut max_x, fx0, fx1);
                if let Some(&p) = open.last() {
                    let parent = &mut frame_store[p];
                    parent.x0 = parent.x0.min(fx0 - frame_nest);
                    parent.x1 = parent.x1.max(fx1 + frame_nest);
                }
            } else if let Some(view) = views[i].as_mut() {
                let (a, b) = (&cols[view.a], &cols[view.b]);
                let (x0, x1);
                if view.kind == V::Note {
                    if m.placement == Some(placement::LEFTOF) {
                        view.x = a.x - NOTE_GAP - view.w;
                    } else if m.placement == Some(placement::RIGHTOF) {
                        view.x = a.x + NOTE_GAP;
                    } else if view.a == view.b {
                        view.x = a.x - view.w / 2.0;
                    } else {
                        let lo = a.x.min(b.x) - note_over;
                        let hi = a.x.max(b.x) + note_over;
                        if m.wrap {
                            view.label = text(msg_str(m).as_deref(), small, true, wrap_w.max(hi - lo - 2.0 * note_pad));
                        }
                        view.w = (hi - lo).max(view.label.width + 2.0 * note_pad);
                        view.x = (lo + hi - view.w) / 2.0;
                    }
                    x0 = view.x;
                    x1 = view.x + view.w;
                } else if view.kind == V::SelfMsg {
                    x0 = a.x;
                    x1 = a.x + ACT_W + SELF_W + 8.0 + view.label.width + badge_width(i);
                } else {
                    let mut lo = a.x.min(b.x);
                    let mut hi = a.x.max(b.x);
                    if m.wrap {
                        let room = wrap_w.max(hi - lo - 2.0 * msg_pad - ACT_W);
                        if room != wrap_w {
                            let known = sized[i].get_or_insert_with(|| vec![(wrap_w, view.label.clone())]);
                            view.label = match known.iter().find(|(r, _)| *r == room) {
                                Some((_, l)) => l.clone(),
                                None => {
                                    let l = message_text(&msg_str(m).unwrap_or_default(), true, room);
                                    known.push((room, l.clone()));
                                    l
                                }
                            };
                        } else if let Some(known) = &sized[i] {
                            view.label = known.iter().find(|(r, _)| *r == room).map(|(_, l)| l.clone()).unwrap_or_else(|| view.label.clone());
                        }
                    }
                    if b.create_at == i as i64 {
                        if b.x > a.x {
                            hi += b.size.w / 2.0;
                        } else {
                            lo -= b.size.w / 2.0;
                        }
                    }
                    x0 = lo;
                    x1 = hi;
                }
                extend(&mut min_x, &mut max_x, x0, x1);
                if let Some(&f) = open.last() {
                    let frame = &mut frame_store[f];
                    frame.x0 = frame.x0.min(x0 - frame_pad);
                    frame.x1 = frame.x1.max(x1 + frame_pad);
                }
            }
        }
    }

    // Vertical placement, top to bottom, drawing as it goes.
    let mut box_title_h: f64 = 0.0;
    for run in &runs {
        box_title_h = box_title_h.max(run.title.height);
    }
    let box_top = if !runs.is_empty() { box_pad + if box_title_h > 0.0 { box_title_h + 4.0 } else { 0.0 } } else { 0.0 };
    let mut row_h: f64 = 0.0;
    for col in &cols {
        if col.create_at < 0 {
            row_h = row_h.max(col.size.h);
        }
    }
    let head_bottom = box_top + row_h;
    for col in cols.iter_mut() {
        col.top = head_bottom - col.size.h;
        col.life_start = head_bottom;
    }

    let xs: Vec<f64> = cols.iter().map(|c| c.x).collect();
    // Whether a lifeline runs between two x positions, where text would be struck through.
    let crosses = |x0: f64, x1: f64| -> bool {
        let (mut lo, mut hi) = (0usize, xs.len());
        while lo < hi {
            let mid = (lo + hi) >> 1;
            if xs[mid] <= x0 {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo < xs.len() && xs[lo] < x1
    };
    let mut stack: Vec<usize> = Vec::new();
    let ink_of = |fill: Option<&str>| -> Option<&'static str> {
        let fill = fill.filter(|f| !f.is_empty())?;
        ink(&resolve_style(&[format!("fill:{}", fill)]).shape, "")
    };
    let ink_on = |fill: Option<&str>| -> String { ink_of(fill).map_or(String::new(), |c| format!(" fill=\"{}\"", c)) };
    // Prefer the innermost rect's fill, then a containing box's.
    let fill_under = |stack: &[usize], frame_store: &[Frame], cols: &[Column], l: f64, r: f64| -> Option<String> {
        for &k in stack.iter().rev() {
            if frame_store[k].kind == "rect" {
                return Some(frame_store[k].fill.clone());
            }
        }
        for run in &runs {
            let (x0, x1) = box_edges(cols, run);
            if l >= x0 && r <= x1 {
                return Some(db.boxes[run.box_].fill.clone());
            }
        }
        None
    };
    let left_edge = |col: &Column, extra: usize| if col.activations.len() + extra > 0 { col.x - ACT_W / 2.0 } else { col.x };
    let right_edge = |col: &Column, extra: usize| {
        let depth = col.activations.len() + extra;
        if depth > 0 {
            col.x + ACT_W / 2.0 + (depth - 1) as f64 * ACT_STEP
        } else {
            col.x
        }
    };

    // Shaded blocks and activations close from the inside out. They are written in reverse, so
    // that the inner ones end up on top.
    let mut rects_out: Vec<String> = Vec::new();
    let mut activations_out: Vec<String> = Vec::new();
    let mut frames_out = String::new();
    let mut edges_out = String::new();
    let mut labels_out = String::new();
    let mut notes_out = String::new();
    let mut actors_out = String::new();
    let mut crosses_out = String::new();
    let end_activation = |col: &mut Column, end: f64, min_x: &mut f64, max_x: &mut f64, activations_out: &mut Vec<String>| -> f64 {
        let start = col.activations.pop().unwrap();
        let bottom = end.max(start + ACT_MIN);
        let x = col.x - ACT_W / 2.0 + col.activations.len() as f64 * ACT_STEP;
        extend(min_x, max_x, x, x + ACT_W);
        activations_out.push(format!(
            "<rect class=\"pele-activation\" data-id=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"2\"/>",
            esc_text(&col.id),
            num(x),
            num(start),
            js::num_str(ACT_W),
            num(bottom - start)
        ));
        bottom
    };
    // The lifeline of a destroyed participant ends in a cross a little below the message that ends it.
    let destroy = |col: &mut Column, line_y: f64, crosses_out: &mut String| -> f64 {
        let cx = col.x;
        let cy = line_y + 16.0;
        col.life_end = cy;
        crosses_out.push_str(&format!(
            "<path class=\"pele-destroy\" data-id=\"{}\" d=\"M{},{}L{},{}M{},{}L{},{}\"/>",
            esc_text(&col.id),
            num(cx - 6.0),
            num(cy - 6.0),
            num(cx + 6.0),
            num(cy + 6.0),
            num(cx + 6.0),
            num(cy - 6.0),
            num(cx - 6.0),
            num(cy + 6.0)
        ));
        cy + 6.0
    };
    let actor_svg = |col: &Column, top: f64, footer: bool| -> String {
        let cls = match &col.actor.class {
            Some(Value::Str(s)) => class_names(s),
            _ => String::new(),
        };
        let fill = col.actor.box_.map(|b| db.boxes[b].fill.as_str());
        format!(
            "<g class=\"pele-node pele-actor pele-actor-{}{}{}\" data-id=\"{}\">{}</g>",
            col.kind,
            if footer { " pele-actor-footer" } else { "" },
            cls,
            esc_text(&col.id),
            draw_actor(col.kind, col.x, top, col.size, &col.label, &if is_figure(col.kind) { ink_on(fill) } else { String::new() })
        )
    };
    let badge = |i: usize, cx: f64, cy: f64, w: f64| -> String {
        format!(
            "<g class=\"pele-sequence-number\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\" fill=\"var(--_l)\" stroke=\"none\"/><text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-size=\"{}\" fill=\"var(--_bg)\" stroke=\"none\">{}</text></g>",
            num(cx - w / 2.0),
            num(cy - BADGE_H / 2.0),
            num(w),
            js::num_str(BADGE_H),
            js::num_str(BADGE_H / 2.0),
            num(cx),
            num(cy + tiny * 0.35),
            js::num_str(tiny),
            esc(&js::num_str(numbering.index[i]))
        )
    };

    let mut y = head_bottom + 6.0;
    for i in 0..count {
        let m = &messages[i];
        let ty = m.type_;
        let frame = frames[i];

        if ty == Some(lt::ACTIVE_START) {
            if let Some(&c) = m.from.as_ref().and_then(|f| col_of.get(f)) {
                cols[c].activations.push(y);
            }
        } else if ty == Some(lt::ACTIVE_END) {
            if let Some(&c) = m.from.as_ref().and_then(|f| col_of.get(f)) {
                if !cols[c].activations.is_empty() {
                    y = y.max(end_activation(&mut cols[c], y, &mut min_x, &mut max_x, &mut activations_out));
                }
            }
        } else if let Some(f) = frame.filter(|&f| frame_store[f].dividers[0] == i) {
            stack.push(f);
            let fr = &mut frame_store[f];
            if fr.kind == "rect" {
                fr.top = y + 8.0;
                y = fr.top;
            } else {
                fr.top = y + 12.0;
                y = fr.top + TAB_H.max(fr.title.height + 2.0);
            }
        } else if let Some(f) = frame {
            stack.pop();
            let bottom = y + 12.0;
            let fr = frame_store[f].clone();
            let x = num(fr.x0);
            let w = num(fr.x1 - fr.x0);
            if fr.kind == "rect" {
                let fill = if !fr.fill.is_empty() { resolve_style(&[format!("fill:{}", fr.fill)]).shape } else { String::new() };
                rects_out.push(format!(
                    "<rect class=\"pele-rect\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}/>",
                    x,
                    num(fr.top),
                    w,
                    num(bottom - fr.top),
                    rounded(&format!("{}{}", if fill.is_empty() { " fill-opacity=\"0.5\"" } else { "" }, fill))
                ));
            } else {
                let backed = |label: &Label, cx: f64, cy: f64| -> String { backed_label(label, cx, cy, true, &crosses, &|l, r| ink_on(fill_under(&stack, &frame_store, &cols, l, r).as_deref())) };
                let mut inner = format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\"{}/>", x, num(fr.top), w, num(bottom - fr.top), ROUNDED);
                for k in 0..fr.divider_y.len() {
                    let dy = fr.divider_y[k];
                    let label = &fr.divider_labels[k];
                    inner.push_str(&format!("<path d=\"M{},{}H{}\"{}/>", x, num(dy), num(fr.x1), DASH));
                    inner.push_str(&backed(label, fr.x0 + 8.0 + label.width / 2.0, dy + 2.0 + label.height / 2.0));
                }
                inner.push_str(&format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_s)\"{}/>", x, num(fr.top), num(fr.tab_w), js::num_str(TAB_H), ROUNDED));
                inner.push_str(&format!(
                    "<text class=\"pele-frame-kind\" x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-size=\"{}\" font-weight=\"var(--_hw)\" fill=\"var(--_m)\" stroke=\"none\">{}</text>",
                    num(fr.x0 + fr.tab_w / 2.0),
                    num(fr.top + TAB_H / 2.0 + tiny * 0.35),
                    js::num_str(tiny),
                    fr.kind
                ));
                inner.push_str(&backed(&fr.title, fr.x0 + fr.tab_w + 8.0 + fr.title.width / 2.0, fr.top + TAB_H.max(fr.title.height) / 2.0));
                frames_out.push_str(&format!("<g class=\"pele-frame pele-frame-{}\">{}</g>", fr.kind, inner));
            }
            y = bottom;
        } else if matches!(ty, Some(lt::ALT_ELSE) | Some(lt::PAR_AND) | Some(lt::CRITICAL_OPTION)) {
            if let Some(&cur) = stack.last() {
                let fr = &mut frame_store[cur];
                let label_h = fr.divider_labels.get(fr.divider_y.len()).map_or(0.0, |l| l.height);
                fr.divider_y.push(y + 12.0);
                y += 12.0 + if label_h > 0.0 { label_h } else { 0.0 };
            }
        } else if let Some(view) = views[i].as_ref().filter(|v| v.kind == V::Note) {
            let a = &cols[view.a];
            let h = view.label.height + 2.0 * (note_pad - 4.0).max(4.0);
            let top = y + 10.0;
            let mut x = view.x;
            if m.placement == Some(placement::RIGHTOF) {
                x += right_edge(a, 0) - a.x;
            } else if m.placement == Some(placement::LEFTOF) {
                x += left_edge(a, 0) - a.x;
            }
            extend(&mut min_x, &mut max_x, x, x + view.w);
            notes_out.push_str(&format!(
                "<g class=\"pele-note\" data-id=\"i{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}/>{}</g>",
                esc_text(&m.id),
                num(x),
                num(top),
                num(view.w),
                num(h),
                ROUNDED,
                label_svg(&view.label, x + view.w / 2.0, top + h / 2.0, " class=\"pele-label\" stroke=\"none\" fill=\"var(--_fg)\"")
            ));
            y = top + h;
        } else if let Some(view) = views[i].as_ref() {
            let sty = line_style(ty.unwrap()).unwrap();
            let (ai, bi) = (view.a, view.b);
            let label = view.label.clone();
            let bw = badge_width(i);
            // The activation a `+` starts belongs to this message, so the arrow stops at its edge.
            let mut starting = 0usize;
            for k in i + 1..count {
                let t = messages[k].type_;
                if t == Some(lt::ACTIVE_START) {
                    if messages[k].from == m.to {
                        starting += 1;
                    }
                } else if t != Some(lt::ACTIVE_END) && t != Some(lt::CENTRAL_CONNECTION) && t != Some(lt::CENTRAL_CONNECTION_REVERSE) {
                    break;
                }
            }
            let up = if sty.bottom { 1.0 } else { -1.0 };
            let attrs = if sty.dotted { DASH } else { "" };
            let cc = m.central_connection;
            let circle_to = cc == lt::CENTRAL_CONNECTION || cc == lt::CENTRAL_CONNECTION_DUAL;
            let circle_from = cc == lt::CENTRAL_CONNECTION_REVERSE || cc == lt::CENTRAL_CONNECTION_DUAL;
            let line: Option<&'static str>;
            let mut color = "var(--_l)";
            let circle =
                |cx: f64, cy: f64, color: &str| format!("<circle class=\"pele-central\" cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{}\" stroke=\"none\"/>", num(cx), num(cy), js::num_str(CENTRAL_R), color);
            let tail_at_end = sty.at_start && !sty.at_end;
            let head_trim = trim(sty.head);
            let mut inner = String::new();
            let line_y: f64;

            if view.kind == V::SelfMsg {
                let a = &cols[ai];
                let top = y + msg_gap + ((label.height - SELF_H) / 2.0).max(0.0);
                let bottom = top + SELF_H;
                let mut x0 = if circle_from { a.x + CENTRAL_R } else { right_edge(a, 0) };
                let mut x1 = if circle_to { a.x + CENTRAL_R } else { right_edge(a, starting) };
                let out = x0.max(x1) + SELF_W + bw / 2.0;
                line = ink_of(fill_under(&stack, &frame_store, &cols, a.x, out).as_deref());
                color = line.unwrap_or(color);
                if circle_from {
                    inner.push_str(&circle(a.x, top, color));
                }
                if circle_to {
                    inner.push_str(&circle(a.x, bottom, color));
                }
                if bw > 0.0 {
                    inner.push_str(&badge(i, if tail_at_end { x1 } else { x0 }, if tail_at_end { bottom } else { top }, bw));
                    if tail_at_end {
                        x1 += bw / 2.0;
                    } else {
                        x0 += bw / 2.0;
                    }
                }
                // The loop leaves to the right and comes back heading left, which turns a half head over.
                let heads = format!(
                    "{}{}",
                    if sty.at_start { head(sty.head, x0, top, -1.0, up, color) } else { String::new() },
                    if sty.at_end { head(sty.head, x1, bottom, -1.0, -up, color) } else { String::new() }
                );
                let from = num(x0 + if sty.at_start { head_trim } else { 0.0 });
                let to = num(x1 + if sty.at_end { head_trim } else { 0.0 });
                let d = if right_angles {
                    format!("M{},{}H{}V{}H{}", from, num(top), num(out), num(bottom), to)
                } else {
                    format!("M{},{}H{}A6,6 0 0 1 {},{}V{}A6,6 0 0 1 {},{}H{}", from, num(top), num(out - 6.0), num(out), num(top + 6.0), num(bottom - 6.0), num(out - 6.0), num(bottom), to)
                };
                inner = format!("<path d=\"{}\"{}/>{}{}", d, attrs, heads, inner);
                extend(&mut min_x, &mut max_x, a.x, out + 8.0 + label.width);
                if label.width > 0.0 {
                    let fill = ink_on(fill_under(&stack, &frame_store, &cols, out + 8.0, out + 8.0 + label.width).as_deref());
                    labels_out.push_str(&format!(
                        "<g class=\"pele-edge-label\" data-id=\"i{}\">{}</g>",
                        esc_text(&m.id),
                        label_svg(&label, out + 8.0 + label.width / 2.0, (top + bottom) / 2.0, &fill)
                    ));
                }
                // A label taller than the loop reaches below it, so the next row starts under the label.
                line_y = bottom.max((top + bottom) / 2.0 + label.height / 2.0);
            } else {
                let (ax, bx) = (cols[ai].x, cols[bi].x);
                let dir = if bx > ax { 1.0 } else { -1.0 };
                let creating = cols[bi].create_at == i as i64;
                line = ink_of(fill_under(&stack, &frame_store, &cols, ax.min(bx), ax.max(bx)).as_deref());
                color = line.unwrap_or(color);
                line_y = y + msg_gap + (if label.height > 0.0 { label.height + 2.0 } else { 6.0 }).max(if creating { cols[bi].size.h / 2.0 - msg_gap + 6.0 } else { 0.0 });
                let mut x0 = if dir > 0.0 { right_edge(&cols[ai], 0) } else { left_edge(&cols[ai], 0) };
                let mut x1 = if dir > 0.0 { left_edge(&cols[bi], starting) } else { right_edge(&cols[bi], starting) };
                if creating {
                    let b = &mut cols[bi];
                    b.top = line_y - b.size.h / 2.0;
                    b.life_start = b.top + b.size.h;
                    x1 = b.x - dir * if is_figure(b.kind) { 18.0 } else { b.size.w / 2.0 + 1.0 };
                }
                if circle_from {
                    inner.push_str(&circle(ax, line_y, color));
                    x0 = ax + dir * CENTRAL_R;
                }
                if circle_to {
                    inner.push_str(&circle(bx, line_y, color));
                    x1 = bx - dir * CENTRAL_R;
                }
                if bw > 0.0 {
                    // The number sits on the lifeline at the tail of the arrow, past a connection circle if there is one.
                    if tail_at_end {
                        let cx = if circle_to { x1 - dir * (bw / 2.0 + 2.0) } else { x1 };
                        inner.push_str(&badge(i, cx, line_y, bw));
                        x1 = cx - (dir * bw) / 2.0;
                    } else {
                        let cx = if circle_from { x0 + dir * (bw / 2.0 + 2.0) } else { x0 };
                        inner.push_str(&badge(i, cx, line_y, bw));
                        x0 = cx + (dir * bw) / 2.0;
                    }
                }
                let heads = format!(
                    "{}{}",
                    if sty.at_start { head(sty.head, x0, line_y, -dir, up * dir, color) } else { String::new() },
                    if sty.at_end { head(sty.head, x1, line_y, dir, up * dir, color) } else { String::new() }
                );
                let from = x0 + if sty.at_start { dir * head_trim } else { 0.0 };
                let to = x1 - if sty.at_end { dir * head_trim } else { 0.0 };
                inner = format!("<path d=\"M{},{}H{}\"{}/>{}{}", num(from), num(line_y), num(to), attrs, heads, inner);
                if label.width > 0.0 {
                    let l = x0.min(x1);
                    let r = x0.max(x1);
                    let cx = if align < 0 {
                        l + msg_pad + label.width / 2.0
                    } else if align > 0 {
                        r - msg_pad - label.width / 2.0
                    } else {
                        (l + r) / 2.0
                    };
                    extend(&mut min_x, &mut max_x, cx - label.width / 2.0, cx + label.width / 2.0);
                    let b = backed_label(&label, cx, line_y - 4.0 - label.height / 2.0, false, &crosses, &|l, r| ink_on(fill_under(&stack, &frame_store, &cols, l, r).as_deref()));
                    labels_out.push_str(&format!("<g class=\"pele-edge-label\" data-id=\"i{}\">{}</g>", esc_text(&m.id), b));
                }
                if creating {
                    y = line_y + cols[bi].size.h / 2.0 - 6.0;
                }
            }
            edges_out.push_str(&format!("<g class=\"pele-edge pele-message\" data-id=\"i{}\"{}>{}</g>", esc_text(&m.id), line.map_or(String::new(), |l| format!(" stroke=\"{}\"", l)), inner));
            y = y.max(line_y);
            if cols[ai].destroy_at == i as i64 {
                y = y.max(destroy(&mut cols[ai], line_y, &mut crosses_out));
            }
            if cols[bi].destroy_at == i as i64 && bi != ai {
                y = y.max(destroy(&mut cols[bi], line_y, &mut crosses_out));
            }
        }
    }

    y += 20.0;
    for c in 0..cols.len() {
        while !cols[c].activations.is_empty() {
            end_activation(&mut cols[c], y, &mut min_x, &mut max_x, &mut activations_out);
        }
    }
    let mut bottom = y;
    if mirror {
        let mut foot_h: f64 = 0.0;
        for col in &cols {
            if col.life_end >= 0.0 {
                continue;
            }
            foot_h = foot_h.max(col.size.h);
            actors_out.push_str(&actor_svg(col, y, true));
        }
        bottom = y + foot_h;
    }
    let mut lifelines = String::new();
    for col in &cols {
        actors_out.push_str(&actor_svg(col, col.top, false));
        let end = if col.life_end >= 0.0 { col.life_end } else { y };
        if end > col.life_start {
            lifelines.push_str(&format!("<path class=\"pele-lifeline\" data-id=\"{}\" d=\"M{},{}V{}\"/>", esc_text(&col.id), num(col.x), num(col.life_start), num(end)));
        }
    }

    let mut boxes_out = String::new();
    for run in &runs {
        let (x0, x1) = box_edges(&cols, run);
        let bx = &db.boxes[run.box_];
        let fill = if bx.fill == "transparent" { String::new() } else { resolve_style(&[format!("fill:{}", bx.fill)]).shape };
        let label_fill = ink_on(Some(&bx.fill));
        boxes_out.push_str(&format!(
            "<g class=\"pele-cluster pele-box\"{}><rect x=\"{}\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"var(--_a)\" stroke=\"var(--_b)\"{}/>{}</g>",
            bx.name.as_deref().filter(|n| !n.is_empty()).map_or(String::new(), |n| format!(" data-id=\"{}\"", esc_text(n))),
            num(x0),
            num(x1 - x0),
            num(bottom + box_pad),
            rounded(&format!("{}{}", if fill.is_empty() { " fill-opacity=\"0.5\"" } else { "" }, fill)),
            label_svg(
                &run.title,
                (x0 + x1) / 2.0,
                box_pad / 2.0 + 2.0 + run.title.height / 2.0,
                &format!(" class=\"pele-cluster-label\"{}", if label_fill.is_empty() { " fill=\"var(--_m)\"".to_string() } else { label_fill })
            )
        ));
    }
    if !runs.is_empty() {
        bottom += box_pad;
    }

    let mut min_y = 0.0;
    let mut title_out = String::new();
    let title = title_label(db.title.as_deref(), size, f64::INFINITY);
    if min_x > max_x {
        extend(&mut min_x, &mut max_x, 0.0, title.width.max(40.0));
    }
    if title.width > 0.0 {
        let cx = (min_x + max_x) / 2.0;
        extend(&mut min_x, &mut max_x, cx - title.width / 2.0, cx + title.width / 2.0);
        min_y = -title.height - 12.0;
        title_out = label_svg(&title, cx, min_y + title.height / 2.0, " class=\"pele-title\" font-weight=\"var(--_tw)\"");
    }

    let width = (max_x - min_x + 2.0 * pad).ceil();
    let height = (bottom - min_y + 2.0 * pad).ceil();
    let mut content = format!("<g transform=\"translate({},{})\">{}", num(pad - min_x), num(pad - min_y), title_out);
    if !boxes_out.is_empty() {
        content.push_str(&format!("<g class=\"pele-boxes\" font-size=\"{}\">{}</g>", js::num_str(small), boxes_out));
    }
    if !rects_out.is_empty() {
        rects_out.reverse();
        content.push_str(&format!("<g class=\"pele-rects\" fill=\"var(--_a)\">{}</g>", rects_out.concat()));
    }
    if !lifelines.is_empty() {
        content.push_str(&format!("<g class=\"pele-lifelines\" fill=\"none\" stroke=\"var(--_b)\">{}</g>", lifelines));
    }
    if !activations_out.is_empty() {
        activations_out.reverse();
        content.push_str(&format!("<g class=\"pele-activations\" fill=\"var(--_s)\" stroke=\"var(--_b)\">{}</g>", activations_out.concat()));
    }
    if !frames_out.is_empty() {
        content.push_str(&format!("<g class=\"pele-frames\" font-size=\"{}\" stroke=\"var(--_b)\">{}</g>", js::num_str(small), frames_out));
    }
    if !edges_out.is_empty() {
        content.push_str(&format!("<g class=\"pele-edges\" fill=\"none\" stroke=\"var(--_l)\" stroke-linecap=\"round\">{}</g>", edges_out));
    }
    if !crosses_out.is_empty() {
        content.push_str(&format!("<g class=\"pele-destroys\" fill=\"none\" stroke=\"var(--_l)\" stroke-width=\"1.5\" stroke-linecap=\"round\">{}</g>", crosses_out));
    }
    if !labels_out.is_empty() {
        content.push_str(&format!("<g class=\"pele-edge-labels\" font-size=\"{}\">{}</g>", js::num_str(small), labels_out));
    }
    if !notes_out.is_empty() {
        content.push_str(&format!("<g class=\"pele-notes\" font-size=\"{}\" fill=\"var(--_a)\" stroke=\"var(--_b)\">{}</g>", js::num_str(small), notes_out));
    }
    if !actors_out.is_empty() {
        content.push_str(&format!("<g class=\"pele-nodes\">{}</g>", actors_out));
    }
    content.push_str("</g>");
    let svg = svg_document("sequence", width, height, size, options.responsive, &Accessible { acc_title: db.acc_title.as_deref(), acc_descr: db.acc_descr.as_deref() }, &content);
    Rendered { svg, width, height, links: Vec::new() }
}

// A label on a plate of the page color where a lifeline would strike through it, else in the
// ink of what is under it.
fn backed_label(label: &Label, cx: f64, cy: f64, frame: bool, crosses: &dyn Fn(f64, f64) -> bool, under: &dyn Fn(f64, f64) -> String) -> String {
    if label.width == 0.0 {
        return String::new();
    }
    let l = cx - label.width / 2.0;
    let plate = crosses(l, l + label.width);
    let back = if plate {
        format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"3\" fill=\"var(--_bg)\" stroke=\"none\"/>",
            num(l - 3.0),
            num(cy - label.height / 2.0 + 2.0),
            num(label.width + 6.0),
            num(label.height - 4.0)
        )
    } else {
        String::new()
    };
    let fill = if plate { String::new() } else { under(l, l + label.width) };
    let attrs = if frame { format!(" class=\"pele-frame-label\"{} stroke=\"none\"", if fill.is_empty() { " fill=\"var(--_m)\"".to_string() } else { fill }) } else { fill };
    back + &label_svg(label, cx, cy, &attrs)
}
