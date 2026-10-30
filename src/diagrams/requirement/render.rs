// Port of src/diagrams/requirement/render.ts and graph.ts.

use super::RequirementDb;
use crate::diagrams::common::fit_width::{runs_across, turn_to_fit};
use crate::js;
use crate::layout::compound::{cnode, compound_layout, direction, CEdge, CNode, Dir};
use crate::layout::layered::LayeredOptions;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num, shared_style, span_style, text_style};
use crate::svg::edges::{edge_label_svg, route_path, EdgePath};
use crate::svg::ink::ink_fill;
use crate::svg::root::{svg_document, Accessible};
use crate::svg::theme::{class_names, resolve_style, rounded, ResolvedStyle};
use crate::svg::title::title_label;
use crate::text::label::{layout_label, line_middle, Label};
use crate::text::measurer::{style, width as measure};
use crate::types::{RenderOptions, Rendered};

const NODE_SEP: f64 = 40.0;
const EDGE_SEP: f64 = 16.0;
const RANK_SEP: f64 = 56.0;
const PAD_X: f64 = 14.0;
const PAD_Y: f64 = 10.0;
const ROW_GAP: f64 = 2.0;
const COLUMN_GAP: f64 = 12.0;
const MIN_WIDTH: f64 = 120.0;
const NAME_WRAP: f64 = 280.0;
const VALUE_WRAP: f64 = 240.0;
const LOOP: f64 = 26.0;
const CONTAINS_RADIUS: f64 = 6.0;
const ARROW_LENGTH: f64 = 9.0;
const ARROW_HALF: f64 = 4.5;
const END_STEP: f64 = 28.0;
const END_MARGIN: f64 = 14.0;

struct Row {
    key: Label,
    value: Label,
    height: f64,
}

struct GraphNode<'a> {
    id: &'a str,
    requirement: bool,
    css_styles: &'a [String],
    css_classes: String,
}

struct NodeView<'a> {
    node: GraphNode<'a>,
    style: ResolvedStyle,
    kind: Label,
    name: Label,
    rows: Vec<Row>,
    key_width: f64,
    head_height: f64,
    w: f64,
    h: f64,
}

struct GraphEdge {
    id: String,
    start: Option<String>,
    end: Option<String>,
    type_: String,
    contains: bool,
}

// Adds bold or italic to a whole label after its markdown is read, since emphasis markers only
// toggle styles the label does not start with.
fn restyle(mut label: Label, bits: u32) -> Label {
    if bits == 0 || label.lines.is_empty() {
        return label;
    }
    let mut width = 0.0;
    let mut widths = Vec::new();
    let size = label.size;
    for line in label.lines.iter_mut() {
        let mut total = 0.0;
        for span in line.iter_mut() {
            span.style |= bits;
            span.width = measure(&span.text, size, span.style);
            total += span.width;
        }
        widths.push(total);
        if total > width {
            width = total;
        }
    }
    label.widths = widths;
    label.width = width;
    label
}

// Writes a measured label with its lines starting at x, the first line's box starting at top.
fn start_text(label: &Label, x: f64, top: f64, attrs: &str) -> String {
    let shared = shared_style(label);
    let mut body = String::new();
    for (i, line) in label.lines.iter().enumerate() {
        let middle = top + line_middle(label, i);
        let y = middle + label.size * 0.35;
        let mut first = true;
        for span in line {
            if span.text.is_empty() {
                continue;
            }
            let pos = if first { format!(" x=\"{}\" y=\"{}\"", num(x), num(y)) } else { String::new() };
            body.push_str(&format!("<tspan{}{}>{}</tspan>", pos, span_style(span.style & !shared), esc(&span.text)));
            first = false;
        }
    }
    if body.is_empty() {
        String::new()
    } else {
        format!("<text{} text-anchor=\"start\" xml:space=\"preserve\">{}</text>", text_style(attrs, shared), body)
    }
}

