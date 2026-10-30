// Port of src/diagrams/architecture/render.ts and icons.ts.

use super::layout::{place_on_grid, side_index};
use super::ArchitectureDb;
use crate::diagrams::common::fit_width::tighten;
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc_text, label_svg, num};
use crate::svg::edges::{edge_label_svg, marker, marker_trim};
use crate::svg::root::{svg_document, Accessible};
use crate::svg::theme::ROUNDED;
use crate::text::label::{layout_label, Label, Span};
use crate::text::measurer::{style, width as measure};
use crate::types::{RenderOptions, Rendered};
use crate::util::yaml::Value;

const STUB: f64 = 12.0;
const CORNER: f64 = 6.0;
const LABEL_GAP: f64 = 4.0;
const DOT: f64 = 3.0;
const DX: [f64; 4] = [-1.0, 1.0, 0.0, 0.0];
const DY: [f64; 4] = [0.0, 0.0, -1.0, 1.0];

// Outlines for the icon names Mermaid's architecture diagrams know without an icon pack, drawn on
// a 24 by 24 grid. Any other name is left to the host's icon resolver.
fn builtin_icon(name: &str) -> String {
    let d = match name {
        "database" => "M4 6a8 3 0 1 0 16 0a8 3 0 1 0-16 0M4 6v12a8 3 0 0 0 16 0V6M4 12a8 3 0 0 0 16 0",
        "server" => "M3 4h18v7H3zM3 13h18v7H3zM7 7.5h.01M7 16.5h.01M11 7.5h6M11 16.5h6",
        "disk" => "M5 3h14v18H5zM12 6.5a4 4 0 1 0 0 8a4 4 0 1 0 0-8M12 10.5h.01M8 18h3",
        "internet" => "M3 12a9 9 0 1 0 18 0a9 9 0 1 0-18 0M3 12h18M12 3a13 13 0 0 0 0 18M12 3a13 13 0 0 1 0 18",
        "cloud" => "M7 18a4 4 0 0 1-.5-7.97A6 6 0 0 1 18 9.5a4.25 4.25 0 0 1-.5 8.5z",
        _ => return String::new(),
    };
    format!("<path d=\"{}\" stroke=\"var(--_fg)\" stroke-width=\"1.5\"/>", d)
}

fn setting(config: &Config, key: &str, fallback: f64, min: f64, max: f64) -> f64 {
    match config.get("architecture").and_then(|v| v.as_map()).and_then(|m| m.get(key)) {
        Some(Value::Num(v)) if v.is_finite() => max.min(min.max(*v)),
        _ => fallback,
    }
}

// Maps the grid lines in use to consecutive indexes, so empty rows and columns take no room.
fn rank(mut values: Vec<i64>) -> Vec<(i64, usize)> {
    values.sort();
    let mut out: Vec<(i64, usize)> = Vec::new();
    for v in values {
        if out.last().map(|l| l.0) != Some(v) {
            let k = out.len();
            out.push((v, k));
        }
    }
    out
}

fn get(ranked: &[(i64, usize)], v: i64) -> usize {
    ranked[ranked.binary_search_by_key(&v, |e| e.0).unwrap()].1
}

fn icon_slot(name: &str, x: f64, y: f64, side: f64) -> String {
    format!(
        "<svg class=\"pele-icon\" data-icon=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\">{}</svg>",
        esc_text(name),
        num(x),
        num(y),
        num(side),
        num(side),
        builtin_icon(name)
    )
}

