// Port of src/diagrams/er/graph.ts.

use super::db::ErDb;
use super::types::ErAttribute;
use std::collections::{HashMap, HashSet};

pub struct ErGraphNode {
    pub id: String,
    pub label: String,
    pub alias: String,
    pub attributes: Vec<ErAttribute>,
    pub is_group: bool,
    pub parent_id: Option<String>,
    pub css_styles: Vec<String>,
    pub css_compiled_styles: Vec<String>,
    pub css_classes: String,
    pub dir: Option<String>,
}

pub struct ErGraphEdge {
    pub id: String,
    pub start: String,
    pub end: String,
    pub label: String,
    pub arrow_type_start: String,
    pub arrow_type_end: String,
    pub dashed: bool,
}

pub struct ErGraph {
    pub nodes: Vec<ErGraphNode>,
    pub edges: Vec<ErGraphEdge>,
}

/// Resolves a parsed ER diagram into what is drawn: subgraphs as groups, entities as nodes with
/// their class styles, and relationships as edges with a marker type at each end.
pub fn build_er_graph(model: &ErDb) -> ErGraph {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let subgraphs = &model.subgraphs;
    let compiled = |class_names: &mut dyn Iterator<Item = &str>| -> Vec<String> {
        let mut out = Vec::new();
        for name in class_names {
            let Some(def) = model.class(name) else { continue };
            for s in &def.styles {
                out.push(crate::js::trim(s).to_string());
            }
            for s in &def.text_styles {
                out.push(crate::js::trim(s).to_string());
            }
        }
        out
    };

    let mut parent_of: HashMap<String, String> = HashMap::new();
    let mut subgraph_ids: HashSet<String> = HashSet::new();
    for sg in subgraphs.iter().rev() {
        subgraph_ids.insert(sg.id.clone());
        for id in &sg.nodes {
            parent_of.insert(id.clone(), sg.id.clone());
        }
    }
    for sg in subgraphs.iter().rev() {
        nodes.push(ErGraphNode {
            id: sg.id.clone(),
            label: sg.title.clone(),
            alias: String::new(),
            attributes: Vec::new(),
            is_group: true,
            parent_id: parent_of.get(&sg.id).cloned(),
            css_styles: sg.css_styles.clone(),
            css_compiled_styles: compiled(&mut sg.classes.iter().map(String::as_str)),
            css_classes: sg.classes.join(" "),
            dir: sg.dir.clone(),
        });
    }

    // A name that is also a subgraph id stands for the subgraph. Mermaid drops such an entity and
    // leaves relationships written before the subgraph pointing at nothing; here they follow it.
    let mut moved: HashMap<String, String> = HashMap::new();
    for (name, entity) in &model.entities {
        if subgraph_ids.contains(name) {
            moved.insert(entity.id.clone(), name.clone());
            continue;
        }
        nodes.push(ErGraphNode {
            id: entity.id.clone(),
            label: entity.label.clone(),
            alias: entity.alias.clone(),
            attributes: entity.attributes.clone(),
            is_group: false,
            parent_id: parent_of.get(name).cloned(),
            css_styles: entity.css_styles.clone(),
            css_compiled_styles: compiled(&mut entity.css_classes.split(' ')),
            css_classes: entity.css_classes.clone(),
            dir: None,
        });
    }

    for (index, r) in model.relationships.iter().enumerate() {
        edges.push(ErGraphEdge {
            id: format!("id_{}_{}_{}", r.entity_a, r.entity_b, index),
            start: moved.get(&r.entity_a).cloned().unwrap_or_else(|| r.entity_a.clone()),
            end: moved.get(&r.entity_b).cloned().unwrap_or_else(|| r.entity_b.clone()),
            label: r.role_a.clone(),
            arrow_type_start: r.rel_spec.card_b.to_lowercase(),
            arrow_type_end: r.rel_spec.card_a.to_lowercase(),
            dashed: !r.rel_spec.identifying,
        });
    }
    ErGraph { nodes, edges }
}
