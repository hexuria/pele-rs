// Port of src/diagrams/block/parser.ts.

use super::db::*;
use super::lexer::*;
use crate::errors::{syntax_error, PeleError};
use crate::js;

struct NodeInfo {
    id: String,
    label: Option<String>,
    type_str: Option<String>,
    directions: Option<Vec<String>>,
}

const STATEMENT_START: &str = "'NODE_ID', 'COLUMNS', 'SPACE_BLOCK', 'BLOCK_DIAGRAM_KEY', 'id-block', 'classDef', 'class', 'style'";

// The grammar inlines a document's last statement when it is a list.
fn close_document(mut list: Vec<Stmt>) -> Vec<Stmt> {
    if let Some(Stmt::Chain(_)) = list.last() {
        let Some(Stmt::Chain(last)) = list.pop() else { unreachable!() };
        list.extend(last.into_iter().map(Stmt::One));
    }
    list
}

struct P<'a> {
    src: &'a [u16],
    t: Tokens,
    i: usize,
}

impl P<'_> {
    fn kind(&self) -> u32 {
        self.t.types[self.i]
    }
    fn fail(&self, expected: &str) -> PeleError {
        let t = self.kind();
        if t == ERROR {
            return syntax_error("block", self.src, self.t.starts[self.i], "", true);
        }
        syntax_error("block", self.src, self.t.starts[self.i], &format!("Expecting {}, got '{}'", expected, TOKEN_NAMES[t as usize]), false)
    }
    fn take(&mut self) -> String {
        self.i += 1;
        self.t.texts[self.i - 1].clone()
    }
    fn expect(&mut self, ty: u32) -> Result<String, PeleError> {
        if self.kind() != ty {
            return Err(self.fail(&format!("'{}'", TOKEN_NAMES[ty as usize])));
        }
        Ok(self.take())
    }
    fn node(&mut self) -> Result<NodeInfo, PeleError> {
        let id = self.expect(NODE_ID)?;
        if self.kind() == NODE_DSTART {
            let open = self.take();
            let label = self.expect(STR)?;
            let close = self.expect(NODE_DEND)?;
            return Ok(NodeInfo { id, label: Some(label), type_str: Some(open + &close), directions: None });
        }
        if self.kind() == BLOCK_ARROW_START {
            let open = self.take();
            let label = self.expect(STR)?;
            let mut directions = vec![self.expect(DIR)?];
            while self.kind() == DIR {
                directions.push(self.take());
            }
            let close = self.expect(BLOCK_ARROW_END)?;
            return Ok(NodeInfo { id, label: Some(label), type_str: Some(open + &close), directions: Some(directions) });
        }
        Ok(NodeInfo { id, label: None, type_str: None, directions: None })
    }
    fn node_statement(&mut self) -> Result<Stmt, PeleError> {
        let first = self.node()?;
        let size = if self.kind() == SIZE { js::parse_int(&self.take(), 10) } else { 1.0 };
        let mut from = Block {
            id: Some(first.id),
            label: first.label,
            type_: Some(type_str_to_type(first.type_str.as_deref()).into()),
            directions: first.directions,
            width_in_columns: Some(size),
            ..Default::default()
        };
        let mut chain: Option<Vec<Block>> = None;
        while self.kind() == LINK || self.kind() == START_LINK {
            let mut label = String::new();
            let t = self.kind();
            self.i += 1;
            if t == START_LINK {
                self.expect(LINK_LABEL)?;
                label = self.expect(STR)?;
                self.expect(LINK)?;
            }
            let link = self.t.texts[self.i - 1].clone();
            let to = self.node()?;
            // Mermaid keeps only the last link of `A --> B --> C` and loses its start. Here every link counts.
            let c = chain.get_or_insert_with(|| vec![Block { id: from.id.clone(), label: from.label.clone(), type_: from.type_.clone(), directions: from.directions.clone(), ..Default::default() }]);
            from = Block { id: Some(to.id.clone()), label: to.label, type_: Some(type_str_to_type(to.type_str.as_deref()).into()), directions: to.directions.clone(), ..Default::default() };
            let prev = c[c.len() - 1].id.clone().unwrap_or_default();
            c.push(Block {
                id: Some(format!("{}-{}", prev, to.id)),
                start: Some(prev),
                end: Some(to.id),
                label: Some(label),
                type_: Some("edge".into()),
                thickness: Some(edge_str_to_thickness(&link).into()),
                pattern: Some(edge_str_to_pattern(&link).into()),
                directions: to.directions,
                arrow_type_end: Some(edge_str_to_edge_data(&link).into()),
                arrow_type_start: Some(edge_str_to_edge_start_data(&link).into()),
                ..Default::default()
            });
            c.push(from.clone());
        }
        Ok(match chain {
            Some(c) => Stmt::Chain(c),
            None => Stmt::One(from),
        })
    }
}

