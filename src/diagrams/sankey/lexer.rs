// Port of src/diagrams/sankey/lexer.ts.

use crate::js;
use crate::util::chars::is_word;

pub const END: u32 = 0;
// Not a Jison token: the reference lexer would return the same empty token forever from here.
pub const STUCK: u32 = 1;
pub const SANKEY: u32 = 2;
pub const NEWLINE: u32 = 3;
pub const COMMA: u32 = 4;
pub const DQUOTE: u32 = 5;
pub const NON_ESCAPED_TEXT: u32 = 6;
pub const ESCAPED_TEXT: u32 = 7;
pub const EOF: u32 = 8;

pub const TOKEN_NAMES: [&str; 9] = ["$end", "STUCK", "SANKEY", "NEWLINE", "COMMA", "DQUOTE", "NON_ESCAPED_TEXT", "ESCAPED_TEXT", "EOF"];

pub struct Tokens {
    pub types: Vec<u32>,
    pub texts: Vec<String>,
    pub starts: Vec<usize>,
}

const S_INITIAL: u32 = 0;
const S_CSV: u32 = 1;
const S_ESCAPED_TEXT: u32 = 2;

// RFC 4180 TEXTDATA extended to Unicode, excluding controls and invalid XML characters.
fn is_text_data(c: u32) -> bool {
    ((32..=126).contains(&c) && c != 34 && c != 44) || (0xa0..=0xd7ff).contains(&c) || (0xe000..=0xfffd).contains(&c) || (0x10000..=0x10ffff).contains(&c)
}

fn at(src: &[u16], i: usize) -> u32 {
    src.get(i).map_or(u32::MAX, |&c| c as u32)
}

// `codePointAt`.
fn code_point(src: &[u16], q: usize) -> u32 {
    let c = src[q] as u32;
    if (0xd800..=0xdbff).contains(&c) {
        let d = at(src, q + 1);
        if (0xdc00..=0xdfff).contains(&d) {
            return 0x10000 + ((c - 0xd800) << 10) + (d - 0xdc00);
        }
    }
    c
}

#[allow(unused_assignments)]
pub fn tokenize(src: &[u16]) -> Tokens {
    let n = src.len();
    let mut out = Tokens { types: Vec::new(), texts: Vec::new(), starts: Vec::new() };
    let mut stack: Vec<u32> = Vec::new();
    let mut state = S_INITIAL;
    let mut p = 0usize;
    macro_rules! emit {
        ($ty:expr, $s:expr, $e:expr) => {{
            let (s, e) = ($s, $e);
            out.types.push($ty);
            out.texts.push(js::from16(&src[s..e]));
            out.starts.push(s);
            p = e;
        }};
    }
    let keyword = |p: usize, word: &str| {
        let len = word.len();
        js::from16(&src[p..(p + len).min(n)]).to_lowercase() == word && !is_word(at(src, p + len))
    };
    while p < n {
        let c = src[p] as u32;
        let start = p;
        let mut q = p;
        if state == S_ESCAPED_TEXT {
            if c == 34 && at(src, p + 1) != 34 {
                if let Some(s) = stack.pop() {
                    state = s;
                }
                emit!(DQUOTE, p, p + 1);
                continue;
            }
            while q < n {
                let d = code_point(src, q);
                if is_text_data(d) || d == 44 || d == 13 || d == 10 {
                    q += if d > 0xffff { 2 } else { 1 };
                } else if d == 34 && at(src, q + 1) == 34 {
                    q += 2;
                } else {
                    break;
                }
            }
            emit!(ESCAPED_TEXT, p, q);
        } else {
            if state == S_INITIAL && (c | 32) == 115 {
                let length = if keyword(p, "sankey-beta") {
                    11
                } else if keyword(p, "sankey") {
                    6
                } else {
                    0
                };
                if length > 0 {
                    stack.push(state);
                    state = S_CSV;
                    emit!(SANKEY, p, p + length);
                    continue;
                }
            }
            if c == 10 || (c == 13 && at(src, p + 1) == 10) {
                emit!(NEWLINE, p, if c == 10 { p + 1 } else { p + 2 });
                continue;
            }
            if c == 44 {
                emit!(COMMA, p, p + 1);
                continue;
            }
            if c == 34 {
                stack.push(state);
                state = S_ESCAPED_TEXT;
                emit!(DQUOTE, p, p + 1);
                continue;
            }
            while q < n {
                let d = code_point(src, q);
                if !is_text_data(d) {
                    break;
                }
                q += if d > 0xffff { 2 } else { 1 };
            }
            emit!(NON_ESCAPED_TEXT, p, q);
        }
        // A character outside the CSV alphabet matches as empty text and is never consumed.
        if q == start {
            emit!(STUCK, p, p);
            return out;
        }
    }
    emit!(if state == S_ESCAPED_TEXT { ESCAPED_TEXT } else { EOF }, n, n);
    emit!(END, n, n);
    out
}
