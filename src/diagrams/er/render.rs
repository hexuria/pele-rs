// Port of src/diagrams/er/render.ts, for the default render options the host uses: no width
// limit (so a diagram that runs across is never turned), 16px text, no monospace parts, no
// icon or math resolver.

use super::db::ErDb;
use super::graph::{build_er_graph, ErGraphNode};
use super::markers::{er_marker, MARKER_LENGTH};
use crate::diagrams::common::generics::parse_generic_types;
use crate::js;
use crate::layout::compound::{cnode, compound_layout, direction, shift_layout, CEdge, CNode};
use crate::layout::layered::LayeredOptions;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num, shared_style, span_style, text_style};
use crate::svg::cluster::{cluster_title_x, mark_crossings, Crossings};
use crate::svg::edges::{casing, casings_svg, edge_ink, edge_label_svg, fill_under, hollow_on, route_path, Fill, Under};
use crate::svg::ink::{ink, ink_fill, paints, ON_DARK, ON_LIGHT};
use crate::svg::root::{svg_document, Accessible};
use crate::svg::theme::{class_names, resolve_style, rounded, sets_fill, ResolvedStyle, ROUNDED};
use crate::svg::title::title_label;
use crate::text::entities::decode_entities;
use crate::text::label::{has_math, layout_label, line_middle, Label};
use crate::text::measurer::style;
use crate::types::{LinkInfo, RenderOptions, Rendered};
use crate::util::url::{internal_link, rel_attr};
use crate::util::yaml::Value;

const NODE_SEP: f64 = 48.0;
const EDGE_SEP: f64 = 20.0;
const RANK_SEP: f64 = 64.0;
const GROUP_PAD: f64 = 20.0;
const LOOP: f64 = 28.0;
const LOOP_BAND: f64 = 38.0;
const MIN_WIDTH: f64 = 96.0;
const BOX_PAD_X: f64 = 18.0;
const BOX_PAD_Y: f64 = 12.0;
const HEAD_PAD_Y: f64 = 9.0;
const CELL_PAD_X: f64 = 12.0;
const CELL_GAP: f64 = 16.0;
const ROW_PAD: f64 = 9.0;
const COMMENT_WRAP: f64 = 260.0;
const NO_WRAP: f64 = 1e6;
const PORT_GAP: f64 = 18.0;
const PORT_MARGIN: f64 = 14.0;
const BOX_ATTRS: &str = " fill=\"var(--_s)\" stroke=\"var(--_b)\"";

