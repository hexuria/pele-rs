// Port of src/diagrams/c4/index.ts.

pub mod db;
pub mod layout;
pub mod lexer;
pub mod parser;
pub mod render;
pub mod route;

use crate::diagrams::Diagram;
use db::C4Db;

pub static DIAGRAM: Diagram = Diagram {
    type_: "c4",
    section: None,
    keep_comments: false,
    parse: |source, _config, title, _line_offset| {
        let mut db = C4Db::default();
        if let Some(t) = title.filter(|t| !t.is_empty()) {
            db.title = Some(t.to_string());
        }
        parser::parse_c4(source, &mut db)?;
        Ok(Box::new(db))
    },
    render: |model, config, options| render::render_c4(model.downcast_ref::<C4Db>().unwrap(), config, options),
};
