// Port of src/diagrams/treemap/layout.ts.

use super::TreemapNode;

pub struct Box<'a> {
    pub node: &'a TreemapNode,
    pub parent: Option<usize>,
    // Position among the top-level nodes of the branch this box belongs to, which picks its color.
    pub series: usize,
    // A leaf's own value, or the sum of the leaves under a section.
    pub value: f64,
    pub children: Vec<usize>,
    // Small sections give their header space to their children.
    pub headed: bool,
    pub header: f64,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

pub struct Insets {
    pub gap: f64,
    pub header: f64,
    pub short: f64,
    pub side: f64,
}

fn make<'a>(node: &'a TreemapNode, parent: Option<usize>, series: usize) -> Box<'a> {
    let value = match (node.children.as_ref(), node.value) {
        (None, Some(v)) if v.is_finite() && v > 0.0 => v,
        _ => 0.0,
    };
    Box { node, parent, series, value, children: Vec::new(), headed: false, header: 0.0, x: 0.0, y: 0.0, w: 0.0, h: 0.0 }
}

fn clamp(v: f64, low: f64, high: f64) -> f64 {
    if v >= low {
        v.min(high)
    } else {
        low
    }
}

fn round(v: f64) -> f64 {
    crate::js::round(v)
}

// Lays out the boxes in the rectangle as one squarified treemap.
#[allow(clippy::too_many_arguments)]
fn squarify(all: &mut [Box], boxes: &[usize], total: f64, mut x: f64, mut y: f64, mut w: f64, mut h: f64, gap: f64) {
    let mut rest = total;
    let mut from = 0;
    while from < boxes.len() {
        let scale = (w * h) / rest;
        let side = w.min(h);
        let mut sum = all[boxes[from]].value * scale;
        let largest = sum;
        let mut worst = ((side * side * largest) / (sum * sum)).max((sum * sum) / (side * side * largest));
        let mut to = from + 1;
        while to < boxes.len() {
            let smallest = all[boxes[to]].value * scale;
            let next = sum + smallest;
            let ratio = ((side * side * largest) / (next * next)).max((next * next) / (side * side * smallest));
            if ratio > worst {
                break;
            }
            worst = ratio;
            sum = next;
            to += 1;
        }
        let thickness = sum / side;
        let mut offset = 0.0;
        for i in from..to {
            let b = &mut all[boxes[i]];
            let length = if i == to - 1 { side - offset } else { (b.value * scale) / thickness };
            let bx = if w >= h { x } else { x + offset };
            let by = if w >= h { y + offset } else { y };
            let bw = if w >= h { thickness } else { length };
            let bh = if w >= h { length } else { thickness };
            // Omit the gap if it would erase the box.
            let gx = if round(bx + bw - gap / 2.0) > round(bx + gap / 2.0) { gap / 2.0 } else { 0.0 };
            let gy = if round(by + bh - gap / 2.0) > round(by + gap / 2.0) { gap / 2.0 } else { 0.0 };
            b.x = round(bx + gx);
            b.y = round(by + gy);
            b.w = round(bx + bw - gx) - b.x;
            b.h = round(by + bh - gy) - b.y;
            offset += length;
            rest -= b.value;
        }
        if w >= h {
            x += thickness;
            w -= thickness;
        } else {
            y += thickness;
            h -= thickness;
        }
        from = to;
    }
}

fn place(all: &mut [Box], placed: &mut Vec<usize>, children: &[usize], x: f64, y: f64, w: f64, h: f64, gap: f64) {
    let mut shown: Vec<usize> = children.iter().copied().filter(|&b| all[b].value > 0.0).collect();
    shown.sort_by(|&a, &b| crate::js::ord(all[b].value - all[a].value));
    let mut total = 0.0;
    for &b in &shown {
        total += all[b].value;
    }
    if shown.is_empty() || !total.is_finite() {
        return;
    }
    let w = w.max(0.0);
    let h = h.max(0.0);
    if w > 0.0 && h > 0.0 {
        squarify(all, &shown, total, x - gap / 2.0, y - gap / 2.0, w + gap, h + gap, gap);
    }
    for &k in &shown {
        let b = &mut all[k];
        let x0 = clamp(b.x, x, x + w);
        let y0 = clamp(b.y, y, y + h);
        b.w = clamp(b.x + b.w, x0, x + w) - x0;
        b.h = clamp(b.y + b.h, y0, y + h) - y0;
        b.x = x0;
        b.y = y0;
        placed.push(k);
    }
}

/// Boxes with positive values, including zero-size boxes, parents before children: the arena of
/// every box and the indexes of the placed ones.
pub fn layout_treemap<'a>(roots: &'a [TreemapNode], width: f64, height: f64, insets: &Insets) -> (Vec<Box<'a>>, Vec<usize>) {
    let mut all: Vec<Box<'a>> = roots.iter().enumerate().map(|(i, n)| make(n, None, i)).collect();
    let top: Vec<usize> = (0..all.len()).collect();
    let mut i = 0;
    while i < all.len() {
        let node = all[i].node;
        let series = all[i].series;
        if let Some(children) = node.children.as_ref() {
            for child in children {
                let k = all.len();
                all.push(make(child, Some(i), series));
                all[i].children.push(k);
            }
        }
        i += 1;
    }
    for i in (0..all.len()).rev() {
        if let Some(p) = all[i].parent {
            let v = all[i].value;
            all[p].value += v;
        }
    }
    let mut placed: Vec<usize> = Vec::new();
    place(&mut all, &mut placed, &top, 0.0, 0.0, width, height, insets.gap);
    let mut i = 0;
    while i < placed.len() {
        let k = placed[i];
        i += 1;
        if all[k].children.is_empty() {
            continue;
        }
        let b = &mut all[k];
        let top = if b.h - insets.header - insets.side >= 12.0 { insets.header } else { insets.short };
        b.headed = b.w > 24f64.max(2.0 * insets.side) && b.h - top - insets.side >= 8.0;
        b.header = if b.headed { top } else { 0.0 };
        let (bx, by, bw, bh, headed) = (b.x, b.y, b.w, b.h, b.headed);
        let children = b.children.clone();
        if headed {
            place(&mut all, &mut placed, &children, bx + insets.side, by + top, bw - 2.0 * insets.side, bh - top - insets.side, insets.gap);
        } else {
            place(&mut all, &mut placed, &children, bx, by, bw, bh, insets.gap);
        }
    }
    (all, placed)
}
