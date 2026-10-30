// Port of src/diagrams/usecase/render.ts. With no width limit a diagram that runs across stays
// across (see common/fit_width.rs).

use super::graph::{build_usecase_graph, GraphEdge, GraphNode};
use super::UsecaseModel;
use crate::diagrams::common::fit_width::{runs_across, turn_to_fit};
use crate::js;
use crate::layout::compound::{cnode, compound_layout, direction, shift_layout, CEdge, CNode, Dir};
use crate::layout::layered::LayeredOptions;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num};
use crate::svg::cluster::{mark_crossings, Crossings};
use crate::svg::edges::{casing, casings_svg, edge_ink, edge_label_svg, fill_under, marker, marker_trim, route_path, EdgePath, Fill, Under};
use crate::svg::ink::{ink_fill, paints};
use crate::svg::root::{svg_document, Accessible};
use crate::svg::theme::{class_names, resolve_style, rounded, sets_fill, ResolvedStyle};
use crate::svg::title::title_label;
use crate::text::label::{label_line, layout_label, line_middle, Label};
use crate::text::measurer::style as text_style;
use crate::types::{RenderOptions, Rendered};
use crate::util::yaml::Value;

const NODE_SEP: f64 = 32.0;
const EDGE_SEP: f64 = 16.0;
const RANK_SEP: f64 = 72.0;
const GROUP_PAD: f64 = 20.0;
const LOOP: f64 = 26.0;
const FIGURE_W: f64 = 40.0;
const FIGURE_H: f64 = 60.0;
const FIGURE_GAP: f64 = 6.0;
const TAB_PAD: f64 = 6.0;
const CELL_PAD: f64 = 10.0;
const ROW_PAD: f64 = 8.0;
const FOLD: f64 = 9.0;
const EXTENSION: f64 = 12.0;
const PORT_GAP: f64 = 14.0;
const SHAPE_ATTRS: &str = " fill=\"var(--_s)\" stroke=\"var(--_b)\"";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Actor,
    Ellipse,
    Box,
    Json,
    Boundary,
}

struct View<'a> {
    node: &'a GraphNode,
    kind: Kind,
    style: ResolvedStyle,
    label: Label,
    stereotype: Label,
    // The size of what is drawn; the layout box is larger when the node carries a self-relationship.
    w: f64,
    h: f64,
    rows: Vec<(Label, Label)>,
    key_w: f64,
    head_h: f64,
}

fn number_option(config: &Config, key: &str, fallback: f64) -> f64 {
    match config.get("usecase").and_then(|v| v.as_map()).and_then(|m| m.get(key)) {
        Some(Value::Num(v)) if *v > 0.0 => v.min(2000.0),
        _ => fallback,
    }
}

// A plain label is shown as written: no tags, no markdown, no `\n` breaks. Its markup characters
// are handed to the label parser as character references, which it turns back into characters.
fn literal(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' | '<' | '>' | '\\' => out.push_str(&format!("&#{};", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn text(raw: Option<&str>, markdown: bool, size: f64, wrap: f64, base: u32) -> Label {
    match raw {
        Some(r) if !r.is_empty() && !markdown => layout_label(Some(&literal(r)), markdown, size, wrap, base),
        _ => layout_label(raw, markdown, size, wrap, base),
    }
}

// The point of a node's outline on one of its four sides (0 top, 1 right, 2 bottom, 3 left),
// `along` away from the middle of that side. Lines meet an actor at its figure, not at its label.
fn anchor(view: &View, c: &CNode, side: usize, along: f64) -> (f64, f64) {
    let sign = if side == 1 || side == 2 { 1.0 } else { -1.0 };
    let upright = side == 1 || side == 3;
    let mut reach = (if upright { view.w } else { view.h }) / 2.0;
    let mut middle = 0.0;
    if view.kind == Kind::Actor {
        if upright {
            reach = FIGURE_W / 2.0 + 3.0;
            middle = (FIGURE_H - view.h) / 2.0;
        } else {
            reach += 2.0;
        }
    } else if view.kind == Kind::Ellipse {
        let span = (if upright { view.h } else { view.w }) / 2.0;
        reach *= (1.0 - (along / span).powi(2)).max(0.0).sqrt();
    }
    if upright {
        (c.x + sign * reach, c.y + middle + along)
    } else {
        (c.x + along, c.y + sign * reach)
    }
}

