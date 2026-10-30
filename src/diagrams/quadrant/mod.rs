// Port of src/diagrams/quadrant/index.ts.

pub mod db;
pub mod lexer;
pub mod parser;
pub mod render;

use crate::diagrams::Diagram;
use db::QuadrantDb;

pub static DIAGRAM: Diagram = Diagram {
    type_: "quadrantChart",
    section: None,
    keep_comments: false,
    parse: |source, _config, title, _line_offset| {
        let mut db = QuadrantDb { title: title.map(str::to_string), ..Default::default() };
        parser::parse_quadrant(source, &mut db)?;
        Ok(Box::new(db))
    },
    render: |model, config, options| Ok(render::render_quadrant(model.downcast_ref::<QuadrantDb>().unwrap(), config, options)),
};