// The containment mark: a circled plus whose rim touches (x, y), where (dx, dy) points at the node.
fn contains_marker(x: f64, y: f64, dx: f64, dy: f64) -> String {
    let r = CONTAINS_RADIUS;
    let cx = x - dx * r;
    let cy = y - dy * r;
    format!(
        "<circle class=\"pele-marker\" cx=\"{}\" cy=\"{}\" r=\"{}\"/><path class=\"pele-marker\" d=\"M{},{}L{},{}M{},{}L{},{}\"/>",
        num(cx),
        num(cy),
        js::num_str(r),
        num(cx - dx * r),
        num(cy - dy * r),
        num(cx + dx * r),
        num(cy + dy * r),
        num(cx + dy * r),
        num(cy - dx * r),
        num(cx - dy * r),
        num(cy + dx * r)
    )
}

// An open arrowhead with its tip at (x, y), pointing along (dx, dy).
fn arrow_marker(x: f64, y: f64, dx: f64, dy: f64) -> String {
    let bx = x - dx * ARROW_LENGTH;
    let by = y - dy * ARROW_LENGTH;
    format!(
        "<path class=\"pele-marker\" d=\"M{},{}L{},{}L{},{}\" stroke-linejoin=\"round\"/>",
        num(bx - dy * ARROW_HALF),
        num(by + dx * ARROW_HALF),
        num(x),
        num(y),
        num(bx + dy * ARROW_HALF),
        num(by - dx * ARROW_HALF)
    )
}

// The layout attaches every edge to the middle of a node's side. Ends that share a side are
// spread along it, in the order of where they head, so their markers do not sit on each other.
fn spread_ends(cnodes: &[CNode], cedges: &mut [CEdge], views: &[NodeView]) {
    // (edge, at, toward), by node * 4 + side, in insertion order.
    let mut sides: Vec<(usize, Vec<(usize, usize, f64)>)> = Vec::new();
    for (e, edge) in cedges.iter().enumerate() {
        let route = &edge.route;
        if edge.src == edge.dst || route.len() < 6 {
            continue;
        }
        for at in [0, route.len() - 3] {
            let node = if at == 0 { edge.src } else { edge.dst };
            let c = &cnodes[node];
            let dx = route[at] - c.x;
            let dy = route[at + 1] - c.y;
            let next = if at == 0 { 3 } else { at - 3 };
            let side: usize = if (dy.abs() - c.h / 2.0).abs() < 0.5 {
                if dy < 0.0 {
                    0
                } else {
                    2
                }
            } else if (dx.abs() - c.w / 2.0).abs() < 0.5 {
                if dx < 0.0 {
                    3
                } else {
                    1
                }
            } else {
                continue;
            };
            let key = node * 4 + side;
            let end = (e, at, route[if side % 2 == 0 { next } else { next + 1 }]);
            match sides.iter_mut().find(|(k, _)| *k == key) {
                Some((_, list)) => list.push(end),
                None => sides.push((key, vec![end])),
            }
        }
    }
    for (key, mut list) in sides {
        if list.len() < 2 {
            continue;
        }
        let node = key >> 2;
        let vertical = key % 2 == 1;
        let span = (if vertical { views[node].h } else { views[node].w }) - 2.0 * END_MARGIN;
        let step = 0f64.max(END_STEP.min(span / (list.len() - 1) as f64));
        list.sort_by(|a, b| js::ord(a.2 - b.2));
        let n = list.len() as f64;
        for (j, &(e, at, _)) in list.iter().enumerate() {
            let offset = (j as f64 - (n - 1.0) / 2.0) * step;
            if vertical {
                cedges[e].route[at + 1] = cnodes[node].y + offset;
            } else {
                cedges[e].route[at] = cnodes[node].x + offset;
            }
        }
    }
}

pub fn render_requirement(model: &RequirementDb, config: &Config, options: &RenderOptions) -> Rendered {
    if !runs_across(Some(&model.direction)) {
        return draw(model, config, options, false);
    }
    turn_to_fit(options, |down| draw(model, config, options, down))
}

// `.replace(/^default\s?/, '')`
fn without_default(classes: &str) -> &str {
    match classes.strip_prefix("default") {
        Some(rest) => rest.strip_prefix(|c: char| js::is_space(c)).unwrap_or(rest),
        None => classes,
    }
}

