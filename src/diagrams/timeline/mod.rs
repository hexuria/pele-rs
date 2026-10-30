// Port of src/diagrams/timeline/index.ts.

pub mod db;
pub mod lexer;
pub mod parser;
pub mod render;

use crate::diagrams::Diagram;
use db::TimelineDb;

pub static DIAGRAM: Diagram = Diagram {
    type_: "timeline",
    section: None,
    keep_comments: false,
    parse: |source, _config, title, _line_offset| {
        let mut db = TimelineDb::default();
        db.title = title.map(str::to_string);
        parser::parse_timeline(source, &mut db)?;
        Ok(Box::new(db))
    },
    render: |model, config, options| Ok(render::render_timeline(model.downcast_ref::<TimelineDb>().unwrap(), config, options)),
};