fn number_option(config: &Config, key: &str, fallback: f64) -> f64 {
    match config.get("er").and_then(|v| v.as_map()).and_then(|m| m.get(key)) {
        Some(Value::Num(v)) if *v > 0.0 => v.min(2000.0),
        _ => fallback,
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

struct Row {
    // Type, name, keys, comment.
    cells: [Label; 4],
    h: f64,
}

struct NodeView {
    style: ResolvedStyle,
    label: Label,
    w: f64,
    h: f64,
    head_h: f64,
    rows: Vec<Row>,
    // Left edge of each column, measured from the left edge of the box.
    cols: Vec<f64>,
    row_size: f64,
    key_size: f64,
    // Whether self-relationships loop out of the bottom side rather than the right.
    below: bool,
}

// A relationship from an entity to itself, with the stretch of the side it loops out of.
struct Loop {
    label: Label,
    band: f64,
    at: f64,
}

// Writes a measured label starting at x, with the baseline of its first line at y.
fn text_left(label: &Label, x: f64, y: f64, attrs: &str) -> String {
    let lines = &label.lines;
    if lines.is_empty() {
        return String::new();
    }
    if lines.len() == 1 && lines[0].len() == 1 && lines[0][0].style == 0 {
        return format!("<text{} x=\"{}\" y=\"{}\">{}</text>", text_style(attrs, 0), num(x), num(y), esc(&lines[0][0].text));
    }
    let shared = shared_style(label);
    let shift = label.size * 0.35;
    let top = y - shift - label.line_height / 2.0;
    let mut body = String::new();
    for (i, line) in lines.iter().enumerate() {
        let middle = top + line_middle(label, i);
        debug_assert!(!has_math(line));
        let y = middle + shift;
        let mut sx = x;
        let mut placed = false;
        for span in line {
            if !span.text.is_empty() {
                let pos = if !placed { format!(" x=\"{}\" y=\"{}\"", num(sx), num(y)) } else { String::new() };
                body.push_str(&format!("<tspan{}{}>{}</tspan>", pos, span_style(span.style & !shared), esc(&span.text)));
                placed = true;
            }
            sx += span.width;
        }
    }
    format!("<text{} xml:space=\"preserve\">{}</text>", text_style(attrs, shared), body)
}

// Mermaid reads `~` pairs as angle brackets. They are written as entities so the label parser
// does not take `<int>` for a tag.
fn cell_text(raw: &str) -> String {
    let converted = parse_generic_types(raw);
    if converted == raw {
        raw.to_string()
    } else {
        converted.replace('<', "&lt;").replace('>', "&gt;")
    }
}

pub fn render_er(db: &ErDb, config: &Config, options: &RenderOptions) -> Rendered {
    // With no width limit, turnToFit draws across.
    draw(db, config, options)
}

fn draw(db: &ErDb, config: &Config, options: &RenderOptions) -> Rendered {
    let graph = build_er_graph(db);
    let size = 16.0;
    let small_size = js::round(size * 0.875);
    let pad = 8.0;

    let index_of = |id: &str| graph.nodes.iter().position(|n| n.id == id);
    let mut index: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for (i, node) in graph.nodes.iter().enumerate() {
        index.insert(node.id.as_str(), i);
    }
    let _ = index_of;
    let mut views: Vec<NodeView> = Vec::new();
    let mut cnodes: Vec<CNode> = Vec::new();

    let mut cedges: Vec<CEdge> = Vec::new();
    let mut edge_labels: Vec<Label> = Vec::new();
    let mut drawn: Vec<usize> = Vec::new();
    let mut loop_store: Vec<Loop> = Vec::new();
    // Map<number, Loop[]>, in insertion order.
    let mut loops: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut loop_of: Vec<Option<usize>> = Vec::new();
    for (ei, edge) in graph.edges.iter().enumerate() {
        let (Some(&src), Some(&dst)) = (index.get(edge.start.as_str()), index.get(edge.end.as_str())) else { continue };
        let label = layout_label(Some(&edge.label), true, small_size, 200.0, 0);
        let mut lp = None;
        if src == dst {
            loop_store.push(Loop { label: label.clone(), band: 0.0, at: 0.0 });
            let id = loop_store.len() - 1;
            match loops.iter_mut().find(|(k, _)| *k == src) {
                Some((_, list)) => list.push(id),
                None => loops.push((src, vec![id])),
            }
            lp = Some(id);
        }
        loop_of.push(lp);
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

    // The direction of the level a node is laid out in: its nearest subgraph that sets one, or the diagram's.
    let horizontal = |i: usize| -> bool {
        let mut at = i;
        // Subgraphs can name each other as members, so the walk up is bounded.
        for _ in 0..graph.nodes.len() {
            let up = graph.nodes[at].parent_id.as_deref().and_then(|p| index.get(p).copied());
            let Some(up) = up else { break };
            if let Some(dir) = graph.nodes[up].dir.as_deref().filter(|d| !d.is_empty()) {
                return dir == "LR" || dir == "RL";
            }
            at = up;
        }
        db.direction == "LR" || db.direction == "RL"
    };

    for (i, node) in graph.nodes.iter().enumerate() {
        let decls: Vec<String> = if !node.css_styles.is_empty() { node.css_compiled_styles.iter().chain(&node.css_styles).cloned().collect() } else { node.css_compiled_styles.clone() };
        let st = resolve_style(&decls);
        let base = (if st.bold { style::BOLD } else { 0 }) | (if st.italic { style::ITALIC } else { 0 });
        let name_size = st.font_size.unwrap_or(size);
        let row_size = st.font_size.unwrap_or(small_size);
        let key_size = st.font_size.unwrap_or(js::round(size * 0.75));
        let label = if node.is_group {
            layout_label(Some(&node.label), true, name_size, 4000.0, base)
        } else {
            layout_label(Some(if node.alias.is_empty() { &node.label } else { &node.alias }), true, name_size, 240.0, base)
        };
        let own: Vec<usize> = loops.iter().find(|(k, _)| *k == i).map(|(_, l)| l.clone()).unwrap_or_default();
        // Loops leave by a side the layout keeps free of other edges: the right when ranks run
        // down, the bottom when they run across.
        let below = !own.is_empty() && !node.is_group && horizontal(i);
        let mut view = NodeView { style: st, label, w: 0.0, h: 0.0, head_h: 0.0, rows: Vec::new(), cols: Vec::new(), row_size, key_size, below };
        let mut reach: f64 = 0.0;
        let mut bands = 0.0;
        for &l in &own {
            let lp = &mut loop_store[l];
            lp.band = LOOP_BAND.max(if below { lp.label.width + 12.0 } else { lp.label.height + 6.0 });
            bands += lp.band;
            reach = reach.max(if below { lp.label.height } else { lp.label.width });
        }
        let min_w = MIN_WIDTH.max(if below { bands + 8.0 } else { 0.0 });
        let mut c;
        if node.is_group {
            c = cnode(0.0, 0.0);
            c.is_group = true;
            c.dir = node.dir.as_deref().filter(|d| !d.is_empty()).map(|d| direction(Some(d)));
            c.pad_x = GROUP_PAD;
            c.pad_top = if view.label.height > 0.0 { view.label.height + 16.0 } else { GROUP_PAD };
            c.pad_bottom = GROUP_PAD;
            c.min_w = (view.label.width + 2.0 * GROUP_PAD - 12.0).max(MIN_WIDTH);
        } else {
            let name_h = view.label.height.max(js::round(name_size * 1.5));
            if node.attributes.is_empty() {
                view.w = (view.label.width + 2.0 * BOX_PAD_X).max(min_w);
                view.h = (name_h + 2.0 * BOX_PAD_Y).max(if below { 0.0 } else { bands + 8.0 });
            } else {
                let row_line = js::round(row_size * 1.5);
                let mut widths = [0f64; 4];
                for attribute in &node.attributes {
                    let cells = [
                        layout_label(Some(&cell_text(&attribute.type_)), true, row_size, NO_WRAP, base),
                        layout_label(Some(&cell_text(&attribute.name)), true, row_size, NO_WRAP, base),
                        layout_label(Some(&attribute.keys.join(", ")), false, key_size, NO_WRAP, base),
                        layout_label(Some(&cell_text(&attribute.comment)), true, row_size, COMMENT_WRAP, base),
                    ];
                    let mut h = row_line;
                    for k in 0..4 {
                        if cells[k].width > widths[k] {
                            widths[k] = cells[k].width;
                        }
                        if cells[k].height > h {
                            h = cells[k].height;
                        }
                    }
                    view.rows.push(Row { cells, h: h + ROW_PAD });
                    view.h += h + ROW_PAD;
                }
                let mut columns = 0.0;
                let mut inner = 0.0;
                for &w in &widths {
                    if w == 0.0 {
                        continue;
                    }
                    inner += w + if columns > 0.0 { CELL_GAP } else { 0.0 };
                    columns += 1.0;
                }
                view.w = (inner + 2.0 * CELL_PAD_X).max(view.label.width + 2.0 * BOX_PAD_X).max(min_w);
                // A box wider than its table shares the spare width between the columns.
                let spare = if columns > 0.0 { (view.w - inner - 2.0 * CELL_PAD_X) / columns } else { 0.0 };
                let mut x = CELL_PAD_X;
                for &w in &widths {
                    view.cols.push(x);
                    if w > 0.0 {
                        x += w + spare + CELL_GAP;
                    }
                }
                view.head_h = name_h + 2.0 * HEAD_PAD_Y;
                view.h += view.head_h;
            }
            // The layout keeps a node centered in its box, so room for loops is added on both sides.
            let room = if !own.is_empty() { 2.0 * (MARKER_LENGTH + LOOP * 0.75 + 10.0 + reach) } else { 0.0 };
            c = if below { cnode(view.w, view.h + room) } else { cnode(view.w + room, view.h) };
        }
        c.seq = views.len() as f64;
        views.push(view);
        cnodes.push(c);
    }
    for (i, node) in graph.nodes.iter().enumerate() {
        cnodes[i].parent = node.parent_id.as_deref().map_or(-1, |p| index.get(p).map_or(-1, |&k| k as i64));
    }

    let layout = compound_layout(
        &mut cnodes,
        &mut cedges,
        direction(Some(&db.direction)),
        &LayeredOptions {
            node_sep: number_option(config, "nodeSpacing", NODE_SEP),
            edge_sep: EDGE_SEP,
            rank_sep: number_option(config, "rankSpacing", RANK_SEP),
            port_sep: 24.0,
            ..Default::default()
        },
    );

    spread_ends(&mut cedges, &cnodes, &views);

    // A subgraph gets its size from the layout, so its loops hang outside it and may widen the diagram.
    let mut layout_width = layout.width;
    for (i, own) in &loops {
        let c = &cnodes[*i];
        if !c.is_group {
            continue;
        }
        let mut reach: f64 = 0.0;
        for &l in own {
            reach = reach.max(loop_store[l].label.width);
        }
        layout_width = layout_width.max(c.x + c.w / 2.0 + MARKER_LENGTH + LOOP * 0.75 + 10.0 + reach);
    }

    let title = title_label(db.title.as_deref(), size, 4000.0);
    let title_height = if title.height > 0.0 { title.height + 12.0 } else { 0.0 };
    let inner = layout_width.max(title.width);
    let ox = pad + (inner - layout_width) / 2.0;
    let oy = pad + title_height;
    shift_layout(&mut cnodes, &mut cedges, ox, oy);
    let width = (inner + 2.0 * pad).ceil();
    let height = (layout.height + title_height + 2.0 * pad).ceil();
    let mut links: Vec<LinkInfo> = Vec::new();

    let mut crossings: Crossings = Crossings::new();
    if cnodes.iter().any(|c| c.is_group) {
        for e in &cedges {
            mark_crossings(&mut crossings, &e.route);
        }
    }

    let mut clusters = String::new();
    let mut fills: Vec<Fill> = Vec::new();
    let mut nodes_out = String::new();
    for i in 0..views.len() {
        let node: &ErGraphNode = &graph.nodes[i];
        let c = &cnodes[i];
        let view = &mut views[i];
        let classes = class_names(strip_default(&node.css_classes));
        if c.is_group {
            view.w = c.w;
            view.h = c.h;
            let x = c.x - c.w / 2.0;
            let y = c.y - c.h / 2.0;
            let title_x = cluster_title_x(c, view.label.width, &crossings, GROUP_PAD);
            if paints(&view.style.shape) {
                fills.push(Fill { bx: [x, y, x + c.w, y + c.h], shape: view.style.shape.clone() });
            }
            clusters.push_str(&format!(
                "<g class=\"pele-cluster{}\" data-id=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_a)\"{} stroke=\"var(--_b)\"{}/>{}</g>",
                classes,
                esc_text(&node.id),
                num(x),
                num(y),
                num(c.w),
                num(c.h),
                if sets_fill(&view.style.shape) { "" } else { " fill-opacity=\"0.5\"" },
                rounded(&view.style.shape),
                label_svg(
                    &view.label,
                    title_x,
                    y + 8.0 + view.label.height / 2.0,
                    &format!(" class=\"pele-cluster-label\"{}{}", ink_fill(&view.style.shape, &view.style.text, Some("var(--_m)")), view.style.text)
                )
            ));
            continue;
        }

        let (w, h) = (view.w, view.h);
        let (x0, y0) = (-w / 2.0, -h / 2.0);
        let text = &view.style.text;
        let color = ink_fill(&view.style.shape, &view.style.text, None);
        let muted = if color.is_empty() { " fill=\"var(--_m)\"".to_string() } else { color.clone() };
        let shape = format!("x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"", num(x0), num(y0), num(w), num(h));
        let mut name = label_svg(&view.label, 0.0, if !view.rows.is_empty() { y0 + view.head_h / 2.0 } else { 0.0 }, &format!(" class=\"pele-label\"{}{}", color, text));
        let display = decode_entities(if node.alias.is_empty() { &node.label } else { &node.alias });
        if format!("{} ", classes).contains(" internal-link ") && !name.is_empty() {
            // The name ends up in an href, so it gets the same check as a URL.
            let safe = internal_link(&display);
            let target = esc(&display);
            name = format!("<a class=\"internal-link\"{} data-href=\"{}\">{}</a>", if safe { format!(" href=\"{}\"{}", target, rel_attr()) } else { String::new() }, target, name);
            links.push(LinkInfo { id: node.label.clone(), href: display.clone(), internal: true });
        }

        let body = if view.rows.is_empty() {
            format!("<rect {}{}/>{}", shape, rounded(&format!("{}{}", BOX_ATTRS, view.style.shape)), name)
        } else {
            let row_line = js::round(view.row_size * 1.5);
            let line_color = view.style.stroke.as_deref().map_or("var(--_b)".to_string(), esc);
            let mut stripes = String::new();
            let mut cells = String::new();
            let mut y = y0 + view.head_h;
            for (r, row) in view.rows.iter().enumerate() {
                if r % 2 == 0 {
                    let span = format!("x=\"{}\" width=\"{}\"", num(x0), num(w));
                    if r == view.rows.len() - 1 {
                        // The last row follows the box's rounded bottom corners: a rounded rect, squared off on top.
                        stripes.push_str(&format!("<rect {} y=\"{}\" height=\"{}\"{}/><rect {} y=\"{}\" height=\"{}\"/>", span, num(y), num(row.h), ROUNDED, span, num(y), num(row.h / 2.0)));
                    } else {
                        stripes.push_str(&format!("<rect {} y=\"{}\" height=\"{}\"/>", span, num(y), num(row.h)));
                    }
                }
                let baseline = y + ROW_PAD / 2.0 + row_line / 2.0 + view.row_size * 0.35;
                let [ty, attribute, keys, comment] = &row.cells;
                cells.push_str(&text_left(ty, x0 + view.cols[0], baseline, &format!(" class=\"pele-er-type\"{}{}", muted, text)));
                cells.push_str(&text_left(attribute, x0 + view.cols[1], baseline, &format!(" class=\"pele-er-name\"{}{}", color, text)));
                cells.push_str(&text_left(keys, x0 + view.cols[2], baseline, &format!(" class=\"pele-er-keys\" font-size=\"{}\"{}{}", js::num_str(view.key_size), muted, text)));
                cells.push_str(&text_left(comment, x0 + view.cols[3], baseline, &format!(" class=\"pele-er-comment\"{}{}", muted, text)));
                y += row.h;
            }
            let divider_y = num(y0 + view.head_h);
            // Keep row shading light on light fills and dark on dark fills to preserve text contrast.
            let tone = ink(&view.style.shape, "");
            let shade = if tone == Some(ON_LIGHT) {
                ON_DARK
            } else if tone == Some(ON_DARK) {
                ON_LIGHT
            } else {
                "var(--_bg)"
            };
            format!(
                "<rect {}{}/><g class=\"pele-er-stripes\" fill=\"{}\"{}>{}</g><path class=\"pele-er-divider\" d=\"M{},{}H{}\" stroke=\"{}\"/><rect {} fill=\"none\" stroke=\"var(--_b)\"{}/>{}<g class=\"pele-er-attributes\" font-size=\"{}\">{}</g>",
                shape,
                rounded(&format!("{}{}", BOX_ATTRS, view.style.shape)),
                shade,
                if view.style.shape.contains("fill:") { " opacity=\"0.6\"" } else { "" },
                stripes,
                num(x0),
                divider_y,
                num(x0 + w),
                line_color,
                shape,
                rounded(&view.style.line),
                name,
                js::num_str(view.row_size),
                cells
            )
        };
        nodes_out.push_str(&format!("<g class=\"pele-node pele-entity{}\" data-id=\"{}\" transform=\"translate({},{})\">{}</g>", classes, esc_text(&node.label), num(c.x), num(c.y), body));
    }

    // Self-relationships share the side they loop out of, squeezed together if it is too short for them.
    for (i, own) in &loops {
        let view = &views[*i];
        let total: f64 = own.iter().map(|&l| loop_store[l].band).sum();
        let scale = (((if view.below { view.w } else { view.h }) - 8.0).max(0.0) / total).min(1.0);
        let mut at = (-total * scale) / 2.0;
        for &l in own {
            let lp = &mut loop_store[l];
            lp.band *= scale;
            lp.at = at + lp.band / 2.0;
            at += lp.band;
        }
    }

    let mut edges_out = String::new();
    let mut casings = String::new();
    let mut labels_out = String::new();
    for i in 0..cedges.len() {
        let edge = &graph.edges[drawn[i]];
        let label = &edge_labels[i];
        let d: String;
        let mut start: String;
        let mut end: String;
        if let Some(l) = loop_of[i] {
            let lp = &loop_store[l];
            let c = &cnodes[cedges[i].src];
            let view = &views[cedges[i].src];
            let spread = (lp.band / 2.0 - 6.0).min(13.0).max(1.0);
            let far = MARKER_LENGTH + LOOP * 0.75 + 6.0;
            if view.below {
                let x = c.x + lp.at;
                let y = c.y + view.h / 2.0;
                let turn = num(y + MARKER_LENGTH);
                let out = num(y + MARKER_LENGTH + LOOP);
                d = format!("M{},{}V{}C{},{} {},{} {},{}V{}", num(x - spread), num(y), turn, num(x - spread), out, num(x + spread), out, num(x + spread), turn, num(y));
                start = er_marker(&edge.arrow_type_start, x - spread, y, 0.0, -1.0);
                end = er_marker(&edge.arrow_type_end, x + spread, y, 0.0, -1.0);
                cedges[i].label_x = x;
                cedges[i].label_y = y + far + label.height / 2.0;
            } else {
                let x = c.x + view.w / 2.0;
                let y = c.y + lp.at;
                let turn = num(x + MARKER_LENGTH);
                let out = num(x + MARKER_LENGTH + LOOP);
                d = format!("M{},{}H{}C{},{} {},{} {},{}H{}", num(x), num(y - spread), turn, out, num(y - spread), out, num(y + spread), turn, num(y + spread), num(x));
                start = er_marker(&edge.arrow_type_start, x, y - spread, -1.0, 0.0);
                end = er_marker(&edge.arrow_type_end, x, y + spread, -1.0, 0.0);
                cedges[i].label_x = x + far + label.width / 2.0;
                cedges[i].label_y = y;
            }
        } else {
            let ce = &cedges[i];
            let mut route = straighten(&ce.route);
            if route.len() < 6 {
                continue;
            }
            inset(&mut route, 0, &views[ce.src], &cnodes[ce.src]);
            let last = route.len() - 3;
            inset(&mut route, last, &views[ce.dst], &cnodes[ce.dst]);
            // A curve stops short of each box so that the marker sits on a straight run of line.
            let dx = (route[route.len() - 3] - route[0]).abs();
            let dy = (route[route.len() - 2] - route[1]).abs();
            let direct = route.len() == 6 && (dx < 0.01 || dy < 0.01 || js::hypot(dx, dy) <= 3.0 * MARKER_LENGTH);
            let stub = if direct { 0.0 } else { MARKER_LENGTH };
            let path = route_path(&route, None, stub, stub, None, None);
            d = if stub > 0.0 { format!("M{},{}L{}L{},{}", num(path.sx), num(path.sy), &path.d[1..], num(path.ex), num(path.ey)) } else { path.d.clone() };
            start = er_marker(&edge.arrow_type_start, path.sx, path.sy, path.sdx, path.sdy);
            end = er_marker(&edge.arrow_type_end, path.ex, path.ey, path.edx, path.edy);
        }
        let under = fill_under(&d, &[], &fills, None);
        let over: Option<String> = match &under {
            Under::Off => None,
            Under::Crosses => Some(String::new()),
            Under::On(f) => Some(edge_ink(f, "")),
        };
        if over.as_deref() == Some("") {
            casings.push_str(&casing(&d, &format!("{}{}", start, end), "", false));
        }
        if let (Under::On(f), Some(o)) = (&under, &over) {
            if !o.is_empty() {
                start = hollow_on(&start, f);
                end = hollow_on(&end, f);
            }
        }
        let dashed = edge.dashed;
        edges_out.push_str(&format!(
            "<g class=\"pele-edge pele-er-{}\" data-id=\"{}\"{}><path d=\"{}\"{}/>{}{}</g>",
            if dashed { "non-identifying" } else { "identifying" },
            esc_text(&edge.id),
            match &over {
                Some(o) if !o.is_empty() => format!(" stroke=\"{}\"", o),
                _ => String::new(),
            },
            d,
            if dashed { " stroke-dasharray=\"4 3\"" } else { "" },
            start,
            end
        ));
        if label.width > 0.0 {
            labels_out.push_str(&edge_label_svg(&edge.id, label, cedges[i].label_x, cedges[i].label_y, ""));
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
        content.push_str(&format!("<g class=\"pele-edge-labels\" font-size=\"{}\">{}</g>", js::num_str(small_size), labels_out));
    }
    content.push_str(&format!("<g class=\"pele-nodes\">{}</g>", nodes_out));
    let svg = svg_document("er", width, height, size, options.responsive, &Accessible { acc_title: db.acc_title.as_deref(), acc_descr: db.acc_descr.as_deref() }, &content);
    Rendered { svg, width, height, links }
}

// raw.replace(/^default\s?/, '')
fn strip_default(raw: &str) -> &str {
    match raw.strip_prefix("default") {
        Some(rest) => match rest.chars().next() {
            Some(c) if js::is_space(c) => &rest[c.len_utf8()..],
            _ => rest,
        },
        None => raw,
    }
}

fn straighten(route: &[f64]) -> Vec<f64> {
    let mut out: Vec<f64> = route[..3.min(route.len())].to_vec();
    let mut k = 3;
    while k < route.len() {
        let prev = out.len() - 3;
        let flat = |axis: usize| (out[prev + axis] - route[k + axis]).abs() < 0.01 && k + 3 + axis < route.len() && (route[k + axis] - route[k + 3 + axis]).abs() < 0.01;
        if k + 3 < route.len() && (flat(0) || flat(1)) {
            k += 3;
            continue;
        }
        out.extend_from_slice(&route[k..k + 3]);
        k += 3;
    }
    out
}

struct End {
    edge: usize,
    at: usize,
    toward: f64,
}

// The layout brings every edge that ends on one side of a box to the middle of that side, where
// their markers would cover each other. This spaces them along the side, in the order they fan out.
fn spread_ends(cedges: &mut [CEdge], cnodes: &[CNode], views: &[NodeView]) {
    // Map<number, End[]>, in insertion order.
    let mut sides: Vec<(usize, Vec<End>)> = Vec::new();
    let mut add = |node: usize, edge: usize, route: &[f64], at: usize, next: usize| {
        let c = &cnodes[node];
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
        let key = node * 4 + side;
        let end = End { edge, at, toward: route[next + if side & 1 == 1 { 1 } else { 0 }] };
        match sides.iter_mut().find(|(k, _)| *k == key) {
            Some((_, list)) => list.push(end),
            None => sides.push((key, vec![end])),
        }
    };
    for (ei, e) in cedges.iter().enumerate() {
        if e.route.len() < 3 {
            continue;
        }
        let last = e.route.len() - 3;
        if e.src == e.dst || last < 3 {
            continue;
        }
        add(e.src, ei, &e.route, 0, 3);
        add(e.dst, ei, &e.route, last, last - 3);
    }
    for (key, ends) in sides.iter_mut() {
        if ends.len() < 2 {
            continue;
        }
        let node = *key >> 2;
        let vertical = *key & 1 == 1;
        let (bw, bh) = if cnodes[node].is_group { (cnodes[node].w, cnodes[node].h) } else { (views[node].w, views[node].h) };
        let length = if vertical { bh } else { bw };
        let step = PORT_GAP.min((length - 2.0 * PORT_MARGIN) / (ends.len() as f64 - 1.0)).max(0.0);
        let center = if vertical { cnodes[node].y } else { cnodes[node].x };
        ends.sort_by(|a, b| js::ord(a.toward - b.toward));
        let n = ends.len() as f64;
        for (i, end) in ends.iter().enumerate() {
            cedges[end.edge].route[end.at + if vertical { 1 } else { 0 }] = center + (i as f64 - (n - 1.0) / 2.0) * step;
        }
    }
}

// An entity with a self-relationship is laid out larger than it is drawn; this moves a route end
// from the side of that larger box onto the entity's own side.
fn inset(route: &mut [f64], at: usize, view: &NodeView, c: &CNode) {
    if c.is_group {
        return;
    }
    let dx = route[at] - c.x;
    let dy = route[at + 1] - c.y;
    if c.w != view.w && (dx.abs() - c.w / 2.0).abs() < 0.5 {
        route[at] -= (sign(dx) * (c.w - view.w)) / 2.0;
    }
    if c.h != view.h && (dy.abs() - c.h / 2.0).abs() < 0.5 {
        route[at + 1] -= (sign(dy) * (c.h - view.h)) / 2.0;
    }
}
