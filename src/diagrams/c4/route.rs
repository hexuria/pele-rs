// Port of src/diagrams/c4/route.ts. Boxes are indexes into one list of laid out shapes and
// boundaries; upstream compares them by identity.

use crate::js::hypot;
use crate::svg::builder::num;
use std::collections::HashMap;

/// A laid out shape or boundary: top-left corner, size, and the shared shape it is drawn as.
#[derive(Clone, Debug, Default)]
pub struct BoxR {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub form: &'static str,
}

pub struct Route {
    pub d: String,
    pub sx: f64,
    pub sy: f64,
    pub sdx: f64,
    pub sdy: f64,
    pub ex: f64,
    pub ey: f64,
    pub edx: f64,
    pub edy: f64,
    // The stretch the label may sit on.
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
    // The side of that stretch the label is pushed to; zero centers it on the stretch.
    pub nx: f64,
    pub ny: f64,
    // When set, the label stays over the line and moves that way by at most this much.
    pub lean: f64,
    // The widest the label should be.
    pub room: f64,
    pub points: Vec<f64>,
}

const LOOP: f64 = 28.0;
pub const LANE: f64 = 20.0;
const LANE_STEP: f64 = 7.0;
const CORNER: f64 = 8.0;
const LEAN: f64 = 30.0;
pub const NARROW: f64 = 88.0;
const WIDE: f64 = 160.0;
pub const LABEL_PAD: f64 = 8.0;
pub const HEAD: f64 = 26.0;

pub fn cyl_ry(w: f64) -> f64 {
    12f64.min(6f64.max(w / 14.0))
}

