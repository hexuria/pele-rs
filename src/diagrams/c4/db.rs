// Port of src/diagrams/c4/db.ts and types.ts. Shapes and boundaries live in two lists; a parent
// is an index into the boundaries.

use crate::diagrams::info::newline_dedent;
use crate::js;
use std::collections::HashMap;

/// A `$key="value"` argument.
#[derive(Clone, Debug)]
pub struct Named {
    pub key: String,
    pub value: String,
}

#[derive(Clone, Debug)]
pub enum Attr {
    Str(String),
    Named(Named),
}

#[derive(Clone, Default, Debug)]
pub struct Element {
    pub alias: String,
    pub label: String,
    pub alias_attr: Option<Named>,
    pub label_attr: Option<Named>,
    pub parent: Option<usize>,
    pub type_: Option<String>,
    pub descr: Option<String>,
    pub techn: Option<String>,
    pub sprite: Option<String>,
    pub tags: Option<String>,
    pub link: Option<String>,
    pub bg_color: Option<String>,
    pub font_color: Option<String>,
    pub border_color: Option<String>,
    pub shadowing: Option<String>,
    pub shape: Option<String>,
    pub legend_text: Option<String>,
    pub legend_sprite: Option<String>,
    // Shapes: Mermaid's typeC4Shape, as person, external_system_db, container_queue.
    pub kind: String,
    // Deployment nodes: node, nodeL or nodeR.
    pub node_type: Option<String>,
    pub extra: Vec<(String, String)>,
}

#[derive(Clone, Default, Debug)]
pub struct Rel {
    pub type_: String,
    pub from: String,
    pub to: String,
    pub label: String,
    pub from_attr: Option<Named>,
    pub to_attr: Option<Named>,
    pub label_attr: Option<Named>,
    pub techn: Option<String>,
    pub descr: Option<String>,
    pub sprite: Option<String>,
    pub tags: Option<String>,
    pub link: Option<String>,
    pub text_color: Option<String>,
    pub line_color: Option<String>,
    pub offset_x: Option<f64>,
    pub offset_y: Option<f64>,
    pub extra: Vec<(String, String)>,
}

trait Target {
    fn field(&mut self, key: &str) -> Option<&mut Option<String>>;
    fn label_mut(&mut self) -> &mut String;
    fn label_attr_mut(&mut self) -> &mut Option<Named>;
    fn extra_mut(&mut self) -> &mut Vec<(String, String)>;
    fn offset(&mut self, _key: &str) -> Option<&mut Option<f64>> {
        None
    }
}

impl Target for Element {
    fn field(&mut self, key: &str) -> Option<&mut Option<String>> {
        Some(match key {
            "type" => &mut self.type_,
            "descr" => &mut self.descr,
            "techn" => &mut self.techn,
            "sprite" => &mut self.sprite,
            "tags" => &mut self.tags,
            "link" => &mut self.link,
            "bgColor" => &mut self.bg_color,
            "fontColor" => &mut self.font_color,
            "borderColor" => &mut self.border_color,
            "shadowing" => &mut self.shadowing,
            "shape" => &mut self.shape,
            "legendText" => &mut self.legend_text,
            "legendSprite" => &mut self.legend_sprite,
            _ => return None,
        })
    }
    fn label_mut(&mut self) -> &mut String {
        &mut self.label
    }
    fn label_attr_mut(&mut self) -> &mut Option<Named> {
        &mut self.label_attr
    }
    fn extra_mut(&mut self) -> &mut Vec<(String, String)> {
        &mut self.extra
    }
}

impl Target for Rel {
    fn field(&mut self, key: &str) -> Option<&mut Option<String>> {
        Some(match key {
            "descr" => &mut self.descr,
            "techn" => &mut self.techn,
            "sprite" => &mut self.sprite,
            "tags" => &mut self.tags,
            "link" => &mut self.link,
            "textColor" => &mut self.text_color,
            "lineColor" => &mut self.line_color,
            _ => return None,
        })
    }
    fn label_mut(&mut self) -> &mut String {
        &mut self.label
    }
    fn label_attr_mut(&mut self) -> &mut Option<Named> {
        &mut self.label_attr
    }
    fn extra_mut(&mut self) -> &mut Vec<(String, String)> {
        &mut self.extra
    }
    fn offset(&mut self, key: &str) -> Option<&mut Option<f64>> {
        match key {
            "offsetX" => Some(&mut self.offset_x),
            "offsetY" => Some(&mut self.offset_y),
            _ => None,
        }
    }
}

