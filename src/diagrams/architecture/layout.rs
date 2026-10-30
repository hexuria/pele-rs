// Port of src/diagrams/architecture/layout.ts: grid placement. Every service and junction takes a
// cell, an edge `a:R -- L:b` puts b in the cell to the right of a, and a group is a solid
// rectangle of cells. Each container (the diagram, then every group) is laid out on its own,
// innermost first, and a finished group is placed in its parent as one block.

use super::{ArchEdge, ArchGroup, ArchLayoutHint, ArchNode};
use std::collections::HashMap;

pub struct Grid {
    // Cell of each node, in the order of the nodes passed in.
    pub col: Vec<i64>,
    pub row: Vec<i64>,
    // Cell rectangle and parent of each group. A group with nothing in it takes one cell.
    pub group_col: Vec<i64>,
    pub group_row: Vec<i64>,
    pub group_cols: Vec<i64>,
    pub group_rows: Vec<i64>,
    pub group_parent: Vec<i64>,
    pub group_empty: Vec<bool>,
    pub node_parent: Vec<i64>,
}

const DX: [i64; 4] = [-1, 1, 0, 0];
const DY: [i64; 4] = [0, 0, -1, 1];

pub fn side_index(side: char) -> usize {
    match side {
        'L' => 0,
        'R' => 1,
        'T' => 2,
        _ => 3,
    }
}

// Blocks up to this many cells are found through the cell map; larger ones are compared one by one.
const SMALL: i64 = 64;

struct State {
    n: usize,
    parent: Vec<usize>,
    x: Vec<i64>,
    y: Vec<i64>,
    w: Vec<i64>,
    h: Vec<i64>,
    placed: Vec<bool>,
    order: Vec<usize>,
    cells: HashMap<(i64, i64), usize>,
    large: Vec<usize>,
    reach: HashMap<(i64, i64, i64, i64, i64, i64), i64>,
    first: usize,
    link_a: Vec<usize>,
    link_b: Vec<usize>,
    side_a: Vec<usize>,
    side_b: Vec<usize>,
    item_a: Vec<usize>,
    item_b: Vec<usize>,
    edge_links: Vec<Option<Vec<usize>>>,
}

impl State {
    fn anchor(&self, node: usize, item: usize) -> (i64, i64) {
        let (mut ax, mut ay) = (0, 0);
        let mut cur = node;
        while cur != item {
            ax += self.x[cur];
            ay += self.y[cur];
            cur = self.n + self.parent[cur];
        }
        (ax, ay)
    }

    fn put(&mut self, item: usize, px: i64, py: i64) {
        self.x[item] = px;
        self.y[item] = py;
        self.placed[item] = true;
        self.order.push(item);
        if self.w[item] * self.h[item] > SMALL {
            self.large.push(item);
            return;
        }
        for cy in py..py + self.h[item] {
            for cx in px..px + self.w[item] {
                self.cells.insert((cx, cy), item);
            }
        }
    }

    fn overlaps(&self, item: usize, rx: i64, ry: i64, rw: i64, rh: i64) -> bool {
        self.x[item] < rx + rw && rx < self.x[item] + self.w[item] && self.y[item] < ry + rh && ry < self.y[item] + self.h[item]
    }

    fn collide(&self, rx: i64, ry: i64, rw: i64, rh: i64) -> Option<usize> {
        if rw * rh > SMALL {
            return self.order[self.first..].iter().copied().find(|&item| self.overlaps(item, rx, ry, rw, rh));
        }
        for cy in ry..ry + rh {
            for cx in rx..rx + rw {
                if let Some(&hit) = self.cells.get(&(cx, cy)) {
                    return Some(hit);
                }
            }
        }
        self.large.iter().copied().find(|&item| self.overlaps(item, rx, ry, rw, rh))
    }

    // How far a block must slide along (sx, sy) to be clear. Each step jumps past what it hit, and
    // the result is kept, because a later block starting from the same place cannot stop sooner.
    fn slide(&mut self, rx: i64, ry: i64, rw: i64, rh: i64, sx: i64, sy: i64) -> i64 {
        let key = (rx, ry, rw, rh, sx, sy);
        let mut k = self.reach.get(&key).copied().unwrap_or(0);
        while let Some(hit) = self.collide(rx + k * sx, ry + k * sy, rw, rh) {
            k = if sx > 0 {
                self.x[hit] + self.w[hit] - rx
            } else if sx < 0 {
                rx + rw - self.x[hit]
            } else if sy > 0 {
                self.y[hit] + self.h[hit] - ry
            } else {
                ry + rh - self.y[hit]
            };
        }
        self.reach.insert(key, k);
        k
    }

