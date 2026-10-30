// Port of src/diagrams/class (index.ts): the class diagram.

pub mod db;
pub mod graph;
pub mod lexer;
pub mod markers;
pub mod members;
pub mod parser;
pub mod render;
pub mod tokens;
pub mod types;

use crate::diagrams::Diagram;
use db::ClassDb;

pub static DIAGRAM: Diagram = Diagram {
    type_: "class",
    section: None,
    keep_comments: false,
    parse: |source, _config, title, _line_offset| {
        let mut db = ClassDb::new();
        if let Some(t) = title.filter(|t| !t.is_empty()) {
            db.title = Some(t.to_string());
        }
        parser::parse_class_diagram(source, &mut db)?;
        Ok(Box::new(db))
    },
    render: |model, config, options| Ok(render::render_class(model.downcast_ref::<ClassDb>().unwrap(), config, options)),
};
