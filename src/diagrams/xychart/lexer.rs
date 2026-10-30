// Port of src/diagrams/xychart/lexer.ts and tokens.ts.

use crate::diagrams::common::caseless::word_at;
use crate::js;
use crate::util::chars::{is_space, is_word};

pub const END: u32 = 0;
pub const ERROR: u32 = 1;
pub const NEWLINE: u32 = 2;
pub const TITLE: u32 = 3;
pub const ACC_TITLE: u32 = 4;
pub const ACC_TITLE_VALUE: u32 = 5;
pub const ACC_DESCR: u32 = 6;
pub const ACC_DESCR_VALUE: u32 = 7;
pub const ACC_DESCR_MULTILINE_VALUE: u32 = 8;
pub const XYCHART: u32 = 9;
pub const CHART_ORIENTATION: u32 = 10;
pub const X_AXIS: u32 = 11;
pub const Y_AXIS: u32 = 12;
pub const ARROW_DELIMITER: u32 = 13;
pub const LINE: u32 = 14;
pub const BAR: u32 = 15;
pub const NUMBER_WITH_DECIMAL: u32 = 16;
pub const SQUARE_BRACES_START: u32 = 17;
pub const SQUARE_BRACES_END: u32 = 18;
pub const STR: u32 = 19;
pub const ALPHA: u32 = 20;
pub const COLON: u32 = 21;
pub const PLUS: u32 = 22;
pub const COMMA: u32 = 23;
pub const EQUALS: u32 = 24;
pub const MULT: u32 = 25;
pub const BRKT: u32 = 26;
pub const UNDERSCORE: u32 = 27;
pub const DOT: u32 = 28;
pub const AMP: u32 = 29;
pub const MINUS: u32 = 30;
pub const NUM: u32 = 31;
pub const SEMI: u32 = 32;
pub const EOF: u32 = 33;

pub const TOKEN_NAMES: [&str; 34] = [
    "$end",
    "INVALID",
    "NEWLINE",
    "title",
    "acc_title",
    "acc_title_value",
    "acc_descr",
    "acc_descr_value",
    "acc_descr_multiline_value",
    "XYCHART",
    "CHART_ORIENTATION",
    "X_AXIS",
    "Y_AXIS",
    "ARROW_DELIMITER",
    "LINE",
    "BAR",
    "NUMBER_WITH_DECIMAL",
    "SQUARE_BRACES_START",
    "SQUARE_BRACES_END",
    "STR",
    "ALPHA",
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
    "SEMI",
    "EOF",
];

pub struct Tokens {
    pub types: Vec<u32>,
    pub texts: Vec<String>,
    pub starts: Vec<usize>,
}

const S_INITIAL: usize = 0;
const S_AXIS_DATA: usize = 1;
const S_AXIS_BAND_DATA: usize = 2;
const S_DATA: usize = 3;
const S_DATA_INNER: usize = 4;
const S_STRING: usize = 5;
const S_ACC_TITLE: usize = 6;
const S_ACC_DESCR: usize = 7;
const S_ACC_DESCR_MULTILINE: usize = 8;

// The token each state gives for the empty rest of the input.
const AT_END: [u32; 9] = [EOF, EOF, EOF, EOF, EOF, STR, ACC_TITLE_VALUE, ACC_DESCR_VALUE, ACC_DESCR_MULTILINE_VALUE];

// Mermaid's grammar means to open a markdown string at `"` followed by a backtick, but its
// quoting turns the rule into one that matches this text and leaves the current state.
fn broken_rule() -> String {
    format!("`){}{{ this.pushstate(md_string); }}\n<md_string>(?:(?!`\").)+{}{{ return md_str; }}\n<md_string>(?:`", " ".repeat(36), " ".repeat(18))
}

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
        59 => SEMI,
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