// Mermaid assigns a named argument to whatever property it names, structural ones included.
// Here it reaches only the known fields; any other name is kept aside in `extra`.
fn named<T: Target>(target: &mut T, attr: &Named) {
    if let Some(slot) = target.offset(&attr.key) {
        *slot = Some(js::parse_int(&attr.value, 10));
        return;
    }
    if attr.key == "label" {
        *target.label_mut() = attr.value.clone();
        return;
    }
    if let Some(slot) = target.field(&attr.key) {
        *slot = Some(attr.value.clone());
        return;
    }
    let extra = target.extra_mut();
    match extra.iter_mut().find(|(k, _)| *k == attr.key) {
        Some(slot) => slot.1 = attr.value.clone(),
        None => extra.push((attr.key.clone(), attr.value.clone())),
    }
}

// A positional argument fills the field of its slot. A named one fills the field it names.
fn put<T: Target>(target: &mut T, field: &str, attr: Option<&Attr>) {
    match attr {
        None => {}
        Some(Attr::Str(s)) => {
            if field == "label" {
                *target.label_mut() = s.clone();
            } else if let Some(slot) = target.field(field) {
                *slot = Some(s.clone());
            }
        }
        Some(Attr::Named(n)) => named(target, n),
    }
}

fn text<T: Target>(target: &mut T, field: &str, attr: Option<&Attr>) {
    match attr {
        None => {
            let slot = target.field(field).unwrap();
            if slot.is_none() {
                *slot = Some(String::new());
            }
        }
        Some(_) => put(target, field, attr),
    }
}

