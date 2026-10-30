// Port of src/diagrams/venn/layout.ts.

use super::{sort16, VennSubset};
use crate::js;
use std::f64::consts::PI;

#[derive(Clone)]
pub struct Circle {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub r: f64,
}

// Where the content of a region goes: the point inside all of `inside` and outside every other
// circle that is farthest from their outlines, and how far that is. A margin of 0 or less means
// the circles as placed leave the region empty.
#[derive(Clone, Copy)]
pub struct Spot {
    pub x: f64,
    pub y: f64,
    pub margin: f64,
}

const TAU: f64 = PI * 2.0;

/// `JSON.stringify` of a list of strings.
pub fn json_list(ids: &[String]) -> String {
    let mut out = String::from("[");
    for (i, id) in ids.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('"');
        for c in id.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\u{8}' => out.push_str("\\b"),
                '\u{c}' => out.push_str("\\f"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                c => out.push(c),
            }
        }
        out.push('"');
    }
    out.push(']');
    out
}

// For every overlap of three or more sets, adds the pairwise overlaps it implies and the diagram
// does not state, a quarter of the smaller set each, so that the circles are drawn overlapping.
pub fn ensure_pairwise_subsets(subsets: &[VennSubset]) -> Vec<VennSubset> {
    let mut present: Vec<String> = Vec::new();
    let mut sizes: Vec<(String, f64)> = Vec::new();
    for subset in subsets {
        let mut sorted = subset.sets.clone();
        sort16(&mut sorted);
        let key = json_list(&sorted);
        if !present.contains(&key) {
            present.push(key);
        }
        if subset.sets.len() == 1 {
            match sizes.iter_mut().find(|(k, _)| *k == subset.sets[0]) {
                Some(slot) => slot.1 = subset.size,
                None => sizes.push((subset.sets[0].clone(), subset.size)),
            }
        }
    }
    let size_of = |id: &str| sizes.iter().find(|(k, _)| k == id).map(|(_, v)| *v);
    let mut out = subsets.to_vec();
    for subset in subsets {
        if subset.sets.len() < 3 {
            continue;
        }
        let mut members = subset.sets.clone();
        sort16(&mut members);
        for i in 0..members.len() - 1 {
            for j in i + 1..members.len() {
                let pair = vec![members[i].clone(), members[j].clone()];
                let key = json_list(&pair);
                if present.contains(&key) {
                    continue;
                }
                present.push(key);
                let size = match (size_of(&pair[0]), size_of(&pair[1])) {
                    (Some(a), Some(b)) => a.min(b) / 4.0,
                    _ => 2.5,
                };
                out.push(VennSubset { sets: pair, size, label: Some(String::new()) });
            }
        }
    }
    out
}

// The area two circles share when their centers are d apart.
pub fn lens_area(r1: f64, r2: f64, d: f64) -> f64 {
    if d >= r1 + r2 {
        return 0.0;
    }
    let small = r1.min(r2);
    if d <= (r1 - r2).abs() {
        return PI * small * small;
    }
    let a1 = r1 * r1 * ((d * d + r1 * r1 - r2 * r2) / (2.0 * d * r1)).acos();
    let a2 = r2 * r2 * ((d * d + r2 * r2 - r1 * r1) / (2.0 * d * r2)).acos();
    let a3 = 0.5 * ((-d + r1 + r2) * (d + r1 - r2) * (d - r1 + r2) * (d + r1 + r2)).max(0.0).sqrt();
    a1 + a2 - a3
}

// How far apart two circles sit for their overlap to have the given area. The area falls as the
// distance grows, so bisection finds it.
pub fn distance_for_overlap(r1: f64, r2: f64, area: f64) -> f64 {
    let mut low = (r1 - r2).abs();
    let mut high = r1 + r2;
    if !(area > 0.0) {
        return high;
    }
    if area >= PI * r1.min(r2).powi(2) {
        return low;
    }
    for _ in 0..48 {
        let middle = (low + high) / 2.0;
        if lens_area(r1, r2, middle) > area {
            low = middle;
        } else {
            high = middle;
        }
    }
    (low + high) / 2.0
}

