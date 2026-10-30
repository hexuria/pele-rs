// Port of src/diagrams/block/render.ts. With no `maxWidth`, `tighten` draws once with room to
// spare, so `tight` is always false. Views and their cells are in breadth-first order from the
// root; upstream's object identities are indexes into that order.

use super::arrows::{arrow_axis, arrow_mask, arrow_size, draw_arrow, OVERHANG};
use super::db::{Block, BlockDb};
use super::layout::{layout, Cell};
use crate::diagrams::common::fit_width::tighten;
use crate::errors::PeleError;
use crate::js::{self, hypot};
use crate::preprocess::Config;
use crate::svg::builder::{esc_text, label_svg, num};
use crate::svg::edges::{casings_svg, edge_casing, edge_label_svg, marker, marker_trim, over_fills, EdgePath, Fill, ARROW};
use crate::svg::ink::{ink_fill, paints};
use crate::svg::root::{svg_document, Accessible};
use crate::svg::shapes::{draw_shape, shape_filled, shape_inset, shape_size};
use crate::svg::theme::{class_names, resolve_style, rounded, sets_fill, ResolvedStyle};
use crate::svg::title::title_label;
use crate::text::label::{layout_label, Label};
use crate::text::measurer::style as st;
use crate::types::{RenderOptions, Rendered};
use crate::util::yaml::Value;
use std::collections::HashMap;
use std::f64::consts::{FRAC_1_SQRT_2, PI};

const GAP: f64 = 12.0;
const WRAP: f64 = 200.0;
const CORNER: f64 = 8.0;
const SHAPE_ATTRS: &str = " fill=\"var(--_s)\" stroke=\"var(--_b)\"";
const LINE_ATTRS: &str = " stroke=\"var(--_b)\"";
// Checking every edge against every block is quadratic, so large diagrams route without it.
const OBSTACLE_BUDGET: usize = 2_000_000;
const LABEL_GAP: f64 = 4.0;
const SPOTS: [f64; 5] = [0.5, 0.3, 0.7, 0.15, 0.85];

fn shape_of(type_: &str) -> &'static str {
    match type_ {
        "square" => "rect",
        "round" => "rounded",
        "circle" => "circle",
        "doublecircle" => "dbl-circ",
        "diamond" => "diam",
        "hexagon" => "hex",
        "stadium" => "stadium",
        "subroutine" => "fr-rect",
        "cylinder" => "cyl",
        "rect_left_inv_arrow" => "odd",
        "lean_right" => "lean-r",
        "lean_left" => "lean-l",
        "trapezoid" => "trap-b",
        "inv_trapezoid" => "trap-t",
        _ => "rect",
    }
}

// Shapes that keep their own size in a larger cell, where the others stretch to fill it.
fn fixed(shape: &str) -> bool {
    matches!(shape, "circle" | "dbl-circ" | "diam")
}

// Shapes that only widen, because their slants and curves are set by their height.
fn wide(shape: &str) -> bool {
    matches!(shape, "stadium" | "hex" | "odd" | "lean-r" | "lean-l" | "trap-b" | "trap-t")
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Leaf,
    Arrow,
    Composite,
    Space,
}

struct View {
    block: usize,
    kind: Kind,
    shape: &'static str,
    label: Label,
    style: ResolvedStyle,
    mask: u32,
    dx: f64,
    dy: f64,
    bw: f64,
    bh: f64,
}

// What the geometry helpers read of a view.
#[derive(Clone, Copy)]
struct Vb {
    i: usize,
    x: f64,
    y: f64,
    bw: f64,
    bh: f64,
    kind: Kind,
    shape: &'static str,
    mask: u32,
}

// An obstacle: a view, or a container's title (no index).
#[derive(Clone, Copy)]
struct Ob {
    i: Option<usize>,
    x: f64,
    y: f64,
    bw: f64,
    bh: f64,
}

struct Route {
    edge: usize,
    // Two points for a straight line, three for a bend, none for a self-loop.
    points: Vec<f64>,
    loop_: usize,
    label: Label,
    lx: f64,
    ly: f64,
}

fn is_round(v: &Vb) -> bool {
    v.kind == Kind::Leaf && (v.shape == "circle" || v.shape == "dbl-circ")
}

// Where a vertical line at x meets the outline, below the center (sign 1) or above it (sign -1).
fn edge_y(v: &Vb, x: f64, sign: f64) -> f64 {
    let hw = v.bw / 2.0;
    let hh = v.bh / 2.0;
    let dx = (x - v.x).abs();
    let mut reach = hh;
    if is_round(v) {
        reach = (hw * hw - dx * dx).max(0.0).sqrt();
    } else if v.kind == Kind::Leaf && v.shape == "diam" {
        reach = hh * (1.0 - dx / hw).max(0.0);
    } else if v.kind == Kind::Leaf {
        reach = hh - shape_inset(v.shape, v.bw, v.bh, if sign > 0.0 { 2 } else { 0 }, 0.0);
    } else if v.kind == Kind::Arrow && arrow_axis(v.mask) == 'x' {
        reach = hh - OVERHANG;
    }
    v.y + sign * reach
}