// How much of a side edges may spread over.
fn side_length(view: &View, side: usize) -> f64 {
    let upright = side == 1 || side == 3;
    if view.kind == Kind::Actor {
        return if upright { FIGURE_H * 0.5 } else { FIGURE_W * 0.6 };
    }
    let length = if upright { view.h } else { view.w };
    if view.kind == Kind::Ellipse {
        length * 0.6
    } else {
        length - 16.0
    }
}

// The layout brings every edge that ends on one side of a node to the middle of that side. This
// spaces them along the side in the order they fan out, and puts each on the node's outline.
fn attach_ends(cedges: &mut [CEdge], cnodes: &[CNode], views: &[View]) {
    // (edge, at, toward), by node * 4 + side, in insertion order.
    let mut sides: Vec<(usize, Vec<(usize, usize, f64)>)> = Vec::new();
    let add = |sides: &mut Vec<(usize, Vec<(usize, usize, f64)>)>, e: usize, index: usize, route: &[f64], at: usize, next: usize| {
        let c = &cnodes[index];
        if c.is_group {
            return;
        }
        let dx = route[at] - c.x;
        let dy = route[at + 1] - c.y;
        let side = if (dy.abs() - c.h / 2.0).abs() < 0.5 {
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
            return;
        };
        let key = index * 4 + side;
        let end = (e, at, route[next + if side & 1 == 1 { 1 } else { 0 }]);
        match sides.iter_mut().find(|(k, _)| *k == key) {
            Some((_, list)) => list.push(end),
            None => sides.push((key, vec![end])),
        }
    };
    for (i, e) in cedges.iter().enumerate() {
        if e.route.len() < 6 || e.src == e.dst {
            continue;
        }
        let last = e.route.len() - 3;
        add(&mut sides, i, e.src, &e.route, 0, 3);
        add(&mut sides, i, e.dst, &e.route, last, last - 3);
    }
    for (key, mut ends) in sides {
        let index = key >> 2;
        let side = key & 3;
        let view = &views[index];
        let step = if ends.len() > 1 { PORT_GAP.min(side_length(view, side) / (ends.len() - 1) as f64) } else { 0.0 };
        ends.sort_by(|a, b| js::ord(a.2 - b.2));
        let n = ends.len() as f64;
        for (i, &(e, at, _)) in ends.iter().enumerate() {
            let (x, y) = anchor(view, &cnodes[index], side, (i as f64 - (n - 1.0) / 2.0) * step);
            cedges[e].route[at] = x;
            cedges[e].route[at + 1] = y;
        }
    }
}

// The hollow triangle of a generalization, with its tip at (x, y).
fn extension(x: f64, y: f64, dx: f64, dy: f64, color: &str) -> String {
    let bx = x - dx * EXTENSION;
    let by = y - dy * EXTENSION;
    let half = EXTENSION * 0.5;
    format!(
        "<path class=\"pele-marker\" d=\"M{},{}L{},{}L{},{}Z\" fill=\"var(--_bg)\" stroke=\"{}\" stroke-linejoin=\"round\"/>",
        num(x),
        num(y),
        num(bx - dy * half),
        num(by + dx * half),
        num(bx + dy * half),
        num(by - dx * half),
        color
    )
}

fn trim(type_: &str) -> f64 {
    if type_ == "extension" {
        EXTENSION
    } else {
        marker_trim(type_)
    }
}

fn mark(type_: &str, x: f64, y: f64, dx: f64, dy: f64, color: &str, stroke: &str) -> String {
    if type_ == "extension" {
        extension(x, y, dx, dy, color)
    } else {
        marker(type_, x, y, dx, dy, color, stroke)
    }
}

// Port of svg/edges.ts hollowOn: markers that show the page through take the fill they sit on.
fn hollow_on(markers: &str, fill: &Fill) -> String {
    let shape = &fill.shape;
    let mut color = None;
    let b = shape.as_bytes();
    let mut from = 0;
    while let Some(p) = shape[from..].find("fill:") {
        let at = from + p;
        if at > 0 && (b[at - 1] == b'"' || b[at - 1] == b';') {
            let s = at + 5;
            let e = shape[s..].find([';', '"']).map_or(shape.len(), |q| q + s);
            color = Some(js::trim(&shape[s..e]).to_string());
            break;
        }
        from = at + 1;
    }
    match color.filter(|c| !c.is_empty()) {
        Some(c) => markers.replace(" fill=\"var(--_bg)\"", &format!(" fill=\"{}\"", c)),
        None => markers.to_string(),
    }
}