fn label<T: Target>(target: &mut T, attr: Option<&Attr>) {
    *target.label_attr_mut() = None;
    match attr {
        Some(Attr::Named(n)) => {
            *target.label_mut() = String::new();
            *target.label_attr_mut() = Some(n.clone());
            named(target, n);
        }
        Some(Attr::Str(s)) => *target.label_mut() = s.clone(),
        None => *target.label_mut() = String::new(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ElRef {
    Shape(usize),
    Boundary(usize),
}

pub struct C4Db {
    // The header keyword: C4Context, C4Container, C4Component, C4Dynamic or C4Deployment.
    pub c4_type: Option<String>,
    pub title: Option<String>,
    pub acc_descr: Option<String>,
    pub shapes: Vec<Element>,
    // The first boundary is the unnamed one that holds everything at the top level.
    pub boundaries: Vec<Element>,
    pub rels: Vec<Rel>,
    pub shape_in_row: f64,
    pub boundary_in_row: f64,
    shape_index: HashMap<String, usize>,
    // The top-level boundary is not listed, so a boundary a diagram names `global` is its own box.
    boundary_index: HashMap<String, usize>,
    rel_index: HashMap<String, HashMap<String, usize>>,
    current: usize,
    stack: Vec<usize>,
}

impl Default for C4Db {
    fn default() -> Self {
        let global = Element { alias: "global".into(), label: "global".into(), type_: Some("global".into()), ..Default::default() };
        C4Db {
            c4_type: None,
            title: None,
            acc_descr: None,
            shapes: Vec::new(),
            boundaries: vec![global],
            rels: Vec::new(),
            shape_in_row: 4.0,
            boundary_in_row: 2.0,
            shape_index: HashMap::new(),
            boundary_index: HashMap::new(),
            rel_index: HashMap::new(),
            current: 0,
            stack: Vec::new(),
        }
    }
}

fn element(list: &mut Vec<Element>, index: &mut HashMap<String, usize>, alias: &Attr, title: Option<&Attr>) -> usize {
    let found = match alias {
        Attr::Str(a) => index.get(a).copied(),
        _ => None,
    };
    let k = match found {
        Some(k) => k,
        None => {
            let k = list.len();
            list.push(Element::default());
            match alias {
                Attr::Str(a) => {
                    list[k].alias = a.clone();
                    index.insert(a.clone(), k);
                }
                Attr::Named(n) => {
                    list[k].alias_attr = Some(n.clone());
                    named(&mut list[k], n);
                }
            }
            k
        }
    };
    label(&mut list[k], title);
    k
}

impl C4Db {
    pub fn set_c4_type(&mut self, t: &str) {
        self.c4_type = Some(t.to_string());
    }
    pub fn set_title(&mut self, t: &str) {
        self.title = Some(t.to_string());
    }
    pub fn set_acc_description(&mut self, d: &str) {
        self.acc_descr = Some(newline_dedent(d));
    }

    // alias, label, ?descr, ?sprite, ?tags, $link
    pub fn add_person_or_system(&mut self, kind: &str, attrs: &[Attr]) {
        let k = element(&mut self.shapes, &mut self.shape_index, &attrs[0], attrs.get(1));
        text(&mut self.shapes[k], "descr", attrs.get(2));
        self.finish_shape(k, kind, attrs, 3);
    }

    // alias, label, ?techn, ?descr, ?sprite, ?tags, $link
    pub fn add_container(&mut self, kind: &str, attrs: &[Attr]) {
        let k = element(&mut self.shapes, &mut self.shape_index, &attrs[0], attrs.get(1));
        text(&mut self.shapes[k], "techn", attrs.get(2));
        text(&mut self.shapes[k], "descr", attrs.get(3));
        self.finish_shape(k, kind, attrs, 4);
    }

    fn finish_shape(&mut self, k: usize, kind: &str, attrs: &[Attr], at: usize) {
        let shape = &mut self.shapes[k];
        put(shape, "sprite", attrs.get(at));
        put(shape, "tags", attrs.get(at + 1));
        put(shape, "link", attrs.get(at + 2));
        shape.kind = kind.to_string();
        shape.parent = Some(self.current);
    }

    fn boundary(&mut self, attrs: &[Attr], fallback: &str) -> usize {
        let k = element(&mut self.boundaries, &mut self.boundary_index, &attrs[0], attrs.get(1));
        match attrs.get(2) {
            None => self.boundaries[k].type_ = Some(fallback.to_string()),
            Some(_) => put(&mut self.boundaries[k], "type", attrs.get(2)),
        }
        k
    }

    fn open(&mut self, k: usize) {
        self.boundaries[k].parent = Some(self.current);
        self.stack.push(self.current);
        self.current = k;
    }

    // alias, label, ?type, ?tags, $link
    pub fn add_person_or_system_boundary(&mut self, attrs: &[Attr], fallback: &str) {
        let k = self.boundary(attrs, fallback);
        put(&mut self.boundaries[k], "tags", attrs.get(3));
        put(&mut self.boundaries[k], "link", attrs.get(4));
        self.open(k);
    }

    // alias, label, ?type, ?descr, ?sprite, ?tags, $link
    pub fn add_deployment_node(&mut self, node_type: &str, attrs: &[Attr]) {
        let k = self.boundary(attrs, "node");
        let b = &mut self.boundaries[k];
        text(b, "descr", attrs.get(3));
        put(b, "sprite", attrs.get(4));
        put(b, "tags", attrs.get(5));
        put(b, "link", attrs.get(6));
        b.node_type = Some(node_type.to_string());
        self.open(k);
    }

    pub fn pop_boundary_parse_stack(&mut self) {
        self.current = self.stack.pop().unwrap_or(0);
    }

    // from, to, label, ?techn, ?descr, ?sprite, ?tags, $link
    pub fn add_rel(&mut self, type_: &str, attrs: &[Attr]) {
        let (Some(from), Some(to), Some(_)) = (attrs.first(), attrs.get(1), attrs.get(2)) else { return };
        let known = match (from, to) {
            (Attr::Str(f), Attr::Str(t)) => Some((f.clone(), t.clone())),
            _ => None,
        };
        let existing = known.as_ref().and_then(|(f, t)| self.rel_index.get(f).and_then(|m| m.get(t))).copied();
        let k = match existing {
            Some(k) => k,
            None => {
                let k = self.rels.len();
                let mut rel = Rel { type_: type_.to_string(), ..Default::default() };
                match from {
                    Attr::Str(f) => rel.from = f.clone(),
                    Attr::Named(n) => rel.from_attr = Some(n.clone()),
                }
                match to {
                    Attr::Str(t) => rel.to = t.clone(),
                    Attr::Named(n) => rel.to_attr = Some(n.clone()),
                }
                self.rels.push(rel);
                if let Some((f, t)) = known {
                    self.rel_index.entry(f).or_default().insert(t, k);
                }
                k
            }
        };
        let rel = &mut self.rels[k];
        rel.type_ = type_.to_string();
        label(rel, attrs.get(2));
        text(rel, "techn", attrs.get(3));
        text(rel, "descr", attrs.get(4));
        put(rel, "sprite", attrs.get(5));
        put(rel, "tags", attrs.get(6));
        put(rel, "link", attrs.get(7));
    }

    pub fn get_c4_shape(&self, alias: &str) -> Option<ElRef> {
        self.shape_index.get(alias).map(|&k| ElRef::Shape(k)).or_else(|| self.boundary_index.get(alias).map(|&k| ElRef::Boundary(k)))
    }

    pub fn el_mut(&mut self, r: ElRef) -> &mut Element {
        match r {
            ElRef::Shape(k) => &mut self.shapes[k],
            ElRef::Boundary(k) => &mut self.boundaries[k],
        }
    }

    // elementName, ?bgColor, ?fontColor, ?borderColor, ?shadowing, ?shape, ?sprite, ?techn, ?legendText, ?legendSprite
    pub fn update_el_style(&mut self, attrs: &[Attr]) {
        let Some(Attr::Str(name)) = attrs.first() else { return };
        let Some(r) = self.get_c4_shape(name) else { return };
        let el = self.el_mut(r);
        for (k, field) in ["bgColor", "fontColor", "borderColor", "shadowing", "shape", "sprite", "techn", "legendText", "legendSprite"].iter().enumerate() {
            put(el, field, attrs.get(k + 1));
        }
    }

    // from, to, ?textColor, ?lineColor, ?offsetX, ?offsetY
    pub fn update_rel_style(&mut self, attrs: &[Attr]) {
        let (Some(Attr::Str(from)), Some(Attr::Str(to))) = (attrs.first(), attrs.get(1)) else { return };
        let Some(&k) = self.rel_index.get(from).and_then(|m| m.get(to)) else { return };
        let rel = &mut self.rels[k];
        put(rel, "textColor", attrs.get(2));
        put(rel, "lineColor", attrs.get(3));
        match attrs.get(4) {
            Some(Attr::Str(x)) => rel.offset_x = Some(js::parse_int(x, 10)),
            Some(Attr::Named(n)) => named(rel, n),
            None => {}
        }
        match attrs.get(5) {
            Some(Attr::Str(y)) => rel.offset_y = Some(js::parse_int(y, 10)),
            Some(Attr::Named(n)) => named(rel, n),
            None => {}
        }
    }

    // ?c4ShapeInRow, ?c4BoundaryInRow. As in Mermaid, the slot decides which one a value sets.
    pub fn update_layout_config(&mut self, attrs: &[Attr]) {
        let count = |attr: Option<&Attr>| match attr {
            None => f64::NAN,
            Some(Attr::Str(s)) => js::parse_int(s, 10),
            Some(Attr::Named(n)) => js::parse_int(&n.value, 10),
        };
        let shapes = count(attrs.first());
        let boundaries = count(attrs.get(1));
        if shapes >= 1.0 {
            self.shape_in_row = shapes;
        }
        if boundaries >= 1.0 {
            self.boundary_in_row = boundaries;
        }
    }
}