// Cuts a label down to a box, ending what is left with an ellipsis.
fn fit(label: &Label, max_width: f64, max_lines: usize) -> Label {
    let cut = label.lines.len() > max_lines;
    if !cut && label.width <= max_width {
        return label.clone();
    }
    let dots = measure("…", label.size, 0);
    let mut lines: Vec<Vec<Span>> = Vec::new();
    let mut widths: Vec<f64> = Vec::new();
    let mut widest: f64 = 0.0;
    for i in 0..max_lines.min(label.lines.len()) {
        let mut line = label.lines[i].clone();
        let mut width = label.widths[i];
        if width > max_width || (cut && i == max_lines - 1) {
            loop {
                let Some(span) = line.last_mut() else {
                    line.push(Span { text: "…".into(), style: 0, width: dots });
                    width = dots;
                    break;
                };
                let rest = width - span.width;
                if rest + dots < max_width {
                    let units: Vec<u16> = span.text.encode_utf16().collect();
                    let (mut lo, mut hi) = (0usize, units.len());
                    while lo < hi {
                        let mid = (lo + hi + 1) >> 1;
                        if rest + measure(&js::from16(&units[..mid]), label.size, span.style) + dots <= max_width {
                            lo = mid;
                        } else {
                            hi = mid - 1;
                        }
                    }
                    let code = if lo > 0 { units[lo - 1] } else { 0 };
                    if (0xd800..=0xdbff).contains(&code) {
                        lo -= 1;
                    }
                    span.text = format!("{}…", js::trim_end(&js::from16(&units[..lo])));
                    span.width = measure(&span.text, label.size, span.style);
                    width = rest + span.width;
                    break;
                }
                line.pop();
                width = rest;
            }
        }
        widest = widest.max(width);
        lines.push(line);
        widths.push(width);
    }
    let height = lines.len() as f64 * label.line_height;
    Label { lines, widths, width: widest, height, ..label.clone() }
}

#[derive(Clone, Copy)]
struct Port {
    x: f64,
    y: f64,
    dx: f64,
    dy: f64,
    // The box the route has to stay out of when it cannot leave straight.
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    // How far short of the port an arrowhead stops: the radius of a junction's dot.
    gap: f64,
    // Set where a junction is reached through the border of its group: the line carries on to the junction.
    wire: bool,
}

// An orthogonal route from one port to another, each left along its side's normal.
fn route(s: &Port, e: &Port) -> Vec<f64> {
    let (sx, sy, sdx, sdy) = (s.x, s.y, s.dx, s.dy);
    let (ex, ey, edx, edy) = (e.x, e.y, e.dx, e.dy);
    let s1x = sx + sdx * STUB;
    let s1y = sy + sdy * STUB;
    let e1x = ex + edx * STUB;
    let e1y = ey + edy * STUB;
    if sdx * edx + sdy * edy == 0.0 {
        let cx = if sdx != 0.0 { ex } else { sx };
        let cy = if sdx != 0.0 { sy } else { ey };
        if (cx - sx) * sdx + (cy - sy) * sdy > 0.0 && (cx - ex) * edx + (cy - ey) * edy > 0.0 {
            return vec![sx, sy, cx, cy, ex, ey];
        }
        return if sdx != 0.0 { vec![sx, sy, s1x, s1y, s1x, e1y, e1x, e1y, ex, ey] } else { vec![sx, sy, s1x, s1y, e1x, s1y, e1x, e1y, ex, ey] };
    }
    let sideways = sdx != 0.0;
    let offset = if sideways { ey - sy } else { ex - sx };
    if sdx == -edx && sdy == -edy {
        let ahead = (ex - sx) * sdx + (ey - sy) * sdy;
        if ahead > 0.0 {
            if offset.abs() < 0.5 {
                return vec![sx, sy, ex, ey];
            }
            // Turn midway between the two boxes, which is clear of both titles, when there is such a place.
            if sideways {
                let lo = if sdx > 0.0 { s.x1 } else { e.x1 };
                let hi = if sdx > 0.0 { e.x0 } else { s.x0 };
                let mx = if lo < hi { (lo + hi) / 2.0 } else { (sx + ex) / 2.0 };
                return vec![sx, sy, mx, sy, mx, ey, ex, ey];
            }
            let lo = if sdy > 0.0 { s.y1 } else { e.y1 };
            let hi = if sdy > 0.0 { e.y0 } else { s.y0 };
            let my = if lo < hi { (lo + hi) / 2.0 } else { (sy + ey) / 2.0 };
            return vec![sx, sy, sx, my, ex, my, ex, ey];
        }
    } else if offset.abs() >= 1.0 {
        if sideways {
            let x = if sdx > 0.0 { sx.max(ex) + STUB } else { sx.min(ex) - STUB };
            return vec![sx, sy, x, sy, x, ey, ex, ey];
        }
        let y = if sdy > 0.0 { sy.max(ey) + STUB } else { sy.min(ey) - STUB };
        return vec![sx, sy, sx, y, ex, y, ex, ey];
    }
    // The straight way is blocked by one of the two nodes: go between them, or around both.
    if sideways {
        let lane = if s.y1 + STUB <= e.y0 {
            (s.y1 + e.y0) / 2.0
        } else if e.y1 + STUB <= s.y0 {
            (e.y1 + s.y0) / 2.0
        } else {
            s.y1.max(e.y1) + STUB
        };
        return vec![sx, sy, s1x, sy, s1x, lane, e1x, lane, e1x, ey, ex, ey];
    }
    let lane = if s.x1 + STUB <= e.x0 {
        (s.x1 + e.x0) / 2.0
    } else if e.x1 + STUB <= s.x0 {
        (e.x1 + s.x0) / 2.0
    } else {
        s.x1.max(e.x1) + STUB
    };
    vec![sx, sy, sx, s1y, lane, s1y, lane, e1y, ex, e1y, ex, ey]
}

