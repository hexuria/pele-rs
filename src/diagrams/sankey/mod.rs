// Port of src/diagrams/sankey/index.ts and db.ts.

pub mod layout;
pub mod lexer;
pub mod parser;
pub mod render;

use crate::diagrams::Diagram;

pub struct SankeyNode {
    pub id: String,
    pub index: usize,
}

pub struct SankeyLink {
    pub source: usize,
    pub target: usize,
    pub value: f64,
}

#[derive(Default)]
pub struct SankeyDb {
    pub nodes: Vec<SankeyNode>,
    pub links: Vec<SankeyLink>,
    pub title: Option<String>,
    lookup: std::collections::HashMap<String, usize>,
}

impl SankeyDb {
    pub fn find_or_create_node(&mut self, id: &str) -> usize {
        if let Some(&i) = self.lookup.get(id) {
            return i;
        }
        let index = self.nodes.len();
        self.lookup.insert(id.to_string(), index);
        self.nodes.push(SankeyNode { id: id.to_string(), index });
        index
    }
    pub fn add_link(&mut self, source: usize, target: usize, value: f64) {
        self.links.push(SankeyLink { source, target, value });
    }
}

pub static DIAGRAM: Diagram = Diagram {
    type_: "sankey",
    section: None,
    keep_comments: false,
    parse: |source, _config, title, _line_offset| {
        let mut db = SankeyDb::default();
        if let Some(t) = title.filter(|t| !t.is_empty()) {
            db.title = Some(t.to_string());
        }
        parser::parse_sankey(&parser::prepare_text_for_parsing(source, None), &mut db, source)?;
        Ok(Box::new(db))
    },
    render: |model, config, options| render::render_sankey(model.downcast_ref::<SankeyDb>().unwrap(), config, options),
};
