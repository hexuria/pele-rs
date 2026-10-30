// Port of src/diagrams/timeline/lexer.ts.

use crate::diagrams::common::scan::{accessibility, keyword_at, line_end, skip_trivia, Tokens};
use crate::js;
use crate::util::chars::{is_space, is_word};

pub const END: u32 = 0;
pub const EOF: u32 = 1;
pub const INVALID: u32 = 2;
pub const NEWLINE: u32 = 3;
pub const TIMELINE: u32 = 4;
pub const TIMELINE_LR: u32 = 5;
pub const TIMELINE_TD: u32 = 6;
pub const TITLE: u32 = 7;
pub const ACC_TITLE: u32 = 8;
pub const ACC_DESCR: u32 = 10;
pub const ACC_DESCR_MULTILINE_VALUE: u32 = 12;
pub const SECTION: u32 = 13;
pub const PERIOD: u32 = 14;
pub const EVENT: u32 = 15;

pub const TOKEN_NAMES: [&str; 16] = [
    "$end",
    "EOF",
    "INVALID",
    "NEWLINE",
    "timeline",
    "timeline_lr",
    "timeline_td",
    "title",
    "acc_title",
    "acc_title_value",
    "acc_descr",
    "acc_descr_value",
    "acc_descr_multiline_value",
    "section",
    "period",
    "event",
];

fn at(src: &[u16], i: usize) -> u32 {
    src.get(i).map_or(u32::MAX, |&c| c as u32)
}

fn emit(out: &mut Tokens, src: &[u16], p: &mut usize, ty: u32, e: usize) {
    out.types.push(ty);
    out.texts.push(js::from16(&src[*p..e]));
    out.starts.push(*p);
    *p = e;
}

pub fn tokenize(src: &[u16]) -> Tokens {
    let n = src.len();
    let mut out = Tokens::default();
    let mut p = 0;
    loop {
        p = skip_trivia(src, p);
        if p >= n {
            break;
        }
        let c = src[p] as u32;
        let mut e = p + 1;
        if c == 10 {
            while at(src, e) == 10 {
                e += 1;
            }
            emit(&mut out, src, &mut p, NEWLINE, e);
            continue;
        }
        if keyword_at(src, p, "timeline") {
            e = p + 8;
            while at(src, e) == 32 || at(src, e) == 9 {
                e += 1;
            }
            let lr = keyword_at(src, e, "lr");
            if e > p + 8 && (lr || keyword_at(src, e, "td")) && !is_word(at(src, e + 2)) {
                emit(&mut out, src, &mut p, if lr { TIMELINE_LR } else { TIMELINE_TD }, e + 2);
                continue;
            }
            if !is_word(at(src, p + 8)) {
                {
                    let e = p + 8;
                    emit(&mut out, src, &mut p, TIMELINE, e);
                }
                continue;
            }
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
                let e = line_end(src, p + 6);
                emit(&mut out, src, &mut p, TITLE, e);
                continue;
            }
        }
        e = accessibility(src, p, &mut out, ACC_TITLE);
        if e != p {
            p = e;
            continue;
        }
        // Like Mermaid, a bare section uses the next line as its name.
        if keyword_at(src, p, "section") && is_space(at(src, p + 7)) {
            e = p + 8;
            while e < n && src[e] != 58 && src[e] != 10 {
                e += 1;
            }
            if e > p + 8 {
                emit(&mut out, src, &mut p, SECTION, e);
                continue;
            }
        }
        if c != 58 {
            e = p + 1;
            while e < n && !matches!(src[e], 35 | 58 | 10) {
                e += 1;
            }
            emit(&mut out, src, &mut p, PERIOD, e);
            continue;
        }
        // An event runs to the end of the line or to a colon that is followed by whitespace.
        if is_space(at(src, p + 1)) {
            e = p + 2;
            while e < n {
                let k = src[e] as u32;
                if k == 10 || (k == 58 && is_space(at(src, e + 1))) {
                    break;
                }
                e += 1;
            }
            if e > p + 2 {
                emit(&mut out, src, &mut p, EVENT, e);
                continue;
            }
        }
        {
            let e = p + 1;
            emit(&mut out, src, &mut p, INVALID, e);
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