// How far a point is from the nearest outline it must stay within or without. Gives up, with
// some value below `floor`, as soon as the point cannot reach `floor`.
fn margin(circles: &[Circle], inside: &[usize], outside: &[usize], x: f64, y: f64, floor: f64) -> f64 {
    let mut best = f64::INFINITY;
    for &i in inside {
        let c = &circles[i];
        let dx = x - c.x;
        let dy = y - c.y;
        let room = c.r - (dx * dx + dy * dy).sqrt();
        if room < best {
            if room < floor {
                return room;
            }
            best = room;
        }
    }
    for &i in outside {
        let c = &circles[i];
        let dx = x - c.x;
        let dy = y - c.y;
        let room = (dx * dx + dy * dy).sqrt() - c.r;
        if room < best {
            if room < floor {
                return room;
            }
            best = room;
        }
    }
    best
}

// Finds the roomiest point of a region by sampling the smallest of its circles on a grid and then
// walking uphill from the best sample. Ties go to the point nearest the middle of the region, so
// a symmetric arrangement gets symmetric labels.
pub fn find_spot(circles: &[Circle], inside: &[usize], outside: &[usize]) -> Spot {
    let mut cx = 0.0;
    let mut cy = 0.0;
    let mut smallest = &circles[inside[0]];
    for &i in inside {
        let c = &circles[i];
        cx += c.x / inside.len() as f64;
        cy += c.y / inside.len() as f64;
        if c.r < smallest.r {
            smallest = c;
        }
    }
    let r = smallest.r;
    let (sx, sy) = (smallest.x, smallest.y);
    let mut x = cx;
    let mut y = cy;
    let mut best = margin(circles, inside, outside, x, y, f64::NEG_INFINITY);
    let mut off = 0.0;
    let mut consider = |px: f64, py: f64, x: &mut f64, y: &mut f64| -> bool {
        let m = margin(circles, inside, outside, px, py, best - r * 1e-9);
        if !(m > best - r * 1e-9) {
            return false;
        }
        let distance = js::hypot(px - cx, py - cy);
        if m > best + r * 1e-9 || distance < off - r * 1e-9 {
            best = m;
            off = distance;
            *x = px;
            *y = py;
            return true;
        }
        false
    };
    const GRID: i32 = 10;
    let g = GRID as f64;
    for gy in -GRID..=GRID {
        for gx in -GRID..=GRID {
            if gx * gx + gy * gy <= GRID * GRID {
                consider(sx + (gx as f64 * r) / g, sy + (gy as f64 * r) / g, &mut x, &mut y);
            }
        }
    }
    let mut step = r / g;
    let mut round = 0;
    while round < 40 && step > r * 1e-6 {
        let mut moved = false;
        for k in 0..8 {
            let angle = (k as f64 * TAU) / 8.0;
            let (px, py) = (x + angle.cos() * step, y + angle.sin() * step);
            if consider(px, py, &mut x, &mut y) {
                moved = true;
            }
        }
        if !moved {
            step /= 2.0;
        }
        round += 1;
    }
    Spot { x, y, margin: best }
}

