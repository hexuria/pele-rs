// Port of src/diagrams/class/db.ts. Maps keep insertion order, as upstream's do.

use super::members::ClassMember;
use super::types::{ClassInterface, ClassNode, ClassNote, ClassRelation, NamespaceNode, StyleClass};
use crate::js;
use crate::text::entities::decode_entities;
use crate::util::url::sanitize_url;
use std::collections::HashMap;

pub const LOLLIPOP: u32 = 4;

// Mermaid looks up ids that start with a digit under this prefix, so they never match a class.
const DOM_ID_PREFIX: &str = "classId-";

/// `{ className, type }`: a name with a generic type written `Name~T~`. Offsets are UTF-16.
fn split_name(id: &str) -> (String, String) {
    let u = js::u16s(id);
    let at = u.iter().position(|&c| c == 126);
    match at {
        Some(at) if at > 0 => {
            let end = u[at + 1..].iter().position(|&c| c == 126).map_or(u.len(), |k| at + 1 + k);
            (js::from16(&u[..at]), js::from16(&u[at + 1..end]))
        }
        _ => (id.to_string(), String::new()),
    }
}

fn lookup_id(id: &str) -> String {
    let first = id.encode_utf16().next().unwrap_or(0);
    if (48..=57).contains(&first) {
        split_name(&format!("{}{}", DOM_ID_PREFIX, id)).0
    } else {
        split_name(id).0
    }
}

#[derive(Default)]
pub struct ClassDb {
    pub direction: String,
    pub classes: Vec<ClassNode>,
    class_index: HashMap<String, usize>,
    pub relations: Vec<ClassRelation>,
    pub notes: Vec<ClassNote>,
    pub namespaces: Vec<NamespaceNode>,
    namespace_index: HashMap<String, usize>,
    pub interfaces: Vec<ClassInterface>,
    pub style_classes: Vec<StyleClass>,
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    namespace_stack: Vec<String>,
    /// Map<string, Set<ClassNode>>: class indexes, in the order they were tagged.
    with_css_class: Vec<(String, Vec<usize>)>,
}

impl ClassDb {
    pub fn new() -> Self {
        ClassDb { direction: "TB".into(), ..Default::default() }
    }

    pub fn class(&self, id: &str) -> Option<&ClassNode> {
        self.class_index.get(id).map(|&i| &self.classes[i])
    }

    pub fn class_at(&self, id: &str) -> Option<usize> {
        self.class_index.get(id).copied()
    }

    pub fn namespace(&self, id: &str) -> Option<&NamespaceNode> {
        self.namespace_index.get(id).map(|&i| &self.namespaces[i])
    }

    pub fn style_class(&self, id: &str) -> Option<&StyleClass> {
        self.style_classes.iter().find(|s| s.id == id)
    }

    pub fn add_class(&mut self, id: &str) {
        let (class_name, type_) = split_name(id);
        if self.class_index.contains_key(&class_name) {
            return;
        }
        self.classes.push(ClassNode { id: class_name.clone(), type_, label: class_name.clone(), css_classes: "default".into(), ..Default::default() });
        let i = self.classes.len() - 1;
        self.class_index.insert(class_name, i);
        self.tag(i, "default");
    }

    fn tag(&mut self, node: usize, css_class: &str) {
        match self.with_css_class.iter_mut().find(|(k, _)| k == css_class) {
            Some((_, set)) => {
                if !set.contains(&node) {
                    set.push(node);
                }
            }
            None => self.with_css_class.push((css_class.to_string(), vec![node])),
        }
    }

    // Returns the class for a name that may carry a generic type, creating it if needed.
    fn ensure(&mut self, id: &str) -> usize {
        self.add_class(id);
        self.class_index[&split_name(id).0]
    }

    pub fn set_class_label(&mut self, id: &str, label: &str) {
        let i = self.ensure(id);
        self.classes[i].label = label.to_string();
    }

