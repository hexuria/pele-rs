// Port of src/diagrams/kanban/index.ts and parser.ts.

pub mod db;
pub mod render;

use crate::diagrams::common::outline::parse_outline;
use crate::diagrams::Diagram;
use db::KanbanDb;

pub static DIAGRAM: Diagram = Diagram {
    type_: "kanban",
    section: None,
    keep_comments: false,
    parse: |source, _config, title, _line_offset| {
        let mut db = KanbanDb::default();
        db.title = title.map(str::to_string);
        parse_outline(source, &mut db, true)?;
        Ok(Box::new(db))
    },
    render: |model, config, options| Ok(render::render_kanban(model.downcast_ref::<KanbanDb>().unwrap(), config, options)),
};
