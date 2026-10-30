// Port of src/diagrams/railroad (index.ts, db.ts, types.ts).

pub mod abnf;
pub mod common;
pub mod ebnf;
pub mod ir;
pub mod peg;
pub mod render;

use crate::diagrams::cynefin::collapse_indents;
use crate::diagrams::Diagram;
use crate::errors::{PeleError, PeleErrorCode};
use crate::js;
use common::{Grammar, Nodes, Parts};

// Which of Mermaid's four grammars the text was written in.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Notation {
    Railroad,
    Ebnf,
    Abnf,
    Peg,
}

pub struct RailroadModel {
    pub notation: Notation,
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub nodes: Nodes,
    // Every rule in source order, as (name, definition). A name defined twice appears twice.
    pub rules: Vec<(String, usize)>,
}

// RE_KEYWORD /^\s*railroad-(ebnf-|abnf-|peg-)?beta/i: Mermaid has a detector per keyword, each
// matching without regard to case.
fn notation_of(source: &[u16]) -> Option<Notation> {
    let text = js::from16(source);
    let rest = js::trim_start(&text);
    let lower: String = rest.chars().take(24).collect::<String>().to_ascii_lowercase();
    let after = lower.strip_prefix("railroad-")?;
    for (infix, notation) in [("ebnf-", Notation::Ebnf), ("abnf-", Notation::Abnf), ("peg-", Notation::Peg)] {
        if after.strip_prefix(infix).is_some_and(|a| a.starts_with("beta")) {
            return Some(notation);
        }
    }
    after.starts_with("beta").then_some(Notation::Railroad)
}

fn grammar(source: &[u16], notation: Notation, parts: &mut Parts, nodes: &mut Nodes) -> Result<(Grammar, Vec<(String, usize)>), PeleError> {
    Ok(match notation {
        Notation::Ebnf => {
            let ast = ebnf::parse_ebnf(source, parts)?;
            let rules = ebnf::ebnf_rules(&ast, parts, nodes);
            (ast, rules)
        }
        Notation::Abnf => {
            let ast = abnf::parse_abnf(source, parts)?;
            let rules = abnf::abnf_rules(&ast, parts, nodes);
            (ast, rules)
        }
        Notation::Peg => {
            let ast = peg::parse_peg(source, parts)?;
            let rules = peg::peg_rules(&ast, parts, nodes);
            (ast, rules)
        }
        Notation::Railroad => {
            let ast = ir::parse_ir(source, parts)?;
            let rules = ir::ir_rules(&ast, parts, nodes);
            (ast, rules)
        }
    })
}

pub fn parse_railroad(source: &[u16], title: Option<&str>) -> Result<RailroadModel, PeleError> {
    let Some(notation) = notation_of(source) else {
        return Err(PeleError::typed("No railroad diagram keyword found.", PeleErrorCode::Syntax, "railroad"));
    };
    let mut parts = Parts::default();
    let mut nodes = Nodes::default();
    let (ast, rules) = grammar(source, notation, &mut parts, &mut nodes)?;
    let mut model = RailroadModel { notation, title: title.filter(|t| !t.is_empty()).map(str::to_string), acc_title: None, acc_descr: None, nodes, rules };
    if let Some(d) = ast.acc_descr.filter(|d| !d.is_empty()) {
        model.acc_descr = Some(collapse_indents(&d));
    }
    if let Some(t) = ast.acc_title.filter(|t| !t.is_empty()) {
        model.acc_title = Some(js::trim_start(&t).to_string());
    }
    if let Some(t) = ast.title.filter(|t| !t.is_empty()) {
        model.title = Some(t);
    }
    Ok(model)
}

pub static DIAGRAM: Diagram = Diagram {
    type_: "railroad",
    section: None,
    keep_comments: false,
    parse: |source, _config, title, _line_offset| Ok(Box::new(parse_railroad(source, title)?)),
    render: |model, config, options| Ok(render::render_railroad(model.downcast_ref::<RailroadModel>().unwrap(), config, options)),
};
