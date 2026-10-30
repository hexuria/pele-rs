// Port of src/diagrams/xychart/index.ts.

pub mod db;
pub mod lexer;
pub mod parser;
pub mod render;
pub mod ticks;

use crate::diagrams::Diagram;
use db::XyChartDb;

pub static DIAGRAM: Diagram = Diagram {
    type_: "xychart",
    section: Some("xyChart"),
    keep_comments: false,
    parse: |source, _config, title, _line_offset| {
        let mut db = XyChartDb { title: title.map(str::to_string), ..Default::default() };
        parser::parse_xychart(source, &mut db)?;
        Ok(Box::new(db))
    },
    render: |model, config, options| render::render_xychart(model.downcast_ref::<XyChartDb>().unwrap(), config, options),
};