fn figure(view: &View) -> String {
    let node = view.node;
    let shape = &view.style.shape;
    let line = format!(" fill=\"none\" stroke=\"var(--_b)\"{}", view.style.line);
    let mut head = -21.0;
    let mut radius = 9.0;
    let mut out = match node.actor_type.as_deref() {
        Some("hollow") => {
            radius = 8.0;
            format!(
                "<circle cy=\"-21\" r=\"8\" fill=\"var(--_bg)\" stroke=\"var(--_b)\"{}/><path d=\"M-20,-9H20V0H5.5L20,15.5L12,25.5L0,12L-12,25.5L-20,15.5L-5.5,0H-20Z\" fill=\"var(--_bg)\" stroke=\"var(--_b)\" stroke-linejoin=\"round\"{}/>",
                shape, shape
            )
        }
        Some("awesome") => {
            format!("<path d=\"M0,-30A11,11 0 1 1 0,-8A11,11 0 1 1 0,-30ZM-20,25C-20,9 -12,-3 0,-3C12,-3 20,9 20,25C20,28 18,30 15,30H-15C-18,30 -20,28 -20,25Z\" fill=\"var(--_b)\"{}/>", shape)
        }
        Some("icon") => {
            head = -14.0;
            radius = 12.0;
            format!(
                "<rect x=\"-22\" y=\"-26\" width=\"44\" height=\"44\"{}/><svg class=\"pele-icon\" data-icon=\"{}\" x=\"-14\" y=\"-18\" width=\"28\" height=\"28\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"></svg>",
                rounded(&format!("{}{}", SHAPE_ATTRS, shape)),
                esc_text(node.icon.as_deref().unwrap_or(""))
            )
        }
        _ => format!("<circle cy=\"-21\" r=\"9\"{}{}/><path d=\"M0,-12V10M-14,-3H14M0,10L-12,30M0,10L12,30\"{}/>", SHAPE_ATTRS, shape, line),
    };
    if node.business {
        // The slash of a business actor: a chord across the lower right of the head.
        out.push_str(&format!("<path class=\"pele-business\" d=\"M{},{}L{},{}\"{}/>", num(0.12 * radius), num(head + 0.99 * radius), num(0.92 * radius), num(head - 0.39 * radius), line));
    }
    out
}

