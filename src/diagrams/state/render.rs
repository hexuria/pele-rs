// Port of src/diagrams/state/render.ts, for the default render options the host uses: no width
// limit (so a diagram that runs across is never turned), 16px text, no icon or math resolver.

use super::db::StateDb;
use super::graph::StateNode;
use crate::js;
use crate::layout::compound::{cnode, compound_layout, direction, CEdge, CNode, Dir};
use crate::layout::layered::LayeredOptions;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num};
use crate::svg::edges::{casings_svg, edge_casing, edge_label_svg, marker, marker_trim, over_fills, route_path, EdgePath, Fill, SideReach};
use crate::svg::ink::{ink_fill, paints};
use crate::svg::root::{svg_document, Accessible};
use crate::svg::shapes::end_side;
use crate::svg::theme::{class_names, resolve_style, sets_fill, ResolvedStyle};
use crate::svg::title::with_title;
use crate::text::entities::decode_entities;
use crate::text::label::{layout_label, Label};
use crate::text::measurer::style as st;
use crate::types::{LinkInfo, RenderOptions, Rendered};
use crate::util::url::{linked, safe_url};
use crate::util::yaml::Value;
use std::collections::HashMap;

const NODE_SEP: f64 = 40.0;
const EDGE_SEP: f64 = 16.0;
const RANK_SEP: f64 = 44.0;
const GROUP_PAD: f64 = 16.0;
const REGION_PAD: f64 = 10.0;
const PAD_X: f64 = 16.0;
const PAD_Y: f64 = 10.0;
const LOOP: f64 = 26.0;
const FOLD: f64 = 8.0;
const NOTE_WRAP: f64 = 320.0;
const CORNER: f64 = 10.0;
const BAR: f64 = 8.0;
const SURFACE: &str = " fill=\"var(--_s)\" stroke=\"var(--_b)\"";
const RULE: &str = " fill=\"none\" stroke=\"var(--_b)\"";

#[derive(Clone, Copy, PartialEq, PartialOrd, Debug)]
enum K {
    State,
    Described,
    Start,
    End,
    Choice,
    Fork,
    Join,
    Note,
    Composite,
    Region,
    Hidden,
}

fn kind_of(shape: Option<&str>) -> K {
    match shape {
        Some("rectWithTitle") => K::Described,
        Some("stateStart") => K::Start,
        Some("stateEnd") => K::End,
        Some("choice") => K::Choice,
        Some("fork") => K::Fork,
        Some("join") => K::Join,
        Some("note") => K::Note,
        Some("divider") => K::Hidden,
        _ => K::State,
    }
}

const CLASSES: [&str; 9] = [
    "pele-node pele-state",
    "pele-node pele-state",
    "pele-node pele-state-start",
    "pele-node pele-state-end",
    "pele-node pele-state-choice",
    "pele-node pele-state-fork",
    "pele-node pele-state-join",
    "pele-note",
    "pele-cluster pele-state-composite",
];

struct View {
    // Absent for the unseen helpers the layout needs.
    node: Option<usize>,
    kind: K,
    style: Option<ResolvedStyle>,
    label: Label,
    lines: Vec<Label>,
    w: f64,
    h: f64,
    title_h: f64,
}

fn vertical(dir: Option<Dir>) -> bool {
    matches!(dir, Some(Dir::TB) | Some(Dir::BT))
}

fn number_option(config: &Config, key: &str, fallback: f64) -> f64 {
    match config.get("state").and_then(|v| v.as_map()).and_then(|m| m.get(key)) {
        Some(Value::Num(v)) if *v > 0.0 => v.min(2000.0),
        _ => fallback,
    }
}

// Mermaid keeps the quotes of a click statement's strings and strips them when it draws.
fn unquote(text: &str) -> &str {
    text.trim_matches('"')
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

fn sign(v: f64) -> f64 {
    if v > 0.0 {
        1.0
    } else if v < 0.0 {
        -1.0
    } else {
        v
    }
}

fn rect(x: f64, y: f64, w: f64, h: f64, rest: &str) -> String {
    format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}/>", num(x), num(y), num(w), num(h), rest)
}

fn group(dir: Dir, pad_x: f64, pad_top: f64, pad_bottom: f64) -> CNode {
    let mut c = cnode(0.0, 0.0);
    c.is_group = true;
    c.dir = Some(dir);
    c.pad_x = pad_x;
    c.pad_top = pad_top;
    c.pad_bottom = pad_bottom;
    c
}

