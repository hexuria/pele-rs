// Port of src/layout/components.ts.

use super::compound::{CEdge, CNode, CompoundResult};

struct Part {
    nodes: Vec<usize>,
    edges: Vec<usize>,
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

/// Moves disconnected parts into rows without changing the layout inside a part.
pub fn wrap_components(nodes: &mut [CNode], edges: &mut [CEdge], width: f64, gap: f64) -> Option<CompoundResult> {
    let mut parent: Vec<usize> = (0..nodes.len()).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let join = |parent: &mut [usize], a: usize, b: usize| {
        let (ra, rb) = (root(parent, a), root(parent, b));
        parent[ra] = rb;
    };
    for i in 0..nodes.len() {
        if nodes[i].parent >= 0 {
            join(&mut parent, i, nodes[i].parent as usize);
        }
    }
    for e in edges.iter() {
        join(&mut parent, e.src, e.dst);
    }

    // Map<number, Part>, in insertion order.
    let mut keys: Vec<usize> = Vec::new();
    let mut parts: Vec<Part> = Vec::new();
    let include = |p: &mut Part, x: f64, y: f64, w: f64, h: f64| {
        p.left = p.left.min(x - w / 2.0);
        p.top = p.top.min(y - h / 2.0);
        p.right = p.right.max(x + w / 2.0);
        p.bottom = p.bottom.max(y + h / 2.0);
    };
    let part_of = |parent: &mut [usize], i: usize, keys: &mut Vec<usize>, parts: &mut Vec<Part>| -> usize {
        let id = root(parent, i);
        match keys.iter().position(|&k| k == id) {
            Some(k) => k,
            None => {
                keys.push(id);
                parts.push(Part { nodes: Vec::new(), edges: Vec::new(), left: f64::INFINITY, top: f64::INFINITY, right: f64::NEG_INFINITY, bottom: f64::NEG_INFINITY });
                parts.len() - 1
            }
        }
    };
    for i in 0..nodes.len() {
        let k = part_of(&mut parent, i, &mut keys, &mut parts);
        let c = &nodes[i];
        parts[k].nodes.push(i);
        include(&mut parts[k], c.x, c.y, c.w, c.h);
    }
    if parts.len() < 2 {
        return None;
    }
    for (ei, e) in edges.iter().enumerate() {
        let k = part_of(&mut parent, e.src, &mut keys, &mut parts);
        let part = &mut parts[k];
        part.edges.push(ei);
        let mut i = 0;
        while i < e.route.len() {
            include(part, e.route[i], e.route[i + 1], 0.0, 0.0);
            i += 3;
        }
        if e.label_w > 0.0 {
            include(part, e.label_x, e.label_y, e.label_w, e.label_h);
        }
    }

    let (mut x, mut y, mut row_height, mut right, mut from, mut to) = (0.0, 0.0, 0.0f64, 0.0f64, f64::INFINITY, f64::NEG_INFINITY);
    let mut shifts: Vec<f64> = Vec::new();
    for part in &parts {
        let (w, h) = (part.right - part.left, part.bottom - part.top);
        if x > 0.0 && x + w > width {
            y += row_height + gap;
            x = 0.0;
            row_height = 0.0;
        }
        shifts.push(x - part.left);
        shifts.push(y - part.top);
        from = from.min(part.left);
        to = to.max(part.right);
        right = right.max(x + w);
        row_height = row_height.max(h);
        x += w + gap;
    }
    if right >= to - from {
        return None;
    }
    for (k, part) in parts.iter().enumerate() {
        let (ox, oy) = (shifts[2 * k], shifts[2 * k + 1]);
        for &i in &part.nodes {
            nodes[i].x += ox;
            nodes[i].y += oy;
        }
        for &ei in &part.edges {
            let e = &mut edges[ei];
            let mut i = 0;
            while i < e.route.len() {
                e.route[i] += ox;
                e.route[i + 1] += oy;
                i += 3;
            }
            e.label_x += ox;
            e.label_y += oy;
        }
    }
    Some(CompoundResult { width: right, height: y + row_height })
}