// Where a horizontal line at y meets the outline, right of the center (sign 1) or left of it (sign -1).
fn edge_x(v: &Vb, y: f64, sign: f64) -> f64 {
    let hw = v.bw / 2.0;
    let hh = v.bh / 2.0;
    let dy = (y - v.y).abs();
    let mut reach = hw;
    if is_round(v) {
        reach = (hh * hh - dy * dy).max(0.0).sqrt();
    } else if v.kind == Kind::Leaf && v.shape == "diam" {
        reach = hw * (1.0 - dy / hh).max(0.0);
    } else if v.kind == Kind::Leaf {
        reach = hw - shape_inset(v.shape, v.bw, v.bh, if sign > 0.0 { 1 } else { 3 }, 0.0);
    } else if v.kind == Kind::Arrow && arrow_axis(v.mask) == 'y' {
        reach = hw - OVERHANG;
    }
    v.x + sign * reach
}

// Where the line from the center toward (tx, ty) leaves the outline.
fn border(v: &Vb, tx: f64, ty: f64, out: &mut Vec<f64>) {
    let dx = tx - v.x;
    let dy = ty - v.y;
    let hw = v.bw / 2.0;
    let hh = v.bh / 2.0;
    let mut k = if is_round(v) {
        hw / js::or(hypot(dx, dy), || 1.0)
    } else if v.kind == Kind::Leaf && v.shape == "diam" {
        1.0 / js::or(dx.abs() / hw + dy.abs() / hh, || 1.0)
    } else {
        let a = if dx == 0.0 { f64::INFINITY } else { hw / dx.abs() };
        let b = if dy == 0.0 { f64::INFINITY } else { hh / dy.abs() };
        if a.is_nan() || b.is_nan() {
            f64::NAN
        } else {
            a.min(b)
        }
    };
    if k == f64::INFINITY {
        k = 0.0;
    }
    out.push(v.x + dx * k);
    out.push(v.y + dy * k);
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

fn crosses_box(x1: f64, y1: f64, x2: f64, y2: f64, v: &Ob) -> bool {
    let l = v.x - v.bw / 2.0 + 1.0;
    let r = v.x + v.bw / 2.0 - 1.0;
    let t = v.y - v.bh / 2.0 + 1.0;
    let b = v.y + v.bh / 2.0 - 1.0;
    if (x1 < l && x2 < l) || (x1 > r && x2 > r) || (y1 < t && y2 < t) || (y1 > b && y2 > b) {
        return false;
    }
    if x1 == x2 || y1 == y2 {
        return true;
    }
    // The segment is diagonal and its bounding box overlaps: the box is hit unless all four corners are on one side.
    let dx = x2 - x1;
    let dy = y2 - y1;
    let side = |x: f64, y: f64| sign(dx * (y - y1) - dy * (x - x1));
    let first = side(l, t);
    first == 0.0 || side(r, t) != first || side(r, b) != first || side(l, b) != first
}

fn hits(points: &[f64], s: usize, t: usize, obstacles: &[Ob]) -> usize {
    let mut count = 0;
    for v in obstacles {
        if v.i == Some(s) || v.i == Some(t) {
            continue;
        }
        let mut i = 0;
        while i + 3 < points.len() {
            if crosses_box(points[i], points[i + 1], points[i + 2], points[i + 3], v) {
                count += 1;
                break;
            }
            i += 2;
        }
    }
    count
}

fn js_min(values: &[f64]) -> f64 {
    values.iter().fold(f64::INFINITY, |a, &b| if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) })
}

// A straight line where the two blocks share a row or a column, otherwise one bend between facing sides.
fn route_edge(s: &Vb, t: &Vb, shift: f64, obstacles: Option<&[Ob]>) -> Vec<f64> {
    let (sl, sr, st_, sb) = (s.x - s.bw / 2.0, s.x + s.bw / 2.0, s.y - s.bh / 2.0, s.y + s.bh / 2.0);
    let (tl, tr, tt, tb) = (t.x - t.bw / 2.0, t.x + t.bw / 2.0, t.y - t.bh / 2.0, t.y + t.bh / 2.0);
    let x1 = sl.max(tl);
    let x2 = sr.min(tr);
    let y1 = st_.max(tt);
    let y2 = sb.min(tb);
    let over_x = x2 - x1 > 6.0;
    let over_y = y2 - y1 > 6.0;

    if over_x && over_y {
        // One block is inside the other: join the inner one to the nearest side of the outer one.
        let inner_is_s = s.bw * s.bh <= t.bw * t.bh;
        let (inner, outer) = if inner_is_s { (s, t) } else { (t, s) };
        let up = inner.y - inner.bh / 2.0 - (outer.y - outer.bh / 2.0);
        let down = outer.y + outer.bh / 2.0 - (inner.y + inner.bh / 2.0);
        let left = inner.x - inner.bw / 2.0 - (outer.x - outer.bw / 2.0);
        let right = outer.x + outer.bw / 2.0 - (inner.x + inner.bw / 2.0);
        let least = js_min(&[up, down, left, right]);
        let points = if least == down {
            vec![inner.x, edge_y(inner, inner.x, 1.0), inner.x, outer.y + outer.bh / 2.0]
        } else if least == up {
            vec![inner.x, edge_y(inner, inner.x, -1.0), inner.x, outer.y - outer.bh / 2.0]
        } else if least == right {
            vec![edge_x(inner, inner.y, 1.0), inner.y, outer.x + outer.bw / 2.0, inner.y]
        } else {
            vec![edge_x(inner, inner.y, -1.0), inner.y, outer.x - outer.bw / 2.0, inner.y]
        };
        return if inner_is_s { points } else { vec![points[2], points[3], points[0], points[1]] };
    }
    if over_x {
        let margin = 6f64.min((x2 - x1) / 2.0);
        let x = ((x1 + x2) / 2.0 + shift).max(x1 + margin).min(x2 - margin);
        let down = if t.y > s.y { 1.0 } else { -1.0 };
        return vec![x, edge_y(s, x, down), x, edge_y(t, x, -down)];
    }
    if over_y {
        let margin = 6f64.min((y2 - y1) / 2.0);
        let y = ((y1 + y2) / 2.0 + shift).max(y1 + margin).min(y2 - margin);
        let right = if t.x > s.x { 1.0 } else { -1.0 };
        return vec![edge_x(s, y, right), y, edge_x(t, y, -right), y];
    }

    let right = if t.x > s.x { 1.0 } else { -1.0 };
    let down = if t.y > s.y { 1.0 } else { -1.0 };
    let across = vec![edge_x(s, s.y, right), s.y, t.x, s.y, t.x, edge_y(t, t.x, -down)];
    let along = vec![s.x, edge_y(s, s.x, down), s.x, t.y, edge_x(t, t.y, -right), t.y];
    let wide = (t.x - s.x).abs() >= (t.y - s.y).abs();
    let (mut best, other) = if wide { (across, along) } else { (along, across) };
    let Some(obstacles) = obstacles else { return best };
    let mut least = hits(&best, s.i, t.i, obstacles);
    if least == 0 {
        return best;
    }
    let count = hits(&other, s.i, t.i, obstacles);
    if count < least {
        best = other;
        least = count;
    }
    if least == 0 {
        return best;
    }
    let mut direct = Vec::new();
    border(s, t.x, t.y, &mut direct);
    border(t, s.x, s.y, &mut direct);
    if hits(&direct, s.i, t.i, obstacles) < least {
        direct
    } else {
        best
    }
}

