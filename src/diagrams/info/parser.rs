// Port of src/diagrams/info/parser.ts.

use crate::diagrams::common::tokens::*;
use crate::errors::PeleError;

const SHOW_INFO: i32 = 0;
const INFO: i32 = 1;
const ACC_DESCR: i32 = 2;
const ACC_TITLE: i32 = 3;
const TITLE: i32 = 4;
const NEWLINE: i32 = 5;

fn info_tokens() -> Vec<TokenType> {
    vec![keyword("showInfo"), keyword("info"), acc_descr(), acc_title(), title(), newline_token(), whitespace(), yaml(), directive(), single_line_comment()]
}

#[derive(Default)]
pub struct InfoAst {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
}

pub fn parse_info(src: &[u16]) -> Result<InfoAst, PeleError> {
    let types = info_tokens();
    let tokens = tokenize(src, &types, "info")?;
    let mut r = Reader::new(&tokens, &types, "info");
    let mut ast = InfoAst::default();
    while r.kind() == NEWLINE {
        r.i += 1;
    }
    r.expect(INFO)?;
    while r.kind() == NEWLINE {
        r.i += 1;
    }
    if r.accept(SHOW_INFO) {
        while r.kind() == NEWLINE {
            r.i += 1;
        }
    }
    while r.kind() != -1 {
        let kind = r.kind();
        if kind == ACC_DESCR {
            ast.acc_descr = Some(acc_descr_value(&r.take()));
        } else if kind == ACC_TITLE {
            ast.acc_title = Some(acc_title_value(&r.take()));
        } else if kind == TITLE {
            ast.title = Some(title_value(&r.take()));
        } else {
            return Err(r.fail("a title or the end of input"));
        }
        if r.kind() == -1 {
            break;
        }
        if r.kind() != NEWLINE {
            return Err(r.fail("a line break or the end of input"));
        }
        while r.kind() == NEWLINE {
            r.i += 1;
        }
    }
    Ok(ast)
}