// `turned` draws a diagram that runs across as one that runs down.
fn draw(model: &RequirementDb, _config: &Config, options: &RenderOptions, turned: bool) -> Rendered {
    // The nodes and edges to lay out: requirements first, then elements, as Mermaid orders them.
    let mut graph_nodes: Vec<(GraphNode, Option<&super::Requirement>, Option<&super::RequirementElement>)> = Vec::new();
    for r in &model.requirements {
        graph_nodes.push((GraphNode { id: &r.name, requirement: true, css_styles: &r.css_styles, css_classes: r.classes.join(" ") }, Some(r), None));
    }
    for e in &model.elements {
        graph_nodes.push((GraphNode { id: &e.name, requirement: false, css_styles: &e.css_styles, css_classes: e.classes.join(" ") }, None, Some(e)));
    }
    let has = |id: &str| if model.requirements.iter().any(|r| r.name == id) || model.elements.iter().any(|e| e.name == id) { Some(id.to_string()) } else { None };
    let graph_edges: Vec<GraphEdge> = model
        .relations
        .iter()
        .enumerate()
        .map(|(index, r)| GraphEdge { id: format!("{}-{}-{}", r.src, r.dst, index), start: has(&r.src), end: has(&r.dst), type_: r.type_.clone(), contains: r.type_ == "contains" })
        .collect();

    let size = 16.0;
    let small = js::round(size * 0.875);
    let pad = 8.0;

    let mut index: Vec<(&str, usize)> = Vec::new();
    let mut views: Vec<NodeView> = Vec::new();
    let mut cnodes: Vec<CNode> = Vec::new();
    for (node, requirement, element) in graph_nodes {
        let st = resolve_style(node.css_styles);
        let base = (if st.bold { style::BOLD } else { 0 }) | (if st.italic { style::ITALIC } else { 0 });
        let name_size = st.font_size.unwrap_or(size);
        let body_size = st.font_size.unwrap_or(small);
        let kind = restyle(layout_label(Some(&format!("«{}»", requirement.map_or("Element", |r| r.type_.as_str()))), false, body_size, 4000.0, 0), base);
        let name = restyle(layout_label(Some(node.id), true, name_size, NAME_WRAP, 0), base | style::BOLD);
        let fields: Vec<(&str, &str)> = match (requirement, element) {
            (Some(r), _) => vec![("ID", &r.requirement_id), ("Text", &r.text), ("Risk", &r.risk), ("Verification", &r.verify_method)],
            (_, Some(e)) => vec![("Type", &e.type_), ("Doc Ref", &e.doc_ref)],
            _ => Vec::new(),
        };
        let mut rows: Vec<Row> = Vec::new();
        let mut key_width: f64 = 0.0;
        let mut value_width: f64 = 0.0;
        let mut body_height = 0.0;
        for (label, text) in fields {
            if text.is_empty() {
                continue;
            }
            let key = restyle(layout_label(Some(label), false, body_size, 4000.0, 0), base);
            let value = restyle(layout_label(Some(text), true, body_size, VALUE_WRAP, 0), base);
            let height = key.height.max(value.height);
            key_width = key_width.max(key.width);
            value_width = value_width.max(value.width);
            rows.push(Row { key, value, height });
            body_height += height + ROW_GAP;
        }
        if !rows.is_empty() {
            body_height += 2.0 * PAD_Y - ROW_GAP;
        }
        let head_height = 2.0 * PAD_Y + kind.height + name.height;
        let w = MIN_WIDTH.max(kind.width.max(name.width) + 2.0 * PAD_X).max(if !rows.is_empty() { key_width + COLUMN_GAP + value_width + 2.0 * PAD_X } else { 0.0 });
        // A requirement and an element may share a name; relationships then go to the requirement.
        if !index.iter().any(|(k, _)| *k == node.id) {
            index.push((node.id, views.len()));
        }
        let mut c = cnode(w, head_height + body_height);
        c.seq = views.len() as f64;
        views.push(NodeView { node, style: st, kind, name, rows, key_width, head_height, w, h: head_height + body_height });
        cnodes.push(c);
    }
    let index_of = |id: &str| index.iter().find(|(k, _)| *k == id).map(|(_, v)| *v);

    let root_dir = if turned { Dir::TB } else { direction(Some(&model.direction)) };
    let sideways = matches!(root_dir, Dir::LR | Dir::RL);
    let mut cedges: Vec<CEdge> = Vec::new();
    let mut edge_labels: Vec<Label> = Vec::new();
    let mut drawn: Vec<&GraphEdge> = Vec::new();
    for edge in &graph_edges {
        let src = edge.start.as_deref().and_then(index_of);
        let dst = edge.end.as_deref().and_then(index_of);
        let (Some(src), Some(dst)) = (src, dst) else { continue };
        let label = layout_label(Some(&format!("«{}»", edge.type_)), false, small, 4000.0, 0);
        if src == dst {
            // A self-loop sits beside the node, across the flow, and the node's box grows to hold it.
            if sideways {
                cnodes[src].h += 2.0 * (LOOP + label.height + 8.0);
            } else {
                cnodes[src].w += 2.0 * (LOOP + label.width + 12.0);
            }
        }
        drawn.push(edge);
        cedges.push(CEdge { src, dst, minlen: 1.0, label_w: label.width + 12.0, label_h: label.height + 4.0, route: Vec::new(), label_x: 0.0, label_y: 0.0 });
        edge_labels.push(label);
    }

    let layout = compound_layout(&mut cnodes, &mut cedges, root_dir, &LayeredOptions { node_sep: NODE_SEP, edge_sep: EDGE_SEP, rank_sep: RANK_SEP, port_sep: 20.0, ..Default::default() });

    spread_ends(&cnodes, &mut cedges, &views);

    let title = title_label(model.title.as_deref(), size, 4000.0);
    let title_height = if title.height > 0.0 { title.height + 12.0 } else { 0.0 };
    let inner = layout.width.max(title.width);
    let ox = pad + (inner - layout.width) / 2.0;
    let oy = pad + title_height;
    let width = (inner + 2.0 * pad).ceil();
    let height = (layout.height + title_height + 2.0 * pad).ceil();

    let mut nodes_out = String::new();
    for (i, view) in views.iter().enumerate() {
        let (w, h, st) = (view.w, view.h, &view.style);
        let color = ink_fill(&st.shape, &st.text, None);
        let muted = if color.is_empty() { " fill=\"var(--_m)\"".to_string() } else { color.clone() };
        let left = -w / 2.0;
        let top = -h / 2.0;
        let mut body = format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_s)\" stroke=\"var(--_b)\"{}/>", num(left), num(top), num(w), num(h), rounded(&st.shape));
        body.push_str(&label_svg(&view.kind, 0.0, top + PAD_Y + view.kind.height / 2.0, &format!(" class=\"pele-stereotype\" font-size=\"{}\"{}{}", js::num_str(view.kind.size), muted, st.text)));
        body.push_str(&label_svg(
            &view.name,
            0.0,
            top + PAD_Y + view.kind.height + view.name.height / 2.0,
            &format!(" class=\"pele-label\" font-size=\"{}\" font-weight=\"var(--_hw)\"{}{}", js::num_str(view.name.size), color, st.text),
        ));
        if !view.rows.is_empty() {
            let divider = top + view.head_height;
            body.push_str(&format!("<path class=\"pele-divider\" d=\"M{},{}H{}\" stroke=\"var(--_b)\"{}/>", num(left), num(divider), num(-left), st.line));
            let mut y = divider + PAD_Y;
            let mut rows = String::new();
            for row in &view.rows {
                rows.push_str(&start_text(&row.key, left + PAD_X, y, &format!(" class=\"pele-field\"{}{}", muted, st.text)));
                rows.push_str(&start_text(&row.value, left + PAD_X + view.key_width + COLUMN_GAP, y, &format!(" class=\"pele-value\"{}{}", color, st.text)));
                y += row.height + ROW_GAP;
            }
            body.push_str(&format!("<g class=\"pele-fields\" font-size=\"{}\">{}</g>", js::num_str(view.rows[0].key.size), rows));
        }
        nodes_out.push_str(&format!(
            "<g class=\"pele-node {}{}\" data-id=\"{}\" transform=\"translate({},{})\">{}</g>",
            if view.node.requirement { "pele-requirement" } else { "pele-element" },
            class_names(without_default(&view.node.css_classes)),
            esc_text(view.node.id),
            num(cnodes[i].x + ox),
            num(cnodes[i].y + oy),
            body
        ));
    }

    let mut edges_out = String::new();
    let mut labels_out = String::new();
    let mut loop_seen: Vec<(usize, usize)> = Vec::new();
    for (i, ce) in cedges.iter().enumerate() {
        let edge = drawn[i];
        let label = &edge_labels[i];
        let start_trim = if edge.contains { 2.0 * CONTAINS_RADIUS } else { 0.0 };
        let path: EdgePath;
        let mut label_x = ce.label_x + ox;
        let mut label_y = ce.label_y + oy;
        if ce.src == ce.dst {
            let view = &views[ce.src];
            let cx = cnodes[ce.src].x + ox;
            let cy = cnodes[ce.src].y + oy;
            let k = match loop_seen.iter_mut().find(|(n, _)| *n == ce.src) {
                Some((_, k)) => {
                    *k += 1;
                    *k - 1
                }
                None => {
                    loop_seen.push((ce.src, 1));
                    0
                }
            } as f64;
            let half = (if sideways { view.w } else { view.h }) / 2.0;
            let spread = (half - 4.0).min(12.0 + k * 6.0);
            let reach = LOOP + k * 8.0;
            // Local frame: `out` points away from the node, `along` runs along its side.
            let at = |out: f64, along: f64| -> String {
                if sideways {
                    format!("{},{}", num(cx + along), num(cy + view.h / 2.0 + out))
                } else {
                    format!("{},{}", num(cx + view.w / 2.0 + out), num(cy + along))
                }
            };
            // A cubic whose control points are this far out peaks at `reach`.
            let pull = reach / 0.75;
            path = EdgePath {
                d: format!("M{}C{} {} {}", at(start_trim, -spread), at(pull, -spread), at(pull, spread), at(0.0, spread)),
                tokens: None,
                sx: if sideways { cx - spread } else { cx + view.w / 2.0 },
                sy: if sideways { cy + view.h / 2.0 } else { cy - spread },
                sdx: if sideways { 0.0 } else { -1.0 },
                sdy: if sideways { -1.0 } else { 0.0 },
                ex: if sideways { cx + spread } else { cx + view.w / 2.0 },
                ey: if sideways { cy + view.h / 2.0 } else { cy + spread },
                edx: if sideways { 0.0 } else { -1.0 },
                edy: if sideways { -1.0 } else { 0.0 },
            };
            if sideways {
                label_x = cx;
                label_y = cy + view.h / 2.0 + reach + 4.0 + ce.label_h / 2.0;
            } else {
                label_x = cx + view.w / 2.0 + reach + 4.0 + ce.label_w / 2.0;
                label_y = cy;
            }
        } else {
            let mut route = ce.route.clone();
            let mut k = 0;
            while k < route.len() {
                route[k] += ox;
                route[k + 1] += oy;
                k += 3;
            }
            path = route_path(&route, None, start_trim, 0.0, None, None);
        }
        edges_out.push_str(&format!(
            "<g class=\"pele-edge pele-relationship-{}\" data-id=\"{}\"><path d=\"{}\"{}/>{}</g>",
            if edge.contains { "contains" } else { "arrow" },
            esc_text(&edge.id),
            path.d,
            if edge.contains { "" } else { " stroke-dasharray=\"5 4\"" },
            if edge.contains { contains_marker(path.sx, path.sy, path.sdx, path.sdy) } else { arrow_marker(path.ex, path.ey, path.edx, path.edy) }
        ));
        labels_out.push_str(&edge_label_svg(&edge.id, label, label_x, label_y, ""));
    }

    let mut content = label_svg(&title, width / 2.0, pad + title.height / 2.0, " class=\"pele-title\" font-weight=\"var(--_tw)\"");
    if !edges_out.is_empty() {
        content.push_str(&format!("<g class=\"pele-edges\" fill=\"none\" stroke=\"var(--_l)\" stroke-linecap=\"round\">{}</g>", edges_out));
    }
    if !labels_out.is_empty() {
        content.push_str(&format!("<g class=\"pele-edge-labels\" font-size=\"{}\" fill=\"var(--_m)\">{}</g>", js::num_str(small), labels_out));
    }
    content.push_str(&format!("<g class=\"pele-nodes\">{}</g>", nodes_out));
    let acc = Accessible { acc_title: model.acc_title.as_deref(), acc_descr: model.acc_descr.as_deref() };
    let svg = svg_document("requirement", width, height, size, options.responsive, &acc, &content);
    Rendered { svg, width, height, links: Vec::new() }
}
