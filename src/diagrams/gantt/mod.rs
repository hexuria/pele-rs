// Port of src/diagrams/gantt/index.ts.

pub mod axis;
pub mod clock;
pub mod dates;
pub mod db;
pub mod lexer;
pub mod parse_date;
pub mod parser;
pub mod render;

use crate::diagrams::Diagram;
use crate::util::yaml::Value;
use db::GanttDb;
use std::cell::RefCell;

// Scheduling fills in the tasks' times at render time, as upstream's `getTasks` does.
pub struct GanttModel(pub RefCell<GanttDb>);

pub static DIAGRAM: Diagram = Diagram {
    type_: "gantt",
    section: None,
    keep_comments: false,
    parse: |source, config, title, _line_offset| {
        let mut db = GanttDb::default();
        if let Some(t) = title.filter(|t| !t.is_empty()) {
            db.title = Some(t.to_string());
        }
        if let Some(Value::Str(mode)) = config.get("gantt").and_then(|g| g.as_map()).and_then(|g| g.get("displayMode")) {
            db.display_mode = mode.clone();
        }
        parser::parse_gantt(source, &mut db)?;
        Ok(Box::new(GanttModel(RefCell::new(db))))
    },
    render: |model, config, options| render::render_gantt(&mut model.downcast_ref::<GanttModel>().unwrap().0.borrow_mut(), config, options),
};
