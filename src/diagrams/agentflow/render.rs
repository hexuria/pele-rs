// Port of src/diagrams/agentflow/render.ts, for the default render options the host uses: no
// width limit (so the diagram is never turned to run down), 16px text and no icon resolver.

use super::db::AgentflowDb;
use super::graph::{kind_slot, AgentGraph, GraphEdge, KIND_COUNT};
use super::types::LabelType;
use crate::diagrams::flowchart::shapes::canonical_shape;
use crate::js;
use crate::layout::compound::{cnode, compound_layout, direction, inherit_crossed_directions, shift_layout, CEdge, CNode, Dir};
use crate::layout::layered::LayeredOptions;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num};
use crate::svg::cluster::{cluster_title_x, mark_crossings, Crossings};
use crate::svg::edges::{casings_svg, edge_casing, edge_label_svg, loop_path, marker, marker_trim, over_fills, route_path, EdgePath, Fill, LoopLabel};
use crate::svg::ink::{ink_fill, paints};
use crate::svg::root::{svg_document, Accessible};
use crate::svg::shapes::{draw_shape, inset_route, shape_filled, shape_size, PAD_X, PAD_Y};
use crate::svg::theme::{class_names, resolve_style, rounded, series_color, sets_fill, ResolvedStyle};
use crate::text::entities::decode_entities;
use crate::text::label::{layout_label, Label};
use crate::text::measurer::style;
use crate::types::{LinkInfo, RenderOptions, Rendered};
use crate::util::url::{internal_link, linked, rel_attr};
use crate::util::yaml::Value;
use std::collections::HashMap;

// The number of series tokens, which is the palette Mermaid's slot rules are applied to.
pub const PALETTE: i64 = 8;

const NODE_SEP: f64 = 40.0;
const EDGE_SEP: f64 = 16.0;
const RANK_SEP: f64 = 48.0;
const GROUP_PAD: f64 = 20.0;
const LOOP: f64 = 26.0;
const DOTS: f64 = 14.0;
const COLLAPSED: &str = "collapsed";

// The side a flow runs toward, with sides numbered clockwise from the top.
fn flow_end(dir: Dir) -> usize {
    match dir {
        Dir::TB => 2,
        Dir::RL => 3,
        Dir::BT => 0,
        Dir::LR => 1,
    }
}

fn number_option(config: &Config, key: &str, fallback: f64) -> f64 {
    match config.get("agentflow").and_then(|s| s.as_map()).and_then(|s| s.get(key)) {
        Some(Value::Num(v)) if *v > 0.0 => v.min(2000.0),
        _ => fallback,
    }
}

// A palette color as an outline with a light wash of the same color inside.
// A fill the author set replaces the wash and stays opaque.
fn tint(slot: i64, wash: &str, st: &ResolvedStyle) -> String {
    let color = series_color(slot);
    format!(" fill=\"{}\"{} stroke=\"{}\"", color, if sets_fill(&st.shape) { String::new() } else { format!(" fill-opacity=\"{}\"", wash) }, color)
}

fn dot(x: f64, y: f64) -> String {
    format!("M{},{}a1.75,1.75 0 1 0 3.5,0a1.75,1.75 0 1 0 -3.5,0Z", num(x - 1.75), num(y))
}

struct NodeView {
    style: ResolvedStyle,
    label: Label,
    shape: String,
    w: f64,
    h: f64,
    dy: f64,
}

struct Held<'a> {
    edge: &'a GraphEdge,
    label: Label,
    src: usize,
    dst: usize,
}

// The direction of the level a node sits in.
fn flow_of(cnodes: &[CNode], root_dir: Dir, i: usize) -> Dir {
    let mut dir: Option<Dir> = None;
    let mut p = cnodes[i].parent;
    let mut hops = 0;
    while p >= 0 && dir.is_none() && hops < 10000 {
        dir = cnodes[p as usize].dir;
        p = cnodes[p as usize].parent;
        hops += 1;
    }
    dir.unwrap_or(root_dir)
}