    fn follow(&mut self, from: usize, k: usize) {
        let forward = self.item_a[k] == from;
        let to = if forward { self.item_b[k] } else { self.item_a[k] };
        if self.placed[to] {
            return;
        }
        let sp = if forward { self.side_a[k] } else { self.side_b[k] };
        let sq = if forward { self.side_b[k] } else { self.side_a[k] };
        let (pax, pay) = self.anchor(if forward { self.link_a[k] } else { self.link_b[k] }, from);
        let (ax, ay) = self.anchor(if forward { self.link_b[k] } else { self.link_a[k] }, to);
        let mut dx = DX[sp];
        let mut dy = DY[sp];
        let mut shift_x = 0;
        let mut shift_y = 0;
        let facing = DX[sp] + DX[sq] == 0 && DY[sp] + DY[sq] == 0;
        if sp == sq {
            // Like sides: the edge loops around, so the nodes stack across the direction it leaves in.
            dx = if DX[sp] == 0 { 1 } else { 0 };
            dy = 1 - dx;
        } else if !facing {
            // A bend: one step along the side the edge leaves, one step back from the side it enters.
            shift_x = -DX[sq];
            shift_y = -DY[sq];
        }
        let mut bx;
        let mut by;
        if dx != 0 {
            bx = if dx > 0 { self.x[from] + self.w[from] } else { self.x[from] - self.w[to] };
            by = self.y[from] + pay + shift_y - ay;
        } else {
            by = if dy > 0 { self.y[from] + self.h[from] } else { self.y[from] - self.h[to] };
            bx = self.x[from] + pax + shift_x - ax;
        }
        if facing {
            // When several placed nodes reach this one through the same pair of sides, line it up
            // with the middle one, not with whichever happened to be visited first.
            if let Some(list) = self.edge_links[to].clone().filter(|l| l.len() > 1) {
                let mut wanted: Vec<i64> = Vec::new();
                for j in list {
                    let to_is_b = self.item_b[j] == to;
                    let other = if to_is_b { self.item_a[j] } else { self.item_b[j] };
                    if !self.placed[other] || (if to_is_b { self.side_a[j] } else { self.side_b[j] }) != sp || (if to_is_b { self.side_b[j] } else { self.side_a[j] }) != sq {
                        continue;
                    }
                    let (oax, oay) = self.anchor(if to_is_b { self.link_a[j] } else { self.link_b[j] }, other);
                    let at = if dx != 0 { self.y[other] + oay } else { self.x[other] + oax };
                    let (tax, tay) = self.anchor(if to_is_b { self.link_b[j] } else { self.link_a[j] }, to);
                    wanted.push(at - if dx != 0 { tay } else { tax });
                }
                if wanted.len() > 1 {
                    wanted.sort();
                    let mid = wanted[(wanted.len() - 1) >> 1];
                    if dx != 0 {
                        by = mid;
                    } else {
                        bx = mid;
                    }
                }
            }
            // Taken: move sideways, to whichever side is nearer, so the edge can still leave straight.
            let px = if dx == 0 { 1 } else { 0 };
            let py = 1 - px;
            let (wt, ht) = (self.w[to], self.h[to]);
            let forth = self.slide(bx, by, wt, ht, px, py);
            let back = if forth == 0 { 0 } else { self.slide(bx, by, wt, ht, -px, -py) };
            let near = if forth <= back { forth } else { -back };
            bx += px * near;
            by += py * near;
        } else {
            let (wt, ht) = (self.w[to], self.h[to]);
            let far = self.slide(bx, by, wt, ht, dx, dy);
            bx += dx * far;
            by += dy * far;
        }
        self.put(to, bx, by);
    }
}

