// Port of src/diagrams/sankey/layout.ts.

use super::SankeyDb;
use crate::errors::{PeleError, PeleErrorCode};
use crate::js;
use std::cmp::Ordering;

pub struct LayoutOptions {
    pub width: f64,
    pub height: f64,
    pub node_width: f64,
    pub node_padding: f64,
    // Height of a label line; thin nodes are kept at least this far apart where there is room.
    pub label_height: f64,
    // 'justify', 'left', 'right' or 'center'.
    pub alignment: &'static str,
}

#[derive(Clone)]
pub struct PlacedNode {
    pub layer: usize,
    // Total flow through the node, in the diagram's units.
    pub value: f64,
    pub x: f64,
    pub y: f64,
    pub height: f64,
}

#[derive(Clone)]
pub struct PlacedLink {
    pub source: usize,
    pub target: usize,
    pub value: f64,
    pub thickness: f64,
    pub source_y: f64,
    pub target_y: f64,
}

pub struct SankeyLayout {
    pub nodes: Vec<PlacedNode>,
    pub links: Vec<PlacedLink>,
}

const PASSES: i32 = 6;

// A stable sort: upstream's insertion sort for short lists and Array.prototype.sort agree.
fn sort(list: &mut [usize], compare: impl Fn(usize, usize) -> f64) {
    list.sort_by(|&a, &b| {
        let d = compare(a, b);
        if d > 0.0 {
            Ordering::Greater
        } else if d < 0.0 {
            Ordering::Less
        } else {
            Ordering::Equal
        }
    });
}

