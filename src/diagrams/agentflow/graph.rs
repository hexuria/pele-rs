// Port of src/diagrams/agentflow/graph.ts and colorSlots.ts: resolves a parsed agentflow into
// the nodes and edges that are drawn: flow membership, collapsed flows, shapes reduced to the
// ones agentflow has, class styles and palette slots. Its diagnostics are not kept.

use super::db::AgentflowDb;
use super::shapes::{normalise_shape, resolve_shape_alias};
use super::types::LabelType;
use crate::util::yaml::Value;
use std::collections::{HashMap, HashSet};

// Two rules, as in Mermaid: a node takes the color of its kind, from a fixed slot, so editing the
// diagram around it never recolors it; containers count through the slots above the kinds, in the
// order they were declared, so a frame does not take the color of a node inside it.
pub const KIND_SLOT: &[(&str, i64)] = &[("tool", 0), ("task", 1), ("decision", 2), ("input", 3), ("refdoc", 4), ("connector", 5), ("action", 6)];
pub const KIND_COUNT: i64 = KIND_SLOT.len() as i64;

pub fn kind_slot(kind: &str) -> Option<i64> {
    KIND_SLOT.iter().find(|(k, _)| *k == kind).map(|(_, s)| *s)
}

fn container_slot(n: i64, palette: i64) -> i64 {
    // At least one, so that a palette shorter than the kinds still gives containers a slot.
    let count = (palette - KIND_COUNT).max(1);
    let slot = KIND_COUNT + n % count;
    if palette > 0 {
        slot % palette
    } else {
        slot
    }
}

pub struct GraphNode {
    pub id: String,
    pub label: Option<String>,
    pub label_type: LabelType,
    pub shape: String,
    pub is_group: bool,
    pub parent_id: Option<String>,
    pub css_styles: Vec<String>,
    pub css_compiled_styles: Vec<String>,
    pub css_classes: String,
    pub dir: Option<String>,
    pub link: Option<String>,
    pub link_target: Option<String>,
    pub tooltip: Option<String>,
    // Palette slot of a container.
    pub color_index: Option<i64>,
    // What a node is, which picks its palette slot.
    pub kind: Option<&'static str>,
}

pub struct GraphEdge {
    pub id: String,
    pub start: String,
    pub end: String,
    pub label: String,
    pub label_type: LabelType,
    pub thickness: Option<String>,
    pub minlen: Option<f64>,
    pub arrow_type_end: String,
    pub css_compiled_styles: Vec<String>,
    pub style: Vec<String>,
    pub curve: Option<String>,
}

pub struct AgentGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

fn collapsed(db: &AgentflowDb, i: usize) -> bool {
    matches!(db.subgraphs[i].metadata.as_ref().and_then(|m| m.get("view")), Some(Value::Str(v)) if v == "collapsed")
}

