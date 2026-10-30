// Port of src/diagrams/class/render.ts, for the default render options the host uses: no width
// limit (so a diagram is neither turned, tightened nor wrapped), 16px text, no monospace parts,
// no icon or math resolver (annotations and cardinalities stay literal).

use super::db::ClassDb;
use super::graph::{build_class_graph, NodeShape};
use super::markers::{class_marker, class_marker_trim};
use super::types::ClassNode;
use crate::diagrams::common::generics::parse_generic_types;
use crate::js;
use crate::layout::compound::{cnode, compound_layout, direction, CEdge, CNode, Dir};
use crate::layout::layered::LayeredOptions;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num};
use crate::svg::cluster::{cluster_title_x, mark_crossings, struck_title, title_crossed, Crossings};
use crate::svg::edges::{edge_label_svg, route_path, EdgePath};
use crate::svg::ink::ink_fill;
use crate::svg::root::{svg_document, Accessible};
use crate::svg::shapes::end_side;
use crate::svg::theme::{class_names, resolve_style, rounded, ResolvedStyle, ROUNDED};
use crate::svg::title::with_title;
use crate::text::entities::decode_entities;
use crate::text::label::{layout_label, Label};
use crate::text::measurer::{style, width as measure};
use crate::types::{LinkInfo, RenderOptions, Rendered};
use crate::util::url::linked;
use crate::util::yaml::{Map, Value};
use std::collections::HashMap;

const NODE_SEP: f64 = 40.0;
const EDGE_SEP: f64 = 16.0;
const RANK_SEP: f64 = 56.0;
const GROUP_PAD: f64 = 20.0;
const LOOP: f64 = 26.0;
const PAD_X: f64 = 12.0;
const TITLE_PAD: f64 = 8.0;
const SECTION_PAD: f64 = 6.0;
const EMPTY_SECTION: f64 = 10.0;
const MIN_WIDTH: f64 = 64.0;
const FOLD: f64 = 8.0;
const LINE: &str = "var(--_l)";
const TARGETS: [&str; 4] = ["_self", "_blank", "_parent", "_top"];

fn number_option(conf: Option<&Map>, key: &str, fallback: f64) -> f64 {
    match conf.and_then(|c| c.get(key)) {
        Some(Value::Num(v)) if *v > 0.0 => v.min(2000.0),
        _ => fallback,
    }
}

struct Line {
    text: String,
    width: f64,
    classifier: String,
}

struct View {
    style: ResolvedStyle,
    label: Label,
    w: f64,
    h: f64,
    annotations: Vec<String>,
    attributes: Vec<Line>,
    methods: Vec<Line>,
    title_h: f64,
    attributes_h: f64,
    font_size: f64,
}

struct End {
    edge: usize,
    at: usize,
    toward: f64,
    order: usize,
    room: f64,
    side: i32,
    // Whether the text of this end goes on the right of, or below, its line.
    after: bool,
}

struct Card {
    text: String,
    x: f64,
    y: f64,
}

fn member_line(member: &super::members::ClassMember, size: f64) -> Line {
    let text = js::trim(&decode_entities(&member.display_text())).to_string();
    let st = if member.classifier == "*" { style::ITALIC } else { 0 };
    Line { width: measure(&text, size, st), text, classifier: member.classifier.clone() }
}

pub fn render_class(db: &ClassDb, config: &Config, options: &RenderOptions) -> Rendered {
    // With no width limit, neither tighten nor turnToFit changes the drawing.
    draw(db, config, options)
}