// Builds the path through two or three points, rounding the bend and leaving room for markers.
fn trace(p: &[f64], mut start_trim: f64, mut end_trim: f64) -> EdgePath {
    let n = p.len();
    let (sx, sy, ex, ey) = (p[0], p[1], p[n - 2], p[n - 1]);
    let mut ax = p[2] - sx;
    let mut ay = p[3] - sy;
    let first = hypot(ax, ay);
    ax /= js::or(first, || 1.0);
    ay /= js::or(first, || 1.0);
    let mut bx = ex - p[n - 4];
    let mut by = ey - p[n - 3];
    let last = hypot(bx, by);
    bx /= js::or(last, || 1.0);
    by /= js::or(last, || 1.0);
    if n == 4 && start_trim + end_trim > first {
        let scale = first / (start_trim + end_trim);
        start_trim *= scale;
        end_trim *= scale;
    }
    start_trim = start_trim.min(first);
    end_trim = end_trim.min(last);
    let mut d = format!("M{},{}", num(sx + ax * start_trim), num(sy + ay * start_trim));
    if n == 6 {
        let r = 0f64.max(CORNER.min(first - start_trim).min(last - end_trim));
        d.push_str(&format!("L{},{}Q{},{} {},{}", num(p[2] - ax * r), num(p[3] - ay * r), num(p[2]), num(p[3]), num(p[2] + bx * r), num(p[3] + by * r)));
    }
    d.push_str(&format!("L{},{}", num(ex - bx * end_trim), num(ey - by * end_trim)));
    EdgePath { d, tokens: None, sx, sy, sdx: -ax, sdy: -ay, ex, ey, edx: bx, edy: by }
}

// A self-loop is an arc around the top right corner of the block, or around the matching point
// on a round or diamond outline: its center and radius and the angles where it leaves and returns.
fn loop_arc(v: &Vb, k: usize) -> (f64, f64, f64, f64, f64) {
    let hw = v.bw / 2.0;
    let hh = v.bh / 2.0;
    let r = 1f64.max((10.0 + 5.0 * (k as f64 - 1.0)).min(hw).min(hh));
    if is_round(v) {
        let px = v.x + hw * FRAC_1_SQRT_2;
        let py = v.y - hw * FRAC_1_SQRT_2;
        let half = 2.0 * (1f64.min(r / (2.0 * hw))).asin();
        let angle = |at: f64| (v.y - hw * at.sin() - py).atan2(v.x + hw * at.cos() - px);
        return (px, py, r, angle(PI / 4.0 + half), angle(PI / 4.0 - half));
    }
    if v.kind == Kind::Leaf && v.shape == "diam" {
        let slope = hh.atan2(hw);
        return (v.x + hw / 2.0, v.y - hh / 2.0, r, slope + PI, slope);
    }
    (v.x + hw, v.y - hh, r, PI, PI / 2.0)
}

fn loop_path(v: &Vb, k: usize, start_trim: f64, end_trim: f64) -> EdgePath {
    let (px, py, r, a1, a2) = loop_arc(v, k);
    let from = a1 + start_trim / r;
    let to = a2 - end_trim / r;
    let mut sweep = (to - from) % (2.0 * PI);
    if sweep <= 0.0 {
        sweep += 2.0 * PI;
    }
    let (x1, y1) = (px + r * from.cos(), py + r * from.sin());
    let (x2, y2) = (px + r * to.cos(), py + r * to.sin());
    let (sx, sy) = (px + r * a1.cos(), py + r * a1.sin());
    let (ex, ey) = (px + r * a2.cos(), py + r * a2.sin());
    // Each marker lies along the chord it covers, so it follows the curve instead of the tangent at its tip.
    let sl = hypot(sx - x1, sy - y1);
    let el = hypot(ex - x2, ey - y2);
    EdgePath {
        d: format!("M{},{}A{},{} 0 {} 1 {},{}", num(x1), num(y1), num(r), num(r), if sweep > PI { 1 } else { 0 }, num(x2), num(y2)),
        tokens: None,
        sx,
        sy,
        sdx: if sl > 0.0 { (sx - x1) / sl } else { a1.sin() },
        sdy: if sl > 0.0 { (sy - y1) / sl } else { -a1.cos() },
        ex,
        ey,
        edx: if el > 0.0 { (ex - x2) / el } else { -a2.sin() },
        edy: if el > 0.0 { (ey - y2) / el } else { a2.cos() },
    }
}

