// Port of src/diagrams/sequence (index.ts): the sequence diagram.

pub mod actors;
pub mod colors;
pub mod db;
pub mod lexer;
pub mod parser;
pub mod render;
pub mod tokens;
pub mod types;

use crate::diagrams::Diagram;
use crate::util::yaml::Value;
use db::{SeqDb, SeqDbOptions};

pub static DIAGRAM: Diagram = Diagram {
    type_: "sequence",
    section: None,
    keep_comments: false,
    parse: |source, config, title, _line_offset| {
        let seq = config.get("sequence").and_then(|v| v.as_map());
        let mut db = SeqDb::new(SeqDbOptions {
            wrap: match config.get("wrap") {
                Some(Value::Bool(b)) => Some(*b),
                _ => None,
            },
            sequence_wrap: matches!(seq.and_then(|s| s.get("wrap")), Some(Value::Bool(true))),
        });
        if let Some(t) = title.filter(|t| !t.is_empty()) {
            db.set_diagram_title(t);
        }
        parser::parse_sequence(source, &mut db)?;
        Ok(Box::new(db))
    },
    render: |model, config, options| Ok(render::render_sequence(model.downcast_ref::<SeqDb>().unwrap(), config, options)),
};
