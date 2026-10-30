// Port of src/diagrams/usecase (index.ts, types.ts).

pub mod db;
pub mod graph;
pub mod json;
pub mod lexer;
pub mod parser;
pub mod render;

use crate::diagrams::Diagram;
use json::Json;

/// Arrow types, in the order of upstream's `Arrow` enum.
pub mod arrow {
    pub const SOLID_ARROW: u8 = 0;
    pub const BACK_ARROW: u8 = 1;
    pub const LINE_SOLID: u8 = 2;
    pub const CIRCLE_ARROW: u8 = 3;
    pub const CROSS_ARROW: u8 = 4;
    pub const CIRCLE_ARROW_REVERSED: u8 = 5;
    pub const CROSS_ARROW_REVERSED: u8 = 6;
}

/// A JS `Map` keyed by string: insertion order, a replaced value keeps its place, a deleted key
/// set again goes last.
#[derive(Clone)]
pub struct OMap<T>(pub Vec<(String, T)>);

impl<T> Default for OMap<T> {
    fn default() -> Self {
        OMap(Vec::new())
    }
}

impl<T> OMap<T> {
    pub fn get(&self, k: &str) -> Option<&T> {
        self.0.iter().find(|(key, _)| key == k).map(|(_, v)| v)
    }
    pub fn get_mut(&mut self, k: &str) -> Option<&mut T> {
        self.0.iter_mut().find(|(key, _)| key == k).map(|(_, v)| v)
    }
    pub fn set(&mut self, k: &str, v: T) {
        match self.get_mut(k) {
            Some(slot) => *slot = v,
            None => self.0.push((k.to_string(), v)),
        }
    }
    pub fn delete(&mut self, k: &str) {
        self.0.retain(|(key, _)| key != k);
    }
    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.0.iter().map(|(_, v)| v)
    }
}

#[derive(Clone)]
pub struct Actor {
    pub id: String,
    pub label: String,
    pub markdown: bool,
    // normal, hollow, awesome or icon
    pub type_: String,
    pub icon: Option<String>,
    pub business: bool,
    pub stereotype: Option<String>,
    pub parent_id: Option<String>,
    pub classes: Vec<String>,
    pub styles: Vec<String>,
}

#[derive(Clone)]
pub struct UseCase {
    pub id: String,
    pub label: String,
    pub markdown: bool,
    // ellipse or rect
    pub shape: &'static str,
    pub business: bool,
    pub stereotype: Option<String>,
    pub parent_id: Option<String>,
    pub classes: Vec<String>,
    pub styles: Vec<String>,
}

pub struct Boundary {
    pub id: String,
    pub label: String,
    pub markdown: bool,
    // rect or package
    pub type_: &'static str,
    pub members: Vec<String>,
    pub classes: Vec<String>,
    pub styles: Vec<String>,
}

pub struct Relationship {
    pub id: String,
    pub explicit_id: bool,
    pub source: String,
    pub target: String,
    // association, include, extend or generalization
    pub type_: &'static str,
    pub arrow_type: u8,
    pub label: Option<String>,
    pub markdown: Option<bool>,
    pub minlen: f64,
    pub classes: Vec<String>,
    pub styles: Vec<String>,
    pub animate: bool,
    pub animation: Option<String>,
}

pub struct Note {
    pub id: String,
    pub target: String,
    pub label: String,
    pub markdown: bool,
}

pub struct JsonNode {
    pub id: String,
    pub value: Vec<(String, Json)>,
    pub classes: Vec<String>,
    pub styles: Vec<String>,
}

pub struct UsecaseModel {
    pub actors: OMap<Actor>,
    pub use_cases: OMap<UseCase>,
    pub boundaries: OMap<Boundary>,
    pub relationships: Vec<Relationship>,
    pub notes: Vec<Note>,
    pub json_nodes: OMap<JsonNode>,
    pub class_defs: OMap<Vec<String>>,
    pub direction: String,
    pub title: Option<String>,
    pub acc_title: String,
    pub acc_descr: String,
}

impl Default for UsecaseModel {
    fn default() -> Self {
        UsecaseModel {
            actors: OMap::default(),
            use_cases: OMap::default(),
            boundaries: OMap::default(),
            relationships: Vec::new(),
            notes: Vec::new(),
            json_nodes: OMap::default(),
            class_defs: OMap::default(),
            direction: "LR".into(),
            title: None,
            acc_title: String::new(),
            acc_descr: String::new(),
        }
    }
}

pub static DIAGRAM: Diagram = Diagram {
    type_: "usecase",
    section: None,
    keep_comments: false,
    parse: |source, _config, title, _line_offset| Ok(Box::new(parser::parse_usecase(source, title)?)),
    render: |model, config, options| Ok(render::render_usecase(model.downcast_ref::<UsecaseModel>().unwrap(), config, options)),
};
