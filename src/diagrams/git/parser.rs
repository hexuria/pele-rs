// Port of src/diagrams/git/parser.ts.

use crate::diagrams::common::tokens::*;
use crate::diagrams::packet::parser::int_end;
use crate::errors::PeleError;
use crate::js;
use std::rc::Rc;

const CHERRY_PICK: i32 = 0;
const GIT_GRAPH_COLON: i32 = 1;
const HIGHLIGHT: i32 = 2;
const GIT_GRAPH: i32 = 3;
const CHECKOUT: i32 = 4;
const REVERSE: i32 = 5;
const PARENT: i32 = 6;
const COMMIT: i32 = 7;
const NORMAL: i32 = 8;
const BRANCH: i32 = 9;
const ORDER: i32 = 10;
const SWITCH: i32 = 11;
const TYPE: i32 = 12;
const MERGE: i32 = 13;
const MSG: i32 = 14;
const TAG: i32 = 15;
const ID: i32 = 16;
const LR: i32 = 17;
const TB: i32 = 18;
const BT: i32 = 19;
const COLON: i32 = 20;
const ACC_DESCR: i32 = 21;
const ACC_TITLE: i32 = 22;
const TITLE: i32 = 23;
const INT: i32 = 24;
const STRING: i32 = 25;
const NEWLINE: i32 = 26;
const REFERENCE: i32 = 31;

fn at(src: &[u16], i: usize) -> u32 {
    src.get(i).map_or(u32::MAX, |&c| c as u32)
}

fn is_w(c: u32) -> bool {
    c < 128 && js::is_word(c as u8 as char)
}

// `\w([-\./\w]*[-\w])?`
fn reference(src: &[u16], p: usize) -> Option<usize> {
    if !is_w(at(src, p)) {
        return None;
    }
    let mut end = p + 1;
    let mut q = p + 1;
    while q < src.len() && (is_w(src[q] as u32) || matches!(src[q], 45 | 46 | 47)) {
        if is_w(src[q] as u32) || src[q] == 45 {
            end = q + 1;
        }
        q += 1;
    }
    Some(end)
}

// `[-./\w]*[-\w]` at q.
fn name_rest(src: &[u16], q: usize) -> bool {
    let mut q = q;
    while q < src.len() && (is_w(src[q] as u32) || matches!(src[q], 45 | 46 | 47)) {
        if is_w(src[q] as u32) || src[q] == 45 {
            return true;
        }
        q += 1;
    }
    false
}

pub fn git_tokens() -> Vec<TokenType> {
    let reference_index = REFERENCE as usize;
    // A keyword that is the start of a longer name is part of that name: `commits` is a branch.
    let word = |name: &'static str| {
        let mut t = TokenType::lit(name, name);
        t.longer = vec![reference_index];
        t
    };
    let lit = |name: &'static str| TokenType::lit(name, name);
    let mut graph = keyword("gitGraph");
    graph.longer = vec![reference_index];
    // Without a closing brace the block form scans to the end of the text, and a brace is not a
    // token on its own, so the first failure is final.
    let mut descr = acc_descr();
    descr.opener = Some(Rc::new(|src: &[u16], p: usize| {
        let mut q = blanks(src, p);
        if js::from16(&src[q.min(src.len())..(q + 8).min(src.len())]) != "accDescr" {
            return None;
        }
        q += 8;
        while q < src.len() && js::is_space_u(src[q] as u32) {
            q += 1;
        }
        (at(src, q) == 123).then_some(q + 1)
    }));
    vec![
        word("cherry-pick"),
        lit("gitGraph:"),
        word("HIGHLIGHT"),
        graph,
        word("checkout"),
        word("REVERSE"),
        lit("parent:"),
        word("commit"),
        word("NORMAL"),
        word("branch"),
        lit("order:"),
        word("switch"),
        lit("type:"),
        word("merge"),
        lit("msg:"),
        lit("tag:"),
        lit("id:"),
        word("LR"),
        word("TB"),
        word("BT"),
        lit(":"),
        descr,
        acc_title(),
        title_before(name_rest),
        TokenType::re("INT", None, int_end),
        string(),
        newline_token(),
        whitespace(),
        yaml(),
        directive(),
        single_line_comment(),
        TokenType::re("REFERENCE", None, reference),
    ]
}

#[derive(Clone, Copy, PartialEq)]
pub enum CommitTypeName {
    Normal,
    Reverse,
    Highlight,
}

pub enum StatementAst {
    Commit { id: Option<String>, message: Option<String>, tags: Vec<String>, type_: Option<CommitTypeName> },
    Branch { name: String, order: Option<f64> },
    Merge { branch: String, id: Option<String>, tags: Vec<String>, type_: Option<CommitTypeName> },
    Checkout { branch: String },
    CherryPicking { id: Option<String>, tags: Vec<String>, parent: Option<String> },
}

#[derive(Default)]
pub struct GitGraphAst {
    pub dir: Option<&'static str>,
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub statements: Vec<StatementAst>,
}