fn covers(x: f64, y: f64, w: f64, h: f64, s: usize, t: usize, obstacles: Option<&[Ob]>) -> bool {
    let Some(obstacles) = obstacles else { return false };
    obstacles.iter().any(|v| v.i != Some(s) && v.i != Some(t) && (x - v.x).abs() < (w + v.bw) / 2.0 && (y - v.y).abs() < (h + v.bh) / 2.0)
}

// Picks the point on a leg where a label of the given size covers no block other than the two it joins.
fn label_spot(p: &[f64], at: usize, w: f64, h: f64, s: usize, t: usize, obstacles: Option<&[Ob]>) -> f64 {
    if obstacles.is_none() {
        return 0.5;
    }
    for spot in SPOTS {
        let x = p[at] + (p[at + 2] - p[at]) * spot;
        let y = p[at + 1] + (p[at + 3] - p[at + 1]) * spot;
        if !covers(x, y, w, h, s, t, obstacles) {
            return spot;
        }
    }
    0.5
}

// Stacks overlapping labels along their shared line, if they fit between its markers and cover
// no other block or title. Returns the extra length needed to fit the labels.
fn stack_labels(stack: &[usize], routes: &mut [Route], legs: &HashMap<usize, Vec<f64>>, s: usize, t: usize, obstacles: Option<&[Ob]>) -> f64 {
    let vertical = legs[&stack[0]][0] == legs[&stack[0]][2];
    let mut lo = f64::NEG_INFINITY;
    let mut hi = f64::INFINITY;
    let mut need = -LABEL_GAP;
    let mut crowded = false;
    for a in 0..stack.len() {
        let leg = &legs[&stack[a]];
        if if vertical { leg[0] != leg[2] } else { leg[1] != leg[3] } {
            return 0.0;
        }
        lo = lo.max(if vertical { leg[1].min(leg[3]) } else { leg[0].min(leg[2]) });
        hi = hi.min(if vertical { leg[1].max(leg[3]) } else { leg[0].max(leg[2]) });
        let p = &routes[stack[a]];
        need += (if vertical { p.label.height } else { p.label.width + 8.0 }) + LABEL_GAP;
        for &b in &stack[..a] {
            let q = &routes[b];
            if (p.lx - q.lx).abs() < (p.label.width + q.label.width) / 2.0 + 8.0 && (p.ly - q.ly).abs() < (p.label.height + q.label.height) / 2.0 {
                crowded = true;
            }
        }
    }
    if !crowded {
        return 0.0;
    }
    if need > hi - lo - 2.0 * ARROW {
        return need - (hi - lo - 2.0 * ARROW);
    }
    let mut along = Vec::new();
    let mut next = (lo + hi - need) / 2.0;
    for &i in stack {
        let r = &routes[i];
        let size = if vertical { r.label.height } else { r.label.width + 8.0 };
        let at = next + size / 2.0;
        if covers(if vertical { r.lx } else { at }, if vertical { at } else { r.ly }, r.label.width + 8.0, r.label.height, s, t, obstacles) {
            return 0.0;
        }
        along.push(at);
        next += size + LABEL_GAP;
    }
    for (k, &i) in stack.iter().enumerate() {
        if vertical {
            routes[i].ly = along[k];
        } else {
            routes[i].lx = along[k];
        }
    }
    0.0
}

// Widens the gap in the blocks' nearest shared container. `base` keeps the gaps as they were
// when the labels were measured.
fn widen(parents: &[Option<usize>], cells: &mut [Cell], s: usize, t: usize, vertical: bool, short: f64, base: &mut HashMap<(usize, bool), Vec<f64>>) -> bool {
    let mut chain = std::collections::HashSet::new();
    let mut v = Some(s);
    while let Some(x) = v {
        chain.insert(x);
        v = parents[x];
    }
    let mut tb: Option<usize> = None;
    let mut bx = Some(t);
    while let Some(x) = bx {
        if chain.contains(&x) {
            break;
        }
        tb = Some(x);
        bx = parents[x];
    }
    let (Some(b), Some(tb)) = (bx, tb) else { return false };
    if b == s {
        return false;
    }
    let mut sb = s;
    while parents[sb] != Some(b) {
        sb = parents[sb].unwrap();
    }
    let at;
    if vertical {
        if cells[sb].row == cells[tb].row {
            return false;
        }
        at = cells[sb].row.max(cells[tb].row) - 1.0;
        cells[b].row_gaps.get_or_insert_with(Vec::new);
    } else {
        let (l, r) = if cells[sb].col < cells[tb].col { (sb, tb) } else { (tb, sb) };
        if cells[r].col < cells[l].col + cells[l].span {
            return false;
        }
        at = cells[r].col - 1.0;
        cells[b].col_gaps.get_or_insert_with(Vec::new);
    }
    let gaps = if vertical { cells[b].row_gaps.as_mut().unwrap() } else { cells[b].col_gaps.as_mut().unwrap() };
    let original = base.entry((b, vertical)).or_insert_with(|| gaps.clone());
    let at = at as usize;
    let was = original.get(at).copied().unwrap_or(0.0);
    if gaps.len() <= at {
        gaps.resize(at + 1, 0.0);
    }
    gaps[at] = gaps[at].max(was + short);
    true
}

