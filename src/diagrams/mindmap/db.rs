// Port of src/diagrams/mindmap/db.ts.

use crate::diagrams::common::outline::OutlineDb;
use crate::errors::{PeleError, PeleErrorCode};

pub struct MindmapNode {
    pub id: usize,
    pub node_id: String,
    pub level: i64,
    pub descr: String,
    pub type_: u32,
    pub children: Vec<usize>,
    pub is_root: bool,
    // Index of the first-level branch the node belongs to, wrapping after eleven as in Mermaid.
    pub section: Option<usize>,
    pub class: Option<String>,
    pub icon: Option<String>,
}

const SECTIONS: usize = 11;

#[derive(Default)]
pub struct MindmapDb {
    pub title: Option<String>,
    // In source order, which is depth-first. The first node is the root.
    pub nodes: Vec<MindmapNode>,
    base_level: i64,
    // The chain of nodes that can still take children, with strictly increasing levels.
    open: Vec<usize>,
}

impl OutlineDb for MindmapDb {
    fn add_node(&mut self, level: usize, id: &str, descr: &str, ty: u32, _shape_data: Option<&str>) -> Result<(), PeleError> {
        let is_root = self.nodes.is_empty();
        let mut level = level as i64;
        if is_root {
            self.base_level = level;
        }
        level -= self.base_level;
        // Mermaid's parent is the last node with a smaller indentation, wherever it is in the tree.
        let mut depth = self.open.len();
        while depth > 0 && self.nodes[self.open[depth - 1]].level >= level {
            depth -= 1;
        }
        let parent = if depth > 0 { Some(self.open[depth - 1]) } else { None };
        if parent.is_none() && !is_root {
            return Err(PeleError::typed(format!("There can be only one root. No parent could be found for (\"{}\")", descr), PeleErrorCode::Semantic, "mindmap"));
        }
        let id_ = self.nodes.len();
        let mut node = MindmapNode { id: id_, node_id: id.to_string(), level, descr: descr.to_string(), type_: ty, children: Vec::new(), is_root, section: None, class: None, icon: None };
        if let Some(p) = parent {
            let pn = &self.nodes[p];
            node.section = if pn.is_root { Some(pn.children.len() % SECTIONS) } else { pn.section };
            self.nodes[p].children.push(id_);
        }
        self.open.truncate(depth);
        self.open.push(id_);
        self.nodes.push(node);
        Ok(())
    }

    // Mermaid throws a TypeError when a decoration comes before any node. Here it is ignored.
    fn decorate_node(&mut self, icon: Option<&str>, class: Option<&str>) -> Result<(), PeleError> {
        let Some(node) = self.nodes.last_mut() else { return Ok(()) };
        if let Some(i) = icon.filter(|i| !i.is_empty()) {
            node.icon = Some(i.to_string());
        }
        if let Some(c) = class.filter(|c| !c.is_empty()) {
            node.class = Some(c.to_string());
        }
        Ok(())
    }
}