// Places one circle per set, with areas in proportion to the sizes of the sets and each pair as
// far apart as its overlap asks: one, two and three sets exactly, more by relaxing a ring of
// circles toward those distances. Lengths are in units of the square root of a size.
pub fn layout_circles(subsets: &[VennSubset]) -> Vec<Circle> {
    let mut sizes: Vec<(String, f64)> = Vec::new();
    for subset in subsets {
        for id in &subset.sets {
            if !sizes.iter().any(|(k, _)| k == id) {
                sizes.push((id.clone(), 10.0));
            }
        }
    }
    for subset in subsets {
        if subset.sets.len() == 1 {
            sizes.iter_mut().find(|(k, _)| *k == subset.sets[0]).unwrap().1 = subset.size;
        }
    }

    let mut circles: Vec<Circle> = Vec::new();
    let mut largest = 0.0;
    for &(_, size) in &sizes {
        if size.is_finite() && size > largest {
            largest = size;
        }
    }
    if largest == 0.0 {
        largest = 10.0;
    }
    let widest = (largest / PI).sqrt();
    for (id, size) in &sizes {
        // A set too small to hold its name, or with no size at all, still gets a circle that shows.
        let r = if size.is_finite() && *size > 0.0 { (size / PI).sqrt() } else { 0.0 };
        circles.push(Circle { id: id.clone(), x: 0.0, y: 0.0, r: r.max(widest * 0.3) });
    }
    let index = |id: &str| sizes.iter().position(|(k, _)| k == id).unwrap();
    let n = circles.len();
    if n < 2 {
        return circles;
    }

    // The overlap each pair was given, by the indexes of its circles.
    let mut overlaps: Vec<(usize, f64)> = Vec::new();
    for subset in ensure_pairwise_subsets(subsets) {
        if subset.sets.len() != 2 {
            continue;
        }
        let a = index(&subset.sets[0]);
        let b = index(&subset.sets[1]);
        if a != b {
            let key = a.min(b) * n + a.max(b);
            match overlaps.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = subset.size,
                None => overlaps.push((key, subset.size)),
            }
        }
    }
    // How far apart each overlapping pair should be. Pairs that do not overlap only have to keep a gap.
    let mut wanted_list: Vec<(usize, f64)> = Vec::new();
    for &(key, area) in &overlaps {
        wanted_list.push((key, distance_for_overlap(circles[key / n].r, circles[key % n].r, area)));
    }
    let wanted = |key: usize| wanted_list.iter().find(|(k, _)| *k == key).map(|(_, v)| *v);
    let gap = widest * 0.15;
    let distance = |circles: &[Circle], a: usize, b: usize| wanted(a * n + b).unwrap_or(circles[a].r + circles[b].r + gap);

    if n == 2 {
        circles[1].x = distance(&circles, 0, 1);
        return circles;
    }

    if n == 3 {
        // Two sets side by side and the third below, the corners of a triangle whose sides are the
        // three distances. An overlap of all three has to exist, so the triangle shrinks until it does.
        let mut ab = distance(&circles, 0, 1);
        let mut ac = distance(&circles, 0, 2);
        let mut bc = distance(&circles, 1, 2);
        let triple = subsets.iter().any(|s| {
            let mut u: Vec<&String> = Vec::new();
            for id in &s.sets {
                if !u.contains(&id) {
                    u.push(id);
                }
            }
            u.len() == 3
        });
        for _ in 0..40 {
            let x = if ab > 0.0 { (ab * ab + ac * ac - bc * bc) / (2.0 * ab) } else { 0.0 };
            circles[1].x = ab;
            circles[2].x = (-ac).max(ac.min(x));
            circles[2].y = (ac * ac - x * x).max(0.0).sqrt();
            if !triple || find_spot(&circles, &[0, 1, 2], &[]).margin > widest * 0.12 {
                break;
            }
            ab *= 0.94;
            ac *= 0.94;
            bc *= 0.94;
        }
        return circles;
    }

    let mut total = 0.0;
    for c in &circles {
        total += 2.0 * c.r + gap;
    }
    let rounds = 300usize.min((3e6 / (n * n) as f64).floor() as usize);
    if n <= 64 {
        let ring = total / TAU;
        for (i, c) in circles.iter_mut().enumerate() {
            let angle = (i as f64 * TAU) / n as f64 - PI / 2.0;
            c.x = angle.cos() * ring;
            c.y = angle.sin() * ring;
        }
    } else {
        let columns = (n as f64).sqrt().ceil() as usize;
        let cell = 2.0 * widest + gap;
        for (i, c) in circles.iter_mut().enumerate() {
            c.x = (i % columns) as f64 * cell;
            c.y = (i / columns) as f64 * cell;
        }
    }
    let mut dx = vec![0f64; n];
    let mut dy = vec![0f64; n];
    let mut pulls = vec![0i32; n];
    for _ in 0..rounds {
        dx.fill(0.0);
        dy.fill(0.0);
        pulls.fill(0);
        for a in 0..n {
            for b in a + 1..n {
                let overlap = wanted(a * n + b);
                let apart = circles[a].r + circles[b].r + gap;
                let mut ux = circles[b].x - circles[a].x;
                let mut uy = circles[b].y - circles[a].y;
                let mut length = js::hypot(ux, uy);
                if overlap.is_none() && length >= apart {
                    continue;
                }
                if length < 1e-9 {
                    ux = ((a + b) as f64).cos();
                    uy = ((a + b) as f64).sin();
                    length = 1.0;
                }
                let mv = (length - overlap.unwrap_or(apart)) / 2.0 / length;
                dx[a] += ux * mv;
                dy[a] += uy * mv;
                dx[b] -= ux * mv;
                dy[b] -= uy * mv;
                pulls[a] += 1;
                pulls[b] += 1;
            }
        }
        for i in 0..n {
            if pulls[i] == 0 {
                continue;
            }
            circles[i].x += dx[i] / pulls[i] as f64;
            circles[i].y += dy[i] / pulls[i] as f64;
        }
    }
    circles
}

