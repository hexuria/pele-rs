// Port of src/diagrams/state/parser.ts.

use super::db::StateDb;
use super::lexer::tokenize;
use super::tokens::{self as t, TOKEN_NAMES};
use super::types::{Descr, StateNote, StateStmt, Stmt};
use crate::errors::{locate, syntax_error, PeleError, PeleErrorCode};
use crate::js;

const DIRECTIONS: [&str; 4] = ["TB", "BT", "RL", "LR"];
const STATEMENT: &str = "'NL', 'ID', 'HIDE_EMPTY', 'scale', 'COMPOSIT_STATE', 'STATE_DESCR', 'FORK', 'JOIN', 'CHOICE', 'CONCURRENT', 'note', 'acc_title', 'acc_descr', 'acc_descr_multiline_value', 'CLICK', 'classDef', 'style', 'class', 'direction_tb', 'direction_bt', 'direction_rl', 'direction_lr', 'EDGE_STATE'";

struct P<'a> {
    src: &'a [u16],
    types: Vec<u32>,
    texts: Vec<String>,
    starts: Vec<usize>,
    error: Option<String>,
    i: usize,
}

type R<T> = Result<T, PeleError>;

fn state(id: String, type_: Option<&str>, description: Option<Descr>) -> StateStmt {
    StateStmt { id, type_: type_.map(str::to_string), description, ..Default::default() }
}

impl P<'_> {
    fn ty(&self) -> u32 {
        self.types.get(self.i).copied().unwrap_or(t::END)
    }

    fn ty_at(&self, i: usize) -> u32 {
        self.types.get(i).copied().unwrap_or(u32::MAX)
    }

    fn take(&mut self) -> String {
        let s = self.texts[self.i].clone();
        self.i += 1;
        s
    }

    fn fail(&self, expected: &str) -> PeleError {
        let ty = self.ty();
        let at = self.starts[self.i];
        if ty == t::ERROR {
            if let Some(message) = &self.error {
                let loc = locate(self.src, at);
                return PeleError { message: message.clone(), code: PeleErrorCode::Syntax, type_: Some("state".into()), mermaid_type: None, line: loc.line, column: loc.column, snippet: loc.snippet };
            }
            return syntax_error("state", self.src, at, "", true);
        }
        syntax_error("state", self.src, at, &format!("Expecting {}, got '{}'", expected, TOKEN_NAMES[ty as usize]), false)
    }

    fn expect(&mut self, ty: u32) -> R<String> {
        if self.ty() != ty {
            return Err(self.fail(&format!("'{}'", TOKEN_NAMES[ty as usize])));
        }
        Ok(self.take())
    }

    fn id_statement(&mut self) -> R<StateStmt> {
        let ty = self.ty();
        if ty != t::ID && ty != t::EDGE_STATE {
            return Err(self.fail("'ID', 'EDGE_STATE'"));
        }
        let id = js::trim(&self.take()).to_string();
        if self.ty() != t::STYLE_SEPARATOR {
            return Ok(state(id, Some("default"), Some(Descr::One(String::new()))));
        }
        self.i += 1;
        let class = js::trim(&self.expect(t::ID)?).to_string();
        let mut st = state(id, Some("default"), Some(Descr::One(String::new())));
        st.classes = Some(vec![class]);
        Ok(st)
    }
}