// node.cssClasses.replace(/(?:^| )statediagram-(?:state|cluster-alt|cluster|note)(?= |$)/g, '')
fn strip_mermaid_classes(s: &str) -> String {
    let b = s.as_bytes();
    let names = ["state", "cluster-alt", "cluster", "note"];
    let at = |i: usize| -> Option<usize> {
        let rest = &s[i..];
        let rest = rest.strip_prefix("statediagram-")?;
        for name in names {
            if let Some(after) = rest.strip_prefix(name) {
                if after.is_empty() || after.starts_with(' ') {
                    return Some(i + 13 + name.len());
                }
            }
        }
        None
    };
    let mut out = String::new();
    let mut i = 0;
    while i < s.len() {
        if i == 0 {
            if let Some(e) = at(0) {
                i = e;
                continue;
            }
        }
        if b[i] == b' ' {
            if let Some(e) = at(i + 1) {
                i = e;
                continue;
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

pub fn render_state(db: &StateDb, config: &Config, options: &RenderOptions) -> Rendered {
    // With no width limit, turnToFit draws across.
    draw(db, config, options)
}

fn draw(db: &StateDb, config: &Config, options: &RenderOptions) -> Rendered {
    let graph = &db.graph;
    // State diagrams follow the flowchart curve.
    let curve = config.get("flowchart").and_then(|v| v.as_map()).and_then(|m| m.get("curve"));
    let curve_str = match curve {
        Some(Value::Str(s)) => Some(s.as_str()),
        _ => None,
    };
    let size = 16.0;
    let small = js::round(size * 0.875);
    let wrap_width = number_option(config, "wrappingWidth", 200.0);
    let pad = 8.0;
    let root_dir = direction(Some(&db.direction()));
    let text = |raw: Option<&str>, font_size: f64, wrap: f64, style: u32| layout_label(raw, true, font_size, wrap, style);

    let nodes: &[StateNode] = &graph.nodes;
    let mut index: HashMap<&str, usize> = HashMap::new();
    let mut views: Vec<View> = Vec::new();
    let mut cnodes: Vec<CNode> = Vec::new();
    let new_view = |node: Option<usize>, kind: K| View { node, kind, style: None, label: Label::default(), lines: Vec::new(), w: 0.0, h: 0.0, title_h: 0.0 };
    for (ni, node) in nodes.iter().enumerate() {
        if node.shape.as_deref() == Some("noteGroup") {
            continue;
        }
        let kind = if node.is_group {
            if node.shape.as_deref() == Some("divider") {
                K::Region
            } else {
                K::Composite
            }
        } else {
            kind_of(node.shape.as_deref())
        };
        views.push(new_view(Some(ni), kind));
        index.insert(node.id.as_str(), views.len() - 1);
    }
    let states = views.len();
    let node_of = |views: &[View], i: usize| -> &StateNode { &nodes[views[i].node.unwrap()] };

    let mut logical: Vec<i64> = Vec::new();
    for i in 0..states {
        let parent = node_of(&views, i).parent_id.as_deref().and_then(|p| index.get(p).copied());
        logical.push(match parent {
            Some(pa) if pa != i && node_of(&views, pa).is_group => pa as i64,
            _ => -1,
        });
    }
    // Composites that are redeclared inside each other form a ring with no way in from the root.
    // One member of each ring moves to the root, which leaves a tree.
    {
        let mut kids: Vec<Vec<usize>> = vec![Vec::new(); states];
        let mut queue: Vec<usize> = Vec::new();
        let mut placed = vec![false; states];
        for i in 0..states {
            if logical[i] == -1 {
                placed[i] = true;
                queue.push(i);
            } else {
                kids[logical[i] as usize].push(i);
            }
        }
        let mut done = 0;
        let settle = |done: &mut usize, queue: &mut Vec<usize>, placed: &mut Vec<bool>| {
            while *done < queue.len() {
                for &kid in &kids[queue[*done]] {
                    if placed[kid] {
                        continue;
                    }
                    placed[kid] = true;
                    queue.push(kid);
                }
                *done += 1;
            }
        };
        settle(&mut done, &mut queue, &mut placed);
        let mut seen = vec![-1i64; states];
        for i in 0..states {
            if placed[i] {
                continue;
            }
            let mut ring = i;
            while seen[ring] != i as i64 {
                seen[ring] = i as i64;
                ring = logical[ring] as usize;
            }
            logical[ring] = -1;
            placed[ring] = true;
            queue.push(ring);
            settle(&mut done, &mut queue, &mut placed);
        }
    }
    let mut owners = vec![-1i64; states];
    for i in 0..states {
        let owner = node_of(&views, i).note_for.as_deref().and_then(|n| index.get(n).copied());
        let Some(owner) = owner else { continue };
        owners[i] = owner as i64;
        logical[i] = logical[owner];
    }

    // As in Mermaid, a composite that sets no direction runs top to bottom whatever surrounds it.
    let inner_dir = |views: &[View], composite: usize| -> Dir { direction(node_of(views, composite).dir.as_deref()) };
    // Whether the flow around a state runs top to bottom or bottom to top.
    let across = |views: &[View], logical: &[i64], i: usize| -> bool { vertical(Some(if logical[i] == -1 { root_dir } else { inner_dir(views, logical[i] as usize) })) };

    let mut degree = vec![0i32; states];
    let mut noted = vec![false; states];
    // When each state first takes part in a transition. Layout follows this order rather than
    // the order of declaration, so a state declared up front does not become the top of the flow.
    let mut appear: Vec<f64> = vec![f64::INFINITY; states];
    let mut appeared = 0.0;
    let mut counts = vec![0i32; 2 * states];
    for edge in &graph.edges {
        let (Some(&src), Some(&dst)) = (index.get(edge.start.as_str()), index.get(edge.end.as_str())) else { continue };
        if edge.dashed {
            noted[if views[src].kind == K::Note { dst } else { src }] = true;
            continue;
        }
        counts[2 * src] += 1;
        degree[src] = degree[src].max(counts[2 * src]);
        counts[2 * dst + 1] += 1;
        degree[dst] = degree[dst].max(counts[2 * dst + 1]);
        if appear[src] == f64::INFINITY {
            appear[src] = appeared;
            appeared += 1.0;
        }
        if appear[dst] == f64::INFINITY {
            appear[dst] = appeared;
            appeared += 1.0;
        }
    }

    for i in 0..states {
        let node = node_of(&views, i);
        let decls: Vec<String> = if !node.css_styles.is_empty() { node.css_compiled_styles.iter().chain(&node.css_styles).cloned().collect() } else { node.css_compiled_styles.clone() };
        let style = resolve_style(&decls);
        let text_style = (if style.bold { st::BOLD } else { 0 }) | (if style.italic { st::ITALIC } else { 0 });
        let font_size = style.font_size.unwrap_or(size);
        let kind = views[i].kind;
        let across_i = across(&views, &logical, i);
        let mut c: Option<CNode> = None;
        match kind {
            K::Composite => {
                let label = text(node.label.as_deref(), font_size, 4000.0, text_style);
                let title_h = if label.height > 0.0 { label.height + 12.0 } else { 0.0 };
                let mut g = group(inner_dir(&views, i), GROUP_PAD, title_h + GROUP_PAD, GROUP_PAD);
                g.min_w = label.width + 2.0 * GROUP_PAD;
                views[i].label = label;
                views[i].title_h = title_h;
                c = Some(g);
            }
            K::Region => c = Some(group(inner_dir(&views, i), REGION_PAD, REGION_PAD, REGION_PAD)),
            K::Start => {
                views[i].w = 14.0;
                views[i].h = 14.0;
            }
            K::End => {
                views[i].w = 18.0;
                views[i].h = 18.0;
            }
            K::Choice => {
                views[i].w = 30.0;
                views[i].h = 30.0;
            }
            K::Fork | K::Join => {
                let length = clamp(40.0 * degree[i] as f64 + 16.0, 72.0, 320.0);
                views[i].w = if across_i { length } else { BAR };
                views[i].h = if across_i { BAR } else { length };
            }
            K::Hidden => {}
            K::Note => {
                let label = text(node.label.as_deref(), small, wrap_width.max(NOTE_WRAP), 0);
                views[i].w = label.width + 24.0;
                views[i].h = label.height + 16.0;
                views[i].label = label;
            }
            K::State | K::Described => {
                let label = text(node.label.as_deref(), font_size, wrap_width, text_style);
                let mut width = label.width;
                let mut height = label.height + 2.0 * PAD_Y;
                if kind == K::Described {
                    height += 14.0;
                    for line in &node.description {
                        let l = text(Some(if line.is_empty() { " " } else { line }), small, wrap_width, text_style);
                        if l.width > width {
                            width = l.width;
                        }
                        height += l.height;
                        views[i].lines.push(l);
                    }
                }
                views[i].w = (width + 2.0 * PAD_X).max(56.0);
                views[i].h = height;
                views[i].label = label;
            }
        }
        views[i].style = Some(style);
        let mut c = c.unwrap_or_else(|| cnode(views[i].w, views[i].h));
        // The transitions on one side of a state's box spread along it, clear of its corners.
        if kind == K::State || kind == K::Described {
            c.span = (if across_i { views[i].w } else { views[i].h }) - 24.0;
        }
        cnodes.push(c);
    }

    let mut cedges: Vec<CEdge> = Vec::new();
    let mut edge_labels: Vec<Label> = Vec::new();
    let mut drawn: Vec<usize> = Vec::new();
    // The transitions each state makes to itself: how many, their widest label, and their labels' height.
    let mut loop_count = vec![0i32; states];
    let mut loop_width = vec![0f64; states];
    let mut loop_height = vec![0f64; states];
    // A transition between a composite and a state inside it runs between that state and a point
    // just inside the composite, and is then drawn on to the composite's border.
    let mut anchors: HashMap<usize, usize> = HashMap::new();
    let encloses = |cnodes: &[CNode], composite: usize, item: usize| -> bool {
        if !cnodes[composite].is_group {
            return false;
        }
        let mut p = logical[item];
        while p != -1 {
            if p as usize == composite {
                return true;
            }
            p = logical[p as usize];
        }
        false
    };
    for (ei, edge) in graph.edges.iter().enumerate() {
        let (Some(&s0), Some(&d0)) = (index.get(edge.start.as_str()), index.get(edge.end.as_str())) else { continue };
        let (mut src, mut dst) = (s0, d0);
        let label = text(Some(&edge.label), small, wrap_width, 0);
        let label_w = if label.width > 0.0 { label.width + 12.0 } else { 0.0 };
        if src == dst {
            loop_count[src] += 1;
            loop_width[src] = loop_width[src].max(label_w);
            loop_height[src] += label.height;
        } else if encloses(&cnodes, src, dst) {
            anchors.insert(cedges.len(), src);
            appear.push(-1.0);
            views.push(new_view(None, K::Hidden));
            let mut c = cnode(0.0, 0.0);
            c.parent = src as i64;
            cnodes.push(c);
            src = views.len() - 1;
        } else if encloses(&cnodes, dst, src) {
            anchors.insert(cedges.len(), dst);
            appear.push(f64::INFINITY);
            views.push(new_view(None, K::Hidden));
            let mut c = cnode(0.0, 0.0);
            c.parent = dst as i64;
            cnodes.push(c);
            dst = views.len() - 1;
        }
        drawn.push(ei);
        cedges.push(CEdge { src, dst, minlen: 1.0, label_w, label_h: if label.height > 0.0 { label.height + 4.0 } else { 0.0 }, route: Vec::new(), label_x: 0.0, label_y: 0.0 });
        edge_labels.push(label);
    }

    // A state with notes, or a composite with self-transitions, sits in an unseen group of its
    // own: the notes go beside the state, across the flow, without disturbing its transitions.
    for i in 0..states {
        if owners[i] != -1 {
            continue;
        }
        let across_i = across(&views, &logical, i);
        // Loops hang off the right, or below when the flow is horizontal, with their labels beyond them.
        let mut room = 0.0;
        if loop_count[i] > 0 {
            let reach = LOOP + (loop_count[i] - 1) as f64 * 8.0;
            room = if across_i { reach + loop_width[i] } else { reach * 0.75 + 4.0 + loop_height[i] };
            let item = &mut cnodes[i];
            if !item.is_group {
                if across_i {
                    item.w += 2.0 * room;
                    item.h = item.h.max(loop_height[i]);
                } else {
                    item.h += 2.0 * room;
                    item.w = item.w.max(loop_width[i]);
                }
                room = 0.0;
            }
        }
        if !noted[i] && room == 0.0 {
            cnodes[i].parent = logical[i];
            continue;
        }
        let mut c = if across_i { group(Dir::LR, room, 0.0, 0.0) } else { group(Dir::TB, 0.0, room, room) };
        c.parent = logical[i];
        appear.push(appear[i]);
        views.push(new_view(None, K::Hidden));
        cnodes.push(c);
        cnodes[i].parent = (views.len() - 1) as i64;
    }
    for i in 0..states {
        if owners[i] != -1 {
            cnodes[i].parent = cnodes[owners[i] as usize].parent;
        }
    }

    {
        // Numbers the tree so that, at every level, initial states come first and the rest follow
        // in the order the transitions introduce them.
        let mut members: Vec<Vec<usize>> = vec![Vec::new(); cnodes.len()];
        let mut top: Vec<usize> = Vec::new();
        for i in 0..cnodes.len() {
            let p = cnodes[i].parent;
            if p >= 0 {
                members[p as usize].push(i);
            } else {
                top.push(i);
            }
        }
        let initial = |i: usize| if views[i].kind == K::Start { 0 } else { 1 };
        let by_flow = |a: &usize, b: &usize| -> std::cmp::Ordering {
            let (a, b) = (*a, *b);
            initial(a)
                .cmp(&initial(b))
                .then_with(|| {
                    if appear[a] == appear[b] {
                        std::cmp::Ordering::Equal
                    } else if appear[a] < appear[b] {
                        std::cmp::Ordering::Less
                    } else {
                        std::cmp::Ordering::Greater
                    }
                })
                .then(a.cmp(&b))
        };
        let mut seq = 0.0;
        top.sort_by(by_flow);
        let mut pending: Vec<usize> = top.into_iter().rev().collect();
        while let Some(i) = pending.pop() {
            cnodes[i].seq = seq;
            seq += 1.0;
            let mut inside = std::mem::take(&mut members[i]);
            inside.sort_by(by_flow);
            for &k in inside.iter().rev() {
                pending.push(k);
            }
            members[i] = inside;
        }
    }

    let layout = compound_layout(
        &mut cnodes,
        &mut cedges,
        root_dir,
        &LayeredOptions {
            node_sep: number_option(config, "nodeSpacing", NODE_SEP),
            edge_sep: EDGE_SEP,
            rank_sep: number_option(config, "rankSpacing", RANK_SEP),
            port_sep: 20.0,
            tracks: matches!(curve_str, Some("step" | "stepBefore" | "stepAfter")),
            straight: curve_str == Some("linear"),
            head_room: marker_trim("arrow_point"),
            ..Default::default()
        },
    );

    let width = (layout.width + 2.0 * pad).ceil();
    let height = (layout.height + 2.0 * pad).ceil();
    let mut links: Vec<LinkInfo> = Vec::new();

    // Route points by rounded y, to find the edges that cross a composite's title bar.
    let mut crossings: HashMap<i64, Vec<f64>> = HashMap::new();
    for e in &cedges {
        let route = &e.route;
        let mut k = 3;
        while k + 3 < route.len() {
            crossings.entry(js::round(route[k + 1]) as i64).or_default().push(route[k]);
            k += 3;
        }
    }

    let mut children: Vec<Vec<usize>> = vec![Vec::new(); cnodes.len()];
    let mut order: Vec<usize> = Vec::new();
    for i in 0..cnodes.len() {
        let p = cnodes[i].parent;
        if p >= 0 {
            children[p as usize].push(i);
        } else {
            order.push(i);
        }
    }
    let roots = order.clone();
    let mut q = 0;
    while q < order.len() {
        let kids = children[order[q]].clone();
        order.extend(kids);
        q += 1;
    }

    let mut clusters = String::new();
    let mut fills: Vec<Fill> = Vec::new();
    let mut dividers = String::new();
    let mut nodes_out = String::new();
    let mut notes_out = String::new();
    // Draws the dashed lines between the regions among a composite's members.
    let divide = |dividers: &mut String, items: &[usize], left: f64, top: f64, right: f64, bottom: f64| {
        let regions: Vec<usize> = items.iter().copied().filter(|&i| views[i].kind == K::Region).collect();
        if regions.len() < 2 {
            return;
        }
        let lo = |i: usize, x: bool| if x { cnodes[i].x - cnodes[i].w / 2.0 } else { cnodes[i].y - cnodes[i].h / 2.0 };
        let hi = |i: usize, x: bool| if x { cnodes[i].x + cnodes[i].w / 2.0 } else { cnodes[i].y + cnodes[i].h / 2.0 };
        for x in [true, false] {
            let mut sorted = regions.clone();
            sorted.sort_by(|&a, &b| js::ord(js::or(lo(a, x) - lo(b, x), || a as f64 - b as f64)));
            let apart = (1..sorted.len()).all(|k| hi(sorted[k - 1], x) <= lo(sorted[k], x) + 0.5);
            if !apart {
                continue;
            }
            for k in 1..sorted.len() {
                let at = num((hi(sorted[k - 1], x) + lo(sorted[k], x)) / 2.0 + pad);
                let d = if x { format!("{},{}V{}", at, num(top), num(bottom)) } else { format!("{},{}H{}", num(left), at, num(right)) };
                dividers.push_str(&format!("<path class=\"pele-divider\" d=\"M{}\"/>", d));
            }
            return;
        }
        // Regions that sit neither side by side nor in a stack each get an outline.
        for &i in &regions {
            let c = &cnodes[i];
            dividers.push_str(&rect(c.x - c.w / 2.0 + pad, c.y - c.h / 2.0 + pad, c.w, c.h, &format!(" class=\"pele-divider\" rx=\"{}\"", js::num_str(CORNER))));
        }
    };
    divide(&mut dividers, &roots, pad / 2.0, pad / 2.0, width - pad / 2.0, height - pad / 2.0);

    for &i in &order {
        let view = &views[i];
        let (Some(ni), Some(style)) = (view.node, view.style.as_ref()) else { continue };
        if view.kind >= K::Region {
            continue;
        }
        let node = &nodes[ni];
        let c = &cnodes[i];
        let x = c.x + pad;
        let y = c.y + pad;
        let (w, h) = (view.w, view.h);
        let ink = ink_fill(&style.shape, &style.text, None);
        let label_attrs = format!(" class=\"pele-label\"{}{}", ink, style.text);
        let mut body: String;

        match view.kind {
            K::Composite => {
                let left = x - c.w / 2.0;
                let top = y - c.h / 2.0;
                if paints(&style.shape) {
                    fills.push(Fill { bx: [left, top, left + c.w, top + c.h], shape: style.shape.clone() });
                }
                body = rect(
                    left,
                    top,
                    c.w,
                    c.h,
                    &format!(" rx=\"{}\" fill=\"var(--_a)\"{} stroke=\"var(--_b)\"{}", js::num_str(CORNER), if sets_fill(&style.shape) { "" } else { " fill-opacity=\"0.5\"" }, style.shape),
                );
                if view.title_h > 0.0 {
                    body.push_str(&format!("<path d=\"M{},{}H{}\"{}{}/>", num(left), num(top + view.title_h), num(left + c.w), RULE, style.line));
                    body.push_str(&label_svg(&view.label, title_x(c, view.label.width, &crossings) + pad, top + view.title_h / 2.0, &format!(" class=\"pele-cluster-label\"{}{}", ink, style.text)));
                }
                divide(&mut dividers, &children[i], left, top + view.title_h, left + c.w, top + c.h);
            }
            K::Start => body = format!("<circle r=\"7\" fill=\"var(--_l)\"{}/>", style.shape),
            K::End => body = format!("<circle r=\"8.5\" fill=\"var(--_bg)\" stroke=\"var(--_l)\"{}/><circle r=\"5\" fill=\"var(--_l)\"{}/>", style.line, style.shape),
            K::Choice => body = format!("<polygon points=\"0,{} {},0 0,{} {},0\"{}{}/>", num(-h / 2.0), num(w / 2.0), num(h / 2.0), num(-w / 2.0), SURFACE, style.shape),
            K::Fork | K::Join => body = rect(-w / 2.0, -h / 2.0, w, h, &format!(" rx=\"2\" fill=\"var(--_l)\"{}", style.shape)),
            K::Note => {
                let r = num(w / 2.0);
                let b = num(h / 2.0);
                let fold = num(w / 2.0 - FOLD);
                let corner = num(FOLD - h / 2.0);
                body = format!(
                    "<path d=\"M-{r},-{b}H{fold}L{r},{corner}V{b}H-{r}Z\" fill=\"var(--_bg)\" stroke=\"var(--_b)\"/><path d=\"M{fold},-{b}V{corner}H{r}\"{RULE}/>{}",
                    label_svg(&view.label, 0.0, 0.0, &format!(" class=\"pele-label\" font-size=\"{}\" fill=\"var(--_m)\"", js::num_str(small)))
                );
            }
            _ => {
                body = rect(-w / 2.0, -h / 2.0, w, h, &format!(" rx=\"{}\"{}{}", num(CORNER.min(h / 2.0)), SURFACE, style.shape));
                if view.kind == K::State {
                    body.push_str(&label_svg(&view.label, 0.0, 0.0, &label_attrs));
                } else {
                    let mut at = PAD_Y - h / 2.0 + view.label.height;
                    body.push_str(&format!("<path d=\"M{},{}H{}\"{}{}/>", num(-w / 2.0), num(at + 6.0), num(w / 2.0), RULE, style.line));
                    body.push_str(&label_svg(&view.label, 0.0, at - view.label.height / 2.0, &label_attrs));
                    at += 14.0;
                    for line in &view.lines {
                        body.push_str(&label_svg(line, 0.0, at + line.height / 2.0, &format!(" class=\"pele-state-description\" font-size=\"{}\"{}{}", js::num_str(small), ink, style.text)));
                        at += line.height;
                    }
                }
            }
        }

        if let Some(link) = db.link(&node.id) {
            // Entity codes are resolved first, so the URL is checked in the form a browser would follow.
            let tooltip = decode_entities(unquote(&link.tooltip));
            if !tooltip.is_empty() {
                body = format!("<title>{}</title>{}", esc(&tooltip), body);
            }
            body = linked(&body, &safe_url(unquote(&link.url)), &node.id, &mut links, " target=\"_blank\"");
        }
        let open = format!("<g class=\"{}{}\" data-id=\"{}\"", CLASSES[view.kind as usize], class_names(&strip_mermaid_classes(&node.css_classes)), esc_text(&node.id));
        match view.kind {
            K::Composite => clusters.push_str(&format!("{}>{}</g>", open, body)),
            K::Note => notes_out.push_str(&format!("{} transform=\"translate({},{})\">{}</g>", open, num(x), num(y), body)),
            _ => nodes_out.push_str(&format!("{} transform=\"translate({},{})\">{}</g>", open, num(x), num(y), body)),
        }
    }

    // Moves a route end from the layout box onto the drawn shape, and spreads the ends along a bar.
    let attach = |route: &mut [f64], at: usize, i: usize| {
        let c = &cnodes[i];
        if c.is_group {
            return;
        }
        let view = &views[i];
        let bar = view.kind == K::Fork || view.kind == K::Join;
        let other = if at == 0 { 3 } else { at - 3 };
        // 0 is the x axis and 1 the y axis; the end sits on a side that faces along `axis`.
        for axis in [1usize, 0] {
            let extent = if axis == 1 { c.h } else { c.w };
            let drawn_extent = if axis == 1 { view.h } else { view.w };
            let breadth = if axis == 1 { view.w } else { view.h };
            let center = (if axis == 1 { c.y } else { c.x }) + pad;
            let cross = (if axis == 1 { c.x } else { c.y }) + pad;
            let delta = route[at + axis] - center;
            if (delta.abs() - extent / 2.0).abs() >= 0.5 {
                continue;
            }
            route[at + axis] -= (sign(delta) * (extent - drawn_extent)) / 2.0;
            if bar && breadth > drawn_extent {
                route[at + 1 - axis] = clamp(route[other + 1 - axis], cross - breadth / 2.0 + 6.0, cross + breadth / 2.0 - 6.0);
            }
            return;
        }
    };

    let mut reach = SideReach::default();
    for (i, ce) in cedges.iter().enumerate() {
        if ce.src == ce.dst || ce.route.len() < 6 || graph.edges[drawn[i]].dashed {
            continue;
        }
        reach.add(ce.dst, end_side(&ce.route, ce.route.len() - 3, &cnodes[ce.dst]), "arrow_point");
    }

    let mut casings = String::new();
    let mut edges_out = String::new();
    let mut labels_out = String::new();
    let mut loop_seen = vec![0i32; states];
    let mut loop_text = vec![0f64; states];
    for i in 0..cedges.len() {
        let edge = &graph.edges[drawn[i]];
        let label = &edge_labels[i];
        let note = edge.dashed;
        let end_type = if note { "none" } else { "arrow_point" };
        let path: EdgePath;
        let (src, dst) = (cedges[i].src, cedges[i].dst);
        if src == dst {
            let a = &cnodes[src];
            let view = &views[src];
            let k = loop_seen[src] as f64;
            loop_seen[src] += 1;
            // The loop hangs off a side the flow does not use: the right, or below when the flow is horizontal.
            let side = across(&views, &logical, src);
            let w = if a.is_group { a.w } else { view.w };
            let h = if a.is_group { a.h } else { view.h };
            let out = (if side { w } else { h }) / 2.0;
            let spread = ((if side { h } else { w }) / 2.0 - 4.0).min(8.0 + k * 6.0).max(2.0);
            let lreach = LOOP + k * 8.0;
            let hy = js::hypot(lreach, spread);
            let trim = marker_trim(end_type) / if hy == 0.0 { 1.0 } else { hy };
            // Offsets are given for a loop on the right and turned a quarter for one below.
            let px = |dx: f64, dy: f64| a.x + pad + if side { out + dx } else { -dy };
            let py = |dx: f64, dy: f64| a.y + pad + if side { dy } else { out + dx };
            let pt = |dx: f64, dy: f64| format!("{},{}", num(px(dx, dy)), num(py(dx, dy)));
            let (mut edx, mut edy) = (px(-lreach, -spread) - px(0.0, 0.0), py(-lreach, -spread) - py(0.0, 0.0));
            let len = js::hypot(edx, edy);
            let len = if len == 0.0 { 1.0 } else { len };
            edx /= len;
            edy /= len;
            path = EdgePath {
                d: format!("M{}C{} {} {}", pt(0.0, -spread), pt(lreach, -2.0 * spread), pt(lreach, 2.0 * spread), pt(lreach * trim, spread + spread * trim)),
                tokens: None,
                sx: 0.0,
                sy: 0.0,
                sdx: 0.0,
                sdy: 0.0,
                ex: px(0.0, spread),
                ey: py(0.0, spread),
                edx,
                edy,
            };
            // Labels stack beyond the outermost loop, in the order of their transitions.
            let furthest = LOOP + (loop_count[src] - 1) as f64 * 8.0;
            let below = loop_text[src] + label.height / 2.0;
            loop_text[src] += label.height;
            let ce = &mut cedges[i];
            ce.label_x = if side { a.x + out + furthest + 4.0 + ce.label_w / 2.0 } else { a.x };
            ce.label_y = if side { a.y - loop_height[src] / 2.0 + below } else { a.y + out + furthest * 0.75 + 4.0 + below };
        } else {
            let mut route = cedges[i].route.clone();
            let leaves = reach.of(src, end_side(&route, 0, &cnodes[src]));
            let arrives = reach.of(dst, end_side(&route, route.len() - 3, &cnodes[dst]));
            let mut k = 0;
            while k < route.len() {
                route[k] += pad;
                route[k + 1] += pad;
                k += 3;
            }
            attach(&mut route, 0, src);
            let last = route.len() - 3;
            attach(&mut route, last, dst);
            if let Some(&composite) = anchors.get(&i) {
                if route.len() >= 6 {
                    let g = &cnodes[composite];
                    let inward = views[src].kind == K::Hidden;
                    let at = if inward { 0 } else { route.len() - 3 };
                    let near = matches!(g.dir, Some(Dir::TB) | Some(Dir::LR)) == inward;
                    let point = if vertical(g.dir) {
                        [route[at], g.y + pad + if near { views[composite].title_h - g.h / 2.0 } else { g.h / 2.0 }, 0.0]
                    } else {
                        [g.x + pad + if near { -g.w / 2.0 } else { g.w / 2.0 }, route[at + 1], 1.0]
                    };
                    if inward {
                        route.splice(0..0, point);
                    } else {
                        route.extend_from_slice(&point);
                    }
                }
            }
            path = route_path(&route, curve_str, 0.0, marker_trim(end_type), Some(leaves), Some(arrives));
        }
        let over = over_fills(&path.d, &[path.ex, path.ey], &fills, "", path.tokens.as_deref());
        if over.as_deref() == Some("") {
            casings.push_str(&edge_casing(&path, "none", end_type, "", false));
        }
        let over_or = |fallback: &str| -> String { over.clone().filter(|o| !o.is_empty()).unwrap_or_else(|| fallback.to_string()) };
        let kind = if note {
            format!("pele-note-edge\" stroke=\"{}\" stroke-dasharray=\"3 4", over_or("var(--_b)"))
        } else if over.as_deref().is_some_and(|o| !o.is_empty()) {
            format!("pele-transition\" stroke=\"{}", over.clone().unwrap())
        } else {
            "pele-transition".to_string()
        };
        edges_out.push_str(&format!(
            "<g class=\"pele-edge {}\" data-id=\"{}\"><path d=\"{}\"/>{}</g>",
            kind,
            esc_text(&edge.id),
            path.d,
            marker(end_type, path.ex, path.ey, path.edx, path.edy, &over_or("var(--_l)"), "none")
        ));
        if label.width > 0.0 {
            labels_out.push_str(&edge_label_svg(&edge.id, label, cedges[i].label_x + pad, cedges[i].label_y + pad, ""));
        }
    }

    let mut content = String::new();
    if !clusters.is_empty() {
        content.push_str(&format!("<g class=\"pele-clusters\">{}</g>", clusters));
    }
    if !dividers.is_empty() {
        content.push_str(&format!("<g class=\"pele-dividers\" stroke-dasharray=\"4 4\"{}>{}</g>", RULE, dividers));
    }
    content.push_str(&casings_svg(&casings));
    if !edges_out.is_empty() {
        content.push_str(&format!("<g class=\"pele-edges\" fill=\"none\" stroke=\"var(--_l)\" stroke-linecap=\"round\">{}</g>", edges_out));
    }
    if !labels_out.is_empty() {
        content.push_str(&format!("<g class=\"pele-edge-labels\" font-size=\"{}\">{}</g>", js::num_str(small), labels_out));
    }
    content.push_str(&format!("<g class=\"pele-nodes\">{}{}</g>", nodes_out, notes_out));
    let titled = with_title(db.title.as_deref(), size, pad, width, height, content);
    let svg = svg_document("state", titled.width, titled.height, size, options.responsive, &Accessible { acc_title: db.acc_title.as_deref(), acc_descr: db.acc_descr.as_deref() }, &titled.content);
    Rendered { svg, width: titled.width, height: titled.height, links }
}

// Picks where a composite's title sits in its title bar so that edges entering from above do not cross it.
fn title_x(c: &CNode, width: f64, crossings: &HashMap<i64, Vec<f64>>) -> f64 {
    let Some(xs) = crossings.get(&(js::round(c.y - c.h / 2.0) as i64)) else { return c.x };
    if width == 0.0 {
        return c.x;
    }
    let left = c.x - c.w / 2.0;
    let half = width / 2.0;
    let mut best = c.x;
    let mut best_gap = -1.0;
    for x in [c.x, left + GROUP_PAD + half, left + c.w - GROUP_PAD - half] {
        let mut gap = f64::INFINITY;
        for &cx in xs {
            if cx > left && cx < left + c.w {
                gap = gap.min((cx - x).abs() - half);
            }
        }
        if gap >= 6.0 {
            return x;
        }
        if gap > best_gap + 0.5 {
            best_gap = gap;
            best = x;
        }
    }
    best
}
