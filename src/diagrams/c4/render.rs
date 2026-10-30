// Port of src/diagrams/c4/render.ts. With no `maxWidth`, `tighten` draws once with room to
// spare, so the tight drawings are left out.

use super::db::C4Db;
use super::layout::{layout_c4, HEAD_LEFT, HEAD_TOP};
use super::route::{crosses, cyl_ry, queue_rx, route, Lanes, Route, HEAD, LABEL_PAD, LANE};
use crate::diagrams::common::fit_width::tighten;
use crate::diagrams::common::literal_label::left_label_svg;
use crate::errors::{PeleError, PeleErrorCode};
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num};
use crate::svg::edges::{casings_svg, edge_casing, marker, marker_trim, over_fills, EdgePath, Fill, ARROW};
use crate::svg::ink::{ink_fill, paints};
use crate::svg::root::{svg_document, Accessible};
use crate::svg::shapes::draw_shape;
use crate::svg::theme::{resolve_style, rounded, ResolvedStyle};
use crate::text::label::layout_label;
use crate::text::measurer::style as st;
use crate::types::{LinkInfo, RenderOptions, Rendered};
use crate::util::url::{linked, sanitize_url};
use std::collections::{HashMap, HashSet};

const PAIR_SHIFT: f64 = 7.0;
const MAX_OFFSET: f64 = 2000.0;
// A label may cover this much of a shape's edge, where there is no text.
const EDGE: f64 = 8.0;
// Places along a relation to try for its label, nearest the middle first.
const SLIDE: [f64; 7] = [0.5, 0.4, 0.6, 0.3, 0.7, 0.2, 0.8];
// Above this many box comparisons, labels stay at the middle and lines are not steered around shapes.
const CARE_BUDGET: usize = 2000000;

fn user_style(fill: Option<&str>, stroke: Option<&str>, color: Option<&str>) -> ResolvedStyle {
    let mut declarations: Vec<String> = Vec::new();
    if let Some(f) = fill.filter(|f| !f.is_empty()) {
        declarations.push(format!("fill:{}", f));
    }
    if let Some(s) = stroke.filter(|s| !s.is_empty()) {
        declarations.push(format!("stroke:{}", s));
    }
    if let Some(c) = color.filter(|c| !c.is_empty()) {
        declarations.push(format!("color:{}", c));
    }
    resolve_style(&declarations)
}

