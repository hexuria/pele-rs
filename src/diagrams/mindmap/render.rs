// Port of src/diagrams/mindmap/render.ts. With no `maxWidth`, `turnToFit` never asks for the
// outline drawn down, and with no icon resolver no icon takes room, so those branches are left
// out.

use super::db::MindmapDb;
use crate::diagrams::common::fit_width::{title_room, turn_to_fit};
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc_text, label_svg, num};
use crate::svg::root::{svg_document, Accessible};
use crate::svg::shapes::{draw_shape, shape_inset, shape_size};
use crate::svg::theme::{class_names, series_color};
use crate::svg::title::title_label;
use crate::text::label::{layout_label, Label};
use crate::types::{RenderOptions, Rendered};
use crate::util::yaml::Value;

// Indexed by Mermaid's node type. A node without delimiters is text on a line, except the root.
const SHAPES: [&str; 7] = ["", "rounded", "rect", "circle", "cloud", "bang", "hex"];
const ROOT_SHAPE: &str = "stadium";

const GAP_X: f64 = 36.0;
const GAP_Y: f64 = 8.0;
const GROUP_GAP: f64 = 18.0;
const BRANCH_GAP_X: f64 = 52.0;
const BRANCH_GAP_Y: f64 = 28.0;
const TEXT_PAD: f64 = 6.0;
const PILL_PAD: f64 = 12.0;
const TINT: f64 = 0.22;

pub fn render_mindmap(model: &MindmapDb, config: &Config, options: &RenderOptions) -> Rendered {
    turn_to_fit(options, |_down| draw(model, config, options))
}

struct Stack<'a> {
    extent: &'a [f64],
    rel: &'a mut [f64],
    own: &'a [f64],
    anchor: &'a [f64],
}

