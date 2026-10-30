// Port of src/diagrams/info/index.ts and model.ts.

pub mod parser;
pub mod render;

use crate::diagrams::Diagram;
use crate::js;
use parser::InfoAst;

pub struct InfoModel {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
}

pub fn build_info(ast: InfoAst, title: Option<&str>) -> InfoModel {
    InfoModel {
        title: ast.title.filter(|t| !t.is_empty()).or(title.map(str::to_string)),
        acc_title: ast.acc_title.filter(|t| !t.is_empty()).map(|t| js::trim_start(&t).to_string()),
        acc_descr: ast.acc_descr.filter(|t| !t.is_empty()).map(|d| newline_dedent(&d)),
    }
}

/// `.replace(/\n\s+/g, '\n')`.
pub fn newline_dedent(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        if c[i] == '\n' && i + 1 < c.len() && js::is_space(c[i + 1]) {
            out.push('\n');
            i += 1;
            while i < c.len() && js::is_space(c[i]) {
                i += 1;
            }
            continue;
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

pub static DIAGRAM: Diagram = Diagram {
    type_: "info",
    section: None,
    keep_comments: false,
    parse: |source, _config, title, _line_offset| Ok(Box::new(build_info(parser::parse_info(source)?, title))),
    render: |model, _config, options| Ok(render::render_info(model.downcast_ref::<InfoModel>().unwrap(), options)),
};
