// Port of src/diagrams/journey/lexer.ts.

use crate::diagrams::common::scan::{accessibility, keyword_at, skip_trivia, Tokens};
use crate::js;
use crate::util::chars::{is_space, is_word};

pub const END: u32 = 0;
pub const EOF: u32 = 1;
pub const INVALID: u32 = 2;
pub const NEWLINE: u32 = 3;
pub const JOURNEY: u32 = 4;
pub const TITLE: u32 = 5;
pub const ACC_TITLE: u32 = 6;
pub const ACC_DESCR: u32 = 8;
pub const ACC_DESCR_MULTILINE_VALUE: u32 = 10;
pub const SECTION: u32 = 11;
pub const TASK_NAME: u32 = 12;
pub const TASK_DATA: u32 = 13;
pub const COLON: u32 = 14;

pub const TOKEN_NAMES: [&str; 15] =
    ["$end", "EOF", "INVALID", "NEWLINE", "journey", "title", "acc_title", "acc_title_value", "acc_descr", "acc_descr_value", "acc_descr_multiline_value", "section", "taskName", "taskData", ":"];

fn at(src: &[u16], i: usize) -> u32 {
    src.get(i).map_or(u32::MAX, |&c| c as u32)
}

// `#`, a line break, and `;` end every kind of text; a colon also ends a task name or a section.
fn text_end(src: &[u16], from: usize, colon: bool) -> usize {
    let mut e = from;
    while e < src.len() {
        let c = src[e];
        if c == 35 || c == 10 || c == 59 || (colon && c == 58) {
            break;
        }
        e += 1;
    }
    e
}

pub fn tokenize(src: &[u16]) -> Tokens {
    let n = src.len();
    let mut out = Tokens::default();
    let mut p = 0;
    fn emit(out: &mut Tokens, src: &[u16], p: &mut usize, ty: u32, e: usize) {
        out.types.push(ty);
        out.texts.push(js::from16(&src[*p..e]));
        out.starts.push(*p);
        *p = e;
    }
    loop {
        p = skip_trivia(src, p);
        if p >= n {
            break;
        }
        let c = src[p];
        let mut e = p + 1;
        if c == 10 {
            while at(src, e) == 10 {
                e += 1;
            }
            emit(&mut out, src, &mut p, NEWLINE, e);
            continue;
        }
        if keyword_at(src, p, "journey") && !is_word(at(src, p + 7)) {
            {
                let e = p + 7;
                emit(&mut out, src, &mut p, JOURNEY, e);
            }
            continue;
        }
        if keyword_at(src, p, "title") {
            // Unlike Mermaid, ignore bare titles.
            e = p + 5;
            while at(src, e) != 10 && is_space(at(src, e)) {
                e += 1;
            }
            if e == n || at(src, e) == 10 {
                p = e;
                continue;
            }
            let sep = at(src, p + 5);
            if sep != 13 && is_space(sep) {
                e = text_end(src, p + 6, false);
                if e > p + 6 {
                    emit(&mut out, src, &mut p, TITLE, e);
                    continue;
                }
            }
        }
        e = accessibility(src, p, &mut out, ACC_TITLE);
        if e != p {
            p = e;
            continue;
        }
        // Like Mermaid, a bare section uses the next line as its name.
        if keyword_at(src, p, "section") && is_space(at(src, p + 7)) {
            e = text_end(src, p + 8, true);
            if e > p + 8 {
                emit(&mut out, src, &mut p, SECTION, e);
                continue;
            }
        }
        if c == 58 {
            e = text_end(src, p + 1, false);
            if e > p + 1 {
                emit(&mut out, src, &mut p, TASK_DATA, e);
            } else {
                {
                    let e = p + 1;
                    emit(&mut out, src, &mut p, COLON, e);
                }
            }
        } else if c == 59 {
            {
                let e = p + 1;
                emit(&mut out, src, &mut p, INVALID, e);
            }
        } else {
            let e = text_end(src, p + 1, true);
            emit(&mut out, src, &mut p, TASK_NAME, e);
        }
    }
    // Jison marks its lexer done when a rule runs on empty input, so an accessibility value that
    // matches nothing at the very end of the text is followed by no EOF token.
    if out.starts.last().is_none_or(|&s| s < n) {
        emit(&mut out, src, &mut p, EOF, n);
    }
    p = n;
    emit(&mut out, src, &mut p, END, n);
    out
}
