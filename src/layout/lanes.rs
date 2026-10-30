// Port of src/layout/lanes.ts: layout for swimlanes. Every top-level group is a lane: a band
// that runs the length of the diagram along the flow. Ranks are shared by all lanes, so a step
// sits level with the steps that happen at the same time in other lanes, and each node stays
// inside its own lane.

use super::compound::{CEdge, CNode, CompoundResult, Dir};
use super::layered::{kind, ledge, lnode, orient, rank, LEdge, LNode, LayeredOptions};
use crate::js;

const LANE_HEAD: f64 = 34.0;
pub const LANE_PAD: f64 = 24.0;
const SWEEPS: usize = 8;
const MAX_DUMMIES: i64 = 20000;
const MAX_HOPS: usize = 10000;
const ROUTE_BUDGET: f64 = 2000000.0;
// Parallel lines closer than this are drawn as one.
const NEAR: f64 = 4.0;
const ROUNDS: usize = 3;
const RELAY: f64 = 8.0;
const TURN: f64 = 20.0;
// The room a lane keeps beside a handoff along its margin, and how much of it is between the
// handoff and the lane's items.
const MARGIN: f64 = 20.0;
const LEG: f64 = 14.0;
const CLOSE: f64 = 3.0;

// Math.sign: 0 stays 0.
fn sign(v: f64) -> f64 {
    if v > 0.0 {
        1.0
    } else if v < 0.0 {
        -1.0
    } else {
        v
    }
}

struct Placed {
    width: f64,
    height: f64,
    routed: Vec<u8>,
    margins: Vec<u8>,
    work: f64,
}