pub fn build_agent_graph(db: &AgentflowDb, palette: i64, default_curve: Option<&str>) -> AgentGraph {
    let mut nodes: Vec<GraphNode> = Vec::new();
    let mut edges: Vec<GraphEdge> = Vec::new();
    let subgraphs = &db.subgraphs;

    let compiled = |class_names: &[String]| -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for name in class_names {
            if let Some(def) = db.classes.get(name) {
                out.extend(def.styles.iter().cloned());
                out = out.iter().map(|s| crate::js::trim(s).to_string()).collect();
                out.extend(def.text_styles.iter().cloned());
                out = out.iter().map(|s| crate::js::trim(s).to_string()).collect();
            }
        }
        out
    };

    // Everything inside a collapsed flow is hidden, and stands for its outermost collapsed flow.
    // Mermaid walks the whole inside of each collapsed flow, in the order the flows closed, and the
    // last walk to reach a thing wins. A thing has one container, so a later walk can only enter an
    // earlier one through its root: it stops there, and the root's owner is followed afterwards.
    let mut hidden: Vec<String> = Vec::new();
    let mut hidden_set: HashSet<String> = HashSet::new();
    let mut owner: HashMap<String, String> = HashMap::new();
    let mut walked: HashSet<String> = HashSet::new();
    for (si, sg) in subgraphs.iter().enumerate() {
        if !collapsed(db, si) || hidden_set.contains(&sg.id) {
            continue;
        }
        let mut pending = vec![sg.id.clone()];
        while let Some(next) = pending.pop() {
            let Some(members) = db.subgraph(&next).map(|s| &s.nodes) else { continue };
            for child in members {
                if *child == sg.id || hidden_set.contains(child) {
                    continue;
                }
                hidden_set.insert(child.clone());
                hidden.push(child.clone());
                owner.insert(child.clone(), sg.id.clone());
                if !walked.contains(child) {
                    pending.push(child.clone());
                }
            }
        }
        walked.insert(sg.id.clone());
    }
    let mut collapsed_into: HashMap<String, String> = HashMap::new();
    for id in &hidden {
        let mut path = vec![id.clone()];
        let mut top = owner[id].clone();
        loop {
            let known = collapsed_into.get(&top).cloned();
            let up = known.clone().or_else(|| owner.get(&top).cloned());
            let Some(up) = up else { break };
            if known.is_none() {
                path.push(top.clone());
            }
            top = up;
            if known.is_some() {
                break;
            }
        }
        for p in path {
            collapsed_into.insert(p, top.clone());
        }
    }

    let position: HashMap<&str, usize> = subgraphs.iter().enumerate().map(|(i, sg)| (sg.id.as_str(), i)).collect();

    // Two flows can each name the other. The nesting that would close the loop is dropped.
    let mut parent_of: HashMap<String, String> = HashMap::new();
    for i in (0..subgraphs.len()).rev() {
        let sg = &subgraphs[i];
        if hidden_set.contains(&sg.id) {
            continue;
        }
        for id in &sg.nodes {
            // Only a flow handled earlier in this pass can already be above the parent.
            let closes_loop = match position.get(id.as_str()) {
                Some(&at) if at >= i => {
                    let mut current: Option<&str> = Some(&sg.id);
                    let mut hops = 0;
                    let mut found = false;
                    while let Some(c) = current {
                        if hops > subgraphs.len() {
                            break;
                        }
                        if c == id {
                            found = true;
                            break;
                        }
                        current = parent_of.get(c).map(|s| s.as_str());
                        hops += 1;
                    }
                    found
                }
                _ => false,
            };
            if closes_loop {
                continue;
            }
            parent_of.insert(id.clone(), sg.id.clone());
        }
    }

    let mut by_id: HashMap<String, usize> = HashMap::new();
    for i in (0..subgraphs.len()).rev() {
        let sg = &subgraphs[i];
        if hidden_set.contains(&sg.id) {
            continue;
        }
        let is_collapsed = collapsed(db, i);
        by_id.insert(sg.id.clone(), nodes.len());
        nodes.push(GraphNode {
            id: sg.id.clone(),
            label: Some(sg.title.clone()),
            label_type: sg.label_type,
            parent_id: parent_of.get(&sg.id).cloned(),
            css_styles: Vec::new(),
            css_compiled_styles: compiled(&sg.classes),
            css_classes: sg.classes.join(" "),
            shape: if is_collapsed { "collapsedGroup".into() } else { "flowGroup".into() },
            dir: sg.dir.clone(),
            is_group: !is_collapsed,
            link: None,
            link_target: None,
            tooltip: None,
            color_index: None,
            kind: None,
        });
    }

    for vertex in &db.nodes {
        if hidden_set.contains(&vertex.id) {
            continue;
        }
        if let Some(&k) = by_id.get(&vertex.id) {
            let existing = &mut nodes[k];
            existing.css_styles = vertex.styles.clone();
            existing.css_compiled_styles = compiled(&vertex.classes);
            existing.css_classes = vertex.classes.join(" ");
            continue;
        }
        let type_ = vertex.type_.as_deref().map(|t| if t.is_empty() { t } else { resolve_shape_alias(t) });
        let mut classes = vec!["default".to_string(), "node".to_string()];
        classes.extend(vertex.classes.iter().cloned());
        by_id.insert(vertex.id.clone(), nodes.len());
        nodes.push(GraphNode {
            id: vertex.id.clone(),
            label: vertex.text.clone(),
            label_type: vertex.label_type,
            parent_id: parent_of.get(&vertex.id).cloned(),
            css_styles: vertex.styles.clone(),
            css_compiled_styles: compiled(&classes),
            css_classes: format!("default {}", vertex.classes.join(" ")),
            dir: None,
            link: vertex.link.clone(),
            link_target: vertex.link_target.clone(),
            tooltip: db.tooltips.get(&vertex.id).cloned(),
            is_group: false,
            shape: match type_ {
                None | Some("square") => "squareRect".into(),
                Some(t) => t.to_string(),
            },
            color_index: None,
            kind: None,
        });
    }

    for (index, raw) in db.edges.iter().enumerate() {
        let start = collapsed_into.get(&raw.start).cloned().unwrap_or_else(|| raw.start.clone());
        let end = collapsed_into.get(&raw.end).cloned().unwrap_or_else(|| raw.end.clone());
        // An edge between two things inside one collapsed flow has nothing left to connect.
        if start == end && raw.start != raw.end {
            continue;
        }
        let mut styles = db.default_edge_style.clone().unwrap_or_default();
        if let Some(s) = &raw.style {
            styles.extend(s.iter().cloned());
        }
        let arrow = match raw.type_.as_deref() {
            Some("arrow_open") => "none",
            Some("arrow_cross") => "arrow_cross",
            _ => "arrow_point",
        };
        let curve = [raw.interpolate.as_deref(), db.default_interpolate.as_deref(), default_curve].into_iter().flatten().find(|c| !c.is_empty()).map(str::to_string);
        edges.push(GraphEdge {
            id: if raw.id.is_empty() { format!("L_{}_{}_{}", start, end, index) } else { raw.id.clone() },
            start,
            end,
            label: raw.text.clone(),
            label_type: raw.label_type,
            thickness: raw.stroke.clone(),
            minlen: raw.length,
            arrow_type_end: arrow.into(),
            css_compiled_styles: compiled(&raw.classes),
            style: styles,
            curve,
        });
    }

    for node in nodes.iter_mut() {
        if !node.is_group {
            node.shape = normalise_shape(&node.shape).to_string();
        }
    }

    // Containers are numbered in the order they were written: a walk of the nesting, parents first.
    // `subgraphs` itself is in the order the blocks closed, with a nested flow before its parent.
    let mut children: HashMap<&str, Vec<&str>> = HashMap::new();
    for sg in subgraphs {
        if let Some(parent) = parent_of.get(&sg.id) {
            children.entry(parent.as_str()).or_default().push(&sg.id);
        }
    }
    let mut order: HashMap<&str, i64> = HashMap::new();
    for sg in subgraphs {
        if parent_of.contains_key(&sg.id) {
            continue;
        }
        let mut pending = vec![sg.id.as_str()];
        while let Some(id) = pending.pop() {
            if order.contains_key(id) {
                continue;
            }
            order.insert(id, order.len() as i64);
            if let Some(kids) = children.get(id) {
                for k in kids.iter().rev() {
                    pending.push(k);
                }
            }
        }
    }

    let mut fallback = order.len() as i64;
    for node in nodes.iter_mut() {
        // A collapsed container is drawn as a node but keeps its slot, so collapsing one does not
        // recolor the containers after it.
        if node.is_group || node.shape == "collapsedGroup" {
            let n = match order.get(node.id.as_str()) {
                Some(&n) => n,
                None => {
                    fallback += 1;
                    fallback - 1
                }
            };
            node.color_index = Some(container_slot(n, palette));
            continue;
        }
        node.kind = if db.connectors.contains(&node.id) { Some("connector") } else { db.node(&node.id).map(|v| db.vertex_kind(v)) };
    }

    AgentGraph { nodes, edges }
}
