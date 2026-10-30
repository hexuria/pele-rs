// Port of src/diagrams/state/graph.ts: Mermaid's dataFetcher, which walks the documents and
// resolves them into the nodes and edges that are drawn.

use super::db::StateDb;
use super::types::{Descr, StateStmt, Stmt};
use crate::errors::{PeleError, PeleErrorCode};
use crate::js;
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct StateNode {
    pub id: String,
    // rect, rectWithTitle, stateStart, stateEnd, choice, fork, join, divider, roundedWithTitle, note or noteGroup.
    pub shape: Option<String>,
    pub label: Option<String>,
    pub description: Vec<String>,
    pub css_classes: String,
    pub css_compiled_styles: Vec<String>,
    pub css_styles: Vec<String>,
    pub type_: Option<String>,
    pub is_group: bool,
    pub parent_id: Option<String>,
    // The direction inside a composite: its last `direction` statement, or TB.
    pub dir: Option<String>,
    /// Set on the note and its group; the outer Option is whether the property is there at all.
    pub position: Option<Option<String>>,
    pub note_for: Option<String>,
}

// Labels are markdown throughout, as in Mermaid.
#[derive(Clone, Debug)]
pub struct StateEdge {
    pub id: String,
    pub start: String,
    pub end: String,
    pub label: String,
    pub dashed: bool,
}

#[derive(Clone, Debug, Default)]
pub struct StateGraph {
    pub nodes: Vec<StateNode>,
    pub edges: Vec<StateEdge>,
}

struct Draft {
    shape: Option<String>,
    description: Descr,
    css_classes: String,
    css_styles: Vec<String>,
    type_: Option<String>,
    dir: Option<String>,
    color_index: Option<usize>,
}

fn direction(doc: &[Stmt]) -> String {
    let mut dir = "TB".to_string();
    for item in doc {
        if let Stmt::Dir(v) = item {
            dir = v.clone();
        }
    }
    dir
}

fn descr_len(d: &Descr) -> usize {
    match d {
        Descr::One(s) => js::len16(s),
        Descr::Many(v) => v.len(),
    }
}

fn flat(d: &Descr) -> Vec<String> {
    match d {
        Descr::One(s) => vec![s.clone()],
        Descr::Many(v) => v.clone(),
    }
}

struct Builder<'a> {
    db: &'a StateDb,
    nodes: Vec<StateNode>,
    edges: Vec<StateEdge>,
    by_id: HashMap<String, usize>,
    drafts: HashMap<String, Draft>,
    slots: HashMap<String, Option<usize>>,
    counter: usize,
    next_slot: usize,
}

