// Port of src/diagrams/block/db.ts and types.ts. Blocks live in an arena; upstream's shared
// object references are indexes into it.

use crate::errors::{PeleError, PeleErrorCode};
use crate::js;
use std::collections::{HashMap, HashSet};

// One shape for blocks, edges, and the statements that set columns, classes, and styles.
#[derive(Clone, Default, Debug)]
pub struct Block {
    pub id: Option<String>,
    pub type_: Option<String>,
    pub label: Option<String>,
    // The statements inside a composite, as parsed.
    pub inner: Vec<Stmt>,
    // The blocks inside a composite, as arena indexes, once the hierarchy is set.
    pub children: Vec<usize>,
    pub columns: Option<f64>,
    pub width_in_columns: Option<f64>,
    // Number of cells a `space` statement asked for.
    pub width: Option<f64>,
    pub directions: Option<Vec<String>>,
    pub classes: Option<Vec<String>>,
    pub styles: Option<Vec<String>>,
    pub color_index: Option<usize>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub arrow_type_start: Option<String>,
    pub arrow_type_end: Option<String>,
    pub thickness: Option<String>,
    pub pattern: Option<String>,
    pub css: Option<String>,
    pub style_class: Option<String>,
    pub styles_str: Option<String>,
}

// A statement with links is a list: block, edge, block, and so on.
#[derive(Clone, Debug)]
pub enum Stmt {
    One(Block),
    Chain(Vec<Block>),
}

pub struct BlockClassDef {
    pub styles: Vec<String>,
    pub text_styles: Vec<String>,
}

const MAX_SPACES: f64 = 100000.0;

pub fn type_str_to_type(type_str: Option<&str>) -> &'static str {
    match type_str {
        Some("[]") => "square",
        Some("()") => "round",
        Some("(())") => "circle",
        Some(">]") => "rect_left_inv_arrow",
        Some("{}") => "diamond",
        Some("{{}}") => "hexagon",
        Some("([])") => "stadium",
        Some("[[]]") => "subroutine",
        Some("[()]") => "cylinder",
        Some("((()))") => "doublecircle",
        Some("[//]") => "lean_right",
        Some("[\\\\]") => "lean_left",
        Some("[/\\]") => "trapezoid",
        Some("[\\/]") => "inv_trapezoid",
        Some("<[]>") => "block_arrow",
        _ => "na",
    }
}

pub fn edge_str_to_edge_data(s: &str) -> &'static str {
    match js::trim(s).chars().last() {
        Some('x') => "arrow_cross",
        Some('o') => "arrow_circle",
        Some('>') => "arrow_point",
        _ => "",
    }
}

pub fn edge_str_to_edge_start_data(s: &str) -> &'static str {
    match js::trim(s).chars().next() {
        Some('x') => "arrow_cross",
        Some('o') => "arrow_circle",
        Some('<') => "arrow_point",
        _ => "arrow_open",
    }
}

pub fn edge_str_to_thickness(s: &str) -> &'static str {
    if s.contains("==") {
        "thick"
    } else {
        "normal"
    }
}

pub fn edge_str_to_pattern(s: &str) -> &'static str {
    if s.contains(".-") {
        "dotted"
    } else {
        "solid"
    }
}

struct Frame {
    list: Vec<Block>,
    at: usize,
    parent: usize,
    children: Vec<usize>,
}

fn frame(statements: Vec<Stmt>, parent: usize) -> Frame {
    let mut list = Vec::new();
    for s in statements {
        match s {
            Stmt::One(b) => list.push(b),
            Stmt::Chain(bs) => list.extend(bs),
        }
    }
    Frame { list, at: 0, parent, children: Vec::new() }
}

pub struct BlockDb {
    pub title: Option<String>,
    // Index 0 is the root.
    pub arena: Vec<Block>,
    pub edges: Vec<Block>,
    // A Map, in insertion order.
    pub classes: Vec<(String, BlockClassDef)>,
    // Mermaid keeps the root in here too, which makes a block called `root` vanish. Here it stays out.
    pub by_id: HashMap<String, usize>,
    pending: HashSet<String>,
    edge_count: HashMap<String, usize>,
    ids: usize,
    colors: usize,
    spaces: f64,
}

impl Default for BlockDb {
    fn default() -> Self {
        let root = Block { id: Some("root".into()), type_: Some("composite".into()), columns: Some(-1.0), ..Default::default() };
        BlockDb { title: None, arena: vec![root], edges: Vec::new(), classes: Vec::new(), by_id: HashMap::new(), pending: HashSet::new(), edge_count: HashMap::new(), ids: 0, colors: 0, spaces: 0.0 }
    }
}

impl BlockDb {
    // Mermaid's ids are random. These are counted, so the same text always gives the same output.
    pub fn generate_id(&mut self) -> String {
        self.ids += 1;
        format!("id-{}", self.ids)
    }

    pub fn class(&self, name: &str) -> Option<&BlockClassDef> {
        self.classes.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }

    fn add_style_class(&mut self, id: &str, style_attributes: &str) {
        let k = match self.classes.iter().position(|(c, _)| c == id) {
            Some(k) => k,
            None => {
                self.classes.push((id.to_string(), BlockClassDef { styles: Vec::new(), text_styles: Vec::new() }));
                self.classes.len() - 1
            }
        };
        for attribute in style_attributes.split(',') {
            let fixed = match attribute.find(';') {
                None => attribute.to_string(),
                Some(semi) => format!("{}{}", &attribute[..semi], &attribute[semi + 1..]),
            };
            let fixed = js::trim(&fixed).to_string();
            if attribute.contains("color") {
                self.classes[k].1.text_styles.push(fixed.replacen("fill", "bgFill", 1).replacen("color", "fill", 1));
            }
            self.classes[k].1.styles.push(fixed);
        }
    }

    // Mermaid throws on a style for an unknown id, and a class for one makes the block of that
    // id disappear when it is declared later. Here both wait for the block.
    fn named(&mut self, id: &str) -> usize {
        if let Some(&k) = self.by_id.get(id) {
            return k;
        }
        let k = self.arena.len();
        self.arena.push(Block { id: Some(id.to_string()), type_: Some("na".into()), ..Default::default() });
        self.by_id.insert(id.to_string(), k);
        self.pending.insert(id.to_string());
        k
    }

    pub fn set_hierarchy(&mut self, statements: Vec<Stmt>) -> Result<(), PeleError> {
        let mut stack = vec![frame(statements, 0)];
        while let Some(f) = stack.last_mut() {
            if f.at == f.list.len() {
                let parent = f.parent;
                let children = std::mem::take(&mut f.children);
                self.arena[parent].children = children;
                stack.pop();
                continue;
            }
            let mut block = std::mem::take(&mut f.list[f.at]);
            f.at += 1;
            let parent = f.parent;
            match block.type_.as_deref() {
                Some("classDef") => {
                    self.add_style_class(block.id.as_deref().unwrap_or(""), block.css.as_deref().unwrap_or(""));
                    continue;
                }
                Some("applyClass") => {
                    let class = block.style_class.clone().unwrap_or_default();
                    // Mermaid looks each id up before trimming it; here it is trimmed first.
                    for id in block.id.as_deref().unwrap_or("").split(',') {
                        let k = self.named(js::trim(id));
                        self.arena[k].classes.get_or_insert_with(Vec::new).push(class.clone());
                    }
                    continue;
                }
                Some("applyStyles") => {
                    if let Some(styles) = block.styles_str.as_deref().filter(|s| !s.is_empty()) {
                        let k = self.named(block.id.as_deref().unwrap_or(""));
                        self.arena[k].styles = Some(styles.split(',').map(str::to_string).collect());
                    }
                    continue;
                }
                Some("column-setting") => {
                    self.arena[parent].columns = Some(block.columns.unwrap_or(-1.0));
                    continue;
                }
                Some("edge") => {
                    let id = block.id.clone().unwrap_or_default();
                    let count = self.edge_count.get(&id).copied().unwrap_or(0) + 1;
                    self.edge_count.insert(id.clone(), count);
                    block.id = Some(format!("{}-{}", count, id));
                    self.edges.push(block);
                    continue;
                }
                _ => {}
            }

            // A composite whose header was a link statement has no id of its own.
            if block.id.is_none() {
                block.id = Some(self.generate_id());
            }
            let id = block.id.clone().unwrap();
            if block.label.as_deref().is_none_or(str::is_empty) {
                block.label = Some(if block.type_.as_deref() == Some("composite") { String::new() } else { id.clone() });
            }
            let known = self.by_id.get(&id).copied();
            let fresh = known.is_none() || self.pending.remove(&id);
            let inner = std::mem::take(&mut block.inner);
            let index;
            if !fresh {
                let k = known.unwrap();
                if block.type_.as_deref() != Some("na") {
                    self.arena[k].type_ = block.type_.clone();
                }
                if block.label.as_deref() != Some(id.as_str()) {
                    self.arena[k].label = block.label.clone();
                }
                // The repeat is not in the tree, but what it holds is still read.
                index = self.arena.len();
                self.arena.push(block);
            } else {
                if let Some(k) = known {
                    block.classes = self.arena[k].classes.clone();
                    block.styles = self.arena[k].styles.clone();
                }
                if block.type_.as_deref() == Some("composite") {
                    block.color_index = Some(self.colors);
                    self.colors += 1;
                }
                index = self.arena.len();
                self.arena.push(block);
                self.by_id.insert(id.clone(), index);
            }

            let f = stack.last_mut().unwrap();
            if self.arena[index].type_.as_deref() == Some("space") {
                let width = self.arena[index].width.unwrap_or(1.0);
                if !(width <= MAX_SPACES - self.spaces) {
                    return Err(PeleError::typed(format!("A block diagram can hold at most {} space blocks.", js::num_str(MAX_SPACES)), PeleErrorCode::Limit, "block"));
                }
                self.spaces += width.max(0.0);
                let mut j = 0.0;
                while j < width {
                    let mut copy = self.arena[index].clone();
                    copy.id = Some(format!("{}-{}", id, js::num_str(j)));
                    copy.children = Vec::new();
                    let k = self.arena.len();
                    self.by_id.insert(copy.id.clone().unwrap(), k);
                    self.arena.push(copy);
                    f.children.push(k);
                    j += 1.0;
                }
            } else if fresh {
                f.children.push(index);
            }
            if !inner.is_empty() {
                stack.push(frame(inner, index));
            }
        }
        Ok(())
    }
}