/// Places nodes in columns by their distance from a source, sizes them by their flow, and then
/// lets each column settle next to its neighbours so that the bands cross less.
pub fn layout_sankey(model: &SankeyDb, options: &LayoutOptions) -> Result<SankeyLayout, PeleError> {
    let n = model.nodes.len();
    let mut links: Vec<PlacedLink> = model
        .links
        .iter()
        .map(|l| PlacedLink { source: l.source, target: l.target, value: if l.value.is_finite() && l.value > 0.0 { l.value } else { 0.0 }, thickness: 0.0, source_y: 0.0, target_y: 0.0 })
        .collect();
    let mut outgoing: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut incoming: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (k, link) in links.iter().enumerate() {
        outgoing[link.source].push(k);
        incoming[link.target].push(k);
    }

    // Longest distance from a source, in topological order; what is left over lies on a cycle.
    let mut depth = vec![0i32; n];
    let mut pending = vec![0i32; n];
    let mut order: Vec<usize> = Vec::new();
    for i in 0..n {
        pending[i] = incoming[i].len() as i32;
        if pending[i] == 0 {
            order.push(i);
        }
    }
    let mut q = 0;
    while q < order.len() {
        let i = order[q];
        for &k in &outgoing[i] {
            let t = links[k].target;
            if depth[t] < depth[i] + 1 {
                depth[t] = depth[i] + 1;
            }
            pending[t] -= 1;
            if pending[t] == 0 {
                order.push(t);
            }
        }
        q += 1;
    }
    if order.len() < n {
        let mut stuck = 0;
        while pending[stuck] == 0 {
            stuck += 1;
        }
        let mut seen = vec![false; n];
        while !seen[stuck] {
            seen[stuck] = true;
            for &k in &incoming[stuck] {
                if pending[links[k].source] > 0 {
                    stuck = links[k].source;
                    break;
                }
            }
        }
        return Err(PeleError::typed(format!("Sankey diagrams cannot have circular links, and \"{}\" is part of one.", model.nodes[stuck].id), PeleErrorCode::Semantic, "sankey"));
    }
    let mut layers = 1usize;
    for i in 0..n {
        if depth[i] as usize + 1 > layers {
            layers = depth[i] as usize + 1;
        }
    }
    let mut reach = vec![0i32; n];
    for q in (0..n).rev() {
        let i = order[q];
        for &k in &outgoing[i] {
            if reach[i] < reach[links[k].target] + 1 {
                reach[i] = reach[links[k].target] + 1;
            }
        }
    }

    // Flows are scaled to the largest one, so that sums of huge values stay finite.
    let mut largest = 0.0;
    for link in &links {
        if link.value > largest {
            largest = link.value;
        }
    }
    let unit = if largest > 0.0 { 1.0 / largest } else { 0.0 };

    let mut nodes: Vec<PlacedNode> = Vec::with_capacity(n);
    let mut flow = vec![0.0; n];
    let mut columns: Vec<Vec<usize>> = vec![Vec::new(); layers];
    let step = if layers > 1 { (options.width - options.node_width) / (layers as f64 - 1.0) } else { 0.0 };
    for i in 0..n {
        let layer: i64 = match options.alignment {
            "left" => depth[i] as i64,
            "right" => layers as i64 - 1 - reach[i] as i64,
            "center" => {
                let mut layer = depth[i] as i64;
                if incoming[i].is_empty() && !outgoing[i].is_empty() {
                    layer = layers as i64;
                    for &k in &outgoing[i] {
                        if (depth[links[k].target] as i64) - 1 < layer {
                            layer = depth[links[k].target] as i64 - 1;
                        }
                    }
                }
                layer
            }
            _ => {
                if !outgoing[i].is_empty() {
                    depth[i] as i64
                } else {
                    layers as i64 - 1
                }
            }
        };
        let layer = layer as usize;
        let (mut into, mut out, mut value, mut value_out) = (0.0, 0.0, 0.0, 0.0);
        for &k in &incoming[i] {
            into += links[k].value * unit;
            value += links[k].value;
        }
        for &k in &outgoing[i] {
            out += links[k].value * unit;
            value_out += links[k].value;
        }
        flow[i] = f64::max(into, out);
        nodes.push(PlacedNode { layer, value: f64::max(value, value_out), x: layer as f64 * step, y: 0.0, height: 0.0 });
        columns[layer].push(i);
    }

    let mut tallest = 1;
    for column in &columns {
        if column.len() > tallest {
            tallest = column.len();
        }
    }
    let gap = if tallest > 1 { options.node_padding.min(options.height / (tallest as f64 - 1.0)) } else { options.node_padding };
    let mut scale = f64::INFINITY;
    for column in &columns {
        let mut sum = 0.0;
        for &i in column {
            sum += flow[i];
        }
        if sum > 0.0 {
            scale = scale.min((options.height - (column.len() as f64 - 1.0) * gap) / sum);
        }
    }
    if !scale.is_finite() || scale < 0.0 {
        scale = 0.0;
    }

    // A thin node still needs room for its label.
    let mut claim = 0f64.max(options.label_height - gap);
    let fits = |s: f64, claim: f64| -> bool {
        for column in &columns {
            let mut extent = (column.len() as f64 - 1.0) * gap;
            for &i in column {
                extent += (flow[i] * s).max(claim);
            }
            if extent > options.height + 1e-6 {
                return false;
            }
        }
        true
    };
    if claim > 0.0 && !fits(scale, claim) {
        if fits(0.0, claim) {
            let mut low = 0.0;
            for _ in 0..40 {
                let mid = (low + scale) / 2.0;
                if fits(mid, claim) {
                    low = mid;
                } else {
                    scale = mid;
                }
            }
            scale = low;
        } else {
            claim = 0.0;
        }
    }

    // The room a node keeps clear above and below itself.
    let mut margin = vec![0.0; n];
    for link in links.iter_mut() {
        link.thickness = link.value * unit * scale;
    }
    for column in &columns {
        let mut y = 0.0;
        for &i in column {
            nodes[i].height = flow[i] * scale;
            margin[i] = 0f64.max(claim - nodes[i].height) / 2.0;
            nodes[i].y = y + margin[i];
            y += nodes[i].height + 2.0 * margin[i] + gap;
        }
        // Spare room is shared out between the nodes of a column.
        let spare = (options.height - y + gap) / (column.len() as f64 + 1.0);
        for (k, &i) in column.iter().enumerate() {
            nodes[i].y += spare * (k as f64 + 1.0);
        }
    }

    let mut source_offset = vec![0.0; links.len()];
    let mut target_offset = vec![0.0; links.len()];

    fn center(nodes: &[PlacedNode], i: usize) -> f64 {
        nodes[i].y + nodes[i].height / 2.0
    }
    fn stack_out(i: usize, nodes: &[PlacedNode], links: &[PlacedLink], outgoing: &mut [Vec<usize>], source_offset: &mut [f64]) {
        let mut y = 0.0;
        sort(&mut outgoing[i], |a, b| js::or(center(nodes, links[a].target) - center(nodes, links[b].target), || a as f64 - b as f64));
        for &k in &outgoing[i] {
            source_offset[k] = y + links[k].thickness / 2.0;
            y += links[k].thickness;
        }
    }
    fn stack_in(i: usize, nodes: &[PlacedNode], links: &[PlacedLink], incoming: &mut [Vec<usize>], target_offset: &mut [f64]) {
        let mut y = 0.0;
        sort(&mut incoming[i], |a, b| js::or(center(nodes, links[a].source) - center(nodes, links[b].source), || a as f64 - b as f64));
        for &k in &incoming[i] {
            target_offset[k] = y + links[k].thickness / 2.0;
            y += links[k].thickness;
        }
    }
    struct St<'a> {
        nodes: &'a mut Vec<PlacedNode>,
        links: &'a [PlacedLink],
        outgoing: &'a mut Vec<Vec<usize>>,
        incoming: &'a mut Vec<Vec<usize>>,
        source_offset: &'a mut Vec<f64>,
        target_offset: &'a mut Vec<f64>,
        margin: &'a [f64],
        gap: f64,
        height: f64,
    }
    fn stack_all(s: &mut St, n: usize) {
        for i in 0..n {
            stack_out(i, s.nodes, s.links, s.outgoing, s.source_offset);
            stack_in(i, s.nodes, s.links, s.incoming, s.target_offset);
        }
    }
    // Moves each node of a column towards where its bands would run level, then restores the
    // order and the gaps.
    fn settle(s: &mut St, column: &mut Vec<usize>, pull: f64, forward: bool) {
        for &i in column.iter() {
            let list_len = if forward { s.incoming[i].len() } else { s.outgoing[i].len() };
            if list_len == 0 {
                continue;
            }
            if forward {
                stack_in(i, s.nodes, s.links, s.incoming, s.target_offset);
            } else {
                stack_out(i, s.nodes, s.links, s.outgoing, s.source_offset);
            }
            let list = if forward { &s.incoming[i] } else { &s.outgoing[i] };
            let (mut weighted, mut weight, mut plain) = (0.0, 0.0, 0.0);
            for &k in list {
                let link = &s.links[k];
                let level = if forward { s.nodes[link.source].y + s.source_offset[k] - s.target_offset[k] } else { s.nodes[link.target].y + s.target_offset[k] - s.source_offset[k] };
                weighted += level * link.thickness;
                weight += link.thickness;
                plain += center(s.nodes, if forward { link.source } else { link.target }) - s.nodes[i].height / 2.0;
            }
            let wanted = if weight > 0.0 { weighted / weight } else { plain / list.len() as f64 };
            s.nodes[i].y += (wanted - s.nodes[i].y) * pull;
        }
        let nodes: &Vec<PlacedNode> = s.nodes;
        sort(column, |a, b| js::or(center(nodes, a) - center(nodes, b), || a as f64 - b as f64));
        let mut y = 0.0;
        for &i in column.iter() {
            if s.nodes[i].y - s.margin[i] < y {
                s.nodes[i].y = y + s.margin[i];
            }
            y = s.nodes[i].y + s.nodes[i].height + s.margin[i] + s.gap;
        }
        y = s.height;
        for &i in column.iter().rev() {
            if s.nodes[i].y + s.nodes[i].height + s.margin[i] > y {
                s.nodes[i].y = y - s.nodes[i].height - s.margin[i];
            }
            y = s.nodes[i].y - s.margin[i] - s.gap;
        }
    }

    {
        let mut st = St {
            nodes: &mut nodes,
            links: &links,
            outgoing: &mut outgoing,
            incoming: &mut incoming,
            source_offset: &mut source_offset,
            target_offset: &mut target_offset,
            margin: &margin,
            gap,
            height: options.height,
        };
        stack_all(&mut st, n);
        for pass in 0..PASSES {
            let pull = 0.99f64.powf(pass as f64);
            for l in (0..layers.saturating_sub(1)).rev() {
                settle(&mut st, &mut columns[l], pull, false);
            }
            stack_all(&mut st, n);
            for l in 1..layers {
                settle(&mut st, &mut columns[l], pull, true);
            }
            stack_all(&mut st, n);
        }
    }
    for k in 0..links.len() {
        links[k].source_y = nodes[links[k].source].y + source_offset[k];
        links[k].target_y = nodes[links[k].target].y + target_offset[k];
    }
    Ok(SankeyLayout { nodes, links })
}
