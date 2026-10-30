// Port of src/diagrams/quadrant/lexer.ts and tokens.ts.

use crate::diagrams::common::caseless::word_at;
use crate::js;
use crate::util::chars::{is_space, is_word};

pub const END: u32 = 0;
pub const ERROR: u32 = 1;
pub const NEWLINE: u32 = 2;
pub const TITLE: u32 = 3;
pub const TITLE_VALUE: u32 = 4;
pub const ACC_TITLE: u32 = 5;
pub const ACC_TITLE_VALUE: u32 = 6;
pub const ACC_DESCR: u32 = 7;
pub const ACC_DESCR_VALUE: u32 = 8;
pub const ACC_DESCR_MULTILINE_VALUE: u32 = 9;
pub const X_AXIS: u32 = 10;
pub const Y_AXIS: u32 = 11;
pub const DELIMITER: u32 = 12;
pub const QUADRANT_1: u32 = 13;
pub const CLASSDEF: u32 = 17;
pub const MD_STR: u32 = 18;
pub const STR: u32 = 19;
pub const CLASS_NAME: u32 = 20;
pub const POINT_START: u32 = 21;
pub const POINT_X: u32 = 22;
pub const POINT_Y: u32 = 23;
pub const QUADRANT: u32 = 24;
pub const ALPHA: u32 = 25;
pub const UNICODE_TEXT: u32 = 26;
pub const COLON: u32 = 27;
pub const PLUS: u32 = 28;
pub const COMMA: u32 = 29;
pub const EQUALS: u32 = 30;
pub const MULT: u32 = 31;
pub const BRKT: u32 = 32;
pub const UNDERSCORE: u32 = 33;
pub const DOT: u32 = 34;
pub const AMP: u32 = 35;
pub const MINUS: u32 = 36;
pub const NUM: u32 = 37;
pub const SPACE: u32 = 38;
pub const SEMI: u32 = 39;
pub const PUNCTUATION: u32 = 40;
pub const EOF: u32 = 41;

pub const TOKEN_NAMES: [&str; 42] = [
    "$end",
    "INVALID",
    "NEWLINE",
    "title",
    "title_value",
    "acc_title",
    "acc_title_value",
    "acc_descr",
    "acc_descr_value",
    "acc_descr_multiline_value",
    "X-AXIS",
    "Y-AXIS",
    "AXIS-TEXT-DELIMITER",
    "QUADRANT_1",
    "QUADRANT_2",
    "QUADRANT_3",
    "QUADRANT_4",
    "CLASSDEF",
    "MD_STR",
    "STR",
    "class_name",
    "point_start",
    "point_x",
    "point_y",
    "QUADRANT",
    "ALPHA",
    "UNICODE_TEXT",
    "COLON",
    "PLUS",
    "COMMA",
    "EQUALS",
    "MULT",
    "BRKT",
    "UNDERSCORE",
    "DOT",
    "AMP",
    "MINUS",
    "NUM",
    "SPACE",
    "SEMI",
    "PUNCTUATION",
    "EOF",
];

pub struct Tokens {
    pub types: Vec<u32>,
    pub texts: Vec<String>,
    pub starts: Vec<usize>,
}

const S_INITIAL: usize = 0;
const S_TITLE: usize = 1;
const S_ACC_TITLE: usize = 2;
const S_ACC_DESCR: usize = 3;
const S_ACC_DESCR_MULTILINE: usize = 4;
const S_STRING: usize = 5;
const S_MD_STRING: usize = 6;
const S_CLASS_NAME: usize = 7;
const S_POINT_START: usize = 8;
const S_POINT_X: usize = 9;
const S_POINT_Y: usize = 10;

// The token each state gives for the empty rest of the input, where it gives one.
const AT_END: [u32; 6] = [EOF, TITLE_VALUE, ACC_TITLE_VALUE, ACC_DESCR_VALUE, ACC_DESCR_MULTILINE_VALUE, STR];

fn single(c: u32) -> u32 {
    match c {
        58 => COLON,
        43 => PLUS,
        44 => COMMA,
        61 => EQUALS,
        42 => MULT,
        35 => BRKT,
        95 => UNDERSCORE,
        46 => DOT,
        38 => AMP,
        45 => MINUS,
        32 | 9 | 11 | 12 => SPACE,
        59 => SEMI,
        33 | 36 | 37 | 39 | 96 | 63 | 92 | 47 => PUNCTUATION,
        _ => 0,
    }
}

fn at(src: &[u16], i: usize) -> u32 {
    src.get(i).map_or(u32::MAX, |&c| c as u32)
}

fn is_digit(c: u32) -> bool {
    (48..=57).contains(&c)
}

fn is_letter(c: u32) -> bool {
    (65..=90).contains(&c) || (97..=122).contains(&c)
}