pub fn render_usecase(model: &UsecaseModel, config: &Config, options: &RenderOptions) -> Rendered {
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

// `.replace(/^default relationship relationship-\S+\s?/, '')`
fn without_relationship(classes: &str) -> &str {
    let Some(rest) = classes.strip_prefix("default relationship relationship-") else { return classes };
    let k = rest.find(|c: char| js::is_space(c)).unwrap_or(rest.len());
    if k == 0 {
        return classes;
    }
    let rest = &rest[k..];
    rest.strip_prefix(|c: char| js::is_space(c)).unwrap_or(rest)
}

// `turned` draws a diagram that runs across as one that runs down.
fn draw(model: &UsecaseModel, config: &Config, options: &RenderOptions, turned: bool) -> Rendered {
    let (gnodes, gedges) = build_usecase_graph(model);
    let size = 16.0;
    let small = js::round(size * 0.875);
    let tiny = js::round(size * 0.8125);
    let pad = 8.0;
    let root_dir = if turned { Dir::TB } else { direction(Some(&model.direction)) };
    let sideways = matches!(root_dir, Dir::LR | Dir::RL);

    let index = |id: &str| gnodes.iter().rposition(|n| n.id == id);
    let mut views: Vec<View> = Vec::new();
    let mut cnodes: Vec<CNode> = Vec::new();

    for node in &gnodes {
        let decls: Vec<String> = if !node.css_styles.is_empty() { node.css_compiled_styles.iter().chain(node.css_styles.iter()).cloned().collect() } else { node.css_compiled_styles.clone() };
        let st = resolve_style(&decls);
        let base = (if st.bold { text_style::BOLD } else { 0 }) | (if st.italic { text_style::ITALIC } else { 0 });
        let font_size = st.font_size.unwrap_or(size);
        let markdown = node.markdown;
        let stereotype = text(node.stereotype.as_deref().map(|s| format!("«{}»", s)).as_deref(), false, tiny, 200.0, base);
        let mut view = View { node, kind: Kind::Box, style: st, label: stereotype.clone(), stereotype: stereotype.clone(), w: 0.0, h: 0.0, rows: Vec::new(), key_w: 0.0, head_h: 0.0 };
        let mut c;
        if node.is_group {
            view.kind = Kind::Boundary;
            view.label = text(Some(&node.label), markdown, font_size, 4000.0, base);
            let title_h = view.label.height;
            c = cnode(0.0, 0.0);
            c.is_group = true;
            c.pad_x = GROUP_PAD;
            c.pad_bottom = GROUP_PAD;
            c.pad_top = if node.boundary_type == Some("package") {
                title_h + 2.0 * TAB_PAD + 16.0
            } else if title_h > 0.0 {
                title_h + 16.0
            } else {
                GROUP_PAD
            };
            c.min_w = view.label.width + 2.0 * GROUP_PAD;
        } else {
            if let Some(rows) = &node.json_rows {
                view.kind = Kind::Json;
                view.label = text(Some(&node.label), false, font_size, 4000.0, base | text_style::BOLD);
                let mut value_w: f64 = 0.0;
                for row in rows {
                    let key = text(Some(&row.key), false, small, 4000.0, base);
                    let value = text(Some(&row.value), false, small, 260.0, base);
                    view.key_w = view.key_w.max(key.width);
                    value_w = value_w.max(value.width);
                    view.h += key.height.max(value.height).max(js::round(small * 1.5)) + ROW_PAD;
                    view.rows.push((key, value));
                }
                // The root of an empty object has a value and no key, and then there is no key column.
                if view.key_w > 0.0 {
                    view.key_w += 2.0 * CELL_PAD;
                }
                view.head_h = view.label.height.max(js::round(font_size * 1.5)) + ROW_PAD;
                view.w = (view.label.width + 2.0 * CELL_PAD).max(if !view.rows.is_empty() { view.key_w + value_w + 2.0 * CELL_PAD } else { 0.0 }).max(72.0);
                view.h += view.head_h;
            } else if node.actor_type.is_some() {
                view.kind = Kind::Actor;
                view.label = text(Some(&node.label), markdown, font_size, 140.0, base);
                view.w = FIGURE_W.max(view.label.width).max(stereotype.width);
                view.h = FIGURE_H + FIGURE_GAP + stereotype.height + view.label.height;
            } else {
                view.label = text(Some(&node.label), markdown, if node.shape == "note" { small } else { font_size }, 180.0, base);
                let tw = view.label.width.max(stereotype.width);
                let th = view.label.height + stereotype.height;
                if node.shape == "usecaseEllipse" || node.shape == "usecaseBusiness" {
                    view.kind = Kind::Ellipse;
                    // An oval needs more room around its text than a box, and a business oval room for its slash.
                    view.w = (tw + 2.0 * node.padding + if node.business { 16.0 } else { 4.0 }).max(88.0);
                    view.h = (th + 28.0).max(48.0);
                } else if node.shape == "note" {
                    view.w = tw + 2.0 * node.padding + 6.0;
                    view.h = th + 18.0;
                } else {
                    view.w = (tw + 2.0 * node.padding + 12.0).max(80.0);
                    view.h = (th + 24.0).max(44.0);
                }
            }
            c = cnode(view.w, view.h);
        }
        c.seq = views.len() as f64;
        views.push(view);
        cnodes.push(c);
    }
    for (i, node) in gnodes.iter().enumerate() {
        cnodes[i].parent = match &node.parent_id {
            Some(p) => index(p).map_or(-1, |k| k as i64),
            None => -1,
        };
    }

    let mut cedges: Vec<CEdge> = Vec::new();
    let mut edge_labels: Vec<Label> = Vec::new();
    let mut drawn: Vec<&GraphEdge> = Vec::new();
    for edge in &gedges {
        let (Some(src), Some(dst)) = (index(&edge.start), index(&edge.end)) else { continue };
        let semantic = edge.relationship_type == "include" || edge.relationship_type == "extend";
        let raw = if semantic { edge.label.as_ref().map(|l| format!("«{}»", l)) } else { edge.label.clone() };
        let label = text(raw.as_deref(), edge.markdown, small, 200.0, 0);
        if src == dst {
            // A self-relationship loops out across the flow, and the node's box grows to hold it.
            if sideways {
                cnodes[src].h += 2.0 * (LOOP + if label.height > 0.0 { label.height + 8.0 } else { 0.0 });
            } else {
                cnodes[src].w += 2.0 * (LOOP + if label.width > 0.0 { label.width + 12.0 } else { 0.0 });
            }
        }
        drawn.push(edge);
        cedges.push(CEdge {
            src,
            dst,
            minlen: edge.minlen,
            label_w: if label.width > 0.0 { label.width + 12.0 } else { 0.0 },
            label_h: if label.height > 0.0 { label.height + 4.0 } else { 0.0 },
            route: Vec::new(),
            label_x: 0.0,
            label_y: 0.0,
        });
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

    let title = title_label(model.title.as_deref(), size, 4000.0);
    let title_height = if title.height > 0.0 { title.height + 12.0 } else { 0.0 };
    let inner = layout.width.max(title.width);
    let ox = pad + (inner - layout.width) / 2.0;
    let oy = pad + title_height;
    shift_layout(&mut cnodes, &mut cedges, ox, oy);
    let width = (inner + 2.0 * pad).ceil();
    let height = (layout.height + title_height + 2.0 * pad).ceil();

    // Where edges cross each boundary's top border, so that a title can keep out of their way.
    let mut crossings: Crossings = Crossings::new();
    for e in &cedges {
        mark_crossings(&mut crossings, &e.route);
    }
    attach_ends(&mut cedges, &cnodes, &views);

    let mut clusters = String::new();
    let mut fills: Vec<Fill> = Vec::new();
    let mut nodes_out = String::new();
    for (i, view) in views.iter().enumerate() {
        let c = &cnodes[i];
        let node = view.node;
        let classes = class_names(without_default(&node.css_classes));
        let id = esc_text(&node.id);
        let text_attrs = &view.style.text;
        let shape = &view.style.shape;
        let color = ink_fill(shape, text_attrs, None);
        let muted = if color.is_empty() { " fill=\"var(--_m)\"".to_string() } else { color.clone() };
        let (w, h) = (view.w, view.h);

        if view.kind == Kind::Boundary {
            let x = c.x - c.w / 2.0;
            let y = c.y - c.h / 2.0;
            if paints(shape) {
                fills.push(Fill { bx: [x, y, x + c.w, y + c.h], shape: shape.clone() });
            }
            let bx = format!(" fill=\"var(--_a)\"{} stroke=\"var(--_b)\"{}", if sets_fill(shape) { "" } else { " fill-opacity=\"0.5\"" }, shape);
            let title_attrs = format!(" class=\"pele-cluster-label\"{}{}", muted, text_attrs);
            let body = if node.boundary_type == Some("package") {
                let tab_w = (view.label.width + 2.0 * TAB_PAD + 8.0).min(c.w);
                let tab_h = view.label.height + 2.0 * TAB_PAD;
                format!(
                    "<rect class=\"pele-boundary-tab\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}/><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}/>{}",
                    num(x),
                    num(y),
                    num(tab_w),
                    num(tab_h),
                    bx,
                    num(x),
                    num(y + tab_h),
                    num(c.w),
                    num(c.h - tab_h),
                    bx,
                    label_svg(&view.label, x + tab_w / 2.0, y + tab_h / 2.0, &title_attrs)
                )
            } else {
                format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}/>{}",
                    num(x),
                    num(y),
                    num(c.w),
                    num(c.h),
                    rounded(&bx),
                    label_svg(&view.label, title_x(c, view.label.width, &crossings), y + 8.0 + view.label.height / 2.0, &title_attrs)
                )
            };
            clusters.push_str(&format!("<g class=\"pele-cluster pele-boundary{}\" data-id=\"{}\">{}</g>", classes, id, body));
            continue;
        }

        let body;
        let kind;
        if view.kind == Kind::Actor {
            kind = "pele-actor";
            let top = -h / 2.0;
            let stereotype_y = top + FIGURE_H + FIGURE_GAP + view.stereotype.height / 2.0;
            body = format!(
                "<g class=\"pele-actor-figure\" transform=\"translate(0,{})\">{}</g>{}{}",
                num(top + FIGURE_H / 2.0),
                figure(view),
                label_svg(&view.stereotype, 0.0, stereotype_y, &format!(" class=\"pele-stereotype\" font-size=\"{}\" fill=\"var(--_m)\"", js::num_str(tiny))),
                label_svg(&view.label, 0.0, h / 2.0 - view.label.height / 2.0, &format!(" class=\"pele-label\"{}", text_attrs))
            );
        } else if view.kind == Kind::Json {
            kind = "pele-json";
            let x0 = -w / 2.0;
            let mut y = -h / 2.0 + view.head_h;
            let mut cells = String::new();
            let mut lines = format!("M{},{}H{}", num(x0), num(y), num(x0 + w));
            if view.key_w > 0.0 {
                lines.push_str(&format!("M{},{}V{}", num(x0 + view.key_w), num(y), num(h / 2.0)));
            }
            for (key, value) in &view.rows {
                let row_h = key.height.max(value.height).max(js::round(small * 1.5)) + ROW_PAD;
                cells.push_str(&label_svg(key, x0 + CELL_PAD + key.width / 2.0, y + row_h / 2.0, &format!(" class=\"pele-json-key\"{}", muted)));
                cells.push_str(&left_label(value, x0 + view.key_w + CELL_PAD, y + row_h / 2.0, &format!(" class=\"pele-json-value\"{}{}", color, text_attrs)));
                y += row_h;
            }
            body = format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}/><path class=\"pele-json-lines\" d=\"{}\" fill=\"none\" stroke=\"var(--_b)\"{}/>{}<g font-size=\"{}\">{}</g>",
                num(x0),
                num(-h / 2.0),
                num(w),
                num(h),
                rounded(&format!("{}{}", SHAPE_ATTRS, shape)),
                lines,
                view.style.line,
                label_svg(&view.label, 0.0, -h / 2.0 + view.head_h / 2.0, &format!(" class=\"pele-label\" font-weight=\"var(--_hw)\"{}{}", color, text_attrs)),
                js::num_str(small),
                cells
            );
        } else {
            let label_y = view.stereotype.height / 2.0;
            let mut outline;
            if view.kind == Kind::Ellipse {
                kind = "pele-usecase";
                outline = format!("<ellipse rx=\"{}\" ry=\"{}\"{}{}/>", num(w / 2.0), num(h / 2.0), SHAPE_ATTRS, shape);
                if node.business {
                    // The slash of a business use case, cutting off the right end of the oval beside the text.
                    let from = w / 2.0 - 15.0;
                    let to = w / 2.0 - 5.0;
                    let arc = |at: f64| (h / 2.0) * (1.0 - (at / (w / 2.0)).powi(2)).sqrt();
                    outline.push_str(&format!(
                        "<path class=\"pele-business\" d=\"M{},{}L{},{}\" fill=\"none\" stroke=\"var(--_b)\"{}/>",
                        num(from),
                        num(arc(from)),
                        num(to),
                        num(-arc(to)),
                        view.style.line
                    ));
                }
            } else if node.shape == "note" {
                kind = "pele-note";
                let r = w / 2.0;
                let b = h / 2.0;
                outline = format!(
                    "<path d=\"M{},{}H{}L{},{}V{}H{}Z\" fill=\"var(--_a)\" stroke=\"var(--_b)\" stroke-linejoin=\"round\"{}/><path d=\"M{},{}V{}H{}\" fill=\"none\" stroke=\"var(--_b)\" stroke-linejoin=\"round\"{}/>",
                    num(-r),
                    num(-b),
                    num(r - FOLD),
                    num(r),
                    num(FOLD - b),
                    num(b),
                    num(-r),
                    shape,
                    num(r - FOLD),
                    num(-b),
                    num(FOLD - b),
                    num(r),
                    view.style.line
                );
            } else {
                kind = "pele-usecase";
                outline = format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}/>", num(-w / 2.0), num(-h / 2.0), num(w), num(h), rounded(&format!("{}{}", SHAPE_ATTRS, shape)));
            }
            body = format!(
                "{}{}{}",
                outline,
                label_svg(&view.stereotype, 0.0, -view.label.height / 2.0, &format!(" class=\"pele-stereotype\" font-size=\"{}\"{}", js::num_str(tiny), muted)),
                label_svg(
                    &view.label,
                    0.0,
                    label_y,
                    &format!(" class=\"pele-label\"{}{}{}", if node.shape == "note" { format!(" font-size=\"{}\"", js::num_str(small)) } else { String::new() }, color, text_attrs)
                )
            );
        }
        nodes_out.push_str(&format!("<g class=\"pele-node {}{}\" data-id=\"{}\" transform=\"translate({},{})\">{}</g>", kind, classes, id, num(c.x), num(c.y), body));
    }

    let mut casings = String::new();
    let mut edges_out = String::new();
    let mut labels_out = String::new();
    let mut loop_seen: Vec<(usize, usize)> = Vec::new();
    for i in 0..cedges.len() {
        let edge = drawn[i];
        let label = &edge_labels[i];
        let st = if !edge.style.is_empty() || !edge.css_compiled_styles.is_empty() {
            let decls: Vec<String> = edge.css_compiled_styles.iter().chain(edge.style.iter()).cloned().collect();
            Some(resolve_style(&decls))
        } else {
            None
        };
        let line_style = st.as_ref().map_or("", |s| s.line.as_str());
        let start_type = edge.arrow_type_start;
        let end_type = edge.arrow_type_end;
        let path: EdgePath;
        if cedges[i].src == cedges[i].dst {
            let ce = &cedges[i];
            let c = &cnodes[ce.src];
            let view = &views[ce.src];
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
            let side = if sideways { 2 } else { 1 };
            let spread = (side_length(view, side) / 2.0).min(8.0 + k * 6.0);
            let reach = LOOP + k * 8.0;
            let (ax, ay) = anchor(view, c, side, -spread);
            let (bx, by) = anchor(view, c, side, spread);
            let len = js::or(js::hypot(reach, spread), || 1.0);
            let ux = if sideways { spread / len } else { reach / len };
            let uy = if sideways { reach / len } else { spread / len };
            let st_trim = trim(start_type);
            let et = trim(end_type);
            // Both ends leave the node leaning away from each other, and the curve joins them outside it.
            let out = |x: f64, y: f64, sign: f64, far: f64| -> String {
                let lean = if far != 0.0 { 1.0 } else { 0.0 };
                if sideways {
                    format!("{},{}", num(x + sign * spread * lean), num(y + far))
                } else {
                    format!("{},{}", num(x + far), num(y + sign * spread * lean))
                }
            };
            let sx = if sideways { ax - ux * st_trim } else { ax + ux * st_trim };
            let sy = if sideways { ay + uy * st_trim } else { ay - uy * st_trim };
            let ex = bx + ux * et;
            let ey = by + uy * et;
            path = EdgePath {
                d: format!("M{},{}C{} {} {},{}", num(sx), num(sy), out(ax, ay, -1.0, reach), out(bx, by, 1.0, reach), num(ex), num(ey)),
                tokens: None,
                sx: ax,
                sy: ay,
                sdx: if sideways { ux } else { -ux },
                sdy: if sideways { -uy } else { uy },
                ex: bx,
                ey: by,
                edx: -ux,
                edy: -uy,
            };
            let ce = &mut cedges[i];
            if sideways {
                ce.label_x = c.x;
                ce.label_y = ay.max(by) + reach * 0.75 + 6.0 + label.height / 2.0;
            } else {
                ce.label_x = ax.max(bx) + reach * 0.75 + 8.0 + label.width / 2.0;
                ce.label_y = c.y;
            }
        } else {
            if cedges[i].route.len() < 6 {
                continue;
            }
            path = route_path(&cedges[i].route, None, trim(start_type), trim(end_type), None, None);
        }
        let ce = &cedges[i];
        let under = fill_under(&path.d, &[path.sx, path.sy, path.ex, path.ey], &fills, path.tokens.as_deref());
        // undefined off fills, '' where it crosses a border, else the fill's ink.
        let over: Option<String> = match &under {
            Under::Off => None,
            Under::Crosses => Some(String::new()),
            Under::On(fill) => Some(edge_ink(fill, line_style)),
        };
        let color = match over.as_deref() {
            Some(o) if !o.is_empty() => o.to_string(),
            _ => match st.as_ref().and_then(|s| s.stroke.as_deref()) {
                Some(s) if !s.is_empty() => esc(s),
                _ => "var(--_l)".to_string(),
            },
        };
        if over.as_deref() == Some("") {
            let bg = "var(--_bg)";
            casings.push_str(&casing(&path.d, &(mark(start_type, path.sx, path.sy, path.sdx, path.sdy, bg, bg) + &mark(end_type, path.ex, path.ey, path.edx, path.edy, bg, bg)), line_style, false));
        }
        // Mermaid animates these dashes. A still picture keeps the dashes.
        let dashes = if edge.internal {
            "2 3"
        } else if edge.dotted {
            "5 4"
        } else if edge.animate {
            "9 5"
        } else {
            ""
        };
        let marks = mark(start_type, path.sx, path.sy, path.sdx, path.sdy, &color, "none") + &mark(end_type, path.ex, path.ey, path.edx, path.edy, &color, "none");
        let over_attr = match over.as_deref() {
            Some(o) if !o.is_empty() => format!(" stroke=\"{}\"", o),
            _ => String::new(),
        };
        let marks = match (&under, over.as_deref()) {
            (Under::On(fill), Some(o)) if !o.is_empty() => hollow_on(&marks, fill),
            _ => marks,
        };
        edges_out.push_str(&format!(
            "<g class=\"pele-edge pele-{}{}\" data-id=\"{}\"{}{}><path d=\"{}\"{}/>{}</g>",
            edge.relationship_type,
            class_names(without_relationship(&edge.classes)),
            esc_text(&edge.id),
            over_attr,
            line_style,
            path.d,
            if dashes.is_empty() { String::new() } else { format!(" stroke-dasharray=\"{}\"", dashes) },
            marks
        ));
        if label.width > 0.0 {
            let semantic = edge.relationship_type == "include" || edge.relationship_type == "extend";
            let attrs = format!("{}{}", if semantic { " fill=\"var(--_m)\"" } else { "" }, st.as_ref().map_or("", |s| s.text.as_str()));
            labels_out.push_str(&edge_label_svg(&edge.id, label, ce.label_x, ce.label_y, &attrs));
        }
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
        content.push_str(&format!("<g class=\"pele-edge-labels\" font-size=\"{}\">{}</g>", js::num_str(small), labels_out));
    }
    content.push_str(&format!("<g class=\"pele-nodes\">{}</g>", nodes_out));
    let acc = Accessible { acc_title: Some(model.acc_title.as_str()), acc_descr: Some(model.acc_descr.as_str()) };
    let svg = svg_document("usecase", width, height, size, options.responsive, &acc, &content);
    Rendered { svg, width, height, links: Vec::new() }
}

