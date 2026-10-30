// Port of src/diagrams/git/index.ts.

pub mod db;
pub mod parser;
pub mod render;

use crate::diagrams::Diagram;
use db::GitModel;

pub static DIAGRAM: Diagram = Diagram {
    type_: "gitGraph",
    section: None,
    keep_comments: false,
    parse: |source, config, title, _line_offset| Ok(Box::new(db::build_git(parser::parse_git(source)?, config, title)?)),
    render: |model, config, options| Ok(render::render_git(model.downcast_ref::<GitModel>().unwrap(), config, options)),
};