pub fn render_block(model: &BlockDb, config: &Config, options: &RenderOptions) -> Result<Rendered, PeleError> {
    let mut result = None;
    let rendered = tighten(options, |_tight| match draw(model, config, options) {
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

fn draw(model: &BlockDb, config: &Config, options: &RenderOptions) -> Result<Rendered, PeleError> {
    let size = 16.0;
    let edge_size = js::round(size * 0.875);
    let pad = 8.0;
    let gap = match config.get("block").and_then(|v| v.as_map()).and_then(|m| m.get("padding")) {
        Some(Value::Num(c)) if *c >= 0.0 => c.min(200.0),
        _ => GAP,
    };
    let wrap = WRAP;

    let style_of = |block: &Block| -> ResolvedStyle {
        let mut declarations: Vec<String> = Vec::new();
        // A style list is split on commas only, so one entry can still hold several declarations.
        let mut add = |styles: &[String]| {
            for style in styles {
                for part in style.split(';') {
                    declarations.push(part.to_string());
                }
            }
        };
        let default = vec!["default".to_string()];
        let names = match &block.classes {
            Some(c) if !c.is_empty() => c,
            _ => &default,
        };
        for name in names {
            if let Some(def) = model.class(name) {
                add(&def.styles);
            }
        }
        if let Some(styles) = &block.styles {
            add(styles);
        }
        resolve_style(&declarations)
    };

    let view = |b: usize| -> (View, Cell) {
        let block = &model.arena[b];
        let type_ = block.type_.as_deref().unwrap_or("");
        let style = if type_ == "space" { resolve_style(&[]) } else { style_of(block) };
        let text_style = (if style.bold { st::BOLD } else { 0 }) | (if style.italic { st::ITALIC } else { 0 });
        let font_size = style.font_size.unwrap_or(size);
        let mut kind = Kind::Leaf;
        let mut shape = "";
        let label: Label;
        let (mut nw, mut nh, mut head, mut mask, mut dx, mut dy) = (0.0, 0.0, 0.0, 0, 0.0, 0.0);
        if type_ == "space" {
            kind = Kind::Space;
            label = layout_label(None, false, font_size, wrap, 0);
        } else if type_ == "composite" {
            kind = Kind::Composite;
            label = layout_label(block.label.as_deref(), false, font_size, 4000.0, text_style);
            head = if label.height > 0.0 { label.height + 4.0 } else { 0.0 };
            nw = label.width + 2.0 * gap;
            nh = head + 2.0 * gap;
        } else if type_ == "block_arrow" {
            kind = Kind::Arrow;
            label = layout_label(block.label.as_deref(), false, font_size, wrap, text_style);
            let blank = label.lines.iter().all(|line| line.iter().all(|part| js::trim(&part.text).is_empty()));
            mask = arrow_mask(block.directions.as_deref());
            let s = if blank { arrow_size(mask, 0.0, 0.0) } else { arrow_size(mask, label.width, label.height) };
            nw = s.w;
            nh = s.h;
            dx = s.dx;
            dy = s.dy;
        } else {
            shape = shape_of(type_);
            label = layout_label(block.label.as_deref(), false, font_size, wrap, text_style);
            let s = shape_size(shape, label.width, label.height);
            nw = s.w;
            // A cylinder's caps deepen as it widens, so it is sized for the deepest ones.
            nh = if shape == "cyl" { s.h.max(label.height + 40.0) } else { s.h };
            dy = s.dy;
        }
        let cell = Cell {
            columns: if kind == Kind::Composite { block.columns.unwrap_or(-1.0) } else { -1.0 },
            span: if kind == Kind::Space { 1.0 } else { block.width_in_columns.unwrap_or(1.0) },
            space: kind == Kind::Space,
            head,
            nw,
            nh,
            ..Default::default()
        };
        (View { block: b, kind, shape, label, style, mask, dx, dy, bw: 0.0, bh: 0.0 }, cell)
    };

    let (root_view, mut root_cell) = view(0);
    root_cell.nw = 0.0;
    root_cell.nh = 0.0;
    let mut views = vec![root_view];
    let mut cells = vec![root_cell];
    let mut parents: Vec<Option<usize>> = vec![None];
    let mut by_id: HashMap<&str, usize> = HashMap::new();
    let mut i = 0;
    while i < views.len() {
        if views[i].kind == Kind::Composite {
            let kids = model.arena[views[i].block].children.clone();
            for child in kids {
                let (v, c) = view(child);
                let index = views.len();
                cells[i].children.push(index);
                parents.push(Some(i));
                if v.kind != Kind::Space {
                    by_id.insert(model.arena[child].id.as_deref().unwrap_or(""), index);
                }
                views.push(v);
                cells.push(c);
            }
        }
        i += 1;
    }
    let n = views.len();
    let vb = |views: &[View], cells: &[Cell], i: usize| Vb { i, x: cells[i].x, y: cells[i].y, bw: views[i].bw, bh: views[i].bh, kind: views[i].kind, shape: views[i].shape, mask: views[i].mask };

    let mut obstacles: Vec<Ob> = Vec::new();
    let mut titles: Vec<Ob> = Vec::new();
    // Layout keeps the widest a container has been, so each run starts from the widths it needs.
    let widths: Vec<f64> = cells.iter().map(|c| c.nw).collect();
    let place = |views: &mut [View], cells: &mut [Cell], obstacles: &mut Vec<Ob>, titles: &mut Vec<Ob>| -> Result<(f64, f64), PeleError> {
        for (k, c) in cells.iter_mut().enumerate() {
            c.nw = widths[k];
        }
        let content = layout(cells, gap)?;
        obstacles.clear();
        titles.clear();
        for k in 0..views.len() {
            let v = &mut views[k];
            let c = &cells[k];
            if v.kind == Kind::Leaf && fixed(v.shape) {
                v.bw = c.nw;
                v.bh = c.nh;
            } else if v.kind == Kind::Arrow {
                v.bw = if c.span > 1.0 { c.w } else { c.nw };
                v.bh = c.nh;
            } else {
                v.bw = c.w;
                v.bh = if v.kind == Kind::Leaf && wide(v.shape) { c.nh } else { c.h };
            }
            if v.kind == Kind::Leaf && v.shape == "cyl" {
                v.dy = 12f64.min(6f64.max(v.bw / 14.0)) / 2.0;
            }
            if v.kind == Kind::Leaf || v.kind == Kind::Arrow {
                obstacles.push(Ob { i: Some(k), x: c.x, y: c.y, bw: v.bw, bh: v.bh });
            } else if c.head > 0.0 {
                titles.push(Ob { i: None, x: c.x, y: c.y - c.h / 2.0 + (c.head + gap) / 2.0 + 2.0, bw: v.label.width, bh: v.label.height });
            }
        }
        Ok(content)
    };
    let mut content = place(&mut views, &mut cells, &mut obstacles, &mut titles)?;

    // Edges between the same two blocks are spread apart instead of drawn on top of each other.
    let mut pairs: HashMap<usize, usize> = HashMap::new();
    let mut labelled = std::collections::HashSet::new();
    let mut ends: Vec<(usize, usize)> = Vec::new();
    let mut drawn: Vec<usize> = Vec::new();
    for (k, edge) in model.edges.iter().enumerate() {
        let (Some(&s), Some(&t)) = (by_id.get(edge.start.as_deref().unwrap_or("")), by_id.get(edge.end.as_deref().unwrap_or(""))) else { continue };
        let key = s.min(t) * n + s.max(t);
        *pairs.entry(key).or_insert(0) += 1;
        if edge.label.as_deref().is_some_and(|l| !l.is_empty()) {
            labelled.insert(key);
        }
        ends.push((s, t));
        drawn.push(k);
    }
    let check = drawn.len() * obstacles.len() <= OBSTACLE_BUDGET;
    let mut routes: Vec<Route> = Vec::new();
    // Stacks in insertion order.
    let mut stacks: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut legs: HashMap<usize, Vec<f64>> = HashMap::new();

    let route = |views: &[View], cells: &[Cell], obstacles: &[Ob], routes: &mut Vec<Route>, stacks: &mut Vec<(usize, Vec<usize>)>, legs: &mut HashMap<usize, Vec<f64>>| {
        let mut seen: HashMap<usize, usize> = HashMap::new();
        routes.clear();
        stacks.clear();
        let obs = if check { Some(obstacles) } else { None };
        for i in 0..drawn.len() {
            let (s, t) = ends[i];
            let key = s.min(t) * n + s.max(t);
            let total = pairs.get(&key).copied().unwrap_or(1);
            let k = seen.get(&key).copied().unwrap_or(0);
            seen.insert(key, k + 1);
            let label = layout_label(model.edges[drawn[i]].label.as_deref(), false, edge_size, WRAP, 0);
            let (sv, tv) = (vb(views, cells, s), vb(views, cells, t));
            let points;
            let (lx, ly);
            if s == t {
                let (px, py, r, _, _) = loop_arc(&sv, k + 1);
                points = Vec::new();
                lx = px + r + 6.0 + label.width / 2.0;
                ly = py - r / 2.0;
            } else {
                let step = if labelled.contains(&key) { js::round(edge_size * 1.5) + 2.0 } else { 14.0 };
                points = route_edge(&sv, &tv, (k as f64 - (total as f64 - 1.0) / 2.0) * step, obs);
                let pn = points.len();
                // The label goes on the longer leg.
                let first_leg = (points[2] - points[0]).abs() + (points[3] - points[1]).abs();
                let last_leg = (points[pn - 2] - points[pn - 4]).abs() + (points[pn - 1] - points[pn - 3]).abs();
                let at = if pn == 6 && last_leg > first_leg { 2 } else { 0 };
                let spot = if label.width > 0.0 { label_spot(&points, at, label.width + 8.0, label.height, s, t, obs) } else { 0.5 };
                lx = points[at] + (points[at + 2] - points[at]) * spot;
                ly = points[at + 1] + (points[at + 3] - points[at + 1]) * spot;
                if label.width > 0.0 && total > 1 {
                    legs.insert(i, points[at..at + 4].to_vec());
                    match stacks.iter_mut().find(|(k2, _)| *k2 == key) {
                        Some(st) => st.1.push(i),
                        None => stacks.push((key, vec![i])),
                    }
                }
            }
            routes.push(Route { edge: drawn[i], points, loop_: if s == t { k + 1 } else { 0 }, label, lx, ly });
        }
    };

    let settle = |grow: bool,
                  views: &[View],
                  cells: &mut Vec<Cell>,
                  obstacles: &[Ob],
                  titles: &[Ob],
                  routes: &mut Vec<Route>,
                  stacks: &mut Vec<(usize, Vec<usize>)>,
                  legs: &mut HashMap<usize, Vec<f64>>|
     -> bool {
        route(views, cells, obstacles, routes, stacks, legs);
        let blocked: Option<Vec<Ob>> = if check { Some(obstacles.iter().chain(titles.iter()).copied().collect()) } else { None };
        let mut base: HashMap<(usize, bool), Vec<f64>> = HashMap::new();
        let mut grown = false;
        for (_, stack) in stacks.iter() {
            if stack.len() < 2 {
                continue;
            }
            let (s, t) = ends[stack[0]];
            let short = stack_labels(stack, routes, legs, s, t, blocked.as_deref());
            let vertical = legs[&stack[0]][0] == legs[&stack[0]][2];
            if grow && short > 0.0 && widen(&parents, cells, s, t, vertical, short.ceil(), &mut base) {
                grown = true;
            }
        }
        grown
    };
    let overlap = |routes: &[Route]| -> f64 {
        let mut area = 0.0;
        for i in 0..routes.len() {
            let p = &routes[i];
            for q in &routes[i + 1..] {
                let pw = (p.label.width + 8.0) / 2.0;
                let qw = (q.label.width + 8.0) / 2.0;
                let w = (p.lx + pw).min(q.lx + qw) - (p.lx - pw).max(q.lx - qw);
                let h = (p.ly + p.label.height / 2.0).min(q.ly + q.label.height / 2.0) - (p.ly - p.label.height / 2.0).max(q.ly - q.label.height / 2.0);
                if p.label.width > 0.0 && q.label.width > 0.0 && w > 0.0 && h > 0.0 {
                    area += w * h;
                }
            }
        }
        area
    };
    // Finds the drawing's extent [minX, minY, maxX, maxY] and gives its width.
    let bound = |content: (f64, f64), routes: &[Route], views: &[View], cells: &[Cell], b: &mut [f64; 4]| -> f64 {
        *b = [0.0, 0.0, content.0, content.1];
        let mut grow = |x: f64, y: f64, w: f64, h: f64| {
            if x - w / 2.0 < b[0] {
                b[0] = x - w / 2.0;
            }
            if x + w / 2.0 > b[2] {
                b[2] = x + w / 2.0;
            }
            if y - h / 2.0 < b[1] {
                b[1] = y - h / 2.0;
            }
            if y + h / 2.0 > b[3] {
                b[3] = y + h / 2.0;
            }
        };
        for (i, route) in routes.iter().enumerate() {
            if route.loop_ > 0 {
                let (px, py, r, _, _) = loop_arc(&vb(views, cells, ends[i].0), route.loop_);
                grow(px, py, 2.0 * r + 2.0, 2.0 * r + 2.0);
            }
            if route.label.width > 0.0 {
                grow(route.lx, route.ly, route.label.width + 8.0, route.label.height);
            }
        }
        b[2] - b[0]
    };
    let mut bb = [0.0; 4];
    if settle(drawn.len() * drawn.len() <= OBSTACLE_BUDGET, &views, &mut cells, &obstacles, &titles, &mut routes, &mut stacks, &mut legs) {
        let before = overlap(&routes);
        let _narrow = bound(content, &routes, &views, &cells, &mut bb);
        // A container stretched by its row gives some of a new gap back, so the gap may take a few rounds.
        let mut grown = true;
        let mut round = 0;
        while round < 3 && grown {
            content = place(&mut views, &mut cells, &mut obstacles, &mut titles)?;
            grown = settle(round < 2, &views, &mut cells, &obstacles, &titles, &mut routes, &mut stacks, &mut legs);
            round += 1;
        }
        // The drawing only grows if that leaves less of the labels covering each other.
        let _wide = bound(content, &routes, &views, &cells, &mut bb);
        if overlap(&routes) > before - 1.0 {
            for c in cells.iter_mut() {
                c.row_gaps = None;
                c.col_gaps = None;
            }
            content = place(&mut views, &mut cells, &mut obstacles, &mut titles)?;
            settle(false, &views, &mut cells, &obstacles, &titles, &mut routes, &mut stacks, &mut legs);
        }
    }
    bound(content, &routes, &views, &cells, &mut bb);
    let [min_x, min_y, max_x, max_y] = bb;

    let title = title_label(model.title.as_deref(), size, 4000.0);
    let title_height = if title.height > 0.0 { title.height + 12.0 } else { 0.0 };
    let inner = (max_x - min_x).max(title.width);
    let ox = pad - min_x + (inner - (max_x - min_x)) / 2.0;
    let oy = pad + title_height - min_y;
    for c in cells.iter_mut() {
        c.x += ox;
        c.y += oy;
    }
    let width = (inner + 2.0 * pad).ceil();
    let height = (max_y - min_y + title_height + 2.0 * pad).ceil();

    let mut clusters = String::new();
    let mut nodes = String::new();
    let mut fills: Vec<Fill> = Vec::new();
    for i in 1..n {
        let v = &views[i];
        let c = &cells[i];
        if v.kind == Kind::Space {
            continue;
        }
        let block = &model.arena[v.block];
        let classes = class_names(&block.classes.as_ref().map(|c| c.join(" ")).unwrap_or_default());
        let id = esc_text(block.id.as_deref().unwrap_or(""));
        if v.kind == Kind::Composite {
            let top = c.y - c.h / 2.0;
            if paints(&v.style.shape) {
                fills.push(Fill { bx: [c.x - c.w / 2.0, top, c.x + c.w / 2.0, top + c.h], shape: v.style.shape.clone() });
            }
            clusters.push_str(&format!(
                "<g class=\"pele-cluster{}\" data-id=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_a)\"{} stroke=\"var(--_b)\"{}/>{}</g>",
                classes,
                id,
                num(c.x - c.w / 2.0),
                num(top),
                num(c.w),
                num(c.h),
                if sets_fill(&v.style.shape) { "" } else { " fill-opacity=\"0.5\"" },
                rounded(&v.style.shape),
                label_svg(&v.label, c.x, top + (c.head + gap) / 2.0 + 2.0, &format!(" class=\"pele-cluster-label\"{}{}", ink_fill(&v.style.shape, &v.style.text, Some("var(--_m)")), v.style.text))
            ));
            continue;
        }
        let arrow = v.kind == Kind::Arrow;
        nodes.push_str(&format!(
            "<g class=\"pele-node pele-shape-{}{}\" data-id=\"{}\" transform=\"translate({},{})\">{}{}</g>",
            if arrow { "block-arrow" } else { v.shape },
            classes,
            id,
            num(c.x),
            num(c.y),
            if arrow {
                draw_arrow(v.mask, v.bw, v.bh, &format!("{}{}", SHAPE_ATTRS, v.style.shape))
            } else {
                draw_shape(v.shape, v.bw, v.bh, &format!("{}{}", SHAPE_ATTRS, v.style.shape), &format!("{}{}", LINE_ATTRS, v.style.line))
            },
            label_svg(
                &v.label,
                v.dx,
                v.dy,
                &format!(" class=\"pele-label\"{}{}", if arrow || shape_filled(v.shape) { ink_fill(&v.style.shape, &v.style.text, None) } else { String::new() }, v.style.text)
            )
        ));
    }

    let mut casings = String::new();
    let mut edges = String::new();
    let mut labels = String::new();
    for i in 0..routes.len() {
        let edge = &model.edges[routes[i].edge];
        let start_type = edge.arrow_type_start.as_deref().unwrap_or("");
        let end_type = edge.arrow_type_end.as_deref().unwrap_or("");
        let thick = edge.thickness.as_deref() == Some("thick");
        let path = if routes[i].loop_ > 0 {
            loop_path(&vb(&views, &cells, ends[i].0), routes[i].loop_, marker_trim(start_type), marker_trim(end_type))
        } else {
            let points = &mut routes[i].points;
            let mut k = 0;
            while k < points.len() {
                points[k] += ox;
                points[k + 1] += oy;
                k += 2;
            }
            trace(points, marker_trim(start_type), marker_trim(end_type))
        };
        let over = over_fills(&path.d, &[path.sx, path.sy, path.ex, path.ey], &fills, "", None);
        if over.as_deref() == Some("") {
            casings.push_str(&edge_casing(&path, start_type, end_type, "", thick));
        }
        let ink = over.as_deref().filter(|o| !o.is_empty());
        let id = esc_text(edge.id.as_deref().unwrap_or(""));
        edges.push_str(&format!(
            "<g class=\"pele-edge\" data-id=\"{}\"{}><path d=\"{}\"{}{}/>{}{}</g>",
            id,
            ink.map_or(String::new(), |o| format!(" stroke=\"{}\"", o)),
            path.d,
            if thick { " stroke-width=\"2.5\"" } else { "" },
            if edge.pattern.as_deref() == Some("dotted") { " stroke-dasharray=\"3 4\"" } else { "" },
            marker(start_type, path.sx, path.sy, path.sdx, path.sdy, ink.unwrap_or("var(--_l)"), "none"),
            marker(end_type, path.ex, path.ey, path.edx, path.edy, ink.unwrap_or("var(--_l)"), "none")
        ));
        if routes[i].label.width > 0.0 {
            labels.push_str(&edge_label_svg(edge.id.as_deref().unwrap_or(""), &routes[i].label, routes[i].lx + ox, routes[i].ly + oy, ""));
        }
    }

    let mut content_svg = label_svg(&title, width / 2.0, pad + title.height / 2.0, " class=\"pele-title\" font-weight=\"var(--_tw)\"");
    if !clusters.is_empty() {
        content_svg.push_str(&format!("<g class=\"pele-clusters\">{}</g>", clusters));
    }
    content_svg.push_str(&casings_svg(&casings));
    if !edges.is_empty() {
        content_svg.push_str(&format!("<g class=\"pele-edges\" fill=\"none\" stroke=\"var(--_l)\" stroke-linecap=\"round\">{}</g>", edges));
    }
    content_svg.push_str(&format!("<g class=\"pele-nodes\">{}</g>", nodes));
    if !labels.is_empty() {
        content_svg.push_str(&format!("<g class=\"pele-edge-labels\" font-size=\"{}\">{}</g>", edge_size, labels));
    }
    Ok(Rendered { svg: svg_document("block", width, height, size, options.responsive, &Accessible::default(), &content_svg), width, height, links: Vec::new() })
}
