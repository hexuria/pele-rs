// Port of src/diagrams/state (index.ts): the state diagram.

pub mod db;
pub mod graph;
pub mod lexer;
pub mod parser;
pub mod render;
pub mod tokens;
pub mod types;

use crate::diagrams::Diagram;
use db::StateDb;

pub static DIAGRAM: Diagram = Diagram {
    type_: "state",
    section: None,
    keep_comments: false,
    parse: |source, _config, title, _line_offset| {
        let mut db = StateDb::new();
        if let Some(t) = title.filter(|t| !t.is_empty()) {
            db.title = Some(t.to_string());
        }
        parser::parse_state(source, &mut db)?;
        Ok(Box::new(db))
    },
    render: |model, config, options| Ok(render::render_state(model.downcast_ref::<StateDb>().unwrap(), config, options)),
};