pub fn queue_rx(h: f64) -> f64 {
    12f64.min(h / 4.0)
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

fn jmin(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}

fn jmax(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

// How far along the segment from (px, py) by (dx, dy) it first meets the rectangle; Infinity when it misses.
#[allow(clippy::too_many_arguments)]
fn hit_rect(x0: f64, y0: f64, x1: f64, y1: f64, px: f64, py: f64, dx: f64, dy: f64) -> f64 {
    let mut enter = 0.0;
    let mut leave = 1.0;
    if dx == 0.0 {
        if px < x0 || px > x1 {
            return f64::INFINITY;
        }
    } else {
        let a = (x0 - px) / dx;
        let b = (x1 - px) / dx;
        enter = jmax(enter, jmin(a, b));
        leave = jmin(leave, jmax(a, b));
    }
    if dy == 0.0 {
        if py < y0 || py > y1 {
            return f64::INFINITY;
        }
    } else {
        let a = (y0 - py) / dy;
        let b = (y1 - py) / dy;
        enter = jmax(enter, jmin(a, b));
        leave = jmin(leave, jmax(a, b));
    }
    if enter <= leave {
        enter
    } else {
        f64::INFINITY
    }
}

#[allow(clippy::too_many_arguments)]
fn hit_ellipse(cx: f64, cy: f64, rx: f64, ry: f64, px: f64, py: f64, dx: f64, dy: f64) -> f64 {
    let ux = dx / rx;
    let uy = dy / ry;
    let vx = (px - cx) / rx;
    let vy = (py - cy) / ry;
    let a = ux * ux + uy * uy;
    let b = 2.0 * (ux * vx + uy * vy);
    let c = vx * vx + vy * vy - 1.0;
    let disc = b * b - 4.0 * a * c;
    if a == 0.0 || disc < 0.0 {
        return f64::INFINITY;
    }
    let t = (-b - disc.sqrt()) / (2.0 * a);
    if t >= 0.0 && t <= 1.0 {
        t
    } else if c <= 0.0 {
        0.0
    } else {
        f64::INFINITY
    }
}

// Where a segment from outside first meets the outline of a shape, as a fraction of the segment.
fn hit(b: &BoxR, px: f64, py: f64, qx: f64, qy: f64) -> f64 {
    let (x, y, w, h) = (b.x, b.y, b.w, b.h);
    let dx = qx - px;
    let dy = qy - py;
    match b.form {
        "person" => jmin(hit_rect(x, y + HEAD, x + w, y + h, px, py, dx, dy), hit_ellipse(x + w / 2.0, y + 14.0, 13.0, 13.0, px, py, dx, dy)),
        "cyl" => {
            let ry = cyl_ry(w);
            jmin(
                jmin(hit_rect(x, y + ry, x + w, y + h - ry, px, py, dx, dy), hit_ellipse(x + w / 2.0, y + ry, w / 2.0, ry, px, py, dx, dy)),
                hit_ellipse(x + w / 2.0, y + h - ry, w / 2.0, ry, px, py, dx, dy),
            )
        }
        "h-cyl" => {
            let rx = queue_rx(h);
            jmin(
                jmin(hit_rect(x + rx, y, x + w - rx, y + h, px, py, dx, dy), hit_ellipse(x + rx, y + h / 2.0, rx, h / 2.0, px, py, dx, dy)),
                hit_ellipse(x + w - rx, y + h / 2.0, rx, h / 2.0, px, py, dx, dy),
            )
        }
        _ => hit_rect(x, y, x + w, y + h, px, py, dx, dy),
    }
}

pub fn crosses(points: &[f64], x0: f64, y0: f64, x1: f64, y1: f64) -> bool {
    let mut i = 2;
    while i < points.len() {
        let px = points[i - 2];
        let py = points[i - 1];
        if hit_rect(x0, y0, x1, y1, px, py, points[i] - px, points[i + 1] - py) <= 1.0 {
            return true;
        }
        i += 2;
    }
    false
}

fn contains(outer: &BoxR, inner: &BoxR) -> bool {
    outer.x <= inner.x && outer.y <= inner.y && outer.x + outer.w >= inner.x + inner.w && outer.y + outer.h >= inner.y + inner.h
}

// The straight line from a to b: x, y of its start, then of its end.
fn connect(a: &BoxR, b: &BoxR, shift: f64) -> [f64; 4] {
    let ax = a.x + a.w / 2.0;
    let ay = a.y + a.h / 2.0;
    let bx = b.x + b.w / 2.0;
    let by = b.y + b.h / 2.0;
    let down = contains(a, b);
    if down || contains(b, a) {
        // One holds the other: a short line from the nearest side of the outer box.
        let (outer, inner) = if down { (a, b) } else { (b, a) };
        let ix = inner.x + inner.w / 2.0;
        let iy = inner.y + inner.h / 2.0;
        let left = inner.x - outer.x;
        let right = outer.x + outer.w - inner.x - inner.w;
        let top = inner.y - outer.y;
        let bottom = outer.y + outer.h - inner.y - inner.h;
        let least = jmin(jmin(left, right), jmin(top, bottom));
        let ox = if least == left {
            outer.x
        } else if least == right {
            outer.x + outer.w
        } else {
            ix
        };
        let oy = if least == left || least == right {
            iy
        } else if least == bottom {
            outer.y + outer.h
        } else {
            outer.y
        };
        let t = jmin(hit(inner, ox, oy, ix, iy), 1.0);
        let mx = ox + (ix - ox) * t;
        let my = oy + (iy - oy) * t;
        return if down { [ox, oy, mx, my] } else { [mx, my, ox, oy] };
    }
    let (mut px, mut py, mut qx, mut qy) = (ax, ay, bx, by);
    // Shapes that share a column or a row are joined by an upright or level line through the shared span.
    let x0 = a.x.max(b.x);
    let x1 = (a.x + a.w).min(b.x + b.w);
    let y0 = a.y.max(b.y);
    let y1 = (a.y + a.h).min(b.y + b.h);
    if x0 < x1 {
        px = (x0 + x1) / 2.0;
        qx = px;
    } else if y0 < y1 {
        py = (y0 + y1) / 2.0;
        qy = py;
    }
    let len = crate::js::or(hypot(qx - px, qy - py), || 1.0);
    let nx = (-(qy - py) / len) * shift;
    let ny = ((qx - px) / len) * shift;
    let mut ta = hit(a, qx + nx, qy + ny, px + nx, py + ny);
    let mut tb = hit(b, px + nx, py + ny, qx + nx, qy + ny);
    if ta <= 1.0 && tb <= 1.0 {
        px += nx;
        py += ny;
        qx += nx;
        qy += ny;
    } else {
        px = ax;
        py = ay;
        qx = bx;
        qy = by;
        ta = jmin(hit(a, qx, qy, px, py), 1.0);
        tb = jmin(hit(b, px, py, qx, qy), 1.0);
    }
    [qx + (px - qx) * ta, qy + (py - qy) * ta, px + (qx - px) * tb, py + (qy - py) * tb]
}

const NEAR: f64 = 6.0;
const FRAME_NEAR: f64 = 3.0;
const PORT_STEP: f64 = 12.0;

/// What the routes drawn so far take up, so that the next keeps apart from them.
#[derive(Default)]
pub struct Lanes {
    upright: Vec<f64>,
    level: Vec<f64>,
    frames: [Vec<f64>; 2],
    taken: HashMap<usize, Vec<f64>>,
    pub squeezed: usize,
}

impl Lanes {
    pub fn free(&self, upright: bool, at: f64, a0: f64, a1: f64) -> bool {
        let lo = a0.min(a1);
        let hi = a0.max(a1);
        let near = |list: &[f64], gap: f64| {
            let mut i = 0;
            while i < list.len() {
                if (list[i] - at).abs() < gap && list[i + 1] < hi && list[i + 2] > lo {
                    return true;
                }
                i += 3;
            }
            false
        };
        !near(if upright { &self.upright } else { &self.level }, NEAR) && !near(&self.frames[if upright { 0 } else { 1 }], FRAME_NEAR)
    }

    pub fn crossings(&self, points: &[f64]) -> usize {
        let mut count = 0;
        let mut i = 2;
        while i < points.len() {
            let upright = points[i] == points[i - 2];
            let at = if upright { points[i] } else { points[i + 1] };
            let (u0, u1) = if upright { (points[i - 1], points[i + 1]) } else { (points[i - 2], points[i]) };
            let lo = u0.min(u1);
            let hi = u0.max(u1);
            let list = if upright { &self.level } else { &self.upright };
            let mut j = 0;
            while j < list.len() {
                if list[j] > lo && list[j] < hi && list[j + 1] < at && list[j + 2] > at {
                    count += 1;
                }
                j += 3;
            }
            i += 2;
        }
        count
    }

    pub fn frame(&mut self, b: &BoxR) {
        self.frames[0].extend([b.x, b.y, b.y + b.h, b.x + b.w, b.y, b.y + b.h]);
        self.frames[1].extend([b.y, b.x, b.x + b.w, b.y + b.h, b.x, b.x + b.w]);
    }

    pub fn take(&mut self, points: &[f64]) {
        let mut i = 2;
        while i < points.len() {
            if points[i] == points[i - 2] {
                self.upright.extend([points[i], points[i - 1].min(points[i + 1]), points[i - 1].max(points[i + 1])]);
            } else if points[i + 1] == points[i - 1] {
                self.level.extend([points[i + 1], points[i - 2].min(points[i]), points[i - 2].max(points[i])]);
            }
            i += 2;
        }
    }

    // Where on one side of a shape (0 left, 1 right, 2 top, 3 bottom) a line can leave or arrive.
    pub fn ports(&self, b: usize, side: f64, want: f64, lo: f64, hi: f64) -> Vec<f64> {
        let empty = Vec::new();
        let used = self.taken.get(&b).unwrap_or(&empty);
        let mut out = Vec::new();
        let mut k = 0;
        while k < 9 && out.len() < 4 {
            let at = want + (if k & 1 != 0 { 1.0 } else { -1.0 }) * ((k as f64) / 2.0).ceil() * PORT_STEP;
            k += 1;
            if at < lo || at > hi {
                continue;
            }
            let mut clear = true;
            let mut i = 0;
            while i < used.len() {
                if used[i] == side && (used[i + 1] - at).abs() < PORT_STEP {
                    clear = false;
                }
                i += 2;
            }
            if clear {
                out.push(at);
            }
        }
        if out.is_empty() {
            vec![want]
        } else {
            out
        }
    }

    pub fn claim(&mut self, b: usize, side: f64, at: f64) {
        self.taken.entry(b).or_default().extend([side, at]);
    }
}

fn path(points: &[f64], start_trim: f64, end_trim: f64) -> String {
    let n = points.len();
    let ux = sign(points[2] - points[0]);
    let uy = sign(points[3] - points[1]);
    let mut d = format!("M{},{}", num(points[0] + ux * start_trim), num(points[1] + uy * start_trim));
    let mut i = 2;
    while i < n - 2 {
        let x = points[i];
        let y = points[i + 1];
        let ix = sign(x - points[i - 2]);
        let iy = sign(y - points[i - 1]);
        let ox = sign(points[i + 2] - x);
        let oy = sign(points[i + 3] - y);
        let r = jmin(jmin(CORNER, (x - points[i - 2] + y - points[i - 1]).abs() / 2.0), (points[i + 2] - x + points[i + 3] - y).abs() / 2.0);
        let first = if ix != 0.0 && !ix.is_nan() { format!("H{}", num(x - ix * r)) } else { format!("V{}", num(y - iy * r)) };
        d.push_str(&format!("{}Q{},{} {},{}", first, num(x), num(y), num(x + ox * r), num(y + oy * r)));
        i += 2;
    }
    let lx = sign(points[n - 2] - points[n - 4]);
    let ly = sign(points[n - 1] - points[n - 3]);
    d + &if lx != 0.0 && !lx.is_nan() { format!("H{}", num(points[n - 2] - lx * end_trim)) } else { format!("V{}", num(points[n - 1] - ly * end_trim)) }
}

fn clear_of(points: &[f64], others: &[usize], boxes: &[BoxR], a: usize, b: usize) -> bool {
    for &o in others {
        let other = &boxes[o];
        if o != a && o != b && crosses(points, other.x, other.y, other.x + other.w, other.y + other.h) {
            return false;
        }
    }
    true
}

// The route for a line with corners through `points`. `along` is the stretch its label sits on.
fn bent(points: Vec<f64>, along: usize, start_trim: f64, end_trim: f64, nx: f64, lean: f64, room: f64) -> Route {
    let n = points.len();
    Route {
        d: path(&points, start_trim, end_trim),
        sx: points[0],
        sy: points[1],
        sdx: -sign(points[2] - points[0]),
        sdy: -sign(points[3] - points[1]),
        ex: points[n - 2],
        ey: points[n - 1],
        edx: sign(points[n - 2] - points[n - 4]),
        edy: sign(points[n - 1] - points[n - 3]),
        x0: points[along],
        y0: points[along + 1],
        x1: points[along + 2],
        y1: points[along + 3],
        nx,
        ny: 0.0,
        lean,
        room,
        points,
    }
}

fn across(lo: f64, hi: f64) -> Vec<f64> {
    let mut out = Vec::new();
    let mid = (lo + hi) / 2.0;
    let step = LANE_STEP.max((hi - lo) / 4.0);
    for k in 0..9 {
        let at = mid + (if k & 1 != 0 { 1.0 } else { -1.0 }) * ((k as f64) / 2.0).ceil() * step;
        if at >= lo + NEAR && at <= hi - NEAR {
            out.push(at);
        }
    }
    out
}

struct Best {
    points: Vec<f64>,
    cost: f64,
    upright: bool,
    sides: [f64; 2],
}

fn elbow(boxes: &[BoxR], a: usize, b: usize, others: &[usize], lanes: &mut Lanes, start_trim: f64, end_trim: f64) -> Option<Route> {
    let (ba, bb) = (&boxes[a], &boxes[b]);
    let ax = ba.x + ba.w / 2.0;
    let ay = ba.y + ba.h / 2.0;
    let bx = bb.x + bb.w / 2.0;
    let by = bb.y + bb.h / 2.0;
    let mut best: Option<Best> = None;
    let consider = |best: &mut Option<Best>, lanes: &Lanes, points: Vec<f64>, upright: bool, sides: [f64; 2]| -> bool {
        if !clear_of(&points, others, boxes, a, b) {
            return false;
        }
        let mut i = 2;
        while i < points.len() {
            let up = points[i] == points[i - 2];
            if !lanes.free(up, if up { points[i] } else { points[i + 1] }, if up { points[i - 1] } else { points[i - 2] }, if up { points[i + 1] } else { points[i] }) {
                return false;
            }
            i += 2;
        }
        let mut cost = lanes.crossings(&points) as f64 * 1e6;
        let mut i = 2;
        while i < points.len() {
            cost += (points[i] - points[i - 2]).abs() + (points[i + 1] - points[i - 1]).abs();
            i += 2;
        }
        if best.as_ref().is_none_or(|b| cost < b.cost) {
            *best = Some(Best { points, cost, upright, sides });
            return true;
        }
        false
    };
    if ba.x + ba.w <= bb.x || bb.x + bb.w <= ba.x {
        let right = bx > ax;
        let down = sign(by - ay);
        let sides = if right { [1.0, 0.0] } else { [0.0, 1.0] };
        let lanes_across = across(if right { ba.x + ba.w } else { bb.x + bb.w }, if right { bb.x } else { ba.x });
        'search: for y0 in lanes.ports(a, sides[0], ay + (down * ba.h) / 4.0, ba.y + 8.0, ba.y + ba.h - 8.0) {
            for y1 in lanes.ports(b, sides[1], by - (down * bb.h) / 4.0, bb.y + 8.0, bb.y + bb.h - 8.0) {
                for &lane in &lanes_across {
                    let x0 = lane + (ax - lane) * jmin(hit(ba, lane, y0, ax, y0), 1.0);
                    let x1 = lane + (bx - lane) * jmin(hit(bb, lane, y1, bx, y1), 1.0);
                    if consider(&mut best, lanes, vec![x0, y0, lane, y0, lane, y1, x1, y1], true, sides) {
                        break 'search;
                    }
                }
            }
        }
    }
    if ba.y + ba.h <= bb.y || bb.y + bb.h <= ba.y {
        let below = by > ay;
        let sideways = sign(bx - ax);
        let sides = if below { [3.0, 2.0] } else { [2.0, 3.0] };
        let lanes_across = across(if below { ba.y + ba.h } else { bb.y + bb.h }, if below { bb.y } else { ba.y });
        'search2: for x0 in lanes.ports(a, sides[0], ax + (sideways * ba.w) / 4.0, ba.x + 8.0, ba.x + ba.w - 8.0) {
            for x1 in lanes.ports(b, sides[1], bx - (sideways * bb.w) / 4.0, bb.x + 8.0, bb.x + bb.w - 8.0) {
                for &lane in &lanes_across {
                    let y0 = lane + (ay - lane) * jmin(hit(ba, x0, lane, x0, ay), 1.0);
                    let y1 = lane + (by - lane) * jmin(hit(bb, x1, lane, x1, by), 1.0);
                    if consider(&mut best, lanes, vec![x0, y0, x0, lane, x1, lane, x1, y1], false, sides) {
                        break 'search2;
                    }
                }
            }
        }
    }
    let Best { points, upright, sides, .. } = best?;
    lanes.take(&points);
    lanes.claim(a, sides[0], if upright { points[1] } else { points[0] });
    lanes.claim(b, sides[1], if upright { points[7] } else { points[6] });
    // The label sits on the middle stretch, across the gap.
    Some(bent(points, 2, start_trim, end_trim, 0.0, 0.0, if upright { NARROW } else { WIDE }))
}