impl Builder<'_> {
    fn insert(&mut self, mut node: StateNode) {
        // The two literal ids are what DOMPurify leaves of `<<fork>>` and `<<choice>>` in Mermaid; kept for parity.
        if node.id.is_empty() || node.id == "</join></fork>" || node.id == "</choice>" {
            return;
        }
        if !node.css_classes.is_empty() {
            // One copy, however many classes: a state named in a class statement many times has a long list.
            let mut compiled: Option<Vec<String>> = None;
            for name in node.css_classes.split(' ') {
                let Some(def) = self.db.class(name) else { continue };
                let list = compiled.get_or_insert_with(|| node.css_compiled_styles.clone());
                list.extend(def.styles.iter().cloned());
            }
            if let Some(c) = compiled {
                node.css_compiled_styles = c;
            }
        }
        match self.by_id.get(&node.id) {
            Some(&k) => {
                // Object.assign(existing, node), keeping the parent when the new node names none.
                let existing = &mut self.nodes[k];
                let parent_id = node.parent_id.clone().or_else(|| existing.parent_id.clone());
                let position = node.position.clone().or_else(|| existing.position.clone());
                let note_for = node.note_for.clone().or_else(|| existing.note_for.clone());
                *existing = node;
                existing.parent_id = parent_id;
                existing.position = position;
                existing.note_for = note_for;
            }
            None => {
                self.by_id.insert(node.id.clone(), self.nodes.len());
                self.nodes.push(node);
            }
        }
    }

    fn visit(&mut self, parent: Option<&StateStmt>, item: &StateStmt, alt: bool) {
        let id = item.id.clone();
        if id == "root" {
            return;
        }
        let known = self.db.state(&id);
        let class_str = known.map(|k| k.classes.join(" ")).unwrap_or_default();
        let styles = known.map(|k| k.styles.clone()).unwrap_or_default();

        if !self.drafts.contains_key(&id) {
            let mut shape: Option<String> = Some("rect".into());
            if item.start == Some(true) {
                shape = Some("stateStart".into());
            } else if item.start == Some(false) {
                shape = Some("stateEnd".into());
            }
            if item.type_.as_deref() != Some("default") {
                shape = item.type_.clone();
            }
            self.drafts.insert(
                id.clone(),
                Draft { shape, description: Descr::One(id.clone()), css_classes: format!("{} statediagram-state", class_str), css_styles: styles.clone(), type_: None, dir: None, color_index: None },
            );
        }
        let parent_slot = parent.and_then(|p| self.slots.get(&p.id).copied());
        let draft = self.drafts.get_mut(&id).unwrap();

        let truthy = match &item.description {
            Some(Descr::One(s)) => !s.is_empty(),
            Some(Descr::Many(_)) => true,
            None => false,
        };
        if truthy {
            let d = item.description.as_ref().unwrap();
            match &draft.description {
                Descr::Many(v) => {
                    draft.shape = Some("rectWithTitle".into());
                    let mut v = v.clone();
                    v.extend(flat(d));
                    draft.description = Descr::Many(v);
                }
                Descr::One(s) if !s.is_empty() => {
                    draft.shape = Some("rectWithTitle".into());
                    draft.description = Descr::Many(if *s == id {
                        flat(d)
                    } else {
                        let mut v = vec![s.clone()];
                        v.extend(flat(d));
                        v
                    });
                }
                Descr::One(_) => {
                    draft.shape = Some("rect".into());
                    draft.description = d.clone();
                }
            }
        }
        if descr_len(&draft.description) == 1 && draft.shape.as_deref() == Some("rectWithTitle") {
            draft.shape = Some(if draft.type_.as_deref() == Some("group") { "roundedWithTitle" } else { "rect" }.into());
        }

        if draft.type_.is_none() && item.doc.is_some() {
            draft.type_ = Some("group".into());
            draft.dir = Some(direction(item.doc.as_ref().unwrap()));
            draft.shape = Some(if item.type_.as_deref() == Some("divider") { "divider" } else { "roundedWithTitle" }.into());
            // A container takes the next palette slot unless it is a region, which shares its composite's.
            if draft.shape.as_deref() == Some("divider") && parent_slot.is_some() {
                draft.color_index = parent_slot.unwrap();
            } else {
                let slot = self.next_slot;
                self.next_slot += 1;
                draft.color_index = if !js::trim(&class_str).is_empty() || !styles.is_empty() { None } else { Some(slot) };
            }
            let color_index = draft.color_index;
            draft.css_classes = format!("{} statediagram-cluster {}", draft.css_classes, if alt { "statediagram-cluster-alt" } else { "" });
            self.slots.insert(id.clone(), color_index);
        }
        let draft = self.drafts.get(&id).unwrap();

        let (label, description) = match &draft.description {
            Descr::Many(v) => (v.first().cloned(), v.get(1..).map(<[String]>::to_vec).unwrap_or_default()),
            Descr::One(s) => (Some(s.clone()), Vec::new()),
        };
        let mut node = StateNode {
            id: id.clone(),
            shape: draft.shape.clone(),
            label,
            description,
            css_classes: draft.css_classes.clone(),
            css_compiled_styles: Vec::new(),
            css_styles: draft.css_styles.clone(),
            type_: draft.type_.clone(),
            is_group: draft.type_.as_deref() == Some("group"),
            parent_id: None,
            dir: draft.dir.clone(),
            position: None,
            note_for: None,
        };
        if node.shape.as_deref() == Some("divider") {
            node.label = Some(String::new());
        }
        if let Some(p) = parent.filter(|p| p.id != "root") {
            node.parent_id = Some(p.id.clone());
        }

        let Some(note) = &item.note else {
            self.insert(node);
            return;
        };
        // Mermaid puts each note in a group of its own, named after the state. Pele keeps that
        // group in the model and lays the note out beside its state instead.
        let aside = |aside_id: String, shape: &str, css_classes: String, is_group: bool| StateNode {
            id: aside_id,
            shape: Some(shape.into()),
            label: Some(note.text.clone()),
            description: Vec::new(),
            css_classes,
            css_compiled_styles: Vec::new(),
            css_styles: Vec::new(),
            type_: Some(if is_group { "group" } else { "node" }.into()),
            is_group,
            parent_id: None,
            dir: None,
            position: Some(note.position.clone()),
            note_for: None,
        };
        let mut note_node = aside(format!("{}----note-{}", id, self.counter), "note", "statediagram-note".into(), false);
        self.counter += 1;
        let group_id = format!("{}----parent", id);
        note_node.parent_id = Some(group_id.clone());
        note_node.note_for = Some(id.clone());
        let css = draft.css_classes.clone();
        self.insert(aside(group_id, "noteGroup", css, true));
        let note_id = note_node.id.clone();
        self.insert(note_node);
        self.insert(node);
        let left = note.position.as_deref() == Some("left of");
        let (from, to) = if left { (note_id, id) } else { (id, note_id) };
        self.edges.push(StateEdge { id: format!("{}-{}", from, to), start: from, end: to, label: String::new(), dashed: true });
    }

    fn walk(&mut self, parent: Option<&StateStmt>, doc: &[Stmt], alt: bool) {
        for item in doc {
            match item {
                Stmt::State(st) => {
                    self.visit(parent, st, alt);
                    if let Some(d) = &st.doc {
                        self.walk(Some(st), d, !alt);
                    }
                }
                Stmt::Relation { state1, state2, description } => {
                    self.visit(parent, state1, alt);
                    self.visit(parent, state2, alt);
                    let id = format!("edge{}", self.counter);
                    self.counter += 1;
                    self.edges.push(StateEdge { id, start: state1.id.clone(), end: state2.id.clone(), label: description.clone().unwrap_or_default(), dashed: false });
                }
                _ => {}
            }
        }
    }
}

pub fn build_state_graph(root_doc: &[Stmt], db: &StateDb) -> Result<StateGraph, PeleError> {
    let mut b = Builder { db, nodes: Vec::new(), edges: Vec::new(), by_id: HashMap::new(), drafts: HashMap::new(), slots: HashMap::new(), counter: 0, next_slot: 0 };
    b.walk(None, root_doc, true);
    for node in &b.nodes {
        if node.is_group && !node.description.is_empty() {
            return Err(PeleError::typed(format!("Group nodes can only have label. Remove the additional description for node [{}]", node.id), PeleErrorCode::Semantic, "state"));
        }
    }
    Ok(StateGraph { nodes: b.nodes, edges: b.edges })
}