struct Corner {
    x: f64,
    y: f64,
    a: usize,
    b: usize,
}

// The outline of the region all the given circles share, as an SVG path, or '' if they share none.
pub fn region_path(circles: &[Circle], format: fn(f64) -> String) -> String {
    let whole = |c: &Circle| {
        format!(
            "M{},{}A{},{} 0 1 1 {},{}A{},{} 0 1 1 {},{}Z",
            format(c.x - c.r),
            format(c.y),
            format(c.r),
            format(c.r),
            format(c.x + c.r),
            format(c.y),
            format(c.r),
            format(c.r),
            format(c.x - c.r),
            format(c.y)
        )
    };
    if circles.len() == 1 {
        return whole(&circles[0]);
    }
    let within = |x: f64, y: f64, skip: usize, other: usize| -> bool {
        for (k, c) in circles.iter().enumerate() {
            if k != skip && k != other && js::hypot(x - c.x, y - c.y) > c.r * (1.0 + 1e-9) {
                return false;
            }
        }
        true
    };
    // The corners of the region: where two outlines cross inside all the other circles.
    let mut corners: Vec<Corner> = Vec::new();
    for i in 0..circles.len() {
        for j in i + 1..circles.len() {
            let a = &circles[i];
            let b = &circles[j];
            let d = js::hypot(b.x - a.x, b.y - a.y);
            if d >= a.r + b.r || d <= (a.r - b.r).abs() || d == 0.0 {
                continue;
            }
            let along = (a.r * a.r - b.r * b.r + d * d) / (2.0 * d);
            let up = (a.r * a.r - along * along).max(0.0).sqrt();
            let mx = a.x + ((b.x - a.x) * along) / d;
            let my = a.y + ((b.y - a.y) * along) / d;
            for sign in [1.0, -1.0] {
                let x = mx + (sign * up * (b.y - a.y)) / d;
                let y = my - (sign * up * (b.x - a.x)) / d;
                if within(x, y, i, j) {
                    corners.push(Corner { x, y, a: i, b: j });
                }
            }
        }
    }
    if corners.len() < 2 {
        // No corners: either one circle lies inside all the others, or there is nothing shared.
        let mut smallest = 0;
        for (k, c) in circles.iter().enumerate() {
            if c.r < circles[smallest].r {
                smallest = k;
            }
        }
        let s = &circles[smallest];
        for (k, c) in circles.iter().enumerate() {
            if k != smallest && js::hypot(c.x - s.x, c.y - s.y) + s.r > c.r * (1.0 + 1e-9) {
                return String::new();
            }
        }
        return whole(s);
    }
    let mut cx = 0.0;
    let mut cy = 0.0;
    let count = corners.len() as f64;
    for p in &corners {
        cx += p.x / count;
        cy += p.y / count;
    }
    corners.sort_by(|p, q| js::ord((p.y - cy).atan2(p.x - cx) - (q.y - cy).atan2(q.x - cx)));
    let mut d = String::new();
    for i in 0..corners.len() {
        let from = &corners[i];
        let to = &corners[(i + 1) % corners.len()];
        if i == 0 {
            d = format!("M{},{}", format(from.x), format(from.y));
        }
        // The side between two corners is an arc of a circle that passes through both; of those, the
        // one whose arc stays inside the others.
        let mut arc: Option<usize> = None;
        let mut large = 0;
        let mut narrowest = f64::INFINITY;
        for k in [from.a, from.b] {
            if k != to.a && k != to.b {
                continue;
            }
            let c = &circles[k];
            let start = (from.y - c.y).atan2(from.x - c.x);
            let mut sweep = (to.y - c.y).atan2(to.x - c.x) - start;
            if sweep <= 0.0 {
                sweep += TAU;
            }
            let mx = c.x + c.r * (start + sweep / 2.0).cos();
            let my = c.y + c.r * (start + sweep / 2.0).sin();
            let width = js::hypot(mx - (from.x + to.x) / 2.0, my - (from.y + to.y) / 2.0);
            if width < narrowest {
                narrowest = width;
                arc = Some(k);
                large = if sweep > PI { 1 } else { 0 };
            }
        }
        match arc {
            None => d.push_str(&format!("L{},{}", format(to.x), format(to.y))),
            Some(k) => d.push_str(&format!("A{},{} 0 {} 1 {},{}", format(circles[k].r), format(circles[k].r), large, format(to.x), format(to.y))),
        }
    }
    d + "Z"
}