    pub fn add_relation(&mut self, mut relation: ClassRelation) {
        if relation.type1 == Some(LOLLIPOP) && relation.type2.is_none() {
            self.add_class(&relation.id2.clone());
            relation.id1 = self.add_interface(&relation.id1, &relation.id2);
        } else if relation.type2 == Some(LOLLIPOP) && relation.type1.is_none() {
            self.add_class(&relation.id1.clone());
            relation.id2 = self.add_interface(&relation.id2, &relation.id1);
        } else {
            self.add_class(&relation.id1.clone());
            self.add_class(&relation.id2.clone());
        }
        relation.id1 = split_name(&relation.id1).0;
        relation.id2 = split_name(&relation.id2).0;
        relation.relation_title1 = js::trim(&relation.relation_title1).to_string();
        relation.relation_title2 = js::trim(&relation.relation_title2).to_string();
        self.relations.push(relation);
    }

    fn add_interface(&mut self, label: &str, class_id: &str) -> String {
        let id = format!("interface{}", self.interfaces.len());
        self.interfaces.push(ClassInterface { id: id.clone(), label: label.to_string(), class_id: class_id.to_string() });
        id
    }

    pub fn add_annotation(&mut self, class_name: &str, annotation: &str) {
        let i = self.ensure(class_name);
        self.classes[i].annotations.push(annotation.to_string());
    }

    pub fn add_member(&mut self, class_name: &str, member: &str) {
        let i = self.ensure(class_name);
        let text = js::trim(member);
        let node = &mut self.classes[i];
        if text.starts_with("<<") && text.ends_with(">>") {
            let u = js::u16s(text);
            node.annotations.push(js::from16(&u[2..u.len() - 2]));
        } else if text.find(')').is_some_and(|k| k > 0) {
            let parsed = ClassMember::new(text, true);
            if parsed.method {
                node.methods.push(parsed);
            } else {
                node.members.push(parsed);
            }
        } else if !text.is_empty() {
            node.members.push(ClassMember::new(text, false));
        }
    }

    pub fn add_members(&mut self, class_name: &str, mut members: Vec<String>) {
        members.reverse();
        for m in &members {
            self.add_member(class_name, m);
        }
    }

    pub fn add_note(&mut self, text: &str, class_name: Option<&str>) -> String {
        let index = self.notes.len();
        let id = format!("note{}", index);
        self.notes.push(ClassNote { id: id.clone(), class: class_name.map(str::to_string), text: text.to_string(), index, parent: None });
        id
    }

    pub fn cleanup_label(&self, label: &str) -> String {
        js::trim(label.strip_prefix(':').unwrap_or(label)).to_string()
    }

    pub fn set_css_class(&mut self, ids: &str, class_name: &str) {
        for id in ids.split(',') {
            let Some(&i) = self.class_index.get(&lookup_id(id)) else { continue };
            self.classes[i].css_classes.push(' ');
            self.classes[i].css_classes.push_str(class_name);
            for word in class_name.split(' ') {
                self.tag(i, word);
            }
        }
    }

    pub fn define_class(&mut self, ids: &[String], style: &[String]) {
        for id in ids {
            let k = match self.style_classes.iter().position(|s| &s.id == id) {
                Some(k) => k,
                None => {
                    self.style_classes.push(StyleClass { id: id.clone(), styles: Vec::new(), text_styles: Vec::new() });
                    self.style_classes.len() - 1
                }
            };
            for s in style {
                if s.contains("color") {
                    self.style_classes[k].text_styles.push(s.replacen("fill", "bgFill", 1));
                }
                self.style_classes[k].styles.push(s.clone());
            }
            // Mermaid tests `cssClasses.includes(id)`, which also matches part of a longer name
            // (a classDef named `def` styles every class, through `default`). Whole names are compared here.
            let Some((_, tagged)) = self.with_css_class.iter().find(|(k, _)| k == id) else { continue };
            let parts: Vec<String> = style.iter().flat_map(|s| s.split(',').map(str::to_string).collect::<Vec<_>>()).collect();
            for &node in tagged.clone().iter() {
                for part in &parts {
                    self.classes[node].styles.push(part.clone());
                }
            }
        }
    }

    pub fn set_tooltip(&mut self, ids: &str, tooltip: Option<&str>) {
        let Some(tooltip) = tooltip else { return };
        for id in ids.split(',') {
            if let Some(&i) = self.class_index.get(&split_name(id).0) {
                self.classes[i].tooltip = Some(tooltip.to_string());
            }
        }
    }