fn end_of_line(r: &mut Reader) -> Result<(), PeleError> {
    if r.kind() == -1 {
        return Ok(());
    }
    if r.kind() != NEWLINE {
        return Err(r.fail("a line break or the end of input"));
    }
    while r.kind() == NEWLINE {
        r.i += 1;
    }
    Ok(())
}

fn name(r: &mut Reader) -> Result<String, PeleError> {
    if r.kind() == REFERENCE {
        return Ok(r.take());
    }
    if r.kind() == STRING {
        return Ok(string_value(&r.take()));
    }
    Err(r.fail("a branch name"))
}

fn string_arg(r: &mut Reader) -> Result<String, PeleError> {
    Ok(string_value(&r.expect(STRING)?))
}

fn commit_type(r: &mut Reader) -> Result<CommitTypeName, PeleError> {
    let t = match r.kind() {
        NORMAL => CommitTypeName::Normal,
        REVERSE => CommitTypeName::Reverse,
        HIGHLIGHT => CommitTypeName::Highlight,
        _ => return Err(r.fail("NORMAL, REVERSE, or HIGHLIGHT")),
    };
    r.i += 1;
    Ok(t)
}

pub fn parse_git(src: &[u16]) -> Result<GitGraphAst, PeleError> {
    let types = git_tokens();
    let tokens = tokenize(src, &types, "gitGraph")?;
    let mut r = Reader::new(&tokens, &types, "gitGraph");
    let mut ast = GitGraphAst::default();

    while r.kind() == NEWLINE {
        r.i += 1;
    }
    if !r.accept(GIT_GRAPH_COLON) {
        r.expect(GIT_GRAPH)?;
        let next = tokens.kinds.get(r.i + 1).copied();
        if matches!(r.kind(), LR | TB | BT) && next == Some(COLON) {
            ast.dir = Some(match r.kind() {
                LR => "LR",
                TB => "TB",
                _ => "BT",
            });
            r.i += 2;
        } else {
            r.accept(COLON);
        }
    }

    while r.kind() != -1 {
        match r.kind() {
            NEWLINE => r.i += 1,
            ACC_DESCR => {
                ast.acc_descr = Some(acc_descr_value(&r.take()));
                end_of_line(&mut r)?;
            }
            ACC_TITLE => {
                ast.acc_title = Some(acc_title_value(&r.take()));
                end_of_line(&mut r)?;
            }
            TITLE => {
                ast.title = Some(title_value(&r.take()));
                end_of_line(&mut r)?;
            }
            COMMIT => {
                r.i += 1;
                let (mut id, mut message, mut tags, mut type_) = (None, None, Vec::new(), None);
                loop {
                    if r.accept(ID) {
                        id = Some(string_arg(&mut r)?);
                    } else if r.accept(TAG) {
                        tags.push(string_arg(&mut r)?);
                    } else if r.accept(TYPE) {
                        type_ = Some(commit_type(&mut r)?);
                    } else if r.accept(MSG) || r.kind() == STRING {
                        message = Some(string_arg(&mut r)?);
                    } else {
                        break;
                    }
                }
                end_of_line(&mut r)?;
                ast.statements.push(StatementAst::Commit { id, message, tags, type_ });
            }
            BRANCH => {
                r.i += 1;
                let name = name(&mut r)?;
                let mut order = None;
                if r.accept(ORDER) {
                    order = Some(js::parse_int(&r.expect(INT)?, 10));
                }
                end_of_line(&mut r)?;
                ast.statements.push(StatementAst::Branch { name, order });
            }
            MERGE => {
                r.i += 1;
                let branch = name(&mut r)?;
                let (mut id, mut tags, mut type_) = (None, Vec::new(), None);
                loop {
                    if r.accept(ID) {
                        id = Some(string_arg(&mut r)?);
                    } else if r.accept(TAG) {
                        tags.push(string_arg(&mut r)?);
                    } else if r.accept(TYPE) {
                        type_ = Some(commit_type(&mut r)?);
                    } else {
                        break;
                    }
                }
                end_of_line(&mut r)?;
                ast.statements.push(StatementAst::Merge { branch, id, tags, type_ });
            }
            CHECKOUT | SWITCH => {
                r.i += 1;
                let branch = name(&mut r)?;
                end_of_line(&mut r)?;
                ast.statements.push(StatementAst::Checkout { branch });
            }
            CHERRY_PICK => {
                r.i += 1;
                let (mut id, mut tags, mut parent) = (None, Vec::new(), None);
                loop {
                    if r.accept(ID) {
                        id = Some(string_arg(&mut r)?);
                    } else if r.accept(TAG) {
                        tags.push(string_arg(&mut r)?);
                    } else if r.accept(PARENT) {
                        parent = Some(string_arg(&mut r)?);
                    } else {
                        break;
                    }
                }
                end_of_line(&mut r)?;
                ast.statements.push(StatementAst::CherryPicking { id, tags, parent });
            }
            _ => return Err(r.fail("a statement, a title, or a line break")),
        }
    }
    Ok(ast)
}
