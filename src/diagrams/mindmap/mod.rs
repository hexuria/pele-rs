// Port of src/diagrams/mindmap/index.ts and parser.ts.

pub mod db;
pub mod render;

use crate::diagrams::common::outline::parse_outline;
use crate::diagrams::Diagram;
use db::MindmapDb;

pub static DIAGRAM: Diagram = Diagram {
    type_: "mindmap",
    section: None,
    keep_comments: false,
    parse: |source, _config, title, _line_offset| {
        let mut db = MindmapDb::default();
        db.title = title.map(str::to_string);
        parse_outline(source, &mut db, false)?;
        Ok(Box::new(db))
    },
    render: |model, config, options| Ok(render::render_mindmap(model.downcast_ref::<MindmapDb>().unwrap(), config, options)),
};
