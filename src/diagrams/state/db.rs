// Port of src/diagrams/state/db.ts: Mermaid's StateDB in its version 2 form, which is what
// Mermaid uses for both `stateDiagram` and `stateDiagram-v2`. The legacy version, which numbers
// `[*]` states, is not ported. Maps keep insertion order, as upstream's do.

use super::graph::{build_state_graph, StateGraph};
use super::types::{Descr, StateClassDef, StateLink, StateNote, StateRelation, StateStmt, Stmt};
use crate::errors::PeleError;
use crate::js;
use std::collections::HashMap;

/// A state as the db keeps it: what the graph reads of it.
#[derive(Clone, Debug, Default)]
pub struct KnownState {
    pub id: String,
    pub type_: String,
    pub descriptions: Vec<String>,
    pub classes: Vec<String>,
    pub styles: Vec<String>,
    pub has_doc: bool,
}

#[derive(Default)]
pub struct StateDb {
    pub root_doc: Vec<Stmt>,
    pub states: Vec<KnownState>,
    state_index: HashMap<String, usize>,
    pub relations: Vec<StateRelation>,
    pub classes: Vec<StateClassDef>,
    pub links: Vec<(String, StateLink)>,
    pub graph: StateGraph,
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    divider_count: usize,
}

impl StateDb {
    pub fn new() -> Self {
        StateDb::default()
    }

    /// The direction the root document is laid out in: its last `direction` statement.
    pub fn direction(&self) -> String {
        let mut dir = "TB".to_string();
        for item in &self.root_doc {
            if let Stmt::Dir(v) = item {
                dir = v.clone();
            }
        }
        dir
    }

    pub fn state(&self, id: &str) -> Option<&KnownState> {
        self.state_index.get(id).map(|&i| &self.states[i])
    }

    pub fn class(&self, id: &str) -> Option<&StateClassDef> {
        self.classes.iter().find(|c| c.id == id)
    }

    pub fn link(&self, id: &str) -> Option<&StateLink> {
        self.links.iter().find(|(k, _)| k == id).map(|(_, l)| l)
    }

    pub fn set_root_doc(&mut self, mut doc: Vec<Stmt>) -> Result<(), PeleError> {
        self.translate(&mut doc);
        self.root_doc = doc;
        self.extract()
    }

    // Only the root document's statements become states, relations and classes here; the
    // statements inside composites are read when the graph is built.
    fn extract(&mut self) -> Result<(), PeleError> {
        self.states.clear();
        self.state_index.clear();
        self.relations.clear();
        self.classes.clear();
        let doc = std::mem::take(&mut self.root_doc);
        for item in &doc {
            match item {
                Stmt::State(st) => self.add_state(js::trim(&st.id), st.type_.as_deref(), st.doc.is_some(), st.description.as_ref(), st.note.as_ref(), None),
                Stmt::Relation { state1, state2, description } => self.add_relation(state1, state2, description.as_deref().unwrap_or("")),
                Stmt::ClassDef { id, classes } => self.add_style_class(js::trim(id), classes),
                Stmt::Style { id, style_class } => {
                    let styles: Vec<String> = style_class.split(',').map(|s| js::trim(&s.replace(';', "")).to_string()).collect();
                    for k in self.named(js::trim(id)) {
                        self.states[k].styles = styles.clone();
                    }
                }
                Stmt::ApplyClass { id, style_class } => self.set_css_class(js::trim(id), style_class),
                Stmt::Click { id, url, tooltip } => self.add_link(&id.id, url, tooltip),
                _ => {}
            }
        }
        self.root_doc = doc;
        self.graph = build_state_graph(&self.root_doc, self)?;
        Ok(())
    }

    // Mermaid's docTranslator: gives each `[*]` an id scoped to its parent, and turns a document
    // split by `--` into one region per part. The root document is never split: the lexer reads
    // `--` as a divider only inside a composite.
    fn translate(&mut self, root: &mut [Stmt]) {
        for item in root.iter_mut() {
            self.translate_stmt(item, "root", true);
        }
    }

    fn translate_stmt(&mut self, node: &mut Stmt, parent_id: &str, first: bool) {
        match node {
            Stmt::Relation { state1, state2, .. } => {
                self.translate_state(state1, parent_id, true);
                self.translate_state(state2, parent_id, false);
            }
            Stmt::State(st) => self.translate_state(st, parent_id, first),
            _ => {}
        }
    }

