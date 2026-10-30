// Port of src/diagrams/pie/parser.ts.

use crate::diagrams::common::tokens::*;
use crate::errors::PeleError;
use crate::js;
use std::rc::Rc;

const SHOW_DATA: i32 = 0;
const PIE: i32 = 1;
const COLON: i32 = 2;
const NUMBER: i32 = 3;
const ACC_DESCR: i32 = 4;
const ACC_TITLE: i32 = 5;
const TITLE: i32 = 6;
const STRING: i32 = 7;
const NEWLINE: i32 = 8;

fn at(src: &[u16], i: usize) -> u16 {
    src.get(i).copied().unwrap_or(0xffff)
}

fn digits(src: &[u16], from: usize) -> usize {
    let mut i = from;
    while (48..=57).contains(&at(src, i)) {
        i += 1;
    }
    i
}

// Answers as NUMBER_PIE's pattern does:
// /(?:-?[0-9]+\.[0-9]+(?!\.))|(?:-?(0|[1-9][0-9]*)(?!\.))/y
fn number_scanner() -> Scanner {
    let mut from = 0usize;
    let mut end = 0usize;
    let mut fraction: Option<usize> = None;
    Box::new(move |src: &[u16], p: usize, _| {
        let a = if at(src, p) == 45 { p + 1 } else { p };
        if a < from || a >= end {
            from = a;
            end = digits(src, a);
            fraction = None;
            if end > a && at(src, end) == 46 {
                let stop = digits(src, end + 1);
                let length = stop - end - 1;
                if length > 0 && at(src, stop) != 46 {
                    fraction = Some(stop);
                } else if length > 1 {
                    fraction = Some(stop - 1);
                }
            }
        }
        if end == a {
            return None;
        }
        if let Some(f) = fraction {
            return Some(f);
        }
        if at(src, a) == 48 {
            return if at(src, a + 1) == 46 { None } else { Some(a + 1) };
        }
        if at(src, end) != 46 {
            return Some(end);
        }
        if end - 1 > a {
            Some(end - 1)
        } else {
            None
        }
    })
}

pub fn pie_tokens() -> Vec<TokenType> {
    let mut number = TokenType::re("NUMBER_PIE", Some("-0123456789"), |_, _| None);
    number.scanner = Some(Rc::new(number_scanner));
    vec![keyword("showData"), keyword("pie"), TokenType::lit(":", ":"), number, acc_descr(), acc_title(), title(), string(), newline_token(), whitespace(), yaml(), directive(), single_line_comment()]
}

#[derive(Default)]
pub struct PieAst {
    pub show_data: bool,
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub sections: Vec<(String, f64)>,
}

pub fn parse_pie(src: &[u16]) -> Result<PieAst, PeleError> {
    let types = pie_tokens();
    let tokens = tokenize(src, &types, "pie")?;
    let mut r = Reader::new(&tokens, &types, "pie");
    let mut ast = PieAst::default();
    let end_of_line = |r: &mut Reader| -> Result<(), PeleError> {
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
    };
    while r.kind() == NEWLINE {
        r.i += 1;
    }
    r.expect(PIE)?;
    ast.show_data = r.accept(SHOW_DATA);
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
            STRING => {
                let label = string_value(&r.take());
                r.expect(COLON)?;
                let value = js::to_number(&r.expect(NUMBER)?);
                ast.sections.push((label, value));
                end_of_line(&mut r)?;
            }
            _ => return Err(r.fail("a section, a title, or a line break")),
        }
    }
    Ok(ast)
}