fn draw(db: &ClassDb, config: &Config, options: &RenderOptions) -> Rendered {
    let conf = config.get("class").and_then(|v| v.as_map());
    let graph = build_class_graph(db, !matches!(conf.and_then(|c| c.get("hierarchicalNamespaces")), Some(Value::Bool(false))));
    let hide_empty = matches!(conf.and_then(|c| c.get("hideEmptyMembersBox")), Some(Value::Bool(true)));
    let size = 16.0;
    let small = js::round(size * 0.875);
    let tiny = js::round(size * 0.8);
    let tiny_height = js::round(tiny * 1.4);
    let pad = 8.0;
    let card_width = |raw: &str| if raw.is_empty() { 0.0 } else { measure(&decode_entities(raw), tiny, 0) };
    let root_dir = direction(Some(&db.direction));

    // A class may carry the declarations of many classDefs many times over. Each distinct
    // declaration is checked once, and only the last value of each property that has an effect
    // is resolved.
    let mut effect: HashMap<String, String> = HashMap::new();
    let mut from_defs: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut class_style = |cls: &ClassNode| -> ResolvedStyle {
        // Map<prop, decl>, in insertion order.
        let mut kept: Vec<(String, String)> = Vec::new();
        let mut keep = |kept: &mut Vec<(String, String)>, decl: &str| {
            let prop = effect
                .entry(decl.to_string())
                .or_insert_with(|| {
                    let one = resolve_style(&[decl.to_string()]);
                    if one.shape.is_empty() && one.text.is_empty() {
                        String::new()
                    } else {
                        let at = decl.encode_utf16().position(|c| c == 58).map_or(-1, |k| k as isize);
                        let u = js::u16s(decl);
                        let end = if at < 0 { u.len().saturating_sub(1) } else { at as usize };
                        js::trim(&js::from16(&u[..end])).to_lowercase()
                    }
                })
                .clone();
            if !prop.is_empty() {
                match kept.iter_mut().find(|(k, _)| *k == prop) {
                    Some(slot) => slot.1 = decl.to_string(),
                    None => kept.push((prop, decl.to_string())),
                }
            }
        };
        match from_defs.get(&cls.css_classes) {
            None => {
                for name in cls.css_classes.split(' ') {
                    if let Some(def) = db.style_class(name) {
                        for s in &def.styles {
                            for part in s.split(',') {
                                keep(&mut kept, part);
                            }
                        }
                    }
                }
                from_defs.insert(cls.css_classes.clone(), kept.clone());
            }
            Some(base) => kept = base.clone(),
        }
        for s in &cls.styles {
            keep(&mut kept, s);
        }
        resolve_style(&kept.into_iter().map(|(_, d)| d).collect::<Vec<_>>())
    };

    let n = graph.nodes.len();
    let mut children = vec![0i32; n];
    for node in &graph.nodes {
        if node.parent >= 0 {
            children[node.parent as usize] += 1;
        }
    }

    // Namespaces that an edge may cross into are made wide enough for the title to sit beside it.
    let mut crossed = vec![false; n];
    for edge in &graph.edges {
        if edge.source < 0 || edge.target < 0 {
            continue;
        }
        let a = graph.nodes[edge.source as usize].parent;
        let b = graph.nodes[edge.target as usize].parent;
        if a == b {
            continue;
        }
        for start in [a, b] {
            let mut g = start;
            while g >= 0 && !crossed[g as usize] {
                crossed[g as usize] = true;
                g = graph.nodes[g as usize].parent;
            }
        }
    }

    let base = |label: Label, w: f64, h: f64, font_size: f64| View {
        style: ResolvedStyle::default(),
        label,
        w,
        h,
        annotations: Vec::new(),
        attributes: Vec::new(),
        methods: Vec::new(),
        title_h: 0.0,
        attributes_h: 0.0,
        font_size,
    };

    let mut views: Vec<View> = Vec::new();
    let mut cnodes: Vec<CNode> = Vec::new();
    for (i, node) in graph.nodes.iter().enumerate() {
        let mut view;
        let mut c;
        if node.is_group {
            let label = layout_label(Some(&node.label), false, size, 4000.0, 0);
            view = base(label, 0.0, 0.0, size);
            if children[i] > 0 {
                c = cnode(0.0, 0.0);
                c.is_group = true;
                c.pad_x = GROUP_PAD;
                c.pad_top = if view.label.height > 0.0 { view.label.height + 16.0 } else { GROUP_PAD };
                c.pad_bottom = GROUP_PAD;
                c.min_w = view.label.width + 2.0 * GROUP_PAD - 12.0 + if crossed[i] { (view.label.width + 24.0).min(240.0) } else { 0.0 };
            } else {
                view.w = (view.label.width + 2.0 * GROUP_PAD).max(80.0);
                view.h = view.label.height + 2.0 * GROUP_PAD;
                c = cnode(view.w, view.h);
            }
        } else if let Some(ci) = node.class_node {
            let cls = &db.classes[ci];
            let st = class_style(cls);
            let font_size = st.font_size.unwrap_or(size);
            let member_size = js::round(font_size * 0.875);
            let line_height = js::round(member_size * 1.5);
            // The generic type is escaped so that `<T>` is not read as a tag.
            let generic = if cls.type_.is_empty() { String::new() } else { format!("&lt;{}&gt;", parse_generic_types(&cls.type_).replace('<', "&lt;").replace('>', "&gt;")) };
            let label = layout_label(Some(&format!("{}{}", cls.label, generic)), false, font_size, 4000.0, style::BOLD | if st.italic { style::ITALIC } else { 0 });
            let annotations: Vec<String> = cls.annotations.iter().map(|a| format!("«{}»", js::trim(&decode_entities(a)))).collect();
            let attributes: Vec<Line> = cls.members.iter().map(|m| member_line(m, member_size)).collect();
            let methods: Vec<Line> = cls.methods.iter().map(|m| member_line(m, member_size)).collect();
            let mut inner = label.width;
            let mut annotations_h = 0.0;
            for a in &annotations {
                inner = inner.max(measure(a, tiny, 0));
                annotations_h += tiny_height;
            }
            for line in attributes.iter().chain(&methods) {
                inner = inner.max(line.width);
            }
            let sections = !(hide_empty && attributes.is_empty() && methods.is_empty());
            let section = |count: usize| {
                if !sections {
                    0.0
                } else if count > 0 {
                    2.0 * SECTION_PAD + count as f64 * line_height
                } else {
                    EMPTY_SECTION
                }
            };
            let title_h = 2.0 * TITLE_PAD + annotations_h + label.height.max(js::round(font_size * 1.5));
            let attributes_h = section(attributes.len());
            let h = title_h + attributes_h + section(methods.len());
            view = View { style: st, w: (inner + 2.0 * PAD_X).max(MIN_WIDTH), h, label, annotations, attributes, methods, title_h, attributes_h, font_size };
            c = cnode(view.w, view.h);
        } else if node.shape == NodeShape::Note {
            let label = layout_label(Some(&node.label), true, small, 260.0, 0);
            let (w, h) = ((label.width + 2.0 * PAD_X).max(40.0), label.height + 2.0 * TITLE_PAD);
            view = base(label, w, h, small);
            c = cnode(view.w, view.h);
        } else {
            let label = layout_label(Some(&node.label), false, small, 200.0, 0);
            let (w, h) = (label.width.max(12.0) + 4.0, label.height.max(12.0));
            view = base(label, w, h, small);
            c = cnode(view.w, view.h);
        }
        c.parent = node.parent;
        c.seq = i as f64;
        views.push(view);
        cnodes.push(c);
    }

    let mut cedges: Vec<CEdge> = Vec::new();
    let mut drawn: Vec<usize> = Vec::new();
    let mut edge_labels: Vec<Label> = Vec::new();
    let mut start_room: Vec<f64> = Vec::new();
    let mut end_room: Vec<f64> = Vec::new();
    // Edges leave one side of a class and arrive at the other, so a class with many of them
    // is widened until they, and the text at their ends, fit side by side.
    let mut crowd = vec![0f64; views.len() * 4];
    let mut meet = |item: usize, slot: usize, room: f64| {
        crowd[item * 4 + slot] += 1.0;
        if room > crowd[item * 4 + slot + 1] {
            crowd[item * 4 + slot + 1] = room;
        }
    };
    for (ei, edge) in graph.edges.iter().enumerate() {
        if edge.source < 0 || edge.target < 0 {
            continue;
        }
        let (src, dst) = (edge.source as usize, edge.target as usize);
        let label = layout_label(edge.label.as_deref(), true, small, 200.0, 0);
        let start_w = card_width(&edge.start_label_right);
        let end_w = card_width(&edge.end_label_left);
        if src == dst {
            cnodes[src].w += 2.0 * (LOOP + if label.width > 0.0 { label.width + 12.0 } else { 0.0 });
        } else {
            meet(src, 0, start_w);
            meet(dst, 2, end_w);
        }
        start_room.push(start_w);
        end_room.push(end_w);
        drawn.push(ei);
        cedges.push(CEdge {
            src,
            dst,
            minlen: 1.0,
            label_w: if label.width > 0.0 { label.width + 12.0 } else { 0.0 },
            label_h: if label.height > 0.0 { label.height + 4.0 } else { 0.0 },
            route: Vec::new(),
            label_x: 0.0,
            label_y: 0.0,
        });
        edge_labels.push(label);
    }

    let vertical = matches!(root_dir, Dir::TB | Dir::BT);
    if vertical {
        for i in 0..views.len() {
            if graph.nodes[i].class_node.is_none() {
                continue;
            }
            let mut need: f64 = 0.0;
            for slot in [0, 2] {
                let count = crowd[i * 4 + slot];
                let room = crowd[i * 4 + slot + 1];
                if count > 1.0 {
                    need = need.max((count - 1.0) * if room > 0.0 { room + 24.0 } else { 14.0 } + 24.0);
                }
            }
            need = need.min(480.0);
            if need > views[i].w {
                cnodes[i].w += need - views[i].w;
                views[i].w = need;
            }
        }
    }

    let lettered = (0..drawn.len()).any(|i| start_room[i] > 0.0 || end_room[i] > 0.0);
    // Cardinalities at both ends of a vertical edge need room for their height between the ranks.
    let mut tall_cards: f64 = 0.0;
    if vertical {
        let card_height = |raw: &str| if raw.is_empty() { 0.0 } else { tiny_height };
        for &ei in &drawn {
            let e = &graph.edges[ei];
            tall_cards = tall_cards.max(card_height(&e.start_label_right) + card_height(&e.end_label_left) - 2.0 * tiny_height);
        }
    }
    let layout = compound_layout(
        &mut cnodes,
        &mut cedges,
        root_dir,
        &LayeredOptions {
            node_sep: number_option(conf, "nodeSpacing", NODE_SEP),
            edge_sep: EDGE_SEP,
            rank_sep: number_option(conf, "rankSpacing", if lettered { RANK_SEP + 16.0 } else { RANK_SEP }) + tall_cards,
            port_sep: 20.0,
            apart: true,
            ..Default::default()
        },
    );

    // Every edge reaches the middle of a side. Where several share a side they are spread along
    // it, in the order of where they come from. An end with text runs straight for the length of
    // the text, which sits beside it on the side the line then bends toward, away from the
    // neighbors that bend the other way. Neighbors are set apart far enough for the text between them.
    let mut all_ends: Vec<End> = Vec::new();
    // Map<number, End[]>, in insertion order.
    let mut sides: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut starts: Vec<Option<usize>> = Vec::new();
    let mut ends: Vec<Option<usize>> = Vec::new();
    for i in 0..cedges.len() {
        let routed = cedges[i].src != cedges[i].dst && cedges[i].route.len() >= 6;
        for (first, item, room) in [(true, cedges[i].src, start_room[i]), (false, cedges[i].dst, end_room[i])] {
            let mut found = None;
            if routed {
                let at = if first { 0 } else { cedges[i].route.len() - 3 };
                let c = &cnodes[item];
                let route = &mut cedges[i].route;
                let side = end_side(route, at, c);
                if side >= 0 {
                    let slack = (c.w - views[item].w) / 2.0;
                    if side == 1 {
                        route[at] -= slack;
                    } else if side == 3 {
                        route[at] += slack;
                    }
                    let across = (side & 1) as usize;
                    let toward = route[(if at == 0 { 3 } else { at - 3 }) + across];
                    let bend = toward - route[at + across];
                    all_ends.push(End { edge: i, at, toward, order: i, room, side, after: if across == 1 { bend > 0.0 } else { bend >= 0.0 } });
                    let id = all_ends.len() - 1;
                    let key = item * 4 + side as usize;
                    match sides.iter_mut().find(|(k, _)| *k == key) {
                        Some((_, list)) => list.push(id),
                        None => sides.push((key, vec![id])),
                    }
                    found = Some(id);
                }
            }
            if first {
                starts.push(found);
            } else {
                ends.push(found);
            }
        }
    }
    // Whether a point lies on the border of one of the namespaces around a class.
    let on_border = |item: usize, x: f64, y: f64| -> bool {
        let mut g = cnodes[item].parent;
        let mut hops = 0;
        while g >= 0 && hops < 10000 {
            let c = &cnodes[g as usize];
            let dx = (x - c.x).abs();
            let dy = (y - c.y).abs();
            if ((dy - c.h / 2.0).abs() < 0.5 && dx <= c.w / 2.0 + 0.5) || ((dx - c.w / 2.0).abs() < 0.5 && dy <= c.h / 2.0 + 0.5) {
                return true;
            }
            g = c.parent;
            hops += 1;
        }
        false
    };
    let mut gaps: Vec<f64> = Vec::new();
    for (key, list) in sides.iter_mut() {
        if list.len() < 2 {
            continue;
        }
        let item = *key >> 2;
        let across = *key & 1;
        let length = if across == 1 { views[item].h } else { views[item].w };
        list.sort_by(|&a, &b| js::ord(js::or(all_ends[a].toward - all_ends[b].toward, || all_ends[a].order as f64 - all_ends[b].order as f64)));
        let mut total = 0.0;
        for j in 0..list.len() - 1 {
            let (a, b) = (&all_ends[list[j]], &all_ends[list[j + 1]]);
            let mut gap = if across == 1 { 20.0 } else { 24.0 };
            if a.room > 0.0 && a.after {
                gap += if across == 1 { tiny_height + 6.0 } else { a.room };
            }
            if b.room > 0.0 && !b.after {
                gap += if across == 1 { tiny_height + 6.0 } else { b.room };
            }
            if gaps.len() <= j {
                gaps.resize(j + 1, f64::NAN);
            }
            gaps[j] = gap;
            total += gap;
        }
        let scale = ((length - 24.0) / total).min(1.0);
        if scale <= 0.0 {
            continue;
        }
        let mut at = (if across == 1 { cnodes[item].y } else { cnodes[item].x }) - (total * scale) / 2.0;
        for j in 0..list.len() {
            let end_at = all_ends[list[j]].at;
            let route = &mut cedges[all_ends[list[j]].edge].route;
            // An edge that crosses the border of the class's namespace close by runs straight to
            // it: the point where it crosses moves into line with its end.
            let step: isize = if end_at == 0 { 3 } else { -3 };
            let first = end_at as isize + step;
            let mut border: isize = -1;
            let mut k = first;
            while k >= 0 && (k as usize) < route.len() && (route[k as usize + across] - route[first as usize + across]).abs() < 0.5 {
                if on_border(item, route[k as usize], route[k as usize + 1]) {
                    border = k;
                    break;
                }
                k += step;
            }
            if border >= 0 && (route[first as usize + across] - at).abs() <= 24.0 {
                let mut k = first;
                while k != border + step {
                    route[k as usize + across] = at;
                    k += step;
                }
            }
            route[end_at + across] = at;
            at += gaps.get(j).copied().unwrap_or(f64::NAN) * scale;
        }
    }

    let straighten = |route: &mut Vec<f64>, end: Option<&End>, room: f64, first: bool| {
        let Some(end) = end else { return };
        if room <= 0.0 {
            return;
        }
        let across = (end.side & 1) as usize;
        let at = if first { 0 } else { route.len() - 3 };
        let along = at + 1 - across;
        let reach = (route[(if first { 3 } else { at - 3 }) + 1 - across] - route[along]).abs();
        let length = (if across == 1 { room + 10.0 } else { tiny_height + 6.0 }).min(reach * 0.35);
        if length < 4.0 {
            return;
        }
        let mut point = [route[at], route[at + 1], route[at + 2]];
        point[1 - across] += (if end.side == 0 || end.side == 3 { -1.0 } else { 1.0 }) * length;
        let index = if first { 3 } else { at };
        route.splice(index..index, point);
    };
    for i in 0..cedges.len() {
        straighten(&mut cedges[i].route, starts[i].map(|k| &all_ends[k]), start_room[i], true);
        straighten(&mut cedges[i].route, ends[i].map(|k| &all_ends[k]), end_room[i], false);
    }

    let mut min_x: f64 = 0.0;
    let mut min_y: f64 = 0.0;
    let mut max_x = layout.width;
    let mut max_y = layout.height;
    let mut cards: Vec<Card> = Vec::new();
    let mut card = |raw: &str, width: f64, x: f64, y: f64, side: i32, after: bool| {
        if raw.is_empty() {
            return;
        }
        let h = tiny_height;
        let (mut cx, mut cy) = (x, y);
        if side & 1 == 1 {
            cx += (if side == 3 { -1.0 } else { 1.0 }) * (5.0 + width / 2.0);
            cy += (if after { 1.0 } else { -1.0 }) * (7.0 + h / 2.0);
        } else {
            cx += (if after { 1.0 } else { -1.0 }) * (9.0 + width / 2.0);
            cy += (if side == 0 { -1.0 } else { 1.0 }) * (3.0 + h / 2.0);
        }
        cards.push(Card { text: decode_entities(raw), x: cx, y: cy });
        min_x = min_x.min(cx - width / 2.0);
        max_x = max_x.max(cx + width / 2.0);
        min_y = min_y.min(cy - h / 2.0);
        max_y = max_y.max(cy + h / 2.0);
    };
    let mut loop_seen: HashMap<usize, usize> = HashMap::new();
    let mut loop_at = vec![0usize; cedges.len()];
    for i in 0..cedges.len() {
        let edge = &graph.edges[drawn[i]];
        let ce = &mut cedges[i];
        if ce.src == ce.dst {
            let k = *loop_seen.get(&ce.src).unwrap_or(&0);
            loop_seen.insert(ce.src, k + 1);
            loop_at[i] = k;
            let c = &cnodes[ce.src];
            let view = &views[ce.src];
            let x = c.x + view.w / 2.0;
            let spread = (view.h / 2.0 - 4.0).min(8.0 + k as f64 * 6.0);
            card(&edge.start_label_right, start_room[i], x, c.y - spread, 1, false);
            card(&edge.end_label_left, end_room[i], x, c.y + spread, 1, true);
            ce.label_x = c.x + view.w / 2.0 + LOOP + k as f64 * 8.0 + 4.0 + ce.label_w / 2.0;
            ce.label_y = c.y;
        } else if ce.route.len() >= 6 {
            let route = &ce.route;
            let last = route.len() - 3;
            let from = starts[i].map(|k| &all_ends[k]);
            let to = ends[i].map(|k| &all_ends[k]);
            card(&edge.start_label_right, start_room[i], route[0], route[1], from.map_or(2, |e| e.side), from.is_none_or(|e| e.after));
            card(&edge.end_label_left, end_room[i], route[last], route[last + 1], to.map_or(0, |e| e.side), to.is_none_or(|e| e.after));
        }
    }

    let ox = pad - min_x;
    let oy = pad - min_y;
    let width = (max_x - min_x + 2.0 * pad).ceil();
    let height = (max_y - min_y + 2.0 * pad).ceil();
    let mut links: Vec<LinkInfo> = Vec::new();

    let mut crossings: Crossings = Crossings::new();
    if !graph.nodes.is_empty() && graph.nodes[0].is_group {
        for e in &cedges {
            mark_crossings(&mut crossings, &e.route);
        }
    }

    let mut clusters = String::new();
    let mut struck_titles = String::new();
    let mut nodes_out = String::new();
    for i in 0..views.len() {
        let view = &views[i];
        let c = &cnodes[i];
        let node = &graph.nodes[i];
        let x = c.x + ox;
        let y = c.y + oy;
        let id = esc_text(&node.id);
        let w = if c.is_group { c.w } else { view.w };
        let h = if c.is_group { c.h } else { view.h };
        if node.is_group {
            let title_x = if c.is_group { cluster_title_x(c, view.label.width, &crossings, GROUP_PAD) } else { c.x };
            let title_y = y - h / 2.0 + 8.0 + view.label.height / 2.0;
            let title_attrs = " class=\"pele-cluster-label\" fill=\"var(--_m)\"";
            let struck = c.is_group && title_crossed(c, view.label.width, title_x, &crossings);
            if struck {
                struck_titles.push_str(&struck_title(&view.label, title_x + ox, title_y, title_attrs, &id, &cnodes, i));
            }
            clusters.push_str(&format!(
                "<g class=\"pele-cluster pele-namespace\" data-id=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_a)\" fill-opacity=\"0.5\" stroke=\"var(--_b)\"{}/>{}</g>",
                id,
                num(x - w / 2.0),
                num(y - h / 2.0),
                num(w),
                num(h),
                ROUNDED,
                if struck { String::new() } else { label_svg(&view.label, title_x + ox, title_y, title_attrs) }
            ));
            continue;
        }
        let at = format!(" transform=\"translate({},{})\"", num(x), num(y));
        if let Some(ci) = node.class_node {
            nodes_out.push_str(&class_svg(view, &db.classes[ci], &id, &at, size, tiny, tiny_height, &mut links));
        } else if node.shape == NodeShape::Note {
            let r = w / 2.0;
            let t = -h / 2.0;
            nodes_out.push_str(&format!(
                "<g class=\"pele-node pele-note\" data-id=\"{}\"{}><path d=\"M{},{}H{}L{},{}V{}H{}Z\" fill=\"var(--_a)\" stroke=\"var(--_b)\" stroke-linejoin=\"round\"/><path d=\"M{},{}V{}H{}\" fill=\"none\" stroke=\"var(--_b)\" stroke-linejoin=\"round\"/>{}</g>",
                id,
                at,
                num(-r),
                num(t),
                num(r - FOLD),
                num(r),
                num(t + FOLD),
                num(-t),
                num(-r),
                num(r - FOLD),
                num(t),
                num(t + FOLD),
                num(r),
                label_svg(&view.label, 0.0, 0.0, &format!(" class=\"pele-label\" font-size=\"{}\"", js::num_str(small)))
            ));
        } else {
            nodes_out.push_str(&format!(
                "<g class=\"pele-node pele-interface\" data-id=\"{}\"{}>{}</g>",
                id,
                at,
                label_svg(&view.label, 0.0, 0.0, &format!(" class=\"pele-label\" font-size=\"{}\"", js::num_str(small)))
            ));
        }
    }

    let mut edges_out = String::new();
    let mut labels_out = String::new();
    for i in 0..cedges.len() {
        let edge = &graph.edges[drawn[i]];
        let label = &edge_labels[i];
        let (start_type, end_type) = (edge.arrow_type_start, edge.arrow_type_end);
        let start_trim = class_marker_trim(start_type);
        let end_trim = class_marker_trim(end_type);
        let ce = &mut cedges[i];
        let path: EdgePath = if ce.src == ce.dst {
            let c = &cnodes[ce.src];
            let view = &views[ce.src];
            let k = loop_at[i] as f64;
            let x = c.x + ox + view.w / 2.0;
            let y = c.y + oy;
            let spread = (view.h / 2.0 - 4.0).min(8.0 + k * 6.0);
            let reach = LOOP + k * 8.0;
            EdgePath {
                d: format!(
                    "M{},{}C{},{} {},{} {},{}",
                    num(x + start_trim),
                    num(y - spread),
                    num(x + reach + start_trim),
                    num(y - spread),
                    num(x + reach + end_trim),
                    num(y + spread),
                    num(x + end_trim),
                    num(y + spread)
                ),
                tokens: None,
                sx: x,
                sy: y - spread,
                sdx: -1.0,
                sdy: 0.0,
                ex: x,
                ey: y + spread,
                edx: -1.0,
                edy: 0.0,
            }
        } else {
            let mut k = 0;
            while k < ce.route.len() {
                ce.route[k] += ox;
                ce.route[k + 1] += oy;
                k += 3;
            }
            route_path(&ce.route, None, start_trim, end_trim, None, None)
        };
        let attrs = match edge.pattern {
            "dashed" => " stroke-dasharray=\"6 4\"",
            "dotted" => " stroke-dasharray=\"2 4\"",
            _ => "",
        };
        edges_out.push_str(&format!(
            "<g class=\"pele-edge{}\" data-id=\"{}\"><path d=\"{}\"{}/>{}{}</g>",
            if edge.pattern == "dotted" { " pele-note-edge" } else { " pele-relation" },
            esc_text(&edge.id),
            path.d,
            attrs,
            class_marker(start_type, path.sx, path.sy, path.sdx, path.sdy, LINE),
            class_marker(end_type, path.ex, path.ey, path.edx, path.edy, LINE)
        ));
        if label.width > 0.0 {
            labels_out.push_str(&edge_label_svg(&edge.id, label, ce.label_x + ox, ce.label_y + oy, ""));
        }
    }

    let mut cards_out = String::new();
    for item in &cards {
        cards_out.push_str(&format!("<text class=\"pele-cardinality\" x=\"{}\" y=\"{}\" text-anchor=\"middle\">{}</text>", num(item.x + ox), num(item.y + oy + tiny * 0.35), esc(&item.text)));
    }

    let mut content = String::new();
    if !clusters.is_empty() {
        content.push_str(&format!("<g class=\"pele-clusters\">{}</g>", clusters));
    }
    if !edges_out.is_empty() {
        content.push_str(&format!("<g class=\"pele-edges\" fill=\"none\" stroke=\"{}\" stroke-linecap=\"round\">{}</g>", LINE, edges_out));
    }
    if !struck_titles.is_empty() {
        content.push_str(&format!("<g class=\"pele-cluster-titles\">{}</g>", struck_titles));
    }
    if !labels_out.is_empty() {
        content.push_str(&format!("<g class=\"pele-edge-labels\" font-size=\"{}\">{}</g>", js::num_str(small), labels_out));
    }
    if !cards_out.is_empty() {
        content.push_str(&format!("<g class=\"pele-cardinalities\" font-size=\"{}\" fill=\"var(--_m)\">{}</g>", js::num_str(tiny), cards_out));
    }
    content.push_str(&format!("<g class=\"pele-nodes\">{}</g>", nodes_out));
    let titled = with_title(db.title.as_deref(), size, pad, width, height, content);
    let svg = svg_document("class", titled.width, titled.height, size, options.responsive, &Accessible { acc_title: db.acc_title.as_deref(), acc_descr: db.acc_descr.as_deref() }, &titled.content);
    Rendered { svg, width: titled.width, height: titled.height, links }
}