pub fn place_on_grid(nodes: &[ArchNode], groups: &[ArchGroup], edges: &[ArchEdge], hints: &[ArchLayoutHint]) -> Grid {
    let n = nodes.len();
    let g_count = groups.len();
    let root = g_count;
    let mut node_index: HashMap<&str, usize> = HashMap::new();
    let mut group_index: HashMap<&str, usize> = HashMap::new();
    // Items are nodes (0..N-1) then groups (N..N+G-1). Containers are groups (0..G-1) then the root.
    let mut parent = vec![root; n + g_count];
    let mut depth = vec![0i64; g_count + 1];
    let mut kids: Vec<Vec<usize>> = vec![Vec::new(); g_count + 1];
    for (g, group) in groups.iter().enumerate() {
        group_index.insert(&group.id, g);
        let p = group.in_.as_deref().and_then(|id| group_index.get(id).copied());
        match p {
            Some(p) if p != g => {
                parent[n + g] = p;
                depth[g] = depth[p] + 1;
            }
            _ => depth[g] = 1,
        }
    }
    for (i, node) in nodes.iter().enumerate() {
        node_index.insert(&node.id, i);
        if let Some(p) = node.in_.as_deref().and_then(|id| group_index.get(id).copied()) {
            parent[i] = p;
        }
        kids[parent[i]].push(i);
    }
    for g in 0..g_count {
        kids[parent[n + g]].push(n + g);
    }

    let mut s = State {
        n,
        parent,
        x: vec![0; n + g_count],
        y: vec![0; n + g_count],
        w: vec![1; n + g_count],
        h: vec![1; n + g_count],
        placed: vec![false; n + g_count],
        order: Vec::new(),
        cells: HashMap::new(),
        large: Vec::new(),
        reach: HashMap::new(),
        first: 0,
        link_a: Vec::new(),
        link_b: Vec::new(),
        side_a: Vec::new(),
        side_b: Vec::new(),
        item_a: Vec::new(),
        item_b: Vec::new(),
        edge_links: vec![None; n + g_count],
    };
    let mut hint_links: Vec<Option<Vec<usize>>> = vec![None; n + g_count];

    // A link asks for one node to sit beside another. It belongs to the innermost container that
    // holds both ends, and there it relates the two children of that container the ends are in.
    let add_link = |s: &mut State, a: usize, b: usize, sa: usize, sb: usize, hint: bool, hint_links: &mut Vec<Option<Vec<usize>>>| {
        if a == b {
            return;
        }
        let (mut ia, mut ib) = (a, b);
        let (mut ca, mut cb) = (s.parent[a], s.parent[b]);
        while ca != cb {
            if depth[ca] >= depth[cb] {
                ia = n + ca;
                ca = s.parent[ia];
            } else {
                ib = n + cb;
                cb = s.parent[ib];
            }
        }
        let k = s.link_a.len();
        s.link_a.push(a);
        s.link_b.push(b);
        s.side_a.push(sa);
        s.side_b.push(sb);
        s.item_a.push(ia);
        s.item_b.push(ib);
        let lists = if hint { hint_links } else { &mut s.edge_links };
        lists[ia].get_or_insert_with(Vec::new).push(k);
        lists[ib].get_or_insert_with(Vec::new).push(k);
    };
    for hint in hints {
        for i in 1..hint.members.len() {
            let a = node_index.get(hint.members[i - 1].as_str()).copied();
            let b = node_index.get(hint.members[i].as_str()).copied();
            if let (Some(a), Some(b)) = (a, b) {
                add_link(&mut s, a, b, if hint.row { 1 } else { 3 }, if hint.row { 0 } else { 2 }, true, &mut hint_links);
            }
        }
    }
    // An edge between two like sides says least about where its ends go, so those come last.
    for like_sides in [false, true] {
        for edge in edges {
            if (edge.lhs_dir == edge.rhs_dir) != like_sides {
                continue;
            }
            let a = node_index.get(edge.lhs_id.as_str()).copied();
            let b = node_index.get(edge.rhs_id.as_str()).copied();
            if let (Some(a), Some(b)) = (a, b) {
                add_link(&mut s, a, b, side_index(edge.lhs_dir), side_index(edge.rhs_dir), false, &mut hint_links);
            }
        }
    }

    let layout = |s: &mut State, c: usize| {
        let items = &kids[c];
        if items.is_empty() {
            return;
        }
        s.order.clear();
        let mut starts: Vec<usize> = Vec::new();
        for &start in items {
            if s.placed[start] {
                continue;
            }
            s.first = s.order.len();
            starts.push(s.first);
            s.cells.clear();
            s.large.clear();
            s.reach.clear();
            s.put(start, 0, 0);
            // Declared alignments are followed as soon as a node is placed, ahead of any edge.
            let mut hint_at = s.first;
            let mut edge_at = s.first;
            let mut link_at = 0;
            loop {
                if hint_at < s.order.len() {
                    let from = s.order[hint_at];
                    hint_at += 1;
                    if let Some(list) = hint_links[from].clone() {
                        for k in list {
                            s.follow(from, k);
                        }
                    }
                    continue;
                }
                if edge_at >= s.order.len() {
                    break;
                }
                let from = s.order[edge_at];
                let len = s.edge_links[from].as_ref().map_or(0, |l| l.len());
                if link_at >= len {
                    edge_at += 1;
                    link_at = 0;
                    continue;
                }
                let k = s.edge_links[from].as_ref().unwrap()[link_at];
                link_at += 1;
                s.follow(from, k);
            }
        }
        starts.push(s.order.len());

        // Unconnected parts go side by side, wrapping into rows that keep the whole roughly square.
        let count = starts.len() - 1;
        let mut min_x = vec![0i64; count];
        let mut min_y = vec![0i64; count];
        let mut part_w = vec![0i64; count];
        let mut part_h = vec![0i64; count];
        let mut area = 0i64;
        let mut widest = 0i64;
        for p in 0..count {
            let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
            for i in starts[p]..starts[p + 1] {
                let item = s.order[i];
                x0 = x0.min(s.x[item]);
                y0 = y0.min(s.y[item]);
                x1 = x1.max(s.x[item] + s.w[item]);
                y1 = y1.max(s.y[item] + s.h[item]);
            }
            min_x[p] = x0;
            min_y[p] = y0;
            part_w[p] = x1 - x0;
            part_h[p] = y1 - y0;
            area += part_w[p] * part_h[p];
            widest = widest.max(part_w[p]);
        }
        let limit = widest.max(((area * 2) as f64).sqrt().ceil() as i64);
        let (mut cx, mut cy, mut shelf, mut width) = (0i64, 0i64, 0i64, 0i64);
        for p in 0..count {
            if cx > 0 && cx + part_w[p] > limit {
                cy += shelf;
                cx = 0;
                shelf = 0;
            }
            for i in starts[p]..starts[p + 1] {
                let item = s.order[i];
                s.x[item] += cx - min_x[p];
                s.y[item] += cy - min_y[p];
            }
            cx += part_w[p];
            shelf = shelf.max(part_h[p]);
            width = width.max(cx);
        }
        if c < g_count {
            s.w[n + c] = width;
            s.h[n + c] = cy + shelf;
        }
    };
    for c in (0..g_count).rev() {
        layout(&mut s, c);
    }
    layout(&mut s, root);

    let mut group_col = vec![0i64; g_count];
    let mut group_row = vec![0i64; g_count];
    let mut group_parent = vec![0i64; g_count];
    let mut group_empty = vec![false; g_count];
    for g in 0..g_count {
        let p = s.parent[n + g];
        group_parent[g] = if p == root { -1 } else { p as i64 };
        group_col[g] = s.x[n + g] + if p == root { 0 } else { group_col[p] };
        group_row[g] = s.y[n + g] + if p == root { 0 } else { group_row[p] };
        if kids[g].is_empty() {
            group_empty[g] = true;
        }
    }
    let mut col = vec![0i64; n];
    let mut row = vec![0i64; n];
    let mut node_parent = vec![0i64; n];
    for i in 0..n {
        let p = s.parent[i];
        node_parent[i] = if p == root { -1 } else { p as i64 };
        col[i] = s.x[i] + if p == root { 0 } else { group_col[p] };
        row[i] = s.y[i] + if p == root { 0 } else { group_row[p] };
    }
    Grid { col, row, group_col, group_row, group_cols: s.w[n..].to_vec(), group_rows: s.h[n..].to_vec(), group_parent, group_empty, node_parent }
}