/// Routes a relation from a to b. `shift` moves the line sideways when another runs the opposite
/// way. `others` are the shapes to steer clear of; None skips that check.
pub fn route(boxes: &[BoxR], a: usize, b: usize, shift: f64, others: Option<&[usize]>, lanes: &mut Lanes, start_trim: f64, end_trim: f64) -> Route {
    let (ba, bb) = (&boxes[a], &boxes[b]);
    if a == b {
        let spread = (ba.h / 2.0 - 4.0).min(10.0);
        let len = hypot(LOOP, spread);
        let x = ba.x + ba.w;
        let cy = ba.y + ba.h / 2.0;
        let ux = LOOP / len;
        let uy = spread / len;
        return Route {
            d: format!(
                "M{},{}C{},{} {},{} {},{}",
                num(x + ux * start_trim),
                num(cy - spread - uy * start_trim),
                num(x + LOOP),
                num(cy - spread * 2.0),
                num(x + LOOP),
                num(cy + spread * 2.0),
                num(x + ux * end_trim),
                num(cy + spread + uy * end_trim)
            ),
            sx: x,
            sy: cy - spread,
            sdx: -ux,
            sdy: uy,
            ex: x,
            ey: cy + spread,
            edx: -ux,
            edy: -uy,
            x0: x + LOOP + 4.0,
            y0: cy,
            x1: x + LOOP + 4.0,
            y1: cy,
            nx: 1.0,
            ny: 0.0,
            lean: 0.0,
            room: 64.0,
            points: Vec::new(),
        };
    }

    let [sx, sy, ex, ey] = connect(ba, bb, shift);
    let upright = sx == ex;
    let hits_line = |o: usize| {
        let other = &boxes[o];
        o != a && o != b && hit_rect(other.x, other.y, other.x + other.w, other.y + other.h, sx, sy, ex - sx, ey - sy) <= 1.0
    };
    let mut blocked = false;
    if let Some(others) = others {
        if !contains(ba, bb) && !contains(bb, ba) && others.iter().any(|&o| hits_line(o)) {
            blocked = true;
        }
    }
    if let (Some(others), true) = (others, blocked && !upright && sy != ey) {
        if let Some(around) = elbow(boxes, a, b, others, lanes, start_trim, end_trim) {
            return around;
        }
        lanes.squeezed += 1;
    }
    if let (Some(others), true) = (others, blocked && (upright || sy == ey)) {
        let lane_on = |forward: bool| -> f64 {
            let mut lane = if forward { f64::NEG_INFINITY } else { f64::INFINITY };
            let mut widen = |bx: &BoxR| {
                let edge = if upright { bx.x } else { bx.y };
                lane = if forward { lane.max(edge + if upright { bx.w } else { bx.h }) } else { lane.min(edge) };
            };
            widen(ba);
            widen(bb);
            for &o in others {
                if hits_line(o) {
                    widen(&boxes[o]);
                }
            }
            lane
        };
        let lo = if upright { ba.y.min(bb.y) } else { ba.x.min(bb.x) };
        let hi = if upright { (ba.y + ba.h).max(bb.y + bb.h) } else { (ba.x + ba.w).max(bb.x + bb.w) };
        let outside = |forward: bool, lane: f64| -> bool {
            for &o in others {
                if o == a || o == b {
                    continue;
                }
                let other = &boxes[o];
                let near = if upright { other.y < hi && other.y + other.h > lo } else { other.x < hi && other.x + other.w > lo };
                let start = if upright { other.x } else { other.y };
                let end = start + if upright { other.w } else { other.h };
                if near && (if forward { end > lane } else { start < lane }) {
                    return false;
                }
            }
            true
        };
        let ax = ba.x + ba.w / 2.0;
        let ay = ba.y + ba.h / 2.0;
        let bx = bb.x + bb.w / 2.0;
        let by = bb.y + bb.h / 2.0;
        let towards = if upright { sign(by - ay) } else { sign(bx - ax) };
        struct Way {
            points: Vec<f64>,
            side: f64,
            p0: f64,
            p1: f64,
            step: usize,
        }
        let along = |lanes: &Lanes, forward: bool| -> Way {
            let lane = lane_on(forward);
            let sgn = if forward { 1.0 } else { -1.0 };
            let side = if upright {
                if forward {
                    1.0
                } else {
                    0.0
                }
            } else if forward {
                3.0
            } else {
                2.0
            };
            let p0 = if upright {
                lanes.ports(a, side, ay + (towards * ba.h) / 4.0, ba.y + 8.0, ba.y + ba.h - 8.0)[0]
            } else {
                lanes.ports(a, side, ax + (towards * ba.w) / 4.0, ba.x + 8.0, ba.x + ba.w - 8.0)[0]
            };
            let p1 = if upright {
                lanes.ports(b, side, by - (towards * bb.h) / 4.0, bb.y + 8.0, bb.y + bb.h - 8.0)[0]
            } else {
                lanes.ports(b, side, bx - (towards * bb.w) / 4.0, bb.x + 8.0, bb.x + bb.w - 8.0)[0]
            };
            let points_at = |at: f64| -> Vec<f64> {
                if upright {
                    let x0 = at + (ax - at) * jmin(hit(ba, at, p0, ax, p0), 1.0);
                    let x1 = at + (bx - at) * jmin(hit(bb, at, p1, bx, p1), 1.0);
                    vec![x0, p0, at, p0, at, p1, x1, p1]
                } else {
                    let y0 = at + (ay - at) * jmin(hit(ba, p0, at, p0, ay), 1.0);
                    let y1 = at + (by - at) * jmin(hit(bb, p1, at, p1, by), 1.0);
                    vec![p0, y0, p0, at, p1, at, p1, y1]
                }
            };
            let mut points = points_at(lane + sgn * LANE);
            for k in 0..=4 {
                let at = lane + sgn * (LANE + k as f64 * LANE_STEP);
                let tried = points_at(at);
                if !clear_of(&tried, others, boxes, a, b) {
                    continue;
                }
                points = tried;
                if lanes.free(upright, at, p0, p1) {
                    return Way { points, side, p0, p1, step: k };
                }
            }
            Way { points, side, p0, p1, step: 5 }
        };
        let natural = if upright { ey > sy } else { ex < sx };
        let out = outside(natural, lane_on(natural));
        let back = outside(!natural, lane_on(!natural));
        let first = along(lanes, natural);
        let other = if out == back { Some(along(lanes, !natural)) } else { None };
        let cost = |way: &Way| lanes.crossings(&way.points) as f64 * 8.0 + way.step as f64;
        let forward = if back && !out {
            !natural
        } else if other.as_ref().is_some_and(|o| cost(o) < cost(&first)) {
            !natural
        } else {
            natural
        };
        let way = if forward == natural { first } else { other.unwrap_or_else(|| along(lanes, !natural)) };
        let sgn = if forward { 1.0 } else { -1.0 };
        if way.step > 4 {
            lanes.squeezed += 1;
        }
        lanes.take(&way.points);
        lanes.claim(a, way.side, way.p0);
        lanes.claim(b, way.side, way.p1);
        return if upright { bent(way.points, 2, start_trim, end_trim, sgn, LEAN, NARROW) } else { bent(way.points, 2, start_trim, end_trim, 0.0, 0.0, WIDE) };
    }

    let len = crate::js::or(hypot(ex - sx, ey - sy), || 1.0);
    let dx = (ex - sx) / len;
    let dy = (ey - sy) / len;
    if upright || sy == ey {
        lanes.take(&[sx, sy, ex, ey]);
    }
    Route {
        d: format!("M{},{}L{},{}", num(sx + dx * start_trim), num(sy + dy * start_trim), num(ex - dx * end_trim), num(ey - dy * end_trim)),
        sx,
        sy,
        sdx: -dx,
        sdy: -dy,
        ex,
        ey,
        edx: dx,
        edy: dy,
        x0: sx,
        y0: sy,
        x1: ex,
        y1: ey,
        nx: if shift != 0.0 { -dy } else { 0.0 },
        ny: if shift != 0.0 { dx } else { 0.0 },
        lean: 0.0,
        room: if dx.abs() > dy.abs() { NARROW } else { WIDE },
        points: vec![sx, sy, ex, ey],
    }
}