fn is_line_end(c: u32) -> bool {
    matches!(c, 10 | 13 | 0x2028 | 0x2029)
}

pub fn tokenize(src: &[u16]) -> Tokens {
    let n = src.len();
    let mut out = Tokens { types: Vec::new(), texts: Vec::new(), starts: Vec::new() };
    let mut stack: Vec<usize> = Vec::new();
    let mut state = S_INITIAL;
    let mut p = 0usize;
    // The last run scanned of each kind, so that a long run is not scanned again from each of its characters.
    let (mut space_from, mut space_to, mut dash_from, mut dash_to, mut ws_from, mut ws_to, mut gap_from, mut gap_to) = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize);

    macro_rules! emit {
        ($ty:expr, $s:expr, $e:expr) => {{
            let (s, e) = ($s, $e);
            out.types.push($ty);
            out.texts.push(js::from16(&src[s..e]));
            out.starts.push(s);
            p = e;
        }};
    }
    let line_end = |q: usize| src[q.min(n)..].iter().position(|&c| c == 10).map_or(n, |k| q + k);
    let index_of = |c: u16, q: usize| src[q.min(n)..].iter().position(|&x| x == c).map(|k| q + k);
    let spaces = |mut q: usize| {
        while at(src, q) == 32 {
            q += 1;
        }
        q
    };
    let run = |mut q: usize| {
        while q < n && is_space(src[q] as u32) {
            q += 1;
        }
        q
    };
    // Mermaid's `(1)|(0(.\d+)?)`, where the dot is any character within a line.
    let coordinate = |q: usize| -> Option<usize> {
        let c = at(src, q);
        if c == 49 {
            return Some(q + 1);
        }
        if c != 48 {
            return None;
        }
        let mut e = q + 1;
        if e < n && !is_line_end(src[e] as u32) && is_digit(at(src, e + 1)) {
            e += 2;
            while is_digit(at(src, e)) {
                e += 1;
            }
        }
        Some(e)
    };

    'lexing: while p < n {
        let c = src[p] as u32;
        let next = at(src, p + 1);
        match state {
            S_INITIAL => {}
            S_TITLE | S_ACC_TITLE | S_ACC_DESCR => {
                emit!(AT_END[state], p, line_end(p));
                if let Some(s) = stack.pop() {
                    state = s;
                }
                continue;
            }
            S_ACC_DESCR_MULTILINE => {
                if c == 125 {
                    p += 1;
                    if let Some(s) = stack.pop() {
                        state = s;
                    }
                } else {
                    emit!(ACC_DESCR_MULTILINE_VALUE, p, index_of(125, p).unwrap_or(n));
                }
                continue;
            }
            S_STRING => {
                if c == 34 {
                    p += 1;
                    if let Some(s) = stack.pop() {
                        state = s;
                    }
                } else {
                    emit!(STR, p, index_of(34, p).unwrap_or(n));
                }
                continue;
            }
            S_MD_STRING => {
                if c == 96 && next == 34 {
                    p += 2;
                    if let Some(s) = stack.pop() {
                        state = s;
                    }
                    continue;
                }
                if c != 96 && c != 34 {
                    let mut e = p + 1;
                    while e < n && src[e] != 96 && src[e] != 34 {
                        e += 1;
                    }
                    emit!(MD_STR, p, e);
                    continue;
                }
                break 'lexing;
            }
            S_CLASS_NAME => {
                if is_word(c) {
                    let mut e = p + 1;
                    while is_word(at(src, e)) {
                        e += 1;
                    }
                    emit!(CLASS_NAME, p, e);
                    if let Some(s) = stack.pop() {
                        state = s;
                    }
                    continue;
                }
                break 'lexing;
            }
            S_POINT_START => {
                if let Some(e) = coordinate(p) {
                    emit!(POINT_X, p, e);
                    stack.push(state);
                    state = S_POINT_X;
                    continue;
                }
                let k = run(p);
                if at(src, k) == 93 {
                    p = spaces(k + 1);
                    if let Some(s) = stack.pop() {
                        state = s;
                    }
                    continue;
                }
                break 'lexing;
            }
            S_POINT_X => {
                let k = run(p);
                if at(src, k) == 44 {
                    p = run(k + 1);
                    state = S_POINT_Y;
                    continue;
                }
                break 'lexing;
            }
            _ => {
                if let Some(e) = coordinate(p) {
                    emit!(POINT_Y, p, e);
                    if let Some(s) = stack.pop() {
                        state = s;
                    }
                    continue;
                }
                break 'lexing;
            }
        }

        if c == 37 && next == 37 {
            p = line_end(p + 2);
            continue;
        }
        if c == 10 || c == 13 {
            let mut e = p + 1;
            while matches!(at(src, e), 10 | 13) {
                e += 1;
            }
            emit!(NEWLINE, p, e);
            continue;
        }

        let lower = c | 32;
        if lower == 116 && word_at(src, p, "title") && !is_word(at(src, p + 5)) {
            emit!(TITLE, p, p + 5);
            stack.push(state);
            state = S_TITLE;
            continue;
        }
        if lower == 97 && word_at(src, p, "acc") {
            let descr = word_at(src, p + 3, "descr");
            if descr || word_at(src, p + 3, "title") {
                let k = run(p + 8);
                let d = at(src, k);
                if d == 58 {
                    emit!(if descr { ACC_DESCR } else { ACC_TITLE }, p, run(k + 1));
                    stack.push(state);
                    state = if descr { S_ACC_DESCR } else { S_ACC_TITLE };
                    continue;
                }
                if d == 123 && descr {
                    p = run(k + 1);
                    stack.push(state);
                    state = S_ACC_DESCR_MULTILINE;
                    continue;
                }
            }
        }

        // Axis, quadrant and chart keywords and the axis arrow take the spaces around them.
        let mut j = p;
        if c == 32 {
            if p < space_from || p >= space_to {
                space_from = p;
                space_to = spaces(p);
            }
            j = space_to;
        }
        let d = at(src, j);
        let dl = d | 32;
        let mut ty = 0;
        let mut end = 0;
        if dl == 120 && word_at(src, j, "x-axis") {
            ty = X_AXIS;
            end = j + 6;
        } else if dl == 121 && word_at(src, j, "y-axis") {
            ty = Y_AXIS;
            end = j + 6;
        } else if d == 45 {
            if j < dash_from || j >= dash_to {
                dash_from = j;
                dash_to = j;
                while at(src, dash_to) == 45 {
                    dash_to += 1;
                }
            }
            if dash_to - j >= 2 && at(src, dash_to) == 62 {
                ty = DELIMITER;
                end = dash_to + 1;
            }
        } else if dl == 113 && word_at(src, j, "quadrant") {
            let q = at(src, j + 9);
            if at(src, j + 8) == 45 && (49..=52).contains(&q) {
                ty = QUADRANT_1 + q - 49;
                end = j + 10;
            } else if word_at(src, j + 8, "chart") {
                ty = QUADRANT;
                end = j + 13;
            }
        }
        if ty != 0 {
            emit!(ty, p, spaces(end));
            continue;
        }

        if lower == 99 && word_at(src, p, "classdef") && !is_word(at(src, p + 8)) {
            emit!(CLASSDEF, p, p + 8);
            continue;
        }
        if c == 34 {
            stack.push(state);
            if next == 96 {
                p += 2;
                state = S_MD_STRING;
            } else {
                p += 1;
                state = S_STRING;
            }
            continue;
        }
        if c == 58 && next == 58 && at(src, p + 2) == 58 {
            p += 3;
            stack.push(state);
            state = S_CLASS_NAME;
            continue;
        }
        // A point's `: [`, with any whitespace before, between and after, line breaks included.
        if c == 58 || is_space(c) {
            let mut k = p;
            if c != 58 {
                if p < ws_from || p >= ws_to {
                    ws_from = p;
                    ws_to = run(p);
                }
                k = ws_to;
            }
            if at(src, k) == 58 {
                if k + 1 < gap_from || k + 1 >= gap_to {
                    gap_from = k + 1;
                    gap_to = run(k + 1);
                }
                if at(src, gap_to) == 91 {
                    emit!(POINT_START, p, run(gap_to + 1));
                    stack.push(state);
                    state = S_POINT_START;
                    continue;
                }
            }
        }

        if is_letter(c) {
            let mut e = p + 1;
            while is_letter(at(src, e)) {
                e += 1;
            }
            emit!(ALPHA, p, e);
            continue;
        }
        if c >= 128 {
            let mut e = p + 1;
            while e < n && src[e] >= 128 {
                e += 1;
            }
            emit!(UNICODE_TEXT, p, e);
            continue;
        }
        if is_digit(c) {
            let mut e = p + 1;
            while is_digit(at(src, e)) {
                e += 1;
            }
            emit!(NUM, p, e);
            continue;
        }
        let s = single(c);
        if s != 0 {
            emit!(s, p, p + 1);
            continue;
        }
        break;
    }

    if p < n {
        out.types.push(ERROR);
        out.texts.push(String::new());
        out.starts.push(p);
        return out;
    }
    // Jison tries the rules of the current state once more against the empty rest of the input.
    if state < AT_END.len() {
        out.types.push(AT_END[state]);
        out.texts.push(String::new());
        out.starts.push(n);
    }
    out.types.push(END);
    out.texts.push(String::new());
    out.starts.push(n);
    out
}
