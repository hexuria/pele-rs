// Port of src/diagrams/er (index.ts): the entity relationship diagram.

pub mod db;
pub mod graph;
pub mod lexer;
pub mod markers;
pub mod parser;
pub mod render;
pub mod tokens;
pub mod types;

use crate::diagrams::Diagram;
use db::ErDb;

pub static DIAGRAM: Diagram = Diagram {
    type_: "er",
    section: None,
    keep_comments: false,
    parse: |source, _config, title, _line_offset| {
        let mut db = ErDb::new();
        if let Some(t) = title.filter(|t| !t.is_empty()) {
            db.title = Some(t.to_string());
        }
        parser::parse_er(source, &mut db)?;
        Ok(Box::new(db))
    },
    render: |model, config, options| Ok(render::render_er(model.downcast_ref::<ErDb>().unwrap(), config, options)),
};
