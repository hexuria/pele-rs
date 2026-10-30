// Port of src/diagrams/class/graph.ts.

use super::db::{ClassDb, LOLLIPOP};
use super::types::RelationEnd;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum NodeShape {
    Rect,
    ClassBox,
    Note,
    Interface,
}

pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub is_group: bool,
    pub shape: NodeShape,
    pub parent_id: Option<String>,
    // Index of the namespace node this one is drawn in, or -1.
    pub parent: i64,
    /// Index into the db's classes.
    pub class_node: Option<usize>,
}

pub struct GraphEdge {
    pub id: String,
    // Indexes into the node list. Ids alone are ambiguous: a class may be named `note0` or `interface0`.
    pub source: i64,
    pub target: i64,
    pub label: Option<String>,
    pub arrow_type_start: &'static str,
    pub arrow_type_end: &'static str,
    pub start_label_right: String,
    pub end_label_left: String,
    /// "solid", "dashed" or "dotted".
    pub pattern: &'static str,
}

pub struct ClassGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

const MARKERS: [&str; 5] = ["aggregation", "extension", "composition", "dependency", "lollipop"];

fn marker(t: RelationEnd) -> &'static str {
    t.and_then(|t| MARKERS.get(t as usize).copied()).unwrap_or("none")
}

/// Resolves a parsed class diagram into the nodes and edges that are drawn. With `hierarchical`
/// off, only namespaces the source declares are kept and each is labeled with its full name.
pub fn build_class_graph(model: &ClassDb, hierarchical: bool) -> ClassGraph {
    let mut nodes: Vec<GraphNode> = Vec::new();
    let mut edges: Vec<GraphEdge> = Vec::new();
    let mut namespace_at: HashMap<String, usize> = HashMap::new();
    let mut class_at: HashMap<String, usize> = HashMap::new();
    let mut interface_at: HashMap<String, usize> = HashMap::new();

    let mut declared: HashMap<String, Option<String>> = HashMap::new();
    let mut declared_ancestor = |id: Option<&str>| -> Option<String> {
        if hierarchical || id.is_none() {
            return id.map(str::to_string);
        }
        let mut chain: Vec<String> = Vec::new();
        let mut current: Option<String> = id.map(str::to_string);
        let mut found: Option<String> = None;
        while let Some(cur) = current.clone().filter(|c| !c.is_empty()) {
            if let Some(f) = declared.get(&cur) {
                found = f.clone();
                break;
            }
            let Some(ns) = model.namespace(&cur) else { break };
            if ns.explicit {
                found = Some(cur);
                break;
            }
            chain.push(cur);
            current = ns.parent.clone();
        }
        for visited in chain {
            declared.insert(visited, found.clone());
        }
        found
    };

    for ns in &model.namespaces {
        if !hierarchical && !ns.explicit {
            continue;
        }
        namespace_at.insert(ns.id.clone(), nodes.len());
        nodes.push(GraphNode {
            id: ns.id.clone(),
            label: if hierarchical { ns.label.clone() } else { ns.id.clone() },
            is_group: true,
            shape: NodeShape::Rect,
            parent_id: if hierarchical { ns.parent.clone() } else { None },
            parent: -1,
            class_node: None,
        });
    }
    let group = |namespace_at: &HashMap<String, usize>, id: Option<&str>| -> i64 { id.and_then(|i| namespace_at.get(i)).map_or(-1, |&k| k as i64) };
    for k in 0..nodes.len() {
        nodes[k].parent = group(&namespace_at, nodes[k].parent_id.as_deref());
    }

    for (ci, class_node) in model.classes.iter().enumerate() {
        let parent_id = declared_ancestor(class_node.parent.as_deref());
        class_at.insert(class_node.id.clone(), nodes.len());
        nodes.push(GraphNode {
            id: class_node.id.clone(),
            label: class_node.label.clone(),
            is_group: false,
            shape: NodeShape::ClassBox,
            parent: group(&namespace_at, parent_id.as_deref()),
            parent_id,
            class_node: Some(ci),
        });
    }

    for note in &model.notes {
        let parent_id = declared_ancestor(note.parent.as_deref());
        let target = note.class.as_deref().and_then(|c| class_at.get(c).copied());
        if let Some(target) = target {
            edges.push(GraphEdge {
                id: format!("edgeNote{}", note.index),
                source: nodes.len() as i64,
                target: target as i64,
                label: None,
                arrow_type_start: "none",
                arrow_type_end: "none",
                start_label_right: String::new(),
                end_label_left: String::new(),
                pattern: "dotted",
            });
        }
        nodes.push(GraphNode {
            id: note.id.clone(),
            label: note.text.clone(),
            is_group: false,
            shape: NodeShape::Note,
            parent: group(&namespace_at, parent_id.as_deref()),
            parent_id,
            class_node: None,
        });
    }

    // Mermaid leaves every lollipop interface outside the namespaces. Here it stays beside its class.
    for item in &model.interfaces {
        let owner = class_at.get(&item.class_id).copied();
        interface_at.insert(item.id.clone(), nodes.len());
        let parent = owner.map_or(-1, |o| nodes[o].parent);
        nodes.push(GraphNode { id: item.id.clone(), label: item.label.clone(), is_group: false, shape: NodeShape::Interface, parent_id: None, parent, class_node: None });
    }

    for (n, relation) in model.relations.iter().enumerate() {
        let count = n + 1;
        let (type1, type2) = (relation.type1, relation.type2);
        // addRelation swaps in an interface for the lollipop end of a relation with no other marker.
        let start_is_interface = type1 == Some(LOLLIPOP) && type2.is_none();
        let end_is_interface = !start_is_interface && type2 == Some(LOLLIPOP) && type1.is_none();
        let start_at = if start_is_interface { &interface_at } else { &class_at };
        let end_at = if end_is_interface { &interface_at } else { &class_at };
        edges.push(GraphEdge {
            id: format!("id_{}_{}_{}", relation.id1, relation.id2, count),
            source: start_at.get(&relation.id1).map_or(-1, |&k| k as i64),
            target: end_at.get(&relation.id2).map_or(-1, |&k| k as i64),
            label: relation.title.clone(),
            arrow_type_start: marker(type1),
            arrow_type_end: marker(type2),
            start_label_right: if relation.relation_title1 == "none" { String::new() } else { relation.relation_title1.clone() },
            end_label_left: if relation.relation_title2 == "none" { String::new() } else { relation.relation_title2.clone() },
            pattern: if relation.line_type == 1 { "dashed" } else { "solid" },
        });
    }

    ClassGraph { nodes, edges }
}