fn draw(model: &MindmapDb, config: &Config, options: &RenderOptions) -> Rendered {
    let size = 16.0;
    let pad = 8.0;
    let wrap = match config.get("mindmap").and_then(|v| v.as_map()).and_then(|m| m.get("maxNodeWidth")) {
        Some(Value::Num(m)) if *m > 0.0 => m.min(2000.0),
        _ => 200.0,
    };

    let nodes = &model.nodes;
    let n = nodes.len();
    let mut parent = vec![0usize; n];
    let mut depth = vec![0i32; n];
    for i in 0..n {
        for &c in &nodes[i].children {
            parent[c] = i;
            depth[c] = depth[i] + 1;
        }
    }
    let mut labels: Vec<Label> = Vec::with_capacity(n);
    let mut shapes: Vec<&str> = Vec::with_capacity(n);
    let mut w = vec![0.0; n];
    let mut h = vec![0.0; n];
    let mut dy = vec![0.0; n];
    let mut anchor = vec![0.0; n];

    for i in 0..n {
        let node = &nodes[i];
        let fit = wrap;
        let label = layout_label(Some(&node.descr), true, size, fit, 0);
        let tw = label.width;
        let th = label.height.max(js::round(size * 1.5));
        let mut shape = SHAPES.get(node.type_ as usize).copied().unwrap_or("");
        if shape.is_empty() && i == 0 {
            shape = ROOT_SHAPE;
        }
        shapes.push(shape);
        if shape.is_empty() {
            let pill = !node.children.is_empty();
            w[i] = tw + 2.0 * if pill { PILL_PAD } else { TEXT_PAD };
            h[i] = th + if pill { 6.0 } else { 0.0 };
            anchor[i] = h[i] / 2.0;
        } else {
            let s = shape_size(shape, tw, th);
            w[i] = s.w;
            h[i] = s.h;
            dy[i] = s.dy;
            anchor[i] = s.h / 2.0;
        }
        labels.push(label);
    }

    let mut extent = vec![0.0; n];
    let mut rel = vec![0.0; n];
    let mut own = vec![0.0; n];
    let mut kids = vec![0.0; n];

    // Stacks subtrees top to bottom and returns the height at which their parent's anchor
    // belongs, and the height stacked.
    fn stack(s: &mut Stack, nodes: &[super::db::MindmapNode], list: &[usize], from: usize, to: usize, gap: f64) -> (f64, f64) {
        let mut y = 0.0;
        let mut last = 0.0;
        for k in from..to {
            let c = list[k];
            s.rel[c] = y;
            let grouped = !nodes[list[k]].children.is_empty() || (k + 1 < to && !nodes[list[k + 1]].children.is_empty());
            last = if grouped { gap.max(GROUP_GAP) } else { gap };
            y += s.extent[c] + last;
        }
        let stacked = y - last;
        let a = list[from];
        let b = list[to - 1];
        ((s.rel[a] + s.own[a] + s.anchor[a] + s.rel[b] + s.own[b] + s.anchor[b]) / 2.0, stacked)
    }

    for i in (1..n).rev() {
        let children = &nodes[i].children;
        if children.is_empty() {
            extent[i] = h[i];
            continue;
        }
        let (mid, stacked) = stack(&mut Stack { extent: &extent, rel: &mut rel, own: &own, anchor: &anchor }, nodes, children, 0, children.len(), GAP_Y);
        let top = mid - anchor[i];
        let low = 0f64.min(top);
        extent[i] = stacked.max(top + h[i]) - low;
        own[i] = top - low;
        kids[i] = -low;
    }

    let mut x = vec![0.0; n];
    let mut y = vec![0.0; n];
    let mut band = vec![0.0; n];
    let mut side = vec![0i8; n];

    if n > 0 {
        // The first branches go right and the rest left, split where the two sides are closest in height.
        let branches = &nodes[0].children;
        let count = branches.len();
        let mut total = 0.0;
        for &b in branches {
            total += extent[b];
        }
        let mut split = count;
        let mut best = f64::INFINITY;
        let mut right = 0.0;
        for k in 1..count {
            right += extent[branches[k - 1]];
            let diff = (2.0 * right - total + (2.0 * k as f64 - count as f64) * BRANCH_GAP_Y).abs();
            if diff < best {
                best = diff;
                split = k;
            }
        }
        let (mut from, mut to, mut dir) = (0usize, split, 1i8);
        while from < count {
            let (middle, _) = stack(&mut Stack { extent: &extent, rel: &mut rel, own: &own, anchor: &anchor }, nodes, branches, from, to, BRANCH_GAP_Y);
            for &c in &branches[from..to] {
                band[c] = rel[c] - middle;
                side[c] = dir;
            }
            from = to;
            to = count;
            dir = -1;
        }
        y[0] = -h[0] / 2.0;
        for i in 0..n {
            if i > 0 {
                y[i] = band[i] + own[i];
            }
            for &c in &nodes[i].children {
                if i > 0 {
                    side[c] = side[i];
                    band[c] = band[i] + kids[i] + rel[c];
                }
                x[c] = x[i] + side[c] as f64 * (w[i] / 2.0 + if i > 0 { GAP_X } else { BRANCH_GAP_X } + w[c] / 2.0);
            }
        }
    }

    let (mut min_x, mut max_x, mut min_y, mut max_y) = (0f64, 0f64, 0f64, 0f64);
    let mut out = String::new();
    let mut edges = String::new();
    let mut body = String::new();
    let mut color = String::new();
    let flush = |section: Option<usize>, out: &mut String, edges: &mut String, body: &mut String, color: &str| {
        if body.is_empty() {
            return;
        }
        out.push_str(&format!(
            "<g class=\"pele-branch pele-section-{}\"><g class=\"pele-edges\" fill=\"none\" stroke=\"{}\" stroke-width=\"1.5\">{}</g>{}</g>",
            section.unwrap_or(0),
            color,
            edges,
            body
        ));
        edges.clear();
        body.clear();
    };

    // The root is drawn last, on top of the links that leave it.
    for k in 1..=n {
        let i = k % n;
        let node = &nodes[i];
        let shape = shapes[i];
        let label = &labels[i];
        let cx = x[i];
        let top = y[i];
        min_x = min_x.min(cx - w[i] / 2.0);
        max_x = max_x.max(cx + w[i] / 2.0);
        min_y = min_y.min(top);
        max_y = max_y.max(top + h[i]);

        if i == 0 || parent[i] == 0 {
            flush(nodes[k - 1].section, &mut out, &mut edges, &mut body, &color);
        }
        if i > 0 {
            if parent[i] == 0 {
                color = series_color(node.section.unwrap_or(0) as i64);
            }
            let p = parent[i];
            let dir = side[i] as f64;
            let from = if dir > 0.0 { 1 } else { 3 };
            let x1 = x[p] + dir * (w[p] / 2.0 - if !shapes[p].is_empty() { shape_inset(shapes[p], w[p], h[p], from, 0.0) } else { 0.0 });
            let y1 = y[p] + anchor[p];
            let near = cx - (dir * w[i]) / 2.0;
            let x2 = near + if !shape.is_empty() { dir * shape_inset(shape, w[i], h[i], 4 - from, 0.0) } else { 0.0 };
            let y2 = top + anchor[i];
            let mid = num((x1 + x2) / 2.0);
            edges.push_str(&format!("<path class=\"pele-edge\" d=\"M{},{}C{},{} {},{} {},{}\"/>", num(x1), num(y1), mid, num(y1), mid, num(y2), num(x2), num(y2)));
        }

        let pill = shape.is_empty() && !node.children.is_empty();
        let tint = format!(" fill=\"{}\" fill-opacity=\"{}\" stroke=\"{}\"", color, TINT, color);
        let mut inner = String::new();
        if !shape.is_empty() {
            inner = if i > 0 {
                draw_shape(shape, w[i], h[i], &tint, &format!(" stroke=\"{}\"", color))
            } else {
                draw_shape(shape, w[i], h[i], " fill=\"var(--_s)\" stroke=\"var(--_b)\"", " stroke=\"var(--_b)\"")
            };
        } else if pill {
            inner = format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\"{}/>", num(-w[i] / 2.0), num(-h[i] / 2.0), num(w[i]), num(h[i]), num(h[i] / 2.0), tint);
        }
        let group = format!(
            "<g class=\"pele-node pele-shape-{}{}{}\" data-id=\"{}\" transform=\"translate({},{})\">{}{}</g>",
            if !shape.is_empty() {
                shape
            } else if pill {
                "branch"
            } else {
                "text"
            },
            if i > 0 { "" } else { " pele-root" },
            class_names(node.class.as_deref().unwrap_or("")),
            esc_text(&node.node_id),
            num(cx),
            num(top + h[i] / 2.0),
            inner,
            label_svg(label, 0.0, dy[i], " class=\"pele-label\"")
        );
        if i > 0 {
            body.push_str(&group);
        } else {
            out.push_str(&group);
        }
    }

    let title = title_label(model.title.as_deref(), size, title_room(options));
    let title_height = if title.height > 0.0 { title.height + 16.0 } else { 0.0 };
    let width = (max_x - min_x).max(title.width);
    let total_width = (width + 2.0 * pad).ceil();
    let total_height = (max_y - min_y + title_height + 2.0 * pad).ceil();
    let content = label_svg(&title, total_width / 2.0, pad + title.height / 2.0, " class=\"pele-title\" font-weight=\"var(--_tw)\"")
        + &format!("<g transform=\"translate({},{})\">{}</g>", num(pad - min_x + (width - max_x + min_x) / 2.0), num(pad + title_height - min_y), out);
    Rendered { svg: svg_document("mindmap", total_width, total_height, size, options.responsive, &Accessible::default(), &content), width: total_width, height: total_height, links: Vec::new() }
}