fn flows_sideways(cnodes: &[CNode], root_dir: Dir, i: usize) -> bool {
    matches!(flow_of(cnodes, root_dir, i), Dir::LR | Dir::RL)
}

pub fn render_agentflow(db: &AgentflowDb, graph: &AgentGraph, config: &Config, options: &RenderOptions) -> Rendered {
    let size = 16.0;
    let edge_size = js::round(size * 0.875);
    let wrap_width = number_option(config, "wrappingWidth", 120.0);
    let min_width = number_option(config, "minNodeWidth", 120.0);
    let pad = 8.0;

    let mut index: HashMap<&str, usize> = HashMap::new();
    let mut views: Vec<NodeView> = Vec::new();
    let mut cnodes: Vec<CNode> = Vec::new();
    for (i, node) in graph.nodes.iter().enumerate() {
        index.insert(&node.id, i);
    }

    for node in &graph.nodes {
        let decls: Vec<String> = if !node.css_styles.is_empty() { node.css_compiled_styles.iter().chain(node.css_styles.iter()).cloned().collect() } else { node.css_compiled_styles.clone() };
        let st = resolve_style(&decls);
        let text_style = (if st.bold { style::BOLD } else { 0 }) | (if st.italic { style::ITALIC } else { 0 });
        let markdown = node.label_type == LabelType::Markdown;
        let font_size = st.font_size.unwrap_or(size);
        let mut c;
        let view;
        if node.is_group {
            let label = layout_label(node.label.as_deref(), markdown, font_size, 4000.0, text_style);
            c = cnode(0.0, 0.0);
            c.is_group = true;
            c.dir = node.dir.as_deref().map(|d| direction(Some(d)));
            c.pad_x = GROUP_PAD;
            c.pad_top = if label.height > 0.0 { label.height + 16.0 } else { GROUP_PAD };
            c.pad_bottom = GROUP_PAD;
            c.min_w = label.width + 2.0 * GROUP_PAD - 12.0;
            view = NodeView { style: st, label, shape: "group".into(), w: 0.0, h: 0.0, dy: 0.0 };
        } else if node.shape == "collapsedGroup" {
            let label = layout_label(node.label.as_deref(), markdown, font_size, wrap_width, text_style);
            let w = label.width.max(min_width) + 2.0 * PAD_X;
            let h = label.height + 2.0 * PAD_Y + DOTS;
            c = cnode(w, h);
            view = NodeView { style: st, label, shape: COLLAPSED.into(), w, h, dy: -DOTS / 2.0 };
        } else {
            let shape = canonical_shape(Some(&node.shape));
            let label = layout_label(node.label.as_deref(), markdown, font_size, wrap_width, text_style);
            // A diamond grows twice as fast as its label, so it keeps the label's own width.
            let s = shape_size(&shape, if shape == "diam" { label.width } else { label.width.max(min_width) }, label.height);
            c = cnode(s.w, s.h);
            view = NodeView { style: st, label, shape, w: s.w, h: s.h, dy: s.dy };
        }
        c.seq = views.len() as f64;
        views.push(view);
        cnodes.push(c);
    }
    for (i, node) in graph.nodes.iter().enumerate() {
        cnodes[i].parent = match &node.parent_id {
            Some(p) => index.get(p.as_str()).map_or(-1, |&x| x as i64),
            None => -1,
        };
    }
    let ends: Vec<(i64, i64)> = graph.edges.iter().map(|e| (index.get(e.start.as_str()).map_or(-1, |&x| x as i64), index.get(e.end.as_str()).map_or(-1, |&x| x as i64))).collect();
    inherit_crossed_directions(&mut cnodes, &ends);

    let root_dir = direction(db.direction.as_deref());

    let n = cnodes.len();
    let mut depth: Vec<i64> = vec![-1; n];
    for i in 0..n {
        let mut chain: Vec<usize> = Vec::new();
        let mut j = i as i64;
        while j >= 0 && depth[j as usize] < 0 && chain.len() <= n {
            chain.push(j as usize);
            j = cnodes[j as usize].parent;
        }
        let d = if j >= 0 { depth[j as usize] + 1 } else { 0 };
        for (dk, &k) in (d..).zip(chain.iter().rev()) {
            depth[k] = dk;
        }
    }
    let contains = |cnodes: &[CNode], group: usize, i: usize| -> bool {
        let mut j = i as i64;
        let mut d = depth[i];
        while d > depth[group] && j >= 0 {
            j = cnodes[j as usize].parent;
            d -= 1;
        }
        j == group as i64 && i != group
    };

    let mut cedges: Vec<CEdge> = Vec::new();
    let mut edge_labels: Vec<Label> = Vec::new();
    let mut drawn: Vec<&GraphEdge> = Vec::new();
    // Edges between a node and a container around it. The layout has no place for them, so they
    // are drawn afterwards, between the node and the container's border.
    let mut held: Vec<Held> = Vec::new();
    for edge in &graph.edges {
        let (Some(&src), Some(&dst)) = (index.get(edge.start.as_str()), index.get(edge.end.as_str())) else { continue };
        let label = layout_label(Some(&edge.label), edge.label_type == LabelType::Markdown, edge_size, wrap_width, 0);
        if contains(&cnodes, src, dst) || contains(&cnodes, dst, src) {
            held.push(Held { edge, label, src, dst });
            continue;
        }
        if src == dst {
            // A self-loop sits beside the node, across the flow, and the node's box grows to hold it.
            if flows_sideways(&cnodes, root_dir, src) {
                cnodes[src].h += 2.0 * (LOOP + if label.height > 0.0 { label.height + 8.0 } else { 0.0 });
            } else {
                cnodes[src].w += 2.0 * (LOOP + if label.width > 0.0 { label.width + 12.0 } else { 0.0 });
            }
        }
        cedges.push(CEdge {
            src,
            dst,
            minlen: edge.minlen.unwrap_or(1.0),
            label_w: if label.width > 0.0 { label.width + 12.0 } else { 0.0 },
            label_h: if label.height > 0.0 { label.height + 4.0 } else { 0.0 },
            route: Vec::new(),
            label_x: 0.0,
            label_y: 0.0,
        });
        drawn.push(edge);
        edge_labels.push(label);
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
            ..Default::default()
        },
    );

    let title = layout_label(db.title.as_deref(), false, size, 4000.0, style::BOLD);
    let title_height = if title.height > 0.0 { title.height + 12.0 } else { 0.0 };
    let inner = layout.width.max(title.width);
    let ox = pad + (inner - layout.width) / 2.0;
    let oy = pad + title_height;
    shift_layout(&mut cnodes, &mut cedges, ox, oy);
    let width = (inner + 2.0 * pad).ceil();
    let height = (layout.height + title_height + 2.0 * pad).ceil();
    let mut links: Vec<LinkInfo> = Vec::new();

    // An edge between a node and a container around it runs straight between the node and the
    // container's border: out along the flow, or in against it. If another node is in the way it
    // takes a side instead, and failing that the far end.
    let mut held_paths: Vec<EdgePath> = Vec::new();
    let mut held_labels: Vec<f64> = Vec::new();
    let mut held_routes: Vec<Vec<f64>> = Vec::new();
    let careful = ((held.len() * cnodes.len()) as f64) < 2e6;
    for h in &held {
        let out = contains(&cnodes, h.dst, h.src);
        let outer = if out { h.dst } else { h.src };
        let i = if out { h.src } else { h.dst };
        let g = &cnodes[outer];
        let nn = &cnodes[i];
        let end = flow_end(g.dir.unwrap_or_else(|| flow_of(&cnodes, root_dir, outer)));
        let first = if out { end } else { (end + 2) & 3 };
        let toward = |side: usize| -> Vec<f64> {
            let upright = side & 1 == 0;
            let sign = if side == 0 || side == 3 { -1.0 } else { 1.0 };
            let nx = if upright { nn.x } else { nn.x + (sign * nn.w) / 2.0 };
            let ny = if upright { nn.y + (sign * nn.h) / 2.0 } else { nn.y };
            let bx = if upright { nn.x } else { g.x + (sign * g.w) / 2.0 };
            let by = if upright { g.y + (sign * g.h) / 2.0 } else { nn.y };
            let axis = if upright { 0.0 } else { 1.0 };
            if out {
                vec![nx, ny, axis, bx, by, axis]
            } else {
                vec![bx, by, axis, nx, ny, axis]
            }
        };
        let blocked = |r: &[f64]| -> bool {
            cnodes.iter().enumerate().any(|(k, c)| {
                k != i
                    && !c.is_group
                    && c.x + c.w / 2.0 > r[0].min(r[3]) - 1.0
                    && c.x - c.w / 2.0 < r[0].max(r[3]) + 1.0
                    && c.y + c.h / 2.0 > r[1].min(r[4]) - 1.0
                    && c.y - c.h / 2.0 < r[1].max(r[4]) + 1.0
            })
        };
        let mut route = toward(first);
        if careful && blocked(&route) {
            for side in [(first + 1) & 3, (first + 3) & 3, (first + 2) & 3] {
                let other = toward(side);
                if !blocked(&other) {
                    route = other;
                    break;
                }
            }
        }
        held_labels.push((route[0] + route[3]) / 2.0);
        held_labels.push((route[1] + route[4]) / 2.0);
        let mut drawn_route = route.clone();
        inset_route(&mut drawn_route, if out { 0 } else { 3 }, &views[i].shape, views[i].w, views[i].h, nn);
        held_paths.push(route_path(&drawn_route, Some("linear"), 0.0, marker_trim(&h.edge.arrow_type_end), None, None));
        held_routes.push(route);
    }

    let mut crossings: Crossings = HashMap::new();
    if cnodes.iter().any(|c| c.is_group) {
        for e in &cedges {
            mark_crossings(&mut crossings, &e.route);
        }
        for route in &held_routes {
            mark_crossings(&mut crossings, route);
        }
    }

    let mut clusters = String::new();
    let mut nodes_out = String::new();
    let mut fills: Vec<Fill> = Vec::new();
    for i in 0..views.len() {
        let view = &views[i];
        let c = &cnodes[i];
        let node = &graph.nodes[i];
        let (x, y) = (c.x, c.y);
        let classes = class_names(node.css_classes.strip_prefix("default").map(|s| s.strip_prefix(|ch: char| js::is_space(ch)).unwrap_or(s)).unwrap_or(&node.css_classes));
        let id = esc_text(&node.id);
        let slot = node.color_index.unwrap_or(KIND_COUNT);
        if c.is_group {
            if paints(&view.style.shape) {
                fills.push(Fill { bx: [x - c.w / 2.0, y - c.h / 2.0, x + c.w / 2.0, y + c.h / 2.0], shape: view.style.shape.clone() });
            }
            let title_x = cluster_title_x(c, view.label.width, &crossings, GROUP_PAD);
            clusters.push_str(&format!(
                "<g class=\"pele-cluster pele-flow{}\" data-id=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}/>{}</g>",
                classes,
                id,
                num(x - c.w / 2.0),
                num(y - c.h / 2.0),
                num(c.w),
                num(c.h),
                rounded(&(tint(slot, "0.06", &view.style) + &view.style.shape)),
                label_svg(
                    &view.label,
                    title_x,
                    y - c.h / 2.0 + 8.0 + view.label.height / 2.0,
                    &format!(" class=\"pele-cluster-label\"{}{}", ink_fill(&view.style.shape, &view.style.text, Some("var(--_m)")), view.style.text)
                )
            ));
            continue;
        }

        let (w, h) = (view.w, view.h);
        let mut kind = String::new();
        let shape = if view.shape == COLLAPSED {
            let color = series_color(slot);
            let line = h / 2.0 - DOTS - 2.0;
            let dots = line + (DOTS + 2.0) / 2.0;
            format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}/><path d=\"M{},{}H{}\" fill=\"none\" stroke=\"{}\" stroke-dasharray=\"3 3\"{}/><path class=\"pele-collapsed-mark\" d=\"{}{}{}\" fill=\"{}\" stroke=\"none\"/>",
                num(-w / 2.0),
                num(-h / 2.0),
                num(w),
                num(h),
                rounded(&(tint(slot, "0.06", &view.style) + &view.style.shape)),
                num(-w / 2.0 + 10.0),
                num(line),
                num(w / 2.0 - 10.0),
                color,
                view.style.line,
                dot(-9.0, dots),
                dot(0.0, dots),
                dot(9.0, dots),
                color
            )
        } else {
            let k = node.kind.unwrap_or("task");
            let slot = kind_slot(k).unwrap_or(1);
            kind = format!(" pele-kind-{}", k);
            draw_shape(&view.shape, w, h, &(tint(slot, "0.14", &view.style) + &view.style.shape), &format!(" stroke=\"{}\"{}", series_color(slot), view.style.line))
        };

        let ink = if view.shape == COLLAPSED || shape_filled(&view.shape) { ink_fill(&view.style.shape, &view.style.text, None) } else { String::new() };
        let mut text = label_svg(&view.label, 0.0, view.dy, &format!(" class=\"pele-label\"{}{}", ink, view.style.text));
        let internal = format!("{} ", classes).contains(" internal-link ");
        if internal && node.label.as_deref().is_some_and(|l| !l.is_empty()) {
            // The label is a note name, but it still ends up in an href, so it gets the same check as a URL.
            let name = decode_entities(node.label.as_deref().unwrap());
            let safe = internal_link(&name);
            let target = esc(&name);
            text = format!("<a class=\"internal-link\"{} data-href=\"{}\">{}</a>", if safe { format!(" href=\"{}\"{}", target, rel_attr()) } else { String::new() }, target, text);
            links.push(LinkInfo { id: node.id.clone(), href: name, internal: true });
        }
        let mut body = shape + &text;
        if let Some(tip) = node.tooltip.as_deref().filter(|t| !t.is_empty()) {
            body = format!("<title>{}</title>{}", esc_text(tip), body);
        }
        if let Some(link) = node.link.as_deref().filter(|l| !l.is_empty()) {
            let attrs = match node.link_target.as_deref().filter(|t| !t.is_empty()) {
                Some(t) => format!(" target=\"{}\"", esc(t)),
                None => String::new(),
            };
            body = linked(&body, link, &node.id, &mut links, &attrs);
        }
        nodes_out.push_str(&format!("<g class=\"pele-node pele-shape-{}{}{}\" data-id=\"{}\" transform=\"translate({},{})\">{}</g>", view.shape, kind, classes, id, num(x), num(y), body));
    }

    let mut casings = String::new();
    let mut edges_out = String::new();
    let mut labels_out = String::new();
    let mut put = |edge: &GraphEdge, label: &Label, path: &EdgePath, x: f64, y: f64| {
        let st = if !edge.style.is_empty() || !edge.css_compiled_styles.is_empty() {
            let decls: Vec<String> = edge.css_compiled_styles.iter().chain(edge.style.iter()).cloned().collect();
            Some(resolve_style(&decls))
        } else {
            None
        };
        let line = st.as_ref().map_or("", |s| s.line.as_str());
        let end_type = edge.arrow_type_end.as_str();
        let over = over_fills(&path.d, &[path.ex, path.ey], &fills, line, path.tokens.as_deref());
        let color = match over.as_deref() {
            Some(o) if !o.is_empty() => o.to_string(),
            _ => match st.as_ref().and_then(|s| s.stroke.as_deref()) {
                Some(s) if !s.is_empty() => esc(s),
                _ => "var(--_l)".to_string(),
            },
        };
        let dotted = edge.thickness.as_deref() == Some("dotted");
        let kind = if dotted {
            "reference"
        } else if end_type == "arrow_cross" {
            "failure"
        } else {
            "sequence"
        };
        if over.as_deref() == Some("") {
            casings.push_str(&edge_casing(path, "none", end_type, line, false));
        }
        let over_attr = match over.as_deref() {
            Some(o) if !o.is_empty() => format!(" stroke=\"{}\"", o),
            _ => String::new(),
        };
        edges_out.push_str(&format!(
            "<g class=\"pele-edge pele-edge-{}\" data-id=\"{}\"{}{}><path d=\"{}\"{}/>{}</g>",
            kind,
            esc_text(&edge.id),
            over_attr,
            line,
            path.d,
            if dotted { " stroke-dasharray=\"3 4\"" } else { "" },
            marker(end_type, path.ex, path.ey, path.edx, path.edy, &color, "none")
        ));
        if label.width > 0.0 {
            labels_out.push_str(&edge_label_svg(&edge.id, label, x, y, st.as_ref().map_or("", |s| s.text.as_str())));
        }
    };

    let mut loop_seen: HashMap<usize, usize> = HashMap::new();
    for i in 0..cedges.len() {
        let edge = drawn[i];
        let end_type = edge.arrow_type_end.as_str();
        let (src, dst) = (cedges[i].src, cedges[i].dst);
        let path = if src == dst {
            let view = &views[src];
            let k = *loop_seen.get(&src).unwrap_or(&0);
            loop_seen.insert(src, k + 1);
            let ce = &cedges[i];
            let mut ll = LoopLabel { label_w: ce.label_w, label_h: ce.label_h, label_x: ce.label_x, label_y: ce.label_y };
            let c = &cnodes[src];
            let p = loop_path(c.x, c.y, view.w, view.h, k, flows_sideways(&cnodes, root_dir, src), LOOP, marker_trim(end_type), &mut ll);
            cedges[i].label_x = ll.label_x;
            cedges[i].label_y = ll.label_y;
            p
        } else {
            let mut route = cedges[i].route.clone();
            inset_route(&mut route, 0, &views[src].shape, views[src].w, views[src].h, &cnodes[src]);
            let last = route.len() - 3;
            inset_route(&mut route, last, &views[dst].shape, views[dst].w, views[dst].h, &cnodes[dst]);
            route_path(&route, edge.curve.as_deref(), 0.0, marker_trim(end_type), None, None)
        };
        put(edge, &edge_labels[i], &path, cedges[i].label_x, cedges[i].label_y);
    }
    for (i, h) in held.iter().enumerate() {
        put(h.edge, &h.label, &held_paths[i], held_labels[2 * i], held_labels[2 * i + 1]);
    }

    let mut content = label_svg(&title, width / 2.0, pad + title.height / 2.0, " class=\"pele-title\" font-weight=\"var(--_tw)\"");
    if !clusters.is_empty() {
        content.push_str(&format!("<g class=\"pele-clusters\">{}</g>", clusters));
    }
    content.push_str(&casings_svg(&casings));
    if !edges_out.is_empty() {
        content.push_str(&format!("<g class=\"pele-edges\" fill=\"none\" stroke=\"var(--_l)\" stroke-linecap=\"round\">{}</g>", edges_out));
    }
    if !labels_out.is_empty() {
        content.push_str(&format!("<g class=\"pele-edge-labels\" font-size=\"{}\">{}</g>", js::num_str(edge_size), labels_out));
    }
    content.push_str(&format!("<g class=\"pele-nodes\">{}</g>", nodes_out));
    let svg = svg_document("agentflow", width, height, size, options.responsive, &Accessible { acc_title: db.acc_title.as_deref(), acc_descr: db.acc_descr.as_deref() }, &content);
    Rendered { svg, width, height, links }
}