pub fn lane_layout(nodes: &mut [CNode], edges: &mut [CEdge], dir: Dir, opt: &LayeredOptions, lane_pad: f64) -> CompoundResult {
    let n = nodes.len();
    if n == 0 {
        return CompoundResult { width: 0.0, height: 0.0 };
    }
    let sideways = matches!(dir, Dir::LR | Dir::RL);
    let flipped = matches!(dir, Dir::BT | Dir::RL);

    // Lanes in the order they were written. The graph lists groups last-declared first.
    let lanes: Vec<usize> = (0..n).rev().filter(|&i| nodes[i].parent < 0 && nodes[i].is_group).collect();
    let lane_count = lanes.len().max(1);
    let mut lane_slot = vec![-1i32; n];
    for (k, &id) in lanes.iter().enumerate() {
        lane_slot[id] = k as i32;
    }
    let mut has_child = vec![false; n];
    for i in 0..n {
        if nodes[i].parent >= 0 {
            has_child[nodes[i].parent as usize] = true;
        }
    }
    // Room a lane keeps around its items, so that the groups drawn around them stay inside it.
    let mut lane_pads = vec![lane_pad; lane_count];
    let mut end_pad = lane_pad;

    // What gets ranked and placed: nodes, and groups that are empty and so act as nodes.
    let mut item_of = vec![-1i32; n];
    let mut real: Vec<usize> = Vec::new();
    for i in 0..n {
        if lane_slot[i] >= 0 {
            continue;
        }
        if !nodes[i].is_group || !has_child[i] {
            item_of[i] = real.len() as i32;
            real.push(i);
        }
    }
    let along = |c: &CNode| if sideways { c.w } else { c.h };
    let cross = |c: &CNode| if sideways { c.h } else { c.w };

    let mut lnodes: Vec<LNode> = real.iter().map(|&i| lnode(cross(&nodes[i]), along(&nodes[i]), kind::NODE)).collect();
    let mut ledges: Vec<LEdge> = Vec::new();
    let mut edge_of: Vec<usize> = Vec::new();
    let mut step = 1;
    {
        let mut seen = std::collections::HashSet::new();
        for (ei, e) in edges.iter().enumerate() {
            let (a, b) = (item_of[e.src], item_of[e.dst]);
            if a < 0 || b < 0 || a == b {
                continue;
            }
            let key = a.min(b) as i64 * real.len() as i64 + a.max(b) as i64;
            if e.label_w > 0.0 || seen.contains(&key) {
                step = 2;
            }
            seen.insert(key);
            ledges.push(ledge(a as usize, b as usize, e.minlen, if sideways { e.label_h } else { e.label_w }, if sideways { e.label_w } else { e.label_h }));
            edge_of.push(ei);
        }
    }
    orient(&lnodes, &mut ledges);
    rank(&mut lnodes, &ledges, step);

    let m = ledges.len();
    let item_lane: Vec<i32> = real
        .iter()
        .map(|&i| {
            let mut t = i;
            let mut room = lane_pad;
            let mut hops = 0;
            while nodes[t].parent >= 0 && hops < MAX_HOPS {
                t = nodes[t].parent as usize;
                if lane_slot[t] < 0 {
                    room += nodes[t].pad_x.max(nodes[t].pad_top).max(nodes[t].pad_bottom);
                }
                hops += 1;
            }
            let lane = lane_slot[t].max(0) as usize;
            if room > lane_pads[lane] {
                lane_pads[lane] = room;
            }
            if room > end_pad {
                end_pad = room;
            }
            lane as i32
        })
        .collect();

    // Lays the ranked items out and routes the relations. A plain relation is a handoff that
    // takes a simple route of its own, and so keeps no room through the ranks it passes.
    let place = |nodes: &mut [CNode], edges: &mut [CEdge], plain: &[u8], margins: &[u8]| -> Placed {
        let pad: Vec<f64> = lane_pads.iter().enumerate().map(|(lane, &room)| if margins[lane] != 0 { room.max(MARGIN) } else { room }).collect();
        let mut used = vec![0u8; lane_count];
        // Items: the real ones, then a dummy for every rank an edge passes through.
        let mut dummies: i64 = 0;
        let mut span = vec![0i32; m];
        for k in 0..m {
            span[k] = (lnodes[ledges[k].head].rank - lnodes[ledges[k].tail].rank).abs();
            if plain[k] == 0 {
                dummies += span[k] as i64 - 1;
            }
        }
        let direct = dummies > MAX_DUMMIES;
        let total = real.len() + if direct { 0 } else { dummies.max(0) as usize };
        let mut rank_of = vec![0i32; total];
        let mut lane_of = vec![0i32; total];
        let mut size = vec![0f64; total];
        let mut depth = vec![0f64; total];
        let mut dummy = vec![false; total];
        let mut max_rank = 0i32;
        for k in 0..real.len() {
            rank_of[k] = lnodes[k].rank;
            lane_of[k] = item_lane[k];
            size[k] = lnodes[k].w;
            depth[k] = lnodes[k].h;
            if rank_of[k] > max_rank {
                max_rank = rank_of[k];
            }
        }
        let mut first_dummy = vec![-1i64; m];
        let mut label_item = vec![-1i64; m];
        let mut links: Vec<usize> = Vec::new();
        {
            let mut next = real.len();
            for k in 0..m {
                let e = &ledges[k];
                let a = if e.reversed { e.head } else { e.tail };
                let b = if e.reversed { e.tail } else { e.head };
                if direct || span[k] < 2 || plain[k] != 0 {
                    links.push(a);
                    links.push(b);
                    continue;
                }
                let (ra, rb) = (rank_of[a], rank_of[b]);
                let mut label_rank = -1;
                if e.label_w > 0.0 {
                    label_rank = ra + ((rb - ra) >> 1);
                    if step == 2 && (label_rank & 1) == 0 {
                        label_rank -= 1;
                    }
                }
                first_dummy[k] = next as i64;
                let mut prev = a;
                for r in ra + 1..rb {
                    let d = next;
                    next += 1;
                    rank_of[d] = r;
                    // The edge crosses from its first lane to its last one evenly along the way.
                    lane_of[d] = lane_of[a] + js::round(((lane_of[b] - lane_of[a]) as f64 * (r - ra) as f64) / (rb - ra) as f64) as i32;
                    dummy[d] = true;
                    if r == label_rank {
                        size[d] = e.label_w;
                        depth[d] = e.label_h;
                        label_item[k] = d as i64;
                    }
                    links.push(prev);
                    links.push(d);
                    prev = d;
                }
                links.push(prev);
                links.push(b);
            }
        }

        let mut up_start = vec![0usize; total + 1];
        let mut down_start = vec![0usize; total + 1];
        let mut k = 0;
        while k < links.len() {
            down_start[links[k] + 1] += 1;
            up_start[links[k + 1] + 1] += 1;
            k += 2;
        }
        for i in 0..total {
            up_start[i + 1] += up_start[i];
            down_start[i + 1] += down_start[i];
        }
        let mut up = vec![0usize; links.len() / 2];
        let mut down = vec![0usize; links.len() / 2];
        {
            let mut uf = up_start[..total].to_vec();
            let mut df = down_start[..total].to_vec();
            let mut k = 0;
            while k < links.len() {
                down[df[links[k]]] = links[k + 1];
                df[links[k]] += 1;
                up[uf[links[k + 1]]] = links[k];
                uf[links[k + 1]] += 1;
                k += 2;
            }
        }

        // Order within each rank: by lane first, then by where the item's neighbours sit.
        let mr = max_rank as usize;
        let mut rows: Vec<Vec<usize>> = vec![Vec::new(); mr + 1];
        for i in 0..total {
            rows[rank_of[i] as usize].push(i);
        }
        let mut pos = vec![0f64; total];
        let mut key = vec![0f64; total];
        let settle = |row: &mut Vec<usize>, key: &[f64], pos: &mut [f64]| {
            row.sort_by(|&a, &b| js::ord(js::or((lane_of[a] - lane_of[b]) as f64, || js::or(key[a] - key[b], || a as f64 - b as f64))));
            for (k, &i) in row.iter().enumerate() {
                pos[i] = k as f64;
            }
        };
        for row in rows.iter_mut() {
            for (k, &i) in row.iter().enumerate() {
                key[i] = k as f64;
            }
            settle(row, &key, &mut pos);
        }
        let mut sweep = 0;
        while sweep < SWEEPS && max_rank > 0 {
            let downward = sweep & 1 == 0;
            for s in 1..=mr {
                let r = if downward { s } else { mr - s };
                let start = if downward { &up_start } else { &down_start };
                let list = if downward { &up } else { &down };
                for &i in &rows[r] {
                    let mut sum = 0.0;
                    let count = start[i + 1] - start[i];
                    for k in start[i]..start[i + 1] {
                        sum += pos[list[k]];
                    }
                    key[i] = if count > 0 { sum / count as f64 } else { pos[i] };
                }
                settle(&mut rows[r], &key, &mut pos);
            }
            sweep += 1;
        }

        // The items of one rank that share a lane sit side by side as a run.
        let gap = |a: usize, b: usize| if dummy[a] || dummy[b] { opt.edge_sep } else { opt.node_sep };
        let runs = |row: &[usize]| -> Vec<(usize, usize)> {
            let mut out = Vec::new();
            let mut from = 0;
            while from < row.len() {
                let mut to = from;
                while to + 1 < row.len() && lane_of[row[to + 1]] == lane_of[row[from]] {
                    to += 1;
                }
                out.push((from, to));
                from = to + 1;
            }
            out
        };
        let run_size = |row: &[usize], from: usize, to: usize| -> f64 {
            let mut s = size[row[from]];
            for k in from..to {
                s += gap(row[k], row[k + 1]) + size[row[k + 1]];
            }
            s
        };

        // Lane breadth: enough for its fullest rank, and for its title.
        let mut breadth = vec![0f64; lane_count];
        let mut head = if !lanes.is_empty() { LANE_HEAD } else { 0.0 };
        for (k, &l) in lanes.iter().enumerate() {
            let lane = &nodes[l];
            breadth[k] = (lane.min_w - 2.0 * pad[k]).max(0.0);
            if lane.pad_top > head {
                head = lane.pad_top;
            }
        }
        for row in &rows {
            for (from, to) in runs(row) {
                let s = run_size(row, from, to);
                let l = lane_of[row[from]] as usize;
                if s > breadth[l] {
                    breadth[l] = s;
                }
            }
        }
        let mut lane_start = vec![0f64; lane_count + 1];
        for k in 0..lane_count {
            lane_start[k + 1] = lane_start[k] + breadth[k] + 2.0 * pad[k];
        }

        // Across the flow: each lane's items start centered, then line up with their neighbours
        // in the same lane as far as the lane and the items beside them allow.
        let mut cc = vec![0f64; total];
        let mut want = vec![0f64; total];
        for row in &rows {
            for (from, to) in runs(row) {
                let lane = lane_of[row[from]] as usize;
                let mut at = (lane_start[lane] + pad[lane] + (lane_start[lane + 1] - pad[lane]) - run_size(row, from, to)) / 2.0;
                for k in from..=to {
                    cc[row[k]] = at + size[row[k]] / 2.0;
                    if k < to {
                        at += size[row[k]] + gap(row[k], row[k + 1]);
                    }
                }
            }
        }
        let mut pass = 0;
        while pass < 4 && max_rank > 0 {
            let downward = pass & 1 == 0;
            for s in 1..=mr {
                let row = &rows[if downward { s } else { mr - s }];
                let start = if downward { &up_start } else { &down_start };
                let list = if downward { &up } else { &down };
                for &i in row {
                    let mut sum = 0.0;
                    let mut count = 0;
                    for k in start[i]..start[i + 1] {
                        if lane_of[list[k]] != lane_of[i] {
                            continue;
                        }
                        sum += cc[list[k]];
                        count += 1;
                    }
                    want[i] = if count > 0 { sum / count as f64 } else { cc[i] };
                }
                for (from, to) in runs(row) {
                    let lane = lane_of[row[from]] as usize;
                    let mut floor = lane_start[lane] + pad[lane];
                    for k in from..=to {
                        let i = row[k];
                        cc[i] = want[i].max(floor + size[i] / 2.0);
                        if k < to {
                            floor = cc[i] + size[i] / 2.0 + gap(i, row[k + 1]);
                        }
                    }
                    let mut ceil = lane_start[lane + 1] - pad[lane];
                    for k in (from..=to).rev() {
                        let i = row[k];
                        cc[i] = cc[i].min(ceil - size[i] / 2.0);
                        if k > from {
                            ceil = cc[i] - size[i] / 2.0 - gap(row[k - 1], i);
                        }
                    }
                }
            }
            pass += 1;
        }

        // Along the flow: one band per rank, after the lane titles.
        let mut band_start = vec![0f64; mr + 1];
        let mut band_end = vec![0f64; mr + 1];
        // The room after a rank is for the relations that turn there or are only as long as
        // they were asked to be. Where just plain handoffs and straight lines pass, the ranks
        // close up, as far as the label of a handoff between them leaves it a line to sit on.
        let full = opt.rank_sep / step as f64;
        let mut sep = vec![full; mr + 1];
        if plain.iter().any(|&on| on == 1) {
            sep.fill(full / CLOSE);
            for k in 0..m {
                let e = &ledges[k];
                let a = if e.reversed { e.head } else { e.tail };
                let b = if e.reversed { e.tail } else { e.head };
                if plain[k] != 0 {
                    if e.label_h <= 0.0 || span[k] > step {
                        continue;
                    }
                    let room = full.min((e.label_h + 2.0 * TURN - depth[a].min(depth[b]) / 2.0) / span[k] as f64);
                    for r in rank_of[a]..rank_of[b] {
                        if room > sep[r as usize] {
                            sep[r as usize] = room;
                        }
                    }
                    continue;
                }
                let d0 = first_dummy[k];
                if d0 < 0 || span[k] as f64 <= step as f64 * e.minlen {
                    for r in rank_of[a]..rank_of[b] {
                        sep[r as usize] = full;
                    }
                    continue;
                }
                let last = d0 + span[k] as i64 - 1;
                for d in d0 - 1..last {
                    let u = if d < d0 { a } else { d as usize };
                    let v = if d + 1 < last { (d + 1) as usize } else { b };
                    if lane_of[u] != lane_of[v] || (cc[u] - cc[v]).abs() >= 0.5 {
                        sep[rank_of[u] as usize] = full;
                    }
                }
            }
        }
        let mut at = head + end_pad;
        for r in 0..=mr {
            let mut d = 0.0;
            for &i in &rows[r] {
                if depth[i] > d {
                    d = depth[i];
                }
            }
            band_start[r] = at;
            band_end[r] = at + d;
            at += d + if r == mr { 0.0 } else { sep[r] };
        }
        let length = at + end_pad;
        let breadth_total = lane_start[lane_count];
        let flip = |a: f64| if flipped { head + length - a } else { a };
        let px = |c: f64, a: f64| if sideways { flip(a) } else { c };
        let py = |c: f64, a: f64| if sideways { c } else { flip(a) };
        let axis = if sideways { 1.0 } else { 0.0 };

        for k in 0..real.len() {
            let a = (band_start[rank_of[k] as usize] + band_end[rank_of[k] as usize]) / 2.0;
            let node = &mut nodes[real[k]];
            node.x = px(cc[k], a);
            node.y = py(cc[k], a);
        }
        for (k, &l) in lanes.iter().enumerate() {
            let c = (lane_start[k] + lane_start[k + 1]) / 2.0;
            let s = lane_start[k + 1] - lane_start[k];
            let lane = &mut nodes[l];
            lane.x = if sideways { length / 2.0 } else { c };
            lane.y = if sideways { c } else { length / 2.0 };
            lane.w = if sideways { length } else { s };
            lane.h = if sideways { s } else { length };
            lane.pad_top = head;
        }

        // Groups inside a lane are drawn around their members, deepest first.
        let mut depth_of = vec![0i32; n];
        let mut inner: Vec<usize> = Vec::new();
        for i in 0..n {
            if !nodes[i].is_group || lane_slot[i] >= 0 || item_of[i] >= 0 {
                continue;
            }
            let mut d = 0;
            let mut p = nodes[i].parent;
            while p >= 0 && (d as usize) < MAX_HOPS {
                d += 1;
                p = nodes[p as usize].parent;
            }
            depth_of[i] = d;
            inner.push(i);
        }
        inner.sort_by(|&a, &b| (depth_of[b] - depth_of[a]).cmp(&0).then(a.cmp(&b)));
        let mut bx = vec![[f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY]; n];
        let grow = |bx: &mut [[f64; 4]], g: usize, c: &CNode| {
            let b = &mut bx[g];
            if c.x - c.w / 2.0 < b[0] {
                b[0] = c.x - c.w / 2.0;
            }
            if c.y - c.h / 2.0 < b[1] {
                b[1] = c.y - c.h / 2.0;
            }
            if c.x + c.w / 2.0 > b[2] {
                b[2] = c.x + c.w / 2.0;
            }
            if c.y + c.h / 2.0 > b[3] {
                b[3] = c.y + c.h / 2.0;
            }
        };
        for &i in &real {
            let p = nodes[i].parent;
            if p >= 0 && lane_slot[p as usize] < 0 {
                grow(&mut bx, p as usize, &nodes[i]);
            }
        }
        for &g in &inner {
            if bx[g][0] == f64::INFINITY {
                continue;
            }
            let b = bx[g];
            let c = &mut nodes[g];
            let w = (b[2] - b[0] + 2.0 * c.pad_x).max(c.min_w);
            c.x = (b[0] + b[2]) / 2.0;
            c.w = w;
            c.y = (b[1] - c.pad_top + b[3] + c.pad_bottom) / 2.0;
            c.h = b[3] - b[1] + c.pad_top + c.pad_bottom;
            let p = c.parent;
            if p >= 0 && lane_slot[p as usize] < 0 {
                let c = nodes[g].clone();
                grow(&mut bx, p as usize, &c);
            }
        }

        for e in edges.iter_mut() {
            e.route = Vec::new();
        }
        for k in 0..m {
            let e = &ledges[k];
            let a = if e.reversed { e.head } else { e.tail };
            let b = if e.reversed { e.tail } else { e.head };
            let (ra, rb) = (rank_of[a] as usize, rank_of[b] as usize);
            let mut pts: Vec<f64> = vec![cc[a], band_end[ra] - (band_end[ra] - band_start[ra] - depth[a]) / 2.0];
            let d0 = first_dummy[k];
            if d0 >= 0 {
                for d in d0 as usize..(d0 + span[k] as i64 - 1) as usize {
                    let rd = rank_of[d] as usize;
                    pts.push(cc[d]);
                    pts.push(band_start[rd]);
                    if band_end[rd] > band_start[rd] {
                        pts.push(cc[d]);
                        pts.push(band_end[rd]);
                    }
                }
            }
            pts.push(cc[b]);
            pts.push(band_start[rb] + (band_end[rb] - band_start[rb] - depth[b]) / 2.0);
            let mut route: Vec<f64> = Vec::new();
            let mut p = 0;
            while p < pts.len() {
                route.push(px(pts[p], pts[p + 1]));
                route.push(py(pts[p], pts[p + 1]));
                route.push(axis);
                p += 2;
            }
            if e.reversed {
                let (mut i, mut j) = (0usize, route.len() - 3);
                while i < j {
                    route.swap(i, j);
                    route.swap(i + 1, j + 1);
                    i += 3;
                    j -= 3;
                }
            }
            let out = &mut edges[edge_of[k]];
            let label = label_item[k];
            if label >= 0 {
                let rl = rank_of[label as usize] as usize;
                let mid = (band_start[rl] + band_end[rl]) / 2.0;
                out.label_x = px(cc[label as usize], mid);
                out.label_y = py(cc[label as usize], mid);
            } else {
                out.label_x = (route[0] + route[route.len() - 3]) / 2.0;
                out.label_y = (route[1] + route[route.len() - 2]) / 2.0;
            }
            out.route = route;
        }

        // An edge that starts or ends at a lane or a group runs straight to its border.
        for e in edges.iter_mut() {
            if !e.route.is_empty() || e.src == e.dst {
                continue;
            }
            let (a, b) = (&nodes[e.src], &nodes[e.dst]);
            let (ax, ay) = border(a, b);
            let (bxx, byy) = border(b, a);
            let flat = if (bxx - ax).abs() > (byy - ay).abs() { 1.0 } else { 0.0 };
            e.route = vec![ax, ay, flat, bxx, byy, flat];
            e.label_x = (ax + bxx) / 2.0;
            e.label_y = (ay + byy) / 2.0;
        }

        // A handoff can leave towards the other lane, then arrive along the flow (or vice
        // versa). In particular, a decision's return branch must not leave through its incoming
        // side and retrace the path past the preceding step. Try the two single-corner routes,
        // keeping the ranked route when a node or another label blocks them.
        let mut routed = vec![0u8; m];
        let route_points: f64 = edges.iter().map(|e| (e.route.len() / 3) as f64).sum();
        let work = m as f64 * (real.len() as f64 + edges.len() as f64 + route_points);
        if work <= ROUTE_BUDGET {
            let amid = |i: usize| (band_start[rank_of[i] as usize] + band_end[rank_of[i] as usize]) / 2.0;
            let mut labels: Vec<[f64; 2]> = edges.iter().map(|e| [if sideways { e.label_y } else { e.label_x }, flip(if sideways { e.label_x } else { e.label_y })]).collect();
            #[allow(clippy::too_many_arguments)]
            fn overlaps(c0: f64, a0: f64, c1: f64, a1: f64, lc: f64, la: f64, cw: f64, ah: f64) -> bool {
                c1 > lc - cw / 2.0 && c0 < lc + cw / 2.0 && a1 > la - ah / 2.0 && a0 < la + ah / 2.0
            }
            let clear = |edges: &[CEdge], labels: &[[f64; 2]], c0: f64, a0: f64, c1: f64, a1: f64, src: i64, dst: i64, ei: usize| -> bool {
                for i in 0..real.len() {
                    if i as i64 != src && i as i64 != dst && overlaps(c0, a0, c1, a1, cc[i], amid(i), size[i] + 8.0, depth[i] + 8.0) {
                        return false;
                    }
                }
                for (i, e) in edges.iter().enumerate() {
                    // A step's own loop keeps its label inside the step's box, and has no place here.
                    if i != ei
                        && e.label_w > 0.0
                        && e.src != e.dst
                        && overlaps(c0, a0, c1, a1, labels[i][0], labels[i][1], if sideways { e.label_h } else { e.label_w }, if sideways { e.label_w } else { e.label_h })
                    {
                        return false;
                    }
                }
                true
            };
            // How many other relations have a stretch inside a box. A stretch's whole box
            // counts, including its curve.
            let crossings = |edges: &[CEdge], x0: f64, y0: f64, x1: f64, y1: f64, ei: usize| -> f64 {
                let mut count = 0.0;
                for (i, e) in edges.iter().enumerate() {
                    if i == ei {
                        continue;
                    }
                    let route = &e.route;
                    let mut j = 3;
                    while j < route.len() {
                        if route[j - 3].max(route[j]) > x0 && route[j - 3].min(route[j]) < x1 && route[j - 2].max(route[j + 1]) > y0 && route[j - 2].min(route[j + 1]) < y1 {
                            count += 1.0;
                            break;
                        }
                        j += 3;
                    }
                }
                count
            };
            let meets = |edges: &[CEdge], x: f64, y: f64, ei: usize| -> f64 {
                let mut count = 0.0;
                for (i, e) in edges.iter().enumerate() {
                    let route = &e.route;
                    if i == ei || route.is_empty() {
                        continue;
                    }
                    let last = route.len() - 3;
                    if ((route[0] - x).abs() < 1.0 && (route[1] - y).abs() < 1.0) || ((route[last] - x).abs() < 1.0 && (route[last + 1] - y).abs() < 1.0) {
                        count += 1.0;
                    }
                }
                count
            };
            // Two relations drawn along one line read as a single path. Those that leave the
            // same step or arrive at the same step may share a trunk; any other pair must not.
            let retraces = |edges: &[CEdge], x0: f64, y0: f64, x1: f64, y1: f64, ei: usize| -> bool {
                let upright = (x1 - x0).abs() < 0.5;
                let (lo, hi) = if upright { (y0.min(y1), y0.max(y1)) } else { (x0.min(x1), x0.max(x1)) };
                let line = if upright { x0 } else { y0 };
                for (i, other) in edges.iter().enumerate() {
                    if i == ei || other.src == edges[ei].src || other.dst == edges[ei].dst {
                        continue;
                    }
                    let route = &other.route;
                    let (o_u, o_v) = if upright { (0, 1) } else { (1, 0) };
                    let mut j = 3;
                    while j < route.len() {
                        let (u0, u1) = (route[j - 3 + o_u], route[j + o_u]);
                        if (u0 - line).abs() >= NEAR || (u1 - line).abs() >= NEAR {
                            j += 3;
                            continue;
                        }
                        let (v0, v1) = (route[j - 3 + o_v], route[j + o_v]);
                        if hi.min(v0.max(v1)) - lo.max(v0.min(v1)) > 1.0 {
                            return true;
                        }
                        j += 3;
                    }
                }
                false
            };
            for k in 0..m {
                let e = &ledges[k];
                let ei = edge_of[k];
                let (a, b) = (e.tail, e.head);
                if lane_of[a] == lane_of[b] || (cc[b] - cc[a]).abs() < 1.0 {
                    continue;
                }
                let (ac, bc, aa, ba) = (cc[a], cc[b], amid(a), amid(b));
                let (dc, da) = (sign(bc - ac), sign(ba - aa));
                let attempt = |edges: &mut [CEdge], labels: &mut Vec<[f64; 2]>, pts: &[f64], margin: f64| -> bool {
                    let mut p = 2;
                    while p < pts.len() {
                        if !clear(
                            edges,
                            labels,
                            pts[p - 2].min(pts[p]) - margin,
                            pts[p - 1].min(pts[p + 1]) - margin,
                            pts[p - 2].max(pts[p]) + margin,
                            pts[p - 1].max(pts[p + 1]) + margin,
                            a as i64,
                            b as i64,
                            ei,
                        ) {
                            return false;
                        }
                        if retraces(edges, px(pts[p - 2], pts[p - 1]), py(pts[p - 2], pts[p - 1]), px(pts[p], pts[p + 1]), py(pts[p], pts[p + 1]), ei) {
                            return false;
                        }
                        p += 2;
                    }
                    let (mut lc, mut la, mut longest) = (0.0, 0.0, -1.0);
                    let (out_w, out_h) = (edges[ei].label_w, edges[ei].label_h);
                    if out_w > 0.0 {
                        let (cw, ah) = (e.label_w, e.label_h);
                        let mut p = 2;
                        while p < pts.len() {
                            let (c2, ca) = ((pts[p - 2] + pts[p]) / 2.0, (pts[p - 1] + pts[p + 1]) / 2.0);
                            let (across, alng) = ((pts[p] - pts[p - 2]).abs(), (pts[p + 1] - pts[p - 1]).abs());
                            let run = across + alng;
                            p += 2;
                            if run <= longest || (if across > 0.0 { across < cw + 8.0 } else { alng < ah + 8.0 }) {
                                continue;
                            }
                            if c2 - cw / 2.0 < 0.0 || c2 + cw / 2.0 > breadth_total || ca - ah / 2.0 < 0.0 || ca + ah / 2.0 > length {
                                continue;
                            }
                            // Include the ends here: the label must clear both shapes it connects.
                            if !clear(edges, labels, c2 - cw / 2.0, ca - ah / 2.0, c2 + cw / 2.0, ca + ah / 2.0, -1, -1, ei) {
                                continue;
                            }
                            // A label moved onto a handoff must not cover a return path that keeps
                            // its ranked route. A segment's whole box is kept clear, including its curve.
                            let (lx, ly) = (px(c2, ca), py(c2, ca));
                            if crossings(edges, lx - out_w / 2.0, ly - out_h / 2.0, lx + out_w / 2.0, ly + out_h / 2.0, ei) > 0.0 {
                                continue;
                            }
                            lc = c2;
                            la = ca;
                            longest = run;
                        }
                        if longest < 0.0 {
                            return false;
                        }
                    }
                    // A point short of each corner and one past it keep the turn close to the
                    // corner, so that the line stays under its label and the handoff takes little room.
                    let mut way = vec![pts[0], pts[1]];
                    let mut p = 2;
                    while p < pts.len() {
                        let (dx, dy) = (pts[p] - pts[p - 2], pts[p + 1] - pts[p - 1]);
                        let run = dx.abs() + dy.abs();
                        let first = if p > 2 { TURN } else { 0.0 };
                        let last = if p + 2 < pts.len() { TURN } else { 0.0 };
                        if run > first + last + 1.0 {
                            if first != 0.0 {
                                way.push(pts[p - 2] + (dx / run) * first);
                                way.push(pts[p - 1] + (dy / run) * first);
                            }
                            if last != 0.0 {
                                way.push(pts[p] - (dx / run) * last);
                                way.push(pts[p + 1] - (dy / run) * last);
                            }
                        }
                        way.push(pts[p]);
                        way.push(pts[p + 1]);
                        p += 2;
                    }
                    let mut route = Vec::new();
                    let mut p = 0;
                    while p < way.len() {
                        let crosswise = if p == 0 { way[0] != way[2] } else { way[p] != way[p - 2] };
                        route.push(px(way[p], way[p + 1]));
                        route.push(py(way[p], way[p + 1]));
                        route.push(if crosswise { 1.0 - axis } else { axis });
                        p += 2;
                    }
                    let out = &mut edges[ei];
                    out.route = route;
                    if out_w > 0.0 {
                        out.label_x = px(lc, la);
                        out.label_y = py(lc, la);
                        labels[ei] = [lc, la];
                    }
                    true
                };
                if attempt(edges, &mut labels, &[ac + dc * size[a] / 2.0, aa, bc, aa, bc, ba - da * depth[b] / 2.0], 2.0) {
                    routed[k] = 1;
                    continue;
                }
                if attempt(edges, &mut labels, &[ac, aa + da * depth[a] / 2.0, ac, ba, bc - dc * size[b] / 2.0, ba], 2.0) {
                    routed[k] = 1;
                    continue;
                }
                // With both corners blocked, as for a return past the steps between, the handoff
                // goes around: out to the margin of a lane, along it, and back in level with its
                // end. It takes the margin that crosses the fewest other relations and shares the
                // fewest ends with them, then the shortest way.
                if work > ROUTE_BUDGET / RELAY {
                    continue;
                }
                let mut around: Vec<(Vec<f64>, f64, usize)> = Vec::new();
                let pair = if lane_of[a] < lane_of[b] { [lane_of[b], lane_of[a]] } else { [lane_of[a], lane_of[b]] };
                for lane in pair {
                    let lane = lane as usize;
                    // A lane that keeps no margin yet has less room there, and the handoff stays inside it.
                    let inset = (pad[lane] - LEG).max((pad[lane] / 2.0).min(MARGIN - LEG));
                    for ch in [lane_start[lane] + inset, lane_start[lane + 1] - inset] {
                        if (ch - ac).abs() < size[a] / 2.0 + 4.0 || (ch - bc).abs() < size[b] / 2.0 + 4.0 {
                            continue;
                        }
                        let pts = vec![ac + sign(ch - ac) * size[a] / 2.0, aa, ch, aa, ch, ba, bc + sign(ch - bc) * size[b] / 2.0, ba];
                        let mut cost = (meets(edges, px(pts[0], pts[1]), py(pts[0], pts[1]), ei) + meets(edges, px(pts[6], pts[7]), py(pts[6], pts[7]), ei)) * 1e6;
                        let mut p = 2;
                        while p < pts.len() {
                            let (x0, y0, x1, y1) = (px(pts[p - 2], pts[p - 1]), py(pts[p - 2], pts[p - 1]), px(pts[p], pts[p + 1]), py(pts[p], pts[p + 1]));
                            cost += crossings(edges, x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1), ei) * 1e6 + (x1 - x0).abs() + (y1 - y0).abs();
                            p += 2;
                        }
                        around.push((pts, cost, lane));
                    }
                }
                around.sort_by(|p, q| js::ord(p.1 - q.1));
                for way in &around {
                    if !attempt(edges, &mut labels, &way.0, 1.0) {
                        continue;
                    }
                    routed[k] = 1;
                    used[way.2] = 1;
                    break;
                }
            }
        }

        Placed { width: if sideways { length } else { breadth_total }, height: if sideways { breadth_total } else { length }, routed, margins: used, work }
    };

    let mut plain = vec![0u8; m];
    let mut margins = vec![0u8; lane_count];
    let mut placed = place(nodes, edges, &plain, &margins);
    // A handoff that was given a simple route needs no room kept for it through the ranks it
    // passes, so the lanes are laid out again without it. A handoff that then loses its route
    // goes back to the ranked one. Routing is most of the work, so a large graph is laid out once.
    let mut round = 0;
    while round < ROUNDS && placed.work <= ROUTE_BUDGET / RELAY {
        let next: Vec<u8> = if round == 0 { placed.routed.clone() } else { plain.iter().zip(&placed.routed).map(|(a, b)| a & b).collect() };
        if next == plain && placed.margins == margins {
            break;
        }
        plain = next;
        margins = placed.margins.clone();
        placed = place(nodes, edges, &plain, &margins);
        round += 1;
    }
    if plain.iter().zip(&placed.routed).any(|(&on, &r)| on != 0 && r == 0) {
        placed = place(nodes, edges, &vec![0u8; m], &vec![0u8; lane_count]);
    }
    CompoundResult { width: placed.width, height: placed.height }
}

// The point where the line from one box's center to another's leaves the first box. When the
// other center is inside the box, the point is on the nearest side instead.
fn border(from: &CNode, to: &CNode) -> (f64, f64) {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let hw = (from.w / 2.0).max(0.5);
    let hh = (from.h / 2.0).max(0.5);
    if dx.abs() < hw && dy.abs() < hh {
        let to_side = hw - dx.abs();
        let to_end = hh - dy.abs();
        return if to_side < to_end { (from.x + if dx < 0.0 { -hw } else { hw }, to.y) } else { (to.x, from.y + if dy < 0.0 { -hh } else { hh }) };
    }
    let scale = (if dx != 0.0 { hw / dx.abs() } else { f64::INFINITY }).min(if dy != 0.0 { hh / dy.abs() } else { f64::INFINITY });
    (from.x + dx * scale, from.y + dy * scale)
}
