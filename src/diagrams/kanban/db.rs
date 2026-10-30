// Port of src/diagrams/kanban/db.ts.

use crate::diagrams::common::outline::OutlineDb;
use crate::errors::{PeleError, PeleErrorCode};
use crate::util::yaml::{parse_yaml, Value};

#[derive(Clone, Default)]
pub struct KanbanNode {
    pub id: String,
    pub level: usize,
    pub label: String,
    pub icon: Option<String>,
    pub assigned: Option<String>,
    pub ticket: Option<String>,
    pub priority: Option<String>,
    pub css_classes: Option<String>,
}

pub struct KanbanSection {
    pub node: KanbanNode,
    pub items: Vec<KanbanNode>,
}

#[derive(Clone, Copy)]
enum Last {
    Section(usize),
    Item(usize, usize),
}

#[derive(Default)]
pub struct KanbanDb {
    pub title: Option<String>,
    pub sections: Vec<KanbanSection>,
    last: Option<Last>,
    section_level: usize,
    misplaced: Option<String>,
}

impl OutlineDb for KanbanDb {
    fn add_node(&mut self, level: usize, id: &str, descr: &str, _ty: u32, shape_data: Option<&str>) -> Result<(), PeleError> {
        let mut node = KanbanNode { id: id.to_string(), level, label: descr.to_string(), ..Default::default() };
        if let Some(data) = shape_data {
            let yaml = if data.contains('\n') { format!("{}\n", data) } else { format!("{{\n{}\n}}", data) };
            let doc = parse_yaml(&yaml).map_err(|message| PeleError::typed(format!("Metadata of \"{}\" is not valid YAML. {}", node.id, message), PeleErrorCode::Syntax, "kanban"))?;
            // Mermaid throws a TypeError on metadata that is empty or not a mapping. Here it is ignored.
            if let Value::Map(doc) = doc {
                let text = |key: &str| doc.get(key).filter(|v| v.truthy()).map(|v| v.js_string());
                let shape = text("shape").unwrap_or_default();
                if shape != shape.to_lowercase() || shape.contains('_') {
                    return Err(PeleError::typed(format!("No such shape: {}. Shape names should be lowercase.", shape), PeleErrorCode::Semantic, "kanban"));
                }
                if let Some(v) = text("label") {
                    node.label = v;
                }
                node.icon = text("icon");
                node.assigned = text("assigned");
                node.ticket = text("ticket");
                node.priority = text("priority");
            }
        }
        // A node indented less than the first column becomes a card, and Mermaid rejects whatever follows it.
        if let Some(label) = &self.misplaced {
            return Err(PeleError::typed(format!("Items without section detected, found section (\"{}\")", label), PeleErrorCode::Semantic, "kanban"));
        }
        if self.sections.is_empty() {
            self.section_level = level;
        }
        if level == self.section_level {
            self.last = Some(Last::Section(self.sections.len()));
            self.sections.push(KanbanSection { node, items: Vec::new() });
        } else {
            let s = self.sections.len() - 1;
            if level < self.section_level {
                self.misplaced = Some(node.label.clone());
            }
            self.last = Some(Last::Item(s, self.sections[s].items.len()));
            self.sections[s].items.push(node);
        }
        Ok(())
    }

    // Mermaid throws a TypeError when a decoration comes before any node. Here it is ignored.
    fn decorate_node(&mut self, icon: Option<&str>, class: Option<&str>) -> Result<(), PeleError> {
        let node = match self.last {
            None => return Ok(()),
            Some(Last::Section(s)) => &mut self.sections[s].node,
            Some(Last::Item(s, i)) => &mut self.sections[s].items[i],
        };
        if let Some(i) = icon.filter(|i| !i.is_empty()) {
            node.icon = Some(i.to_string());
        }
        if let Some(c) = class.filter(|c| !c.is_empty()) {
            node.css_classes = Some(c.to_string());
        }
        Ok(())
    }
}