#[allow(clippy::too_many_arguments)]
fn class_svg(view: &View, cls: &ClassNode, id: &str, at: &str, size: f64, tiny: f64, tiny_height: f64, links: &mut Vec<LinkInfo>) -> String {
    let (w, h, st) = (view.w, view.h, &view.style);
    let left = -w / 2.0;
    let top = -h / 2.0;
    let member_size = js::round(view.font_size * 0.875);
    let line_height = js::round(member_size * 1.5);
    let mut body = format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_s)\" stroke=\"var(--_b)\"{}/>", num(left), num(top), num(w), num(h), rounded(&st.shape));
    let ink = ink_fill(&st.shape, &st.text, None);
    let first = top + view.title_h;
    let second = first + view.attributes_h;
    if h > view.title_h {
        let stroke = st.stroke.as_deref().map_or(String::new(), |s| format!(" style=\"stroke:{}\"", esc(s)));
        body.push_str(&format!(
            "<path class=\"pele-divider\" d=\"M{},{}H{}M{},{}H{}\" fill=\"none\" stroke=\"var(--_b)\"{}/>",
            num(left),
            num(first),
            num(-left),
            num(left),
            num(second),
            num(-left),
            stroke
        ));
    }
    let muted = if ink.is_empty() { " fill=\"var(--_m)\"".to_string() } else { ink.clone() };
    let mut y = top + TITLE_PAD;
    for annotation in &view.annotations {
        body.push_str(&format!(
            "<text class=\"pele-annotation\" x=\"0\" y=\"{}\" text-anchor=\"middle\" font-size=\"{}\"{}>{}</text>",
            num(y + tiny_height / 2.0 + tiny * 0.35),
            js::num_str(tiny),
            muted,
            esc(annotation)
        ));
        y += tiny_height;
    }
    let title_size = if view.font_size == size { String::new() } else { format!(" font-size=\"{}\"", num(view.font_size)) };
    body.push_str(&label_svg(&view.label, 0.0, (y + first - TITLE_PAD) / 2.0, &format!(" class=\"pele-label\"{} font-weight=\"var(--_hw)\"{}{}", title_size, ink, st.text)));
    let lines = |list: &[Line], kind: &str, from: f64| -> String {
        let mut out = String::new();
        let mut baseline = from + SECTION_PAD + line_height / 2.0 + member_size * 0.35;
        for line in list {
            let mark = if line.classifier == "$" {
                format!(
                    " pele-static\"{}",
                    if !st.text.is_empty() { format!("{}text-decoration:underline\"", &st.text[..st.text.len() - 1]) } else { " style=\"text-decoration:underline\"".to_string() }
                )
            } else if line.classifier == "*" {
                format!(" pele-abstract\" font-style=\"italic\"{}", st.text)
            } else {
                format!("\"{}", st.text)
            };
            out.push_str(&format!("<text class=\"pele-member pele-{}{} x=\"{}\" y=\"{}\">{}</text>", kind, mark, num(left + PAD_X), num(baseline), esc(&line.text)));
            baseline += line_height;
        }
        out
    };
    if !view.attributes.is_empty() || !view.methods.is_empty() {
        body.push_str(&format!(
            "<g class=\"pele-members\" font-size=\"{}\"{}>{}{}</g>",
            js::num_str(member_size),
            ink,
            lines(&view.attributes, "attribute", first),
            lines(&view.methods, "method", second)
        ));
    }
    if let Some(tip) = cls.tooltip.as_deref().filter(|t| !t.is_empty()) {
        body = format!("<title>{}</title>{}", esc_text(tip), body);
    }
    if let Some(link) = cls.link.as_deref().filter(|l| !l.is_empty()) {
        let target = cls.link_target.as_deref().filter(|t| TARGETS.contains(t)).map_or(String::new(), |t| format!(" target=\"{}\"", t));
        body = linked(&body, link, &cls.id, links, &target);
    }
    let css = cls.css_classes.strip_prefix("default").map_or(cls.css_classes.as_str(), |rest| match rest.chars().next() {
        Some(c) if js::is_space(c) => &rest[c.len_utf8()..],
        _ => rest,
    });
    format!("<g class=\"pele-node pele-class{}\" data-id=\"{}\"{}>{}</g>", class_names(css), id, at, body)
}