// `.replace(/ﬂ°°?/g, '#')`
fn unhide_hashes(url: &str) -> String {
    let c: Vec<char> = url.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        if c[i] == '\u{fb02}' && c.get(i + 1) == Some(&'\u{b0}') {
            out.push('#');
            i += if c.get(i + 2) == Some(&'\u{b0}') { 3 } else { 2 };
            continue;
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

fn link(body: &str, url: Option<&str>, id: &str, links: &mut Vec<LinkInfo>) -> String {
    let Some(url) = url.filter(|u| !u.is_empty()) else { return body.to_string() };
    // Entity encoding has hidden any `#name;` in the address. Put it back.
    let url = if url.contains("\u{b6}\u{df}") { unhide_hashes(url).replace("\u{b6}\u{df}", ";") } else { url.to_string() };
    linked(body, &sanitize_url(&url), id, links, "")
}

pub fn render_c4(db: &C4Db, config: &Config, options: &RenderOptions) -> Result<Rendered, PeleError> {
    let mut result = None;
    let rendered = tighten(options, |_tight| match draw(db, config, options) {
        Ok(r) => r,
        Err(e) => {
            result = Some(e);
            Rendered { svg: String::new(), width: 0.0, height: 0.0, links: Vec::new() }
        }
    });
    match result {
        Some(e) => Err(e),
        None => Ok(rendered),
    }
}

fn draw(db: &C4Db, _config: &Config, options: &RenderOptions) -> Result<Rendered, PeleError> {
    let size = 16.0;
    let small = js::round(size * 0.8125);
    let pad = 8.0;
    let mut links: Vec<LinkInfo> = Vec::new();
    let mut lay = layout_c4(db, size, small);
    let root_box = lay.boxes[lay.groups[0].b].clone();

    let grow = |x0: f64, y0: f64, x1: f64, y1: f64, b: &mut (f64, f64, f64, f64)| {
        b.0 = b.0.min(x0);
        b.1 = b.1.min(y0);
        b.2 = b.2.max(x1);
        b.3 = b.3.max(y1);
    };
    let mut bb = (0f64, 0f64, root_box.w, root_box.h);

    let mut ends: Vec<usize> = Vec::new();
    let mut pairs: HashMap<usize, HashSet<usize>> = HashMap::new();
    for rel in &db.rels {
        let from = if rel.from_attr.is_some() { None } else { db.get_c4_shape(&rel.from) };
        let to = if rel.to_attr.is_some() { None } else { db.get_c4_shape(&rel.to) };
        let a = from.and_then(|f| lay.of.get(&f).copied());
        let b = to.and_then(|t| lay.of.get(&t).copied());
        let (Some(a), Some(b)) = (a, b) else {
            return Err(PeleError::typed(format!("C4 rel \"{}\" -> \"{}\" references an unknown shape or boundary", rel.from, rel.to), PeleErrorCode::Semantic, "c4"));
        };
        ends.push(a);
        ends.push(b);
        pairs.entry(a).or_default().insert(b);
    }

    // What a relation's label should not cover: shapes, arrowheads, boundary headings, and the labels placed before it.
    let mut nodes: Vec<usize> = Vec::new();
    let mut taken: Vec<f64> = Vec::new();
    for &g in &lay.order {
        for node in &lay.groups[g].nodes {
            nodes.push(node.b);
            let n = &lay.boxes[node.b];
            taken.extend([n.x + EDGE, n.y + EDGE, n.x + n.w - EDGE, n.y + n.h - EDGE]);
        }
    }
    let careful = db.rels.len() * (nodes.len() + lay.order.len() + 3 * db.rels.len()) <= CARE_BUDGET;
    let covered = |taken: &[f64], x0: f64, y0: f64, x1: f64, y1: f64| -> f64 {
        let mut area = 0.0;
        let mut i = 0;
        while i < taken.len() {
            let w = x1.min(taken[i + 2]) - x0.max(taken[i]);
            if w > 0.0 {
                let h = y1.min(taken[i + 3]) - y0.max(taken[i + 1]);
                if h > 0.0 {
                    area += w * h;
                }
            }
            i += 4;
        }
        area
    };
    let offset = |value: Option<f64>| match value {
        Some(v) if v != 0.0 && !v.is_nan() => v.min(MAX_OFFSET).max(-MAX_OFFSET),
        _ => 0.0,
    };

    // Every line is drawn before any label is placed, so that labels can keep off the arrowheads.
    let mut casings = String::new();
    let mut edges_out = String::new();
    let trim = marker_trim("arrow_point");
    let mut fills: Vec<Fill> = Vec::new();
    for &g in &lay.order {
        if g == 0 {
            continue;
        }
        let shape = user_style(lay_boundary(db, &lay, g).bg_color.as_deref(), None, None).shape;
        let gb = &lay.boxes[lay.groups[g].b];
        if paints(&shape) {
            fills.push(Fill { bx: [gb.x, gb.y, gb.x + gb.w, gb.y + gb.h], shape });
        }
    }
    let mut lanes = Lanes::default();
    for &g in &lay.order {
        if g != 0 {
            lanes.frame(&lay.boxes[lay.groups[g].b]);
        }
    }
    let mut routes: Vec<Route> = Vec::new();
    for (index, rel) in db.rels.iter().enumerate() {
        let a = ends[index * 2];
        let b = ends[index * 2 + 1];
        let style = user_style(None, rel.line_color.as_deref(), None);
        let start_arrow = rel.type_ == "birel" || rel.type_ == "rel_b";
        let end_arrow = rel.type_ != "rel_b";
        let reverse = a != b && pairs.get(&b).is_some_and(|s| s.contains(&a));
        let r = route(
            &lay.boxes,
            a,
            b,
            if reverse { PAIR_SHIFT } else { 0.0 },
            if careful { Some(&nodes) } else { None },
            &mut lanes,
            if start_arrow { trim } else { 0.0 },
            if end_arrow { trim } else { 0.0 },
        );
        grow(r.x0.min(r.x1), r.y0.min(r.y1), r.x0.max(r.x1), r.y0.max(r.y1), &mut bb);
        if start_arrow && careful {
            taken.extend([r.sx - ARROW, r.sy - ARROW, r.sx + ARROW, r.sy + ARROW]);
        }
        if end_arrow && careful {
            taken.extend([r.ex - ARROW, r.ey - ARROW, r.ex + ARROW, r.ey + ARROW]);
        }
        let over = over_fills(&r.d, &[r.sx, r.sy, r.ex, r.ey], &fills, &style.line, None);
        let ink = over.clone().filter(|o| !o.is_empty());
        let color = ink.clone().unwrap_or_else(|| style.stroke.as_deref().map_or("var(--_l)".to_string(), esc));
        let path = EdgePath { d: r.d.clone(), tokens: None, sx: r.sx, sy: r.sy, sdx: r.sdx, sdy: r.sdy, ex: r.ex, ey: r.ey, edx: r.edx, edy: r.edy };
        if over.as_deref() == Some("") {
            casings.push_str(&edge_casing(&path, if start_arrow { "arrow_point" } else { "none" }, if end_arrow { "arrow_point" } else { "none" }, &style.line, false));
        }
        edges_out.push_str(&format!(
            "<g class=\"pele-edge\" data-id=\"{}\"{}{}><path d=\"{}\"/>{}{}</g>",
            esc_text(&format!("{}-{}", rel.from, rel.to)),
            ink.as_ref().map_or(String::new(), |o| format!(" stroke=\"{}\"", o)),
            style.line,
            r.d,
            if start_arrow { marker("arrow_point", r.sx, r.sy, r.sdx, r.sdy, &color, "none") } else { String::new() },
            if end_arrow { marker("arrow_point", r.ex, r.ey, r.edx, r.edy, &color, "none") } else { String::new() }
        ));
        routes.push(r);
    }

    // A boundary's heading goes at the left, the middle or the right of its top edge: the first of them no line crosses.
    for &g in &lay.order {
        if !careful || lay.groups[g].head_w == 0.0 {
            continue;
        }
        let gb = lay.boxes[lay.groups[g].b].clone();
        let group = &mut lay.groups[g];
        let top = gb.y + HEAD_TOP;
        let bottom = gb.y + group.head - LANE;
        let mut fewest = usize::MAX;
        for x in [HEAD_LEFT, (gb.w - group.head_w) / 2.0, gb.w - HEAD_LEFT - group.head_w] {
            let count = routes.iter().filter(|r| crosses(&r.points, gb.x + x - 4.0, top, gb.x + x + group.head_w + 4.0, bottom)).count();
            if count < fewest {
                fewest = count;
                group.head_x = x;
            }
            if count == 0 {
                break;
            }
        }
        taken.extend([gb.x + group.head_x - 4.0, top, gb.x + group.head_x + group.head_w + 4.0, bottom]);
    }

    let mut labels_out = String::new();
    for (index, rel) in db.rels.iter().enumerate() {
        let r = &routes[index];
        let id = format!("{}-{}", rel.from, rel.to);
        let style = user_style(None, None, rel.text_color.as_deref());
        let text = format!("{}{}", if db.c4_type.as_deref() == Some("C4Dynamic") { format!("{}: ", index + 1) } else { String::new() }, rel.label);
        let label = layout_label(Some(&text), false, small, r.room, 0);
        let techn = layout_label(rel.techn.as_deref().filter(|t| !t.is_empty()).map(|t| format!("[{}]", t)).as_deref(), false, small, r.room, st::ITALIC);
        let lw = label.width.max(techn.width) + LABEL_PAD;
        let lh = label.height + techn.height;
        if lh == 0.0 {
            continue;
        }
        // The label sits on the line, or just beside it when another relation runs the opposite way.
        let beside = r.nx.abs() * (lw / 2.0) + r.ny.abs() * (lh / 2.0) + 2.0;
        let aside = if r.lean != 0.0 { 0f64.max(r.lean.min(beside - 14.0)) } else { beside };
        let mut dx = r.nx * aside + offset(rel.offset_x);
        let mut dy = r.ny * aside + offset(rel.offset_y);
        let mut at = 0.5;
        if careful {
            // Slide along the line to the spot that covers least.
            let mut least = f64::INFINITY;
            let moved = dx != r.nx * aside || dy != r.ny * aside;
            let mut pass = 0;
            while pass < if moved { 2 } else { 1 } && least > 0.0 {
                let px = if pass > 0 { r.nx * aside } else { dx };
                let py = if pass > 0 { r.ny * aside } else { dy };
                for t in SLIDE {
                    let cx = r.x0 + (r.x1 - r.x0) * t + px;
                    let cy = r.y0 + (r.y1 - r.y0) * t + py;
                    let area = covered(&taken, cx - lw / 2.0, cy - lh / 2.0, cx + lw / 2.0, cy + lh / 2.0);
                    if area < least {
                        least = area;
                        at = t;
                        dx = px;
                        dy = py;
                    }
                    if area == 0.0 {
                        break;
                    }
                }
                pass += 1;
            }
        }
        let cx = r.x0 + (r.x1 - r.x0) * at + dx;
        let top = r.y0 + (r.y1 - r.y0) * at + dy - lh / 2.0;
        if careful {
            taken.extend([cx - lw / 2.0, top, cx + lw / 2.0, top + lh]);
        }
        grow(cx - lw / 2.0, top, cx + lw / 2.0, top + lh, &mut bb);
        let body = label_svg(&label, cx, top + label.height / 2.0, &style.text)
            + &label_svg(&techn, cx, top + label.height + techn.height / 2.0, &format!(" class=\"pele-c4-techn\" fill=\"var(--_m)\"{}", style.text));
        labels_out.push_str(&format!(
            "<g class=\"pele-edge-label\" data-id=\"{}\">{}<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"3\" fill=\"var(--_bg)\"/>{}</g>",
            esc_text(&id),
            rel.descr.as_deref().filter(|d| !d.is_empty()).map_or(String::new(), |d| format!("<title>{}</title>", esc_text(d))),
            num(cx - lw / 2.0),
            num(top),
            num(lw),
            num(lh),
            link(&body, rel.link.as_deref(), &id, &mut links)
        ));
    }

    let mut clusters = String::new();
    let mut nodes_out = String::new();
    for &g in &lay.order {
        let group = &lay.groups[g];
        let gb = &lay.boxes[group.b];
        if g != 0 {
            let boundary = &db.boundaries[group.boundary];
            let style = user_style(boundary.bg_color.as_deref(), boundary.border_color.as_deref(), boundary.font_color.as_deref());
            let x = gb.x + group.head_x;
            let mut y = gb.y + HEAD_TOP;
            let color = ink_fill(&style.shape, &style.text, None) + &style.text;
            let muted = ink_fill(&style.shape, &style.text, Some("var(--_m)")) + &style.text;
            let mut text = left_label_svg(&group.labels[0], x, y, &format!(" class=\"pele-cluster-label\" font-weight=\"var(--_hw)\"{}", color));
            y += group.labels[0].height;
            text.push_str(&left_label_svg(&group.labels[1], x, y, &format!(" class=\"pele-c4-type\" font-size=\"{}\"{}", small, muted)));
            y += group.labels[1].height;
            text.push_str(&left_label_svg(&group.labels[2], x, y, &format!(" class=\"pele-c4-descr\" font-size=\"{}\"{}", small, muted)));
            clusters.push_str(&format!(
                "<g class=\"pele-cluster pele-c4-{}\" data-id=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"var(--_b)\"{}{}/>{}</g>",
                if boundary.node_type.is_some() { "node" } else { "boundary" },
                esc_text(&boundary.alias),
                num(gb.x),
                num(gb.y),
                num(gb.w),
                num(gb.h),
                if boundary.node_type.is_some() { "" } else { " stroke-dasharray=\"6 4\"" },
                rounded(&style.shape),
                link(&text, boundary.link.as_deref(), &boundary.alias, &mut links)
            ));
        }
        for node in &group.nodes {
            let shape = &db.shapes[node.shape];
            let nb = &lay.boxes[node.b];
            let external = shape.kind.starts_with("external");
            let style = user_style(shape.bg_color.as_deref(), shape.border_color.as_deref(), shape.font_color.as_deref());
            let stroke = format!(" stroke=\"var(--_b)\"{}", if external { " stroke-dasharray=\"4 3\"" } else { "" });
            let (w, h, form, labels) = (nb.w, nb.h, nb.form, &node.labels);
            let dx = if form == "h-cyl" { -queue_rx(h) / 2.0 } else { 0.0 };
            let dy = if form == "person" {
                HEAD / 2.0
            } else if form == "cyl" {
                cyl_ry(w) / 2.0
            } else {
                0.0
            };
            let color = ink_fill(&style.shape, &style.text, None) + &style.text;
            let muted = ink_fill(&style.shape, &style.text, Some("var(--_m)")) + &style.text;
            let mut y = dy - node.text_h / 2.0;
            let mut text = label_svg(&labels[0], dx, y + labels[0].height / 2.0, &format!(" class=\"pele-c4-type\" font-size=\"{}\"{}", small - 1.0, muted));
            y += labels[0].height;
            text.push_str(&label_svg(&labels[1], dx, y + labels[1].height / 2.0, &format!(" class=\"pele-label\" font-weight=\"var(--_hw)\"{}", color)));
            y += labels[1].height;
            text.push_str(&label_svg(&labels[2], dx, y + labels[2].height / 2.0, &format!(" class=\"pele-c4-techn\" font-size=\"{}\"{}", small, muted)));
            y += labels[2].height + 4.0;
            text.push_str(&label_svg(&labels[3], dx, y + labels[3].height / 2.0, &format!(" class=\"pele-c4-descr\" font-size=\"{}\"{}", small, color)));
            let body = draw_shape(form, w, h, &format!(" fill=\"var({})\"{}{}", if external { "--_bg" } else { "--_s" }, stroke, style.shape), &format!("{}{}", stroke, style.line)) + &text;
            nodes_out.push_str(&format!(
                "<g class=\"pele-node pele-shape-{} pele-c4-{}{}\" data-id=\"{}\" transform=\"translate({},{})\">{}</g>",
                form,
                shape.kind,
                if external { " pele-c4-external" } else { "" },
                esc_text(&shape.alias),
                num(nb.x + w / 2.0),
                num(nb.y + h / 2.0),
                link(&body, shape.link.as_deref(), &shape.alias, &mut links)
            ));
        }
    }

    let (min_x, min_y, max_x, max_y) = bb;
    let title = layout_label(db.title.as_deref(), false, size, 4000.0, st::BOLD);
    let title_height = if title.height > 0.0 { title.height + 12.0 } else { 0.0 };
    let inner = (max_x - min_x).max(title.width);
    let width = (inner + 2.0 * pad).ceil();
    let height = (max_y - min_y + title_height + 2.0 * pad).ceil();
    let ox = pad - min_x + (inner - (max_x - min_x)) / 2.0;
    let oy = pad + title_height - min_y;

    let mut content = label_svg(&title, width / 2.0, pad + title.height / 2.0, " class=\"pele-title\" font-weight=\"var(--_tw)\"");
    content.push_str(&format!("<g transform=\"translate({},{})\">", num(ox), num(oy)));
    if !clusters.is_empty() {
        content.push_str(&format!("<g class=\"pele-clusters\">{}</g>", clusters));
    }
    content.push_str(&casings_svg(&casings));
    if !edges_out.is_empty() {
        content.push_str(&format!("<g class=\"pele-edges\" fill=\"none\" stroke=\"var(--_l)\" stroke-linecap=\"round\">{}</g>", edges_out));
    }
    if !nodes_out.is_empty() {
        content.push_str(&format!("<g class=\"pele-nodes\">{}</g>", nodes_out));
    }
    if !labels_out.is_empty() {
        content.push_str(&format!("<g class=\"pele-edge-labels\" font-size=\"{}\">{}</g>", small, labels_out));
    }
    content.push_str("</g>");
    let acc = Accessible { acc_title: None, acc_descr: db.acc_descr.as_deref() };
    Ok(Rendered { svg: svg_document("c4", width, height, size, options.responsive, &acc, &content), width, height, links })
}

fn lay_boundary<'a>(db: &'a C4Db, lay: &super::layout::Layout, g: usize) -> &'a super::db::Element {
    &db.boundaries[lay.groups[g].boundary]
}
