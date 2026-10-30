// Port of src/diagrams/er/db.ts.

use super::types::{DocItem, ErAttribute, ErClassDef, ErEntity, ErRelationship, ErSubgraph, ParsedAttribute, RelSpec};
use crate::js;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub struct ErDb {
    pub direction: String,
    /// Map<string, ErEntity>, in insertion order.
    pub entities: Vec<(String, ErEntity)>,
    entity_index: HashMap<String, usize>,
    pub relationships: Vec<ErRelationship>,
    /// Map<string, ErClassDef>, in insertion order.
    pub classes: Vec<ErClassDef>,
    pub subgraphs: Vec<ErSubgraph>,
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    subgraph_lookup: HashMap<String, usize>,
    claimed: HashSet<String>,
}

impl ErDb {
    pub fn new() -> Self {
        ErDb { direction: "TB".into(), ..Default::default() }
    }

    pub fn entity(&self, name: &str) -> Option<&ErEntity> {
        self.entity_index.get(name).map(|&i| &self.entities[i].1)
    }

    pub fn class(&self, id: &str) -> Option<&ErClassDef> {
        self.classes.iter().find(|c| c.id == id)
    }

    pub fn add_entity(&mut self, name: &str, alias: &str) -> &mut ErEntity {
        let i = match self.entity_index.get(name) {
            Some(&i) => {
                let e = &mut self.entities[i].1;
                if e.alias.is_empty() && !alias.is_empty() {
                    e.alias = alias.to_string();
                }
                i
            }
            None => {
                let entity = ErEntity {
                    id: format!("entity-{}-{}", name, self.entities.len()),
                    label: name.to_string(),
                    alias: alias.to_string(),
                    attributes: Vec::new(),
                    css_classes: "default".into(),
                    css_styles: Vec::new(),
                };
                self.entities.push((name.to_string(), entity));
                self.entity_index.insert(name.to_string(), self.entities.len() - 1);
                self.entities.len() - 1
            }
        };
        &mut self.entities[i].1
    }

    /// The grammar builds the list back to front, so the last attribute written comes first.
    pub fn add_attributes(&mut self, name: &str, attributes: Vec<ParsedAttribute>) {
        let entity = self.add_entity(name, "");
        for a in attributes.into_iter().rev() {
            entity.attributes.push(ErAttribute { type_: a.type_, name: a.name, keys: a.keys.unwrap_or_default(), comment: a.comment.unwrap_or_default() });
        }
    }

    pub fn add_relationship(&mut self, ent_a: &str, role_a: &str, ent_b: &str, rel_spec: RelSpec) {
        let entity_a = if self.subgraph_lookup.contains_key(ent_a) { ent_a.to_string() } else { self.add_entity(ent_a, "").id.clone() };
        let entity_b = if self.subgraph_lookup.contains_key(ent_b) { ent_b.to_string() } else { self.add_entity(ent_b, "").id.clone() };
        self.relationships.push(ErRelationship { entity_a, role_a: role_a.to_string(), entity_b, rel_spec });
    }

    pub fn set_direction(&mut self, dir: &str) {
        self.direction = dir.to_string();
    }

    pub fn add_css_styles(&mut self, ids: &[String], styles: &[String]) {
        for id in ids {
            if let Some(&i) = self.entity_index.get(id) {
                self.entities[i].1.css_styles.extend(styles.iter().cloned());
            }
            if let Some(&s) = self.subgraph_lookup.get(id) {
                self.subgraphs[s].css_styles.extend(styles.iter().cloned());
            }
        }
    }

    pub fn add_class(&mut self, ids: &[String], styles: &[String]) {
        for id in ids {
            let k = match self.classes.iter().position(|c| &c.id == id) {
                Some(k) => k,
                None => {
                    self.classes.push(ErClassDef { id: id.clone(), styles: Vec::new(), text_styles: Vec::new() });
                    self.classes.len() - 1
                }
            };
            let def = &mut self.classes[k];
            for style in styles {
                if style.contains("color") {
                    def.text_styles.push(style.replacen("fill", "bgFill", 1));
                }
                def.styles.push(style.clone());
            }
        }
    }

    pub fn set_class(&mut self, ids: &[String], class_names: &[String]) {
        for id in ids {
            if let Some(&i) = self.entity_index.get(id) {
                for name in class_names {
                    self.entities[i].1.css_classes.push(' ');
                    self.entities[i].1.css_classes.push_str(name);
                }
            }
            if let Some(&s) = self.subgraph_lookup.get(id) {
                self.subgraphs[s].classes.extend(class_names.iter().cloned());
            }
        }
    }

    pub fn add_sub_graph(&mut self, id_text: &str, list: &[DocItem], title_text: &str) -> String {
        let id = js::trim(id_text).to_string();
        let mut seen: HashSet<String> = HashSet::new();
        let mut nodes: Vec<String> = Vec::new();
        let mut dir: Option<String> = None;
        for item in list {
            let item = match item {
                DocItem::Dir(d) => {
                    dir = Some(d.to_string());
                    continue;
                }
                DocItem::Name(s) => s,
            };
            let trimmed = js::trim(item);
            if trimmed.is_empty() || seen.contains(trimmed) {
                continue;
            }
            seen.insert(trimmed.to_string());
            // A node belongs to the first subgraph that lists it.
            if !self.claimed.contains(item) {
                nodes.push(item.clone());
            }
        }
        for node in &nodes {
            self.claimed.insert(node.clone());
        }
        self.subgraphs.push(ErSubgraph { id: id.clone(), nodes, title: js::trim(title_text).to_string(), classes: Vec::new(), css_styles: Vec::new(), dir, label_type: "markdown" });
        self.subgraph_lookup.insert(id.clone(), self.subgraphs.len() - 1);
        id
    }

    pub fn set_acc_title(&mut self, text: &str) {
        self.acc_title = Some(js::trim_start(text).to_string());
    }

    pub fn set_acc_description(&mut self, text: &str) {
        // text.replace(/\n\s+/g, '\n')
        let c: Vec<char> = text.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < c.len() {
            if c[i] == '\n' && i + 1 < c.len() && js::is_space(c[i + 1]) {
                out.push('\n');
                i += 1;
                while i < c.len() && js::is_space(c[i]) {
                    i += 1;
                }
                continue;
            }
            out.push(c[i]);
            i += 1;
        }
        self.acc_descr = Some(out);
    }
}