    fn translate_state(&mut self, node: &mut StateStmt, parent_id: &str, first: bool) {
        if node.id == "[*]" {
            node.id = format!("{}{}", parent_id, if first { "_start" } else { "_end" });
            node.start = Some(first);
        } else {
            node.id = js::trim(&node.id).to_string();
        }
        let Some(doc) = node.doc.take() else { return };
        let is_divider = |s: &Stmt| matches!(s, Stmt::State(st) if st.type_.as_deref() == Some("divider") && st.doc.is_none());
        let mut doc = doc;
        if doc.iter().any(is_divider) {
            let mut regions: Vec<Stmt> = Vec::new();
            let mut current: Vec<Stmt> = Vec::new();
            for item in doc {
                if is_divider(&item) {
                    let Stmt::State(mut st) = item else { unreachable!() };
                    st.doc = Some(std::mem::take(&mut current));
                    regions.push(Stmt::State(st));
                } else {
                    current.push(item);
                }
            }
            // Mermaid names the region after the last divider with a random id; here it continues
            // the divider numbering. Mermaid also leaves a document that ends with a divider
            // ungrouped and then fails to draw it; here the regions before that divider are kept.
            if !current.is_empty() {
                regions.push(Stmt::State(StateStmt { id: self.get_divider_id(), type_: Some("divider".into()), doc: Some(current), ..Default::default() }));
            }
            doc = regions;
        }
        let id = node.id.clone();
        for item in doc.iter_mut() {
            self.translate_stmt(item, &id, true);
        }
        node.doc = Some(doc);
    }

    #[allow(clippy::too_many_arguments)]
    fn add_state(&mut self, id: &str, type_: Option<&str>, has_doc: bool, descr: Option<&Descr>, _note: Option<&StateNote>, classes: Option<&[String]>) {
        let trimmed = js::trim(id).to_string();
        let type_ = type_.unwrap_or("default");
        match self.state_index.get(&trimmed) {
            None => {
                self.states.push(KnownState { id: trimmed.clone(), type_: type_.to_string(), has_doc, ..Default::default() });
                self.state_index.insert(trimmed.clone(), self.states.len() - 1);
            }
            Some(&k) => {
                let st = &mut self.states[k];
                if !st.has_doc {
                    st.has_doc = has_doc;
                }
                if st.type_.is_empty() {
                    st.type_ = type_.to_string();
                }
            }
        }
        match descr {
            Some(Descr::One(s)) if !s.is_empty() => self.add_description(&trimmed, js::trim(s)),
            Some(Descr::Many(list)) => {
                for s in list {
                    self.add_description(&trimmed, js::trim(s));
                }
            }
            _ => {}
        }
        if let Some(classes) = classes {
            for name in classes {
                self.set_css_class(&trimmed, js::trim(name));
            }
        }
    }

    fn add_link(&mut self, state_id: &str, url: &str, tooltip: &str) {
        let link = StateLink { url: url.to_string(), tooltip: tooltip.to_string() };
        match self.links.iter_mut().find(|(k, _)| k == state_id) {
            Some(slot) => slot.1 = link,
            None => self.links.push((state_id.to_string(), link)),
        }
    }

    fn add_relation(&mut self, item1: &StateStmt, item2: &StateStmt, title: &str) {
        for item in [item1, item2] {
            self.add_state(&item.id, item.type_.as_deref(), item.doc.is_some(), item.description.as_ref(), item.note.as_ref(), item.classes.as_deref());
        }
        self.relations.push(StateRelation { id1: js::trim(&item1.id).to_string(), id2: js::trim(&item2.id).to_string(), relation_title: title.to_string() });
    }

    fn add_description(&mut self, id: &str, descr: &str) {
        let Some(&k) = self.state_index.get(id) else { return };
        let text = if descr.starts_with(':') { js::trim(&descr.replacen(':', "", 1)).to_string() } else { descr.to_string() };
        self.states[k].descriptions.push(text);
    }

    pub fn get_divider_id(&mut self) -> String {
        self.divider_count += 1;
        format!("divider-id-{}", self.divider_count)
    }

    fn add_style_class(&mut self, id: &str, style_attributes: &str) {
        let k = match self.classes.iter().position(|c| c.id == id) {
            Some(k) => k,
            None => {
                self.classes.push(StateClassDef { id: id.to_string(), styles: Vec::new(), text_styles: Vec::new() });
                self.classes.len() - 1
            }
        };
        if style_attributes.is_empty() {
            return;
        }
        for attrib in style_attributes.split(',') {
            let fixed = js::trim(&attrib.replacen(';', "", 1)).to_string();
            if attrib.contains("color") {
                self.classes[k].text_styles.push(fixed.replacen("fill", "bgFill", 1).replacen("color", "fill", 1));
            }
            self.classes[k].styles.push(fixed);
        }
    }

    fn set_css_class(&mut self, item_ids: &str, css_class_name: &str) {
        for k in self.named(item_ids) {
            self.states[k].classes.push(css_class_name.to_string());
        }
    }

    pub fn trim_colon(&self, text: &str) -> String {
        match text.strip_prefix(':') {
            Some(rest) => js::trim(rest).to_string(),
            None => js::trim(text).to_string(),
        }
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

    // The states a comma-separated list of ids names, created where they do not exist yet. As
    // in Mermaid, an id is looked up as written and trimmed only when it has to be created.
    fn named(&mut self, ids: &str) -> Vec<usize> {
        let mut out = Vec::new();
        for id in ids.split(',') {
            let mut k = self.state_index.get(id).copied();
            if k.is_none() {
                self.add_state(id, None, false, None, None, None);
                k = self.state_index.get(js::trim(id)).copied();
            }
            if let Some(k) = k {
                out.push(k);
            }
        }
        out
    }
}
