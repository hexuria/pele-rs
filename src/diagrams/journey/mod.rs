// Port of src/diagrams/journey/index.ts.

pub mod db;
pub mod lexer;
pub mod parser;
pub mod render;

use crate::diagrams::Diagram;
use db::JourneyDb;

pub static DIAGRAM: Diagram = Diagram {
    type_: "journey",
    section: None,
    keep_comments: false,
    parse: |source, _config, title, _line_offset| {
        let mut db = JourneyDb::default();
        db.title = title.map(str::to_string);
        parser::parse_journey(source, &mut db)?;
        Ok(Box::new(db))
    },
    render: |model, _config, options| Ok(render::render_journey(model.downcast_ref::<JourneyDb>().unwrap(), options)),
};