// Drops repeated points and joins runs that continue in a straight line.
fn simplify(p: &[f64]) -> Vec<f64> {
    let mut out = vec![p[0], p[1]];
    let mut i = 2;
    while i < p.len() {
        let x = p[i];
        let y = p[i + 1];
        let n = out.len();
        let same_x = (x - out[n - 2]).abs() < 0.01;
        let same_y = (y - out[n - 1]).abs() < 0.01;
        i += 2;
        if same_x && same_y {
            continue;
        }
        if n >= 4 && ((same_x && (out[n - 4] - out[n - 2]).abs() < 0.01) || (same_y && (out[n - 3] - out[n - 1]).abs() < 0.01)) {
            out[n - 2] = x;
            out[n - 1] = y;
        } else {
            out.push(x);
            out.push(y);
        }
    }
    out
}

fn path_data(p: &[f64], start_trim: f64, end_trim: f64) -> String {
    let n = p.len() / 2;
    let mut xs: Vec<f64> = (0..n).map(|i| p[i * 2]).collect();
    let mut ys: Vec<f64> = (0..n).map(|i| p[i * 2 + 1]).collect();
    let mut trim = |at: usize, toward: usize, amount: f64| {
        let len = js::hypot(xs[toward] - xs[at], ys[toward] - ys[at]);
        let t = amount.min(0f64.max(len - 1.0));
        if len > 0.0 && t > 0.0 {
            xs[at] += ((xs[toward] - xs[at]) / len) * t;
            ys[at] += ((ys[toward] - ys[at]) / len) * t;
        }
    };
    trim(0, 1, start_trim);
    trim(n - 1, n - 2, end_trim);
    let mut d = format!("M{},{}", num(xs[0]), num(ys[0]));
    for i in 1..n.saturating_sub(1) {
        let before = js::hypot(xs[i] - xs[i - 1], ys[i] - ys[i - 1]);
        let after = js::hypot(xs[i + 1] - xs[i], ys[i + 1] - ys[i]);
        let r = CORNER.min(before / 2.0).min(after / 2.0);
        if r < 0.5 {
            d.push_str(&format!("L{},{}", num(xs[i]), num(ys[i])));
            continue;
        }
        let in_x = xs[i] - ((xs[i] - xs[i - 1]) / before) * r;
        let in_y = ys[i] - ((ys[i] - ys[i - 1]) / before) * r;
        let out_x = xs[i] + ((xs[i + 1] - xs[i]) / after) * r;
        let out_y = ys[i] + ((ys[i + 1] - ys[i]) / after) * r;
        d.push_str(&format!("L{},{}Q{},{} {},{}", num(in_x), num(in_y), num(xs[i]), num(ys[i]), num(out_x), num(out_y)));
    }
    d + &format!("L{},{}", num(xs[n - 1]), num(ys[n - 1]))
}

pub fn render_architecture(model: &ArchitectureDb, config: &Config, options: &RenderOptions) -> Rendered {
    tighten(options, |tight| draw(model, config, options, tight))
}