// A boundary's title sits in the middle of its top edge. Where edges enter through that edge it
// moves into the stretch between them that is nearest the middle, or the widest if none has room.
fn title_x(c: &CNode, width: f64, crossings: &Crossings) -> f64 {
    let xs = crossings.get(&(js::round(c.y - c.h / 2.0) as i64));
    let Some(xs) = xs.filter(|_| width != 0.0) else { return c.x };
    let left = c.x - c.w / 2.0 + 6.0;
    let right = c.x + c.w / 2.0 - 6.0;
    let mut cuts: Vec<f64> = xs.iter().copied().filter(|&x| x > left && x < right).collect();
    cuts.sort_by(|a, b| js::ord(a - b));
    cuts.push(right);
    let need = width / 2.0 + 6.0;
    let mut best = c.x;
    let mut best_off = f64::INFINITY;
    let mut widest = 0.0;
    let mut from = left;
    for cut in cuts {
        if cut - from >= 2.0 * need {
            let x = c.x.max(from + need).min(cut - need);
            if (x - c.x).abs() < best_off {
                best_off = (x - c.x).abs();
                best = x;
            }
        } else if best_off == f64::INFINITY && cut - from > widest {
            widest = cut - from;
            best = (from + cut) / 2.0;
        }
        from = cut;
    }
    best
}

fn left_label(label: &Label, x: f64, cy: f64, attrs: &str) -> String {
    if label.lines.len() < 2 {
        return label_svg(label, x + label.width / 2.0, cy, attrs);
    }
    let mut out = String::new();
    for i in 0..label.lines.len() {
        out.push_str(&label_svg(&label_line(label, i), x + label.widths[i] / 2.0, cy - label.height / 2.0 + line_middle(label, i), attrs));
    }
    out
}