pub fn parse_block(src: &[u16], db: &mut BlockDb) -> Result<(), PeleError> {
    let mut r = P { src, t: tokenize(src), i: 0 };
    r.expect(KEY)?;
    // Composites nest without recursion: each open one keeps the statement list of its parent.
    let mut open: Vec<(Vec<Stmt>, Option<Stmt>)> = Vec::new();
    let mut list: Vec<Stmt> = Vec::new();
    loop {
        match r.kind() {
            NODE_ID => list.push(r.node_statement()?),
            COLUMNS => {
                let columns = js::parse_int(&r.take(), 10);
                list.push(Stmt::One(Block { type_: Some("column-setting".into()), columns: Some(columns), ..Default::default() }));
            }
            SPACE_BLOCK => {
                let width = js::parse_int(&r.take(), 10);
                let id = db.generate_id();
                list.push(Stmt::One(Block { id: Some(id), type_: Some("space".into()), label: Some(String::new()), width: Some(width), ..Default::default() }));
            }
            KEY => {
                r.i += 1;
                open.push((std::mem::take(&mut list), None));
            }
            ID_BLOCK => {
                r.i += 1;
                if r.kind() != NODE_ID {
                    return Err(r.fail("'NODE_ID'"));
                }
                let header = r.node_statement()?;
                open.push((std::mem::take(&mut list), Some(header)));
            }
            END_KW => {
                if open.is_empty() || list.is_empty() {
                    return Err(r.fail(STATEMENT_START));
                }
                let (parent, header) = open.pop().unwrap();
                r.i += 1;
                let id = db.generate_id();
                let children = close_document(std::mem::take(&mut list));
                list = parent;
                list.push(Stmt::One(match header {
                    None => Block { id: Some(id), type_: Some("composite".into()), label: Some(String::new()), inner: children, ..Default::default() },
                    Some(Stmt::One(h)) => Block { type_: Some("composite".into()), inner: children, ..h },
                    // Spreading a list gives an object with no id or label.
                    Some(Stmt::Chain(_)) => Block { type_: Some("composite".into()), inner: children, ..Default::default() },
                }));
            }
            CLASSDEF => {
                r.i += 1;
                let id = js::trim(&r.expect(CLASSDEF_ID)?).to_string();
                let css = js::trim(&r.expect(CLASSDEF_STYLEOPTS)?).to_string();
                list.push(Stmt::One(Block { type_: Some("classDef".into()), id: Some(id), css: Some(css), ..Default::default() }));
            }
            CLASS => {
                r.i += 1;
                let id = js::trim(&r.expect(CLASSENTITY_IDS)?).to_string();
                let class = js::trim(&r.expect(STYLECLASS)?).to_string();
                list.push(Stmt::One(Block { type_: Some("applyClass".into()), id: Some(id), style_class: Some(class), ..Default::default() }));
            }
            STYLE => {
                r.i += 1;
                let id = js::trim(&r.expect(STYLE_ENTITY_IDS)?).to_string();
                let styles = js::trim(&r.expect(STYLE_DEFINITION_DATA)?).to_string();
                list.push(Stmt::One(Block { type_: Some("applyStyles".into()), id: Some(id), styles_str: Some(styles), ..Default::default() }));
            }
            EOF => {
                if !open.is_empty() {
                    return Err(r.fail(&format!("{}, 'end'", STATEMENT_START)));
                }
                if list.is_empty() {
                    return Err(r.fail(STATEMENT_START));
                }
                return db.set_hierarchy(close_document(list));
            }
            _ => {
                let also = if list.is_empty() {
                    ""
                } else if !open.is_empty() {
                    ", 'end'"
                } else {
                    ", 'EOF'"
                };
                return Err(r.fail(&format!("{}{}", STATEMENT_START, also)));
            }
        }
    }
}