fn draw(model: &ArchitectureDb, config: &Config, options: &RenderOptions, tight: bool) -> Rendered {
    let icon_size = setting(config, "iconSize", 48.0, 8.0, 512.0);
    let size = setting(config, "fontSize", 16.0, 4.0, 128.0);
    let group_pad = setting(config, "padding", 20.0, 0.0, 256.0).min(if tight { 12.0 } else { f64::INFINITY });
    let small = js::round(size * 0.875);
    let pad = 8.0;
    let half = icon_size / 2.0;

    let nodes = &model.nodes;
    let groups = &model.groups;
    let n = nodes.len();
    let g_count = groups.len();
    let grid = place_on_grid(nodes, groups, &model.edges, &model.layout_hints);
    let index = |id: &str| nodes.iter().rposition(|node| node.id == id);

    // A service is its icon square with the title beneath; a junction takes the same room.
    let mut labels: Vec<Label> = Vec::new();
    let mut box_w = vec![0f64; n];
    let mut box_h = vec![0f64; n];
    let wrap = if tight { icon_size.max(64.0) } else { (icon_size * 2.25).max(96.0) };
    for i in 0..n {
        let node = &nodes[i];
        let label = layout_label(if node.junction { None } else { node.title.as_deref() }, true, size, wrap, 0);
        box_w[i] = icon_size.max(2.0 * (label.width / 2.0).ceil());
        box_h[i] = icon_size + if label.height > 0.0 { LABEL_GAP + label.height } else { 0.0 };
        labels.push(label);
    }

    let mut col_values: Vec<i64> = Vec::new();
    let mut row_values: Vec<i64> = Vec::new();
    for i in 0..n {
        col_values.push(grid.col[i]);
        row_values.push(grid.row[i]);
    }
    for g in 0..g_count {
        if !grid.group_empty[g] {
            continue;
        }
        col_values.push(grid.group_col[g]);
        row_values.push(grid.group_row[g]);
    }
    let col_of = rank(col_values);
    let row_of = rank(row_values);
    let cc = col_of.len();
    let rr = row_of.len();
    let mut col_w = vec![0f64; cc];
    let mut row_h = vec![0f64; rr];
    let gap = if tight { 24f64.max(2.0 * STUB) } else { 40f64.max(js::round(icon_size * 0.75)) };
    let mut gap_x = vec![gap; cc];
    let mut gap_y = vec![gap; rr];
    let mut node_col = vec![0usize; n];
    let mut node_row = vec![0usize; n];
    for i in 0..n {
        let c = get(&col_of, grid.col[i]);
        let r = get(&row_of, grid.row[i]);
        node_col[i] = c;
        node_row[i] = r;
        col_w[c] = col_w[c].max(box_w[i]);
        row_h[r] = row_h[r].max(box_h[i]);
    }
    for g in 0..g_count {
        if !grid.group_empty[g] {
            continue;
        }
        let c = get(&col_of, grid.group_col[g]);
        let r = get(&row_of, grid.group_row[g]);
        col_w[c] = col_w[c].max(icon_size);
        row_h[r] = row_h[r].max(half);
    }

    // An edge label needs the gap its edge crosses to be wide enough to hold it.
    let e_count = model.edges.len();
    let mut edge_labels: Vec<Label> = Vec::new();
    let mut ends: Vec<Option<usize>> = vec![None; e_count * 2];
    for k in 0..e_count {
        let edge = &model.edges[k];
        let a = index(&edge.lhs_id);
        let b = index(&edge.rhs_id);
        ends[k * 2] = a;
        ends[k * 2 + 1] = b;
        let label = layout_label(edge.title.as_deref(), true, small, 160.0, 0);
        let width = label.width;
        let height = label.height;
        edge_labels.push(label);
        let (Some(a), Some(b)) = (a, b) else { continue };
        if width == 0.0 {
            continue;
        }
        if node_col[a] != node_col[b] {
            let c = node_col[a].min(node_col[b]);
            gap_x[c] = gap_x[c].max(width.ceil() + 28.0);
        } else {
            col_w[node_col[a]] = col_w[node_col[a]].max(2.0 * (width / 2.0).ceil() + 8.0);
            if node_row[a] != node_row[b] {
                let r = node_row[a].min(node_row[b]);
                gap_y[r] = gap_y[r].max(height.ceil() + 24.0);
            }
        }
    }

    // Room around each group, kept on the rows and columns at its edges. A group that shares an
    // edge with a group inside it stands that much further out. Children come after their parents,
    // so walking backwards settles every group before the one that holds it.
    let mut group_labels: Vec<Label> = Vec::new();
    let mut c0 = vec![0usize; g_count];
    let mut c1 = vec![0usize; g_count];
    let mut r0 = vec![0usize; g_count];
    let mut r1 = vec![0usize; g_count];
    let mut ins_l = vec![0f64; g_count];
    let mut ins_r = vec![0f64; g_count];
    let mut ins_t = vec![0f64; g_count];
    let mut ins_b = vec![0f64; g_count];
    let mut head_w = vec![0f64; g_count];
    let mut head_h = vec![0f64; g_count];
    let mut col_l = vec![0f64; cc];
    let mut col_r = vec![0f64; cc];
    let mut row_t = vec![0f64; rr];
    let mut row_b = vec![0f64; rr];
    let badge = js::round(size * 1.25);
    for g in 0..g_count {
        c0[g] = get(&col_of, grid.group_col[g]);
        c1[g] = get(&col_of, grid.group_col[g] + grid.group_cols[g] - 1);
        r0[g] = get(&row_of, grid.group_row[g]);
        r1[g] = get(&row_of, grid.group_row[g] + grid.group_rows[g] - 1);
        let label = layout_label(groups[g].title.as_deref(), true, size, if tight { 80f64.max(4000.0 / 2.0) } else { 320.0 }, 0);
        let icon = if groups[g].icon.as_deref().is_some_and(|i| !i.is_empty()) { badge } else { 0.0 };
        head_w[g] = label.width + icon + if label.width > 0.0 && icon > 0.0 { 6.0 } else { 0.0 };
        head_h[g] = label.height.max(icon);
        group_labels.push(label);
    }
    for g in (0..g_count).rev() {
        let left = group_pad + ins_l[g];
        let mut right = group_pad + ins_r[g];
        let top = (if head_h[g] > 0.0 { group_pad.max(head_h[g] + 12.0) } else { group_pad }) + ins_t[g];
        let bottom = group_pad + ins_b[g];
        if head_w[g] > 0.0 {
            let mut inner = left + right;
            for c in c0[g]..=c1[g] {
                inner += col_w[c] + if c < c1[g] { col_r[c] + gap_x[c] + col_l[c + 1] } else { 0.0 };
            }
            if head_w[g] + 20.0 > inner {
                right += (head_w[g] + 20.0 - inner).ceil();
            }
        }
        ins_l[g] = left;
        ins_r[g] = right;
        ins_t[g] = top;
        ins_b[g] = bottom;
        col_l[c0[g]] = col_l[c0[g]].max(left);
        col_r[c1[g]] = col_r[c1[g]].max(right);
        row_t[r0[g]] = row_t[r0[g]].max(top);
        row_b[r1[g]] = row_b[r1[g]].max(bottom);
        let p = grid.group_parent[g];
        if p < 0 {
            continue;
        }
        let p = p as usize;
        if c0[p] == c0[g] {
            ins_l[p] = ins_l[p].max(left);
        }
        if c1[p] == c1[g] {
            ins_r[p] = ins_r[p].max(right);
        }
        if r0[p] == r0[g] {
            ins_t[p] = ins_t[p].max(top);
        }
        if r1[p] == r1[g] {
            ins_b[p] = ins_b[p].max(bottom);
        }
    }

    let mut xs = vec![0f64; cc];
    let mut ys = vec![0f64; rr];
    let mut inner_w = 0.0;
    for c in 0..cc {
        xs[c] = inner_w + col_l[c];
        inner_w = xs[c] + col_w[c] + col_r[c] + if c + 1 < cc { gap_x[c] } else { 0.0 };
    }
    let mut inner_h = 0.0;
    for r in 0..rr {
        ys[r] = inner_h + row_t[r];
        inner_h = ys[r] + row_h[r] + row_b[r] + if r + 1 < rr { gap_y[r] } else { 0.0 };
    }
    // What is drawn, which an edge that has to go around a node can push past the grid.
    let mut min_x: f64 = 0.0;
    let mut min_y: f64 = 0.0;
    let mut max_x: f64 = inner_w;
    let mut max_y: f64 = inner_h;

    let left = |g: usize| xs[c0[g]] - ins_l[g];
    let right = |g: usize| xs[c1[g]] + col_w[c1[g]] + ins_r[g];
    let top = |g: usize| ys[r0[g]] - ins_t[g];
    let bottom = |g: usize| ys[r1[g]] + row_h[r1[g]] + ins_b[g];
    let center_x = |i: usize| xs[node_col[i]] + col_w[node_col[i]] / 2.0;

    let port = |i: usize, side: usize, via_group: bool| -> Port {
        let dx = DX[side];
        let dy = DY[side];
        let cx = center_x(i);
        let y = ys[node_row[i]];
        let mut p = Port { x: cx, y: y + half, dx, dy, x0: cx - box_w[i] / 2.0, y0: y, x1: cx + box_w[i] / 2.0, y1: y + box_h[i], gap: 0.0, wire: false };
        if nodes[i].junction {
            p.x0 = cx;
            p.x1 = cx;
            p.y0 = p.y;
            p.y1 = p.y;
            p.gap = DOT;
        } else if dx != 0.0 {
            p.x = cx + dx * half;
        } else {
            p.y = if dy < 0.0 { y } else { y + box_h[i] + if labels[i].height > 0.0 { 2.0 } else { 0.0 } };
        }
        let g = if via_group { grid.node_parent[i] } else { -1 };
        if g >= 0 {
            let g = g as usize;
            p.x0 = left(g);
            p.x1 = right(g);
            p.y0 = top(g);
            p.y1 = bottom(g);
            p.gap = 0.0;
            p.wire = nodes[i].junction;
            if dx != 0.0 {
                p.x = if dx < 0.0 { p.x0 } else { p.x1 };
            } else {
                p.y = if dy < 0.0 { p.y0 } else { p.y1 };
            }
        }
        p
    };

    let mut edges_out = String::new();
    let mut labels_out = String::new();
    // Vertical runs of the edges, to keep group titles out from under them.
    let mut runs: Vec<f64> = Vec::new();
    let trim_length = marker_trim("arrow_point");
    for k in 0..e_count {
        let edge = &model.edges[k];
        let (Some(a), Some(b)) = (ends[k * 2], ends[k * 2 + 1]) else { continue };
        let mut s = port(a, side_index(edge.lhs_dir), edge.lhs_group);
        let mut e = port(b, side_index(edge.rhs_dir), edge.rhs_group);
        if s.x == e.x && s.y == e.y && s.dx == e.dx {
            // An edge from a side back to the same side: part the two ends so it shows as a loop.
            s.x -= 8.0 * s.dy.abs();
            s.y -= 8.0 * s.dx.abs();
            e.x += 8.0 * s.dy.abs();
            e.y += 8.0 * s.dx.abs();
        }
        let mut routed = route(&s, &e);
        if s.wire {
            routed.splice(0..0, [center_x(a), ys[node_row[a]] + half]);
        }
        if e.wire {
            routed.push(center_x(b));
            routed.push(ys[node_row[b]] + half);
        }
        let points = simplify(&routed);
        if points.len() < 4 {
            continue;
        }
        let id = esc_text(&format!("{}-{}", edge.lhs_id, edge.rhs_id));
        edges_out.push_str(&format!(
            "<g class=\"pele-edge\" data-id=\"{}\"><path d=\"{}\"/>{}{}</g>",
            id,
            path_data(&points, if edge.lhs_into && !s.wire { trim_length + s.gap } else { 0.0 }, if edge.rhs_into && !e.wire { trim_length + e.gap } else { 0.0 }),
            if edge.lhs_into { marker("arrow_point", s.x + s.dx * s.gap, s.y + s.dy * s.gap, -s.dx, -s.dy, "var(--_l)", "none") } else { String::new() },
            if edge.rhs_into { marker("arrow_point", e.x + e.dx * e.gap, e.y + e.dy * e.gap, -e.dx, -e.dy, "var(--_l)", "none") } else { String::new() }
        ));
        let mut best: isize = -1;
        let mut best_length = -1.0;
        let mut flat: isize = -1;
        let mut flat_length = -1.0;
        let label = &edge_labels[k];
        let mut i = 0;
        while i < points.len() {
            min_x = min_x.min(points[i] - 4.0);
            max_x = max_x.max(points[i] + 4.0);
            min_y = min_y.min(points[i + 1] - 4.0);
            max_y = max_y.max(points[i + 1] + 4.0);
            i += 2;
        }
        let mut i = 0;
        while i + 3 < points.len() {
            let horizontal = (points[i + 1] - points[i + 3]).abs() < 0.01;
            let length = (points[i] - points[i + 2]).abs() + (points[i + 1] - points[i + 3]).abs();
            if !horizontal {
                runs.extend([points[i], points[i + 1].min(points[i + 3]), points[i + 1].max(points[i + 3])]);
            }
            if length > best_length {
                best = i as isize;
                best_length = length;
            }
            if horizontal && length > flat_length {
                flat = i as isize;
                flat_length = length;
            }
            i += 2;
        }
        if label.width > 0.0 {
            let at = (if flat_length >= label.width + 8.0 { flat } else { best }) as usize;
            let mut x = (points[at] + points[at + 2]) / 2.0;
            let mut y = (points[at + 1] + points[at + 3]) / 2.0;
            let w = label.width + 8.0;
            let upright = (points[at] - points[at + 2]).abs() < 0.01;
            // On a short run next to a node the label would land on the node, so it moves off to the side.
            for p in [&s, &e] {
                if x + w / 2.0 <= p.x0 || x - w / 2.0 >= p.x1 || y + label.height / 2.0 <= p.y0 || y - label.height / 2.0 >= p.y1 {
                    continue;
                }
                if upright {
                    x = if x >= (p.x0 + p.x1) / 2.0 { p.x1 + w / 2.0 + 4.0 } else { p.x0 - w / 2.0 - 4.0 };
                } else {
                    y = if y >= (p.y0 + p.y1) / 2.0 { p.y1 + label.height / 2.0 + 4.0 } else { p.y0 - label.height / 2.0 - 4.0 };
                }
            }
            min_x = min_x.min(x - w / 2.0);
            max_x = max_x.max(x + w / 2.0);
            min_y = min_y.min(y - label.height / 2.0);
            max_y = max_y.max(y + label.height / 2.0);
            labels_out.push_str(&edge_label_svg(&format!("{}-{}", edge.lhs_id, edge.rhs_id), label, x, y, ""));
        }
    }

    let mut clusters = String::new();
    for g in 0..g_count {
        let group = &groups[g];
        let x = left(g);
        let y = top(g);
        let w = right(g) - x;
        let label = &group_labels[g];
        let mut head = String::new();
        if head_w[g] > 0.0 {
            // The title starts at the left, or after the edges that come down through that corner.
            let band = y + head_h[g] + 12.0;
            let mut crossings: Vec<f64> = Vec::new();
            let mut i = 0;
            while i < runs.len() {
                if runs[i + 1] < band && runs[i + 2] > y && runs[i] > x && runs[i] < x + w {
                    crossings.push(runs[i]);
                }
                i += 3;
            }
            crossings.sort_by(|a, b| js::ord(a - b));
            crossings.push(x + w - 4.0);
            let mut start = x + 10.0;
            for crossing in crossings {
                if crossing - 6.0 - start >= head_w[g] {
                    break;
                }
                start = start.max(crossing + 6.0);
            }
            if start + head_w[g] > x + w - 4.0 {
                start = x + 10.0;
            }
            let cy = y + 6.0 + head_h[g] / 2.0;
            let mut text_x = start;
            if let Some(icon) = group.icon.as_deref().filter(|i| !i.is_empty()) {
                head.push_str(&icon_slot(icon, start, cy - badge / 2.0, badge));
                text_x += badge + 6.0;
            }
            head.push_str(&label_svg(label, text_x + label.width / 2.0, cy, " class=\"pele-cluster-label\" fill=\"var(--_m)\""));
        }
        clusters.push_str(&format!(
            "<g class=\"pele-cluster\" data-id=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_a)\" fill-opacity=\"0.5\" stroke=\"var(--_b)\"{}/>{}</g>",
            esc_text(&group.id),
            num(x),
            num(y),
            num(w),
            num(bottom(g) - y),
            ROUNDED,
            head
        ));
    }

    let mut nodes_out = String::new();
    let glyph = icon_size * 0.5;
    let text_size = 6f64.max(js::round(size * 0.75));
    for i in 0..n {
        let node = &nodes[i];
        let at = format!("data-id=\"{}\" transform=\"translate({},{})\"", esc_text(&node.id), num(center_x(i)), num(ys[node_row[i]]));
        if node.junction {
            nodes_out.push_str(&format!("<g class=\"pele-node pele-junction\" {}><circle cy=\"{}\" r=\"{}\" fill=\"var(--_l)\"/></g>", at, num(half), js::num_str(DOT)));
            continue;
        }
        let mut inner = String::new();
        if let Some(icon) = node.icon.as_deref().filter(|i| !i.is_empty()) {
            inner = icon_slot(icon, -glyph / 2.0, (icon_size - glyph) / 2.0, glyph);
        } else if let Some(text) = node.icon_text.as_deref().filter(|t| !t.is_empty()) {
            let text = layout_label(Some(text), false, text_size, icon_size - 8.0, 0);
            let lh = js::or(text.line_height, || 1.0);
            let lines = 1f64.max(((icon_size - 4.0) / lh).floor()) as usize;
            inner = label_svg(&fit(&text, icon_size - 8.0, lines), 0.0, half, &format!(" class=\"pele-icon-text\" font-size=\"{}\"", js::num_str(text_size)));
        }
        nodes_out.push_str(&format!(
            "<g class=\"pele-node pele-service\" {}><rect x=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_s)\" stroke=\"var(--_b)\"{}/>{}{}</g>",
            at,
            num(-half),
            num(icon_size),
            num(icon_size),
            ROUNDED,
            inner,
            label_svg(&labels[i], 0.0, icon_size + LABEL_GAP + labels[i].height / 2.0, " class=\"pele-label\"")
        ));
    }

    let title = layout_label(model.title.as_deref(), false, size, 4000.0, style::BOLD);
    let title_height = if title.height > 0.0 { title.height + 12.0 } else { 0.0 };
    let width = ((max_x - min_x).max(title.width) + 2.0 * pad).ceil();
    let height = (max_y - min_y + title_height + 2.0 * pad).ceil();
    let ox = js::round((width - (max_x - min_x)) / 2.0 - min_x);
    let oy = js::round(pad + title_height - min_y);
    let mut body = String::new();
    if !clusters.is_empty() {
        body.push_str(&format!("<g class=\"pele-clusters\">{}</g>", clusters));
    }
    if !edges_out.is_empty() {
        body.push_str(&format!("<g class=\"pele-edges\" fill=\"none\" stroke=\"var(--_l)\" stroke-linecap=\"round\" stroke-linejoin=\"round\">{}</g>", edges_out));
    }
    if !labels_out.is_empty() {
        body.push_str(&format!("<g class=\"pele-edge-labels\" font-size=\"{}\">{}</g>", js::num_str(small), labels_out));
    }
    if !nodes_out.is_empty() {
        body.push_str(&format!("<g class=\"pele-nodes\">{}</g>", nodes_out));
    }
    let mut content = label_svg(&title, width / 2.0, pad + title.height / 2.0, " class=\"pele-title\" font-weight=\"var(--_tw)\"");
    if !body.is_empty() {
        content.push_str(&format!("<g transform=\"translate({},{})\">{}</g>", js::num_str(ox), js::num_str(oy), body));
    }
    let acc = Accessible { acc_title: model.acc_title.as_deref(), acc_descr: model.acc_descr.as_deref() };
    let svg = svg_document("architecture", width, height, size, options.responsive, &acc, &content);
    Rendered { svg, width, height, links: Vec::new() }
}