pub fn tokenize(src: &[u16]) -> Tokens {
    let n = src.len();
    let mut out = Tokens { types: Vec::new(), texts: Vec::new(), starts: Vec::new() };
    let mut stack: Vec<usize> = Vec::new();
    let mut state = S_INITIAL;
    let mut p = 0usize;
    let broken = broken_rule();
    let broken_len = broken.encode_utf16().count();

    macro_rules! emit {
        ($ty:expr, $s:expr, $e:expr) => {{
            let (s, e) = ($s, $e);
            out.types.push($ty);
            out.texts.push(js::from16(&src[s..e]));
            out.starts.push(s);
            p = e;
        }};
    }
    macro_rules! push {
        ($s:expr) => {{
            stack.push(state);
            state = $s;
        }};
    }
    macro_rules! pop {
        () => {{
            if let Some(s) = stack.pop() {
                state = s;
            }
        }};
    }
    let line_end = |q: usize| src[q.min(n)..].iter().position(|&c| c == 10).map_or(n, |k| q + k);
    let index_of = |c: u16, q: usize| src[q.min(n)..].iter().position(|&x| x == c).map(|k| q + k);
    let run = |mut q: usize| {
        while q < n && is_space(src[q] as u32) {
            q += 1;
        }
        q
    };
    let digits = |mut q: usize| {
        while is_digit(at(src, q)) {
            q += 1;
        }
        q
    };
    let keyword = |p: usize, word: &str| word_at(src, p, word) && !is_word(at(src, p + word.len()));

    while p < n {
        let c = src[p] as u32;
        if state == S_STRING {
            if c == 34 {
                p += 1;
                pop!();
            } else {
                emit!(STR, p, index_of(34, p).unwrap_or(n));
            }
            continue;
        }
        if state == S_ACC_TITLE || state == S_ACC_DESCR {
            emit!(AT_END[state], p, line_end(p));
            pop!();
            continue;
        }
        if state == S_ACC_DESCR_MULTILINE {
            if c == 125 {
                p += 1;
                pop!();
            } else {
                emit!(ACC_DESCR_MULTILINE_VALUE, p, index_of(125, p).unwrap_or(n));
            }
            continue;
        }

        let next = at(src, p + 1);
        if c == 37 && next == 37 {
            p = line_end(p + 2);
            continue;
        }
        if c == 10 || c == 13 {
            // Only a line break that directly follows an axis or a series ends it; one after
            // spaces is skipped with them, and the state carries on to the next line.
            if (state == S_AXIS_DATA || state == S_DATA) && (c == 10 || next == 10) {
                emit!(NEWLINE, p, if c == 10 { p + 1 } else { p + 2 });
                pop!();
                continue;
            }
            let mut e = p + 1;
            while matches!(at(src, e), 10 | 13) {
                e += 1;
            }
            emit!(NEWLINE, p, e);
            continue;
        }

        match c | 32 {
            116 => {
                if keyword(p, "title") {
                    emit!(TITLE, p, p + 5);
                    continue;
                }
            }
            97 => {
                if word_at(src, p, "acc") {
                    let descr = word_at(src, p + 3, "descr");
                    if descr || word_at(src, p + 3, "title") {
                        let k = run(p + 8);
                        let d = at(src, k);
                        if d == 58 {
                            emit!(if descr { ACC_DESCR } else { ACC_TITLE }, p, run(k + 1));
                            push!(if descr { S_ACC_DESCR } else { S_ACC_TITLE });
                            continue;
                        }
                        if d == 123 && descr {
                            p = run(k + 1);
                            push!(S_ACC_DESCR_MULTILINE);
                            continue;
                        }
                    }
                }
            }
            120 => {
                if keyword(p, "xychart-beta") {
                    emit!(XYCHART, p, p + 12);
                    continue;
                }
                if keyword(p, "xychart") {
                    emit!(XYCHART, p, p + 7);
                    continue;
                }
                if keyword(p, "x-axis") {
                    emit!(X_AXIS, p, p + 6);
                    push!(S_AXIS_DATA);
                    continue;
                }
            }
            121 => {
                if keyword(p, "y-axis") {
                    emit!(Y_AXIS, p, p + 6);
                    push!(S_AXIS_DATA);
                    continue;
                }
            }
            118 => {
                if word_at(src, p, "vertical") {
                    emit!(CHART_ORIENTATION, p, p + 8);
                    continue;
                }
            }
            104 => {
                if word_at(src, p, "horizontal") {
                    emit!(CHART_ORIENTATION, p, p + 10);
                    continue;
                }
            }
            108 => {
                if keyword(p, "line") {
                    emit!(LINE, p, p + 4);
                    push!(S_DATA);
                    continue;
                }
            }
            98 => {
                if keyword(p, "bar") {
                    emit!(BAR, p, p + 3);
                    push!(S_DATA);
                    continue;
                }
            }
            _ => {}
        }

        if c == 91 {
            if state == S_AXIS_DATA {
                push!(S_AXIS_BAND_DATA);
            } else if state == S_DATA {
                push!(S_DATA_INNER);
            }
            emit!(SQUARE_BRACES_START, p, p + 1);
            continue;
        }
        if c == 93 {
            if state == S_DATA_INNER || state == S_AXIS_BAND_DATA {
                pop!();
            }
            emit!(SQUARE_BRACES_END, p, p + 1);
            continue;
        }
        if state == S_AXIS_DATA && c == 45 && next == 45 && at(src, p + 2) == 62 {
            emit!(ARROW_DELIMITER, p, p + 3);
            continue;
        }
        if state == S_AXIS_DATA || state == S_DATA_INNER {
            let q = if c == 43 || c == 45 { p + 1 } else { p };
            let d = at(src, q);
            let mut e = None;
            if is_digit(d) {
                let mut k = digits(q + 1);
                if at(src, k) == 46 && is_digit(at(src, k + 1)) {
                    k = digits(k + 2);
                }
                e = Some(k);
            } else if d == 46 && is_digit(at(src, q + 1)) {
                e = Some(digits(q + 2));
            }
            if let Some(e) = e {
                emit!(NUMBER_WITH_DECIMAL, p, e);
                continue;
            }
        }
        if c == 96 && word_at(src, p, &broken) {
            p += broken_len;
            pop!();
            continue;
        }
        if c == 34 {
            p += 1;
            push!(S_STRING);
            continue;
        }
        if is_letter(c) {
            let mut e = p + 1;
            while is_letter(at(src, e)) {
                e += 1;
            }
            emit!(ALPHA, p, e);
            continue;
        }
        if is_digit(c) {
            emit!(NUM, p, digits(p + 1));
            continue;
        }
        if is_space(c) {
            p = run(p + 1);
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
    out.types.push(AT_END[state]);
    out.texts.push(String::new());
    out.starts.push(n);
    out.types.push(END);
    out.texts.push(String::new());
    out.starts.push(n);
    out
}
