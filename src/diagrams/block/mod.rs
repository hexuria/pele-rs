// Port of src/diagrams/block/index.ts.

pub mod arrows;
pub mod db;
pub mod layout;
pub mod lexer;
pub mod parser;
pub mod render;

use crate::diagrams::Diagram;
use db::BlockDb;

pub static DIAGRAM: Diagram = Diagram {
    type_: "block",
    section: None,
    keep_comments: false,
    parse: |source, _config, title, _line_offset| {
        let mut db = BlockDb::default();
        db.title = title.map(str::to_string);
        parser::parse_block(source, &mut db)?;
        Ok(Box::new(db))
    },
    render: |model, config, options| render::render_block(model.downcast_ref::<BlockDb>().unwrap(), config, options),
};
