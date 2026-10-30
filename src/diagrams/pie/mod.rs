// Port of src/diagrams/pie/index.ts and model.ts.

pub mod parser;
pub mod render;

use crate::diagrams::info::newline_dedent;
use crate::diagrams::Diagram;
use crate::errors::{PeleError, PeleErrorCode};
use crate::js;
use parser::PieAst;

pub struct PieModel {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub show_data: bool,
    // A Map: the first value of a label wins, in insertion order.
    pub sections: Vec<(String, f64)>,
}

pub fn build_pie(ast: PieAst, title: Option<&str>) -> Result<PieModel, PeleError> {
    let mut sections: Vec<(String, f64)> = Vec::new();
    for (label, value) in ast.sections {
        if value < 0.0 {
            return Err(PeleError::typed(
                format!("\"{}\" has invalid value: {}. Negative values are not allowed in pie charts. All slice values must be >= 0.", label, js::num_str(value)),
                PeleErrorCode::Semantic,
                "pie",
            ));
        }
        if !sections.iter().any(|(l, _)| *l == label) {
            sections.push((label, value));
        }
    }
    Ok(PieModel {
        title: ast.title.filter(|t| !t.is_empty()).or(title.map(str::to_string)),
        acc_title: ast.acc_title.filter(|t| !t.is_empty()).map(|t| js::trim_start(&t).to_string()),
        acc_descr: ast.acc_descr.filter(|t| !t.is_empty()).map(|d| newline_dedent(&d)),
        show_data: ast.show_data,
        sections,
    })
}

pub static DIAGRAM: Diagram = Diagram {
    type_: "pie",
    section: None,
    keep_comments: false,
    parse: |source, _config, title, _line_offset| Ok(Box::new(build_pie(parser::parse_pie(source)?, title)?)),
    render: |model, config, options| Ok(render::render_pie(model.downcast_ref::<PieModel>().unwrap(), config, options)),
};