pub fn parse_state(src: &[u16], db: &mut StateDb) -> R<()> {
    let tokens = tokenize(src);
    let mut p = P { src, types: tokens.types, texts: tokens.texts, starts: tokens.starts, error: tokens.error, i: 0 };

    while p.ty() == t::NL {
        p.i += 1;
    }
    if p.ty() != t::SD {
        return Err(p.fail("'NL', 'SD'"));
    }
    p.i += 1;

    // Documents nest on this stack: the open composites' documents, innermost last. The grammar
    // drops a statement whose value is the string it uses for a blank line.
    let mut docs: Vec<Vec<Stmt>> = vec![Vec::new()];
    let add = |docs: &mut Vec<Vec<Stmt>>, value: Stmt| {
        if !matches!(&value, Stmt::Text(s) if s == "nl") {
            docs.last_mut().unwrap().push(value);
        }
    };
    // A composite opens: its statement goes in the current document, with the inner document
    // filled in when it closes.
    let mut opened: Vec<StateStmt> = Vec::new();

    loop {
        let ty = p.ty();
        match ty {
            t::NL => p.i += 1,
            t::ID | t::EDGE_STATE => {
                let mut first = p.id_statement()?;
                if p.ty() == t::DESCR {
                    let text = p.take();
                    first.description = Some(Descr::One(db.trim_colon(&text)));
                    add(&mut docs, Stmt::State(first));
                } else if p.ty() == t::ARROW {
                    p.i += 1;
                    let second = p.id_statement()?;
                    if p.ty() == t::DESCR {
                        let text = p.take();
                        add(&mut docs, Stmt::Relation { state1: first, state2: second, description: Some(db.trim_colon(&text)) });
                    } else {
                        add(&mut docs, Stmt::Relation { state1: first, state2: second, description: None });
                    }
                } else {
                    add(&mut docs, Stmt::State(first));
                }
            }
            t::HIDE_EMPTY => {
                let text = p.take();
                add(&mut docs, Stmt::Text(text));
            }
            t::scale => {
                p.i += 1;
                p.expect(t::WIDTH)?;
                add(&mut docs, Stmt::Text(p.texts[p.i - 2].clone()));
            }
            t::COMPOSIT_STATE => {
                if p.ty_at(p.i + 1) == t::STRUCT_START {
                    let id = p.take();
                    p.i += 1;
                    opened.push(state(id, Some("default"), Some(Descr::One(String::new()))));
                    docs.push(Vec::new());
                } else {
                    let text = p.take();
                    add(&mut docs, Stmt::Text(text));
                }
            }
            t::STATE_DESCR => {
                let description = p.take();
                p.expect(t::AS)?;
                let id = p.expect(t::ID)?;
                if p.ty() == t::STRUCT_START {
                    p.i += 1;
                    opened.push(state(id, Some("default"), Some(Descr::One(description))));
                    docs.push(Vec::new());
                } else if id.contains(':') {
                    let parts: Vec<&str> = id.split(':').collect();
                    add(&mut docs, Stmt::State(state(parts[0].to_string(), Some("default"), Some(Descr::Many(vec![js::trim(&description).to_string(), parts[1].to_string()])))));
                } else {
                    add(&mut docs, Stmt::State(state(id, Some("default"), Some(Descr::One(js::trim(&description).to_string())))));
                }
            }
            t::FORK | t::JOIN | t::CHOICE => {
                let id = p.take();
                let kind = match ty {
                    t::FORK => "fork",
                    t::JOIN => "join",
                    _ => "choice",
                };
                add(&mut docs, Stmt::State(state(id, Some(kind), None)));
            }
            t::CONCURRENT => {
                p.i += 1;
                let id = db.get_divider_id();
                add(&mut docs, Stmt::State(state(id, Some("divider"), None)));
            }
            t::note => {
                let keyword = p.take();
                if p.ty() == t::left_of || p.ty() == t::right_of {
                    let position = js::trim(&p.take()).to_string();
                    let id = js::trim(&p.expect(t::ID)?).to_string();
                    let text = js::trim(&p.expect(t::NOTE_TEXT)?).to_string();
                    add(&mut docs, Stmt::State(StateStmt { id, note: Some(StateNote { position: Some(position), text }), ..Default::default() }));
                } else {
                    if p.ty() != t::NOTE_TEXT {
                        return Err(p.fail("'NOTE_TEXT', 'left_of', 'right_of'"));
                    }
                    p.i += 1;
                    p.expect(t::AS)?;
                    p.expect(t::ID)?;
                    add(&mut docs, Stmt::Text(keyword));
                }
            }
            t::direction_tb | t::direction_bt | t::direction_rl | t::direction_lr => {
                let value = DIRECTIONS[(ty - t::direction_tb) as usize];
                p.i += 1;
                // The grammar's setDirection lands in the previous root document (see db.ts).
                add(&mut docs, Stmt::Dir(value.to_string()));
            }
            t::acc_title => {
                p.i += 1;
                let value = js::trim(&p.expect(t::acc_title_value)?).to_string();
                db.set_acc_title(&value);
                add(&mut docs, Stmt::Text(value));
            }
            t::acc_descr => {
                p.i += 1;
                let value = js::trim(&p.expect(t::acc_descr_value)?).to_string();
                db.set_acc_description(&value);
                add(&mut docs, Stmt::Text(value));
            }
            t::acc_descr_multiline_value => {
                let value = js::trim(&p.take()).to_string();
                db.set_acc_description(&value);
                add(&mut docs, Stmt::Text(value));
            }
            t::CLICK => {
                p.i += 1;
                let id = p.id_statement()?;
                let url;
                let mut tooltip = String::new();
                if p.ty() == t::HREF {
                    p.i += 1;
                    url = p.expect(t::STRING)?;
                } else {
                    if p.ty() != t::STRING {
                        return Err(p.fail("'STRING', 'HREF'"));
                    }
                    url = p.take();
                    tooltip = p.expect(t::STRING)?;
                }
                p.expect(t::NL)?;
                add(&mut docs, Stmt::Click { id, url, tooltip });
            }
            t::classDef => {
                p.i += 1;
                let id = js::trim(&p.expect(t::CLASSDEF_ID)?).to_string();
                let classes = js::trim(&p.expect(t::CLASSDEF_STYLEOPTS)?).to_string();
                add(&mut docs, Stmt::ClassDef { id, classes });
            }
            t::style => {
                p.i += 1;
                let id = js::trim(&p.expect(t::STYLE_IDS)?).to_string();
                let style_class = js::trim(&p.expect(t::STYLEDEF_STYLEOPTS)?).to_string();
                add(&mut docs, Stmt::Style { id, style_class });
            }
            t::class_ => {
                p.i += 1;
                let id = js::trim(&p.expect(t::CLASSENTITY_IDS)?).to_string();
                let style_class = js::trim(&p.expect(t::STYLECLASS)?).to_string();
                add(&mut docs, Stmt::ApplyClass { id, style_class });
            }
            t::STRUCT_STOP => {
                if opened.is_empty() {
                    return Err(p.fail(&format!("'$end', {}", STATEMENT)));
                }
                p.i += 1;
                let inner = docs.pop().unwrap();
                let mut composite = opened.pop().unwrap();
                composite.doc = Some(inner);
                docs.last_mut().unwrap().push(Stmt::State(composite));
            }
            t::END => {
                if !opened.is_empty() {
                    return Err(p.fail(&format!("'STRUCT_STOP', {}", STATEMENT)));
                }
                let doc = docs.pop().unwrap();
                return db.set_root_doc(doc);
            }
            _ => {
                return Err(p.fail(&if !opened.is_empty() { format!("'STRUCT_STOP', {}", STATEMENT) } else { format!("'$end', {}", STATEMENT) }));
            }
        }
    }
}