    pub fn set_link(&mut self, ids: &str, link: &str, target: Option<&str>) {
        // Entity codes are resolved first, so that what is checked is what a browser would follow.
        let url = js::trim(&decode_entities(link)).to_string();
        for id in ids.split(',') {
            if let Some(&i) = self.class_index.get(&lookup_id(id)) {
                self.classes[i].link = if url.is_empty() { None } else { Some(sanitize_url(&url)) };
                self.classes[i].link_target = Some(target.unwrap_or("_blank").to_string());
            }
        }
        self.set_css_class(ids, "clickable");
    }

    // Callbacks are never executed; the class is only marked clickable, as in Mermaid's strict mode.
    pub fn set_click_event(&mut self, ids: &str) {
        for id in ids.split(',') {
            if let Some(&i) = self.class_index.get(&split_name(id).0) {
                self.classes[i].have_callback = true;
            }
        }
        self.set_css_class(ids, "clickable");
    }

    pub fn set_direction(&mut self, dir: &str) {
        self.direction = dir.to_string();
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

    pub fn add_namespace(&mut self, id: &str, label: Option<&str>) -> String {
        let prefix = self.namespace_stack.last().cloned();
        let qualified = match &prefix {
            Some(p) if !p.is_empty() => format!("{}.{}", p, id),
            _ => id.to_string(),
        };
        self.namespace_stack.push(qualified.clone());
        let label = label.filter(|l| !l.is_empty());

        if let Some(&k) = self.namespace_index.get(&qualified) {
            self.namespaces[k].explicit = true;
            if let Some(l) = label {
                self.namespaces[k].label = l.to_string();
            }
            return qualified;
        }

        // The enclosing namespace and its ancestors exist already, so only the parts of `id` are walked.
        let prefix = prefix.filter(|p| !p.is_empty());
        let mut parent_id: Option<String> = prefix.clone();
        let mut from = prefix.as_ref().map_or(0, |p| p.len() + 1);
        loop {
            let dot = qualified[from.min(qualified.len())..].find('.').map(|k| from + k);
            let leaf = dot.is_none();
            let current = match dot {
                None => qualified.clone(),
                Some(d) => qualified[..d].to_string(),
            };
            match self.namespace_index.get(&current) {
                None => {
                    let part = qualified[from..dot.unwrap_or(qualified.len())].to_string();
                    let node = NamespaceNode { id: current.clone(), label: if leaf && label.is_some() { label.unwrap().to_string() } else { part }, parent: parent_id.clone(), explicit: leaf };
                    self.namespaces.push(node);
                    self.namespace_index.insert(current.clone(), self.namespaces.len() - 1);
                }
                Some(&k) => {
                    if leaf {
                        self.namespaces[k].explicit = true;
                    }
                }
            }
            if let Some(pid) = &parent_id {
                if self.namespace_index.contains_key(pid) {
                    let k = self.namespace_index[&current];
                    if self.namespaces[k].parent.is_none() {
                        self.namespaces[k].parent = Some(pid.clone());
                    }
                }
            }
            if leaf {
                return qualified;
            }
            parent_id = Some(current);
            from = dot.unwrap() + 1;
        }
    }

    pub fn pop_namespace(&mut self) {
        self.namespace_stack.pop();
    }

    pub fn add_classes_to_namespace(&mut self, id: &str, class_names: &[String], note_names: &[String]) {
        if !self.namespace_index.contains_key(id) {
            return;
        }
        for name in class_names {
            if let Some(&i) = self.class_index.get(&split_name(name).0) {
                self.classes[i].parent = Some(id.to_string());
            }
        }
        for name in note_names {
            if let Some(note) = self.notes.iter_mut().find(|n| &n.id == name) {
                note.parent = Some(id.to_string());
            }
        }
    }

    pub fn set_css_style(&mut self, id: &str, styles: &[String]) {
        let Some(&i) = self.class_index.get(id) else { return };
        for s in styles {
            for part in s.split(',') {
                self.classes[i].styles.push(part.to_string());
            }
        }
    }
}
