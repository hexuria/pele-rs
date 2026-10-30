// Port of src/diagrams/requirement/tokens.ts and lexer.ts.

use crate::js;
use crate::util::chars::{is_space, is_word};

pub mod t {
    pub const END: i32 = 0;
    pub const ERROR: i32 = 1;
    pub const TITLE: i32 = 2;
    pub const ACC_TITLE: i32 = 3;
    pub const ACC_TITLE_VALUE: i32 = 4;
    pub const ACC_DESCR: i32 = 5;
    pub const ACC_DESCR_VALUE: i32 = 6;
    pub const ACC_DESCR_MULTILINE_VALUE: i32 = 7;
    pub const DIRECTION_TB: i32 = 8;
    pub const DIRECTION_BT: i32 = 9;
    pub const DIRECTION_RL: i32 = 10;
    pub const DIRECTION_LR: i32 = 11;
    pub const NEWLINE: i32 = 12;
    pub const EOF: i32 = 13;
    pub const RD: i32 = 14;
    pub const STRUCT_START: i32 = 15;
    pub const STRUCT_STOP: i32 = 16;
    pub const STYLE_SEPARATOR: i32 = 17;
    pub const COLONSEP: i32 = 18;
    pub const ID: i32 = 19;
    pub const TEXT: i32 = 20;
    pub const RISK: i32 = 21;
    pub const VERIFYMTHD: i32 = 22;
    pub const REQUIREMENT: i32 = 23;
    pub const FUNCTIONAL_REQUIREMENT: i32 = 24;
    pub const INTERFACE_REQUIREMENT: i32 = 25;
    pub const PERFORMANCE_REQUIREMENT: i32 = 26;
    pub const PHYSICAL_REQUIREMENT: i32 = 27;
    pub const DESIGN_CONSTRAINT: i32 = 28;
    pub const LOW_RISK: i32 = 29;
    pub const MED_RISK: i32 = 30;
    pub const HIGH_RISK: i32 = 31;
    pub const VERIFY_ANALYSIS: i32 = 32;
    pub const VERIFY_DEMONSTRATION: i32 = 33;
    pub const VERIFY_INSPECTION: i32 = 34;
    pub const VERIFY_TEST: i32 = 35;
    pub const ELEMENT: i32 = 36;
    pub const CONTAINS: i32 = 37;
    pub const COPIES: i32 = 38;
    pub const DERIVES: i32 = 39;
    pub const SATISFIES: i32 = 40;
    pub const VERIFIES: i32 = 41;
    pub const REFINES: i32 = 42;
    pub const TRACES: i32 = 43;
    pub const TYPE: i32 = 44;
    pub const DOCREF: i32 = 45;
    pub const STYLE: i32 = 46;
    pub const CLASSDEF: i32 = 47;
    pub const CLASS: i32 = 48;
    pub const ALPHA: i32 = 49;
    pub const COLON: i32 = 50;
    pub const SEMICOLON: i32 = 51;
    pub const PERCENT: i32 = 52;
    pub const MINUS: i32 = 53;
    pub const BRKT: i32 = 54;
    pub const END_ARROW_L: i32 = 55;
    pub const END_ARROW_R: i32 = 56;
    pub const LINE: i32 = 57;
    pub const Q_STRING: i32 = 58;
    pub const UNQ_STRING: i32 = 59;
    pub const COMMA: i32 = 60;
}

pub const TOKEN_NAMES: [&str; 61] = [
    "$end",
    "INVALID",
    "title",
    "acc_title",
    "acc_title_value",
    "acc_descr",
    "acc_descr_value",
    "acc_descr_multiline_value",
    "direction_tb",
    "direction_bt",
    "direction_rl",
    "direction_lr",
    "NEWLINE",
    "EOF",
    "RD",
    "STRUCT_START",
    "STRUCT_STOP",
    "STYLE_SEPARATOR",
    "COLONSEP",
    "ID",
    "TEXT",
    "RISK",
    "VERIFYMTHD",
    "REQUIREMENT",
    "FUNCTIONAL_REQUIREMENT",
    "INTERFACE_REQUIREMENT",
    "PERFORMANCE_REQUIREMENT",
    "PHYSICAL_REQUIREMENT",
    "DESIGN_CONSTRAINT",
    "LOW_RISK",
    "MED_RISK",
    "HIGH_RISK",
    "VERIFY_ANALYSIS",
    "VERIFY_DEMONSTRATION",
    "VERIFY_INSPECTION",
    "VERIFY_TEST",
    "ELEMENT",
    "CONTAINS",
    "COPIES",
    "DERIVES",
    "SATISFIES",
    "VERIFIES",
    "REFINES",
    "TRACES",
    "TYPE",
    "DOCREF",
    "STYLE",
    "CLASSDEF",
    "CLASS",
    "ALPHA",
    "COLON",
    "SEMICOLON",
    "PERCENT",
    "MINUS",
    "BRKT",
    "END_ARROW_L",
    "END_ARROW_R",
    "LINE",
    "qString",
    "unqString",
    "COMMA",
];

// Every keyword is matched without regard to case and must end at a word boundary.
fn keyword(word: &str) -> Option<i32> {
    Some(match word {
        "requirementdiagram" => t::RD,
        "id" => t::ID,
        "text" => t::TEXT,
        "risk" => t::RISK,
        "verifymethod" => t::VERIFYMTHD,
        "requirement" => t::REQUIREMENT,
        "functionalrequirement" => t::FUNCTIONAL_REQUIREMENT,
        "interfacerequirement" => t::INTERFACE_REQUIREMENT,
        "performancerequirement" => t::PERFORMANCE_REQUIREMENT,
        "physicalrequirement" => t::PHYSICAL_REQUIREMENT,
        "designconstraint" => t::DESIGN_CONSTRAINT,
        "low" => t::LOW_RISK,
        "medium" => t::MED_RISK,
        "high" => t::HIGH_RISK,
        "analysis" => t::VERIFY_ANALYSIS,
        "demonstration" => t::VERIFY_DEMONSTRATION,
        "inspection" => t::VERIFY_INSPECTION,
        "test" => t::VERIFY_TEST,
        "element" => t::ELEMENT,
        "contains" => t::CONTAINS,
        "copies" => t::COPIES,
        "derives" => t::DERIVES,
        "satisfies" => t::SATISFIES,
        "verifies" => t::VERIFIES,
        "refines" => t::REFINES,
        "traces" => t::TRACES,
        "type" => t::TYPE,
        "docref" => t::DOCREF,
        "style" => t::STYLE,
        "classdef" => t::CLASSDEF,
        "class" => t::CLASS,
        _ => return None,
    })
}

pub struct Tokens {
    pub types: Vec<i32>,
    pub texts: Vec<String>,
    pub starts: Vec<usize>,
}

#[derive(Clone, Copy, PartialEq)]
enum S {
    Initial,
    String,
    Style,
    AccTitle,
    AccDescr,
    AccDescrMultiline,
}

const LONGEST_KEYWORD: usize = 22;

fn is_line_end(c: u32) -> bool {
    c == 10 || c == 13 || c == 0x2028 || c == 0x2029
}

// Characters that end an unquoted string: `:,{<>-=` and line breaks.
fn stops(c: u32) -> bool {
    matches!(c, 58 | 44 | 13 | 10 | 123 | 60 | 62 | 45 | 61)
}

struct Lexer<'a> {
    src: &'a [u16],
    n: usize,
    p: usize,
    types: Vec<i32>,
    texts: Vec<String>,
    starts: Vec<usize>,
    stack: Vec<S>,
    state: S,
    dir_starts: [Vec<usize>; 4],
    dir_ends: [Vec<usize>; 4],
    dir_seen: [usize; 4],
    any_dir: bool,
    line_end: isize,
}

fn c_at(src: &[u16], i: usize) -> u32 {
    src.get(i).map_or(u32::MAX, |&c| c as u32)
}

// Case-insensitive ASCII word at `at`.
fn word_ci(src: &[u16], at: usize, word: &str) -> bool {
    word.bytes().enumerate().all(|(k, w)| {
        let c = c_at(src, at + k);
        c < 128 && (c as u8).eq_ignore_ascii_case(&w)
    })
}

// `/<word>\s*<sep>\s*/iy`: the end of the match.
fn word_sep(src: &[u16], p: usize, word: &str, sep: u32) -> Option<usize> {
    if !word_ci(src, p, word) {
        return None;
    }
    let mut q = p + word.len();
    while is_space(c_at(src, q)) {
        q += 1;
    }
    if c_at(src, q) != sep {
        return None;
    }
    q += 1;
    while is_space(c_at(src, q)) {
        q += 1;
    }
    Some(q)
}

impl<'a> Lexer<'a> {
    fn emit(&mut self, type_: i32, s: usize, e: usize) {
        self.types.push(type_);
        self.texts.push(js::from16(&self.src[s..e]));
        self.starts.push(s);
        self.p = e;
    }

    fn push(&mut self, s: S) {
        self.stack.push(self.state);
        self.state = s;
    }

    fn pop(&mut self) {
        if let Some(s) = self.stack.pop() {
            self.state = s;
        }
    }

    fn line_break(&self, q: usize) -> usize {
        if q >= self.n {
            return self.n;
        }
        self.src[q..].iter().position(|&c| c == 10).map_or(self.n, |k| q + k)
    }

    fn find(&self, from: usize, c: u16) -> Option<usize> {
        if from >= self.n {
            return None;
        }
        self.src[from..].iter().position(|&x| x == c).map(|k| from + k)
    }

    // `.*direction\s+XX[^\n]*`: the last such statement that starts on this line decides the token.
    fn direction(&mut self) -> bool {
        let p = self.p;
        if self.line_end < p as isize {
            self.line_end = (p..self.n).find(|&k| is_line_end(self.src[k] as u32)).unwrap_or(self.n) as isize;
            for k in 0..4 {
                let list = &self.dir_starts[k];
                let mut j = self.dir_seen[k];
                while j < list.len() && list[j] as isize <= self.line_end {
                    j += 1;
                }
                self.dir_seen[k] = j;
            }
        }
        for k in 0..4 {
            if self.dir_seen[k] == 0 {
                continue;
            }
            let j = self.dir_seen[k] - 1;
            if self.dir_starts[k][j] >= p {
                let end = self.line_break(self.dir_ends[k][j]);
                self.emit(t::DIRECTION_TB + k as i32, p, end);
                return true;
            }
        }
        false
    }

    fn initial(&mut self, c: u32) -> bool {
        let src = self.src;
        let n = self.n;
        let p = self.p;
        let lower = c | 32;
        if lower == 116 && word_ci(src, p + 1, "itle") && is_space(c_at(src, p + 5)) {
            let mut q = p + 6;
            while q < n {
                let d = src[q];
                if d == 35 || d == 10 || d == 59 {
                    break;
                }
                q += 1;
            }
            if q > p + 6 {
                self.emit(t::TITLE, p, q);
                return true;
            }
        }
        if lower == 97 && (c_at(src, p + 1) | 32) == 99 && (c_at(src, p + 2) | 32) == 99 {
            if let Some(e) = word_sep(src, p, "accTitle", 58) {
                self.push(S::AccTitle);
                self.emit(t::ACC_TITLE, p, e);
                return true;
            }
            if let Some(e) = word_sep(src, p, "accDescr", 58) {
                self.push(S::AccDescr);
                self.emit(t::ACC_DESCR, p, e);
                return true;
            }
            if let Some(e) = word_sep(src, p, "accDescr", 123) {
                self.push(S::AccDescrMultiline);
                self.p = e;
                return true;
            }
        }
        if self.any_dir && !is_line_end(c) && self.direction() {
            return true;
        }

        if c == 10 || (c == 13 && c_at(src, p + 1) == 10) {
            let mut q = p;
            while q < n {
                let d = src[q];
                if d == 10 {
                    q += 1;
                } else if d == 13 && c_at(src, q + 1) == 10 {
                    q += 2;
                } else {
                    break;
                }
            }
            self.emit(t::NEWLINE, p, q);
            return true;
        }
        if is_space(c) {
            let mut q = p + 1;
            while q < n && is_space(src[q] as u32) {
                q += 1;
            }
            self.p = q;
            return true;
        }
        if c == 35 || c == 37 {
            self.p = self.line_break(p);
            return true;
        }

        if is_word(c) {
            let mut q = p + 1;
            while q < n && is_word(src[q] as u32) {
                q += 1;
            }
            if q - p <= LONGEST_KEYWORD {
                if let Some(k) = keyword(&js::from16(&src[p..q]).to_ascii_lowercase()) {
                    if k == t::STYLE || k == t::CLASSDEF || k == t::CLASS {
                        self.push(S::Style);
                    }
                    self.emit(k, p, q);
                    return true;
                }
            }
            while q < n {
                let d = src[q] as u32;
                if d < 128 && stops(d) {
                    break;
                }
                q += 1;
            }
            self.types.push(t::UNQ_STRING);
            self.texts.push(js::trim(&js::from16(&src[p..q])).to_string());
            self.starts.push(p);
            self.p = q;
            return true;
        }

        match c {
            123 => self.emit(t::STRUCT_START, p, p + 1),
            125 => self.emit(t::STRUCT_STOP, p, p + 1),
            58 => {
                if c_at(src, p + 1) == 58 && c_at(src, p + 2) == 58 {
                    self.emit(t::STYLE_SEPARATOR, p, p + 3)
                } else {
                    self.emit(t::COLONSEP, p, p + 1)
                }
            }
            60 => {
                if c_at(src, p + 1) != 45 {
                    return false;
                }
                self.emit(t::END_ARROW_L, p, p + 2);
            }
            45 => {
                if c_at(src, p + 1) == 62 {
                    self.emit(t::END_ARROW_R, p, p + 2)
                } else {
                    self.emit(t::LINE, p, p + 1)
                }
            }
            34 => {
                self.push(S::String);
                self.p += 1;
            }
            44 => self.emit(t::COMMA, p, p + 1),
            _ => return false,
        }
        true
    }

    fn style(&mut self, c: u32) -> bool {
        let p = self.p;
        if is_word(c) {
            let mut q = p + 1;
            while q < self.n && is_word(self.src[q] as u32) {
                q += 1;
            }
            self.emit(t::ALPHA, p, q);
            return true;
        }
        match c {
            58 => self.emit(t::COLON, p, p + 1),
            59 => self.emit(t::SEMICOLON, p, p + 1),
            37 => self.emit(t::PERCENT, p, p + 1),
            45 => self.emit(t::MINUS, p, p + 1),
            35 => self.emit(t::BRKT, p, p + 1),
            32 => self.p += 1,
            34 => {
                self.push(S::String);
                self.p += 1;
            }
            10 => {
                self.pop();
                self.p += 1;
            }
            44 => self.emit(t::COMMA, p, p + 1),
            _ => return false,
        }
        true
    }
}

// /direction\s+(?:(TB)|(BT)|(RL)|LR)/gi over the whole text: [kind, start, end] of each match.
fn directions(src: &[u16]) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < src.len() {
        if word_ci(src, i, "direction") {
            let mut q = i + 9;
            while is_space(c_at(src, q)) {
                q += 1;
            }
            if q > i + 9 {
                let kind = ["TB", "BT", "RL", "LR"].iter().position(|w| word_ci(src, q, w));
                if let Some(kind) = kind {
                    out.push((kind, i, q + 2));
                    i = q + 2;
                    continue;
                }
            }
        }
        i += 1;
    }
    out
}

// Produces the token stream of Mermaid's requirement lexer. Its rules are tried in order and
// the first match wins, so a line that holds a direction statement anywhere is one token.
pub fn tokenize(src: &[u16]) -> Tokens {
    let n = src.len();
    let mut lx = Lexer {
        src,
        n,
        p: 0,
        types: Vec::new(),
        texts: Vec::new(),
        starts: Vec::new(),
        stack: Vec::new(),
        state: S::Initial,
        dir_starts: Default::default(),
        dir_ends: Default::default(),
        dir_seen: [0; 4],
        any_dir: false,
        line_end: -1,
    };
    for (kind, start, end) in directions(src) {
        lx.dir_starts[kind].push(start);
        lx.dir_ends[kind].push(end);
        lx.any_dir = true;
    }

    while lx.p < n {
        let c = src[lx.p] as u32;
        let mut ok = true;
        match lx.state {
            S::Initial => ok = lx.initial(c),
            S::Style => ok = lx.style(c),
            S::String => {
                if c == 34 {
                    lx.pop();
                    lx.p += 1;
                } else {
                    let e = lx.find(lx.p, 34).unwrap_or(n);
                    lx.emit(t::Q_STRING, lx.p, e);
                }
            }
            S::AccTitle => {
                lx.pop();
                let e = lx.line_break(lx.p);
                lx.emit(t::ACC_TITLE_VALUE, lx.p, e);
            }
            S::AccDescr => {
                lx.pop();
                let e = lx.line_break(lx.p);
                lx.emit(t::ACC_DESCR_VALUE, lx.p, e);
            }
            S::AccDescrMultiline => {
                if c == 125 {
                    lx.pop();
                    lx.p += 1;
                } else {
                    let e = lx.find(lx.p, 125).unwrap_or(n);
                    lx.emit(t::ACC_DESCR_MULTILINE_VALUE, lx.p, e);
                }
            }
        }
        if !ok {
            lx.types.push(t::ERROR);
            lx.texts.push(String::new());
            lx.starts.push(lx.p);
            return Tokens { types: lx.types, texts: lx.texts, starts: lx.starts };
        }
    }

    // At the end of input Jison still tries the rules that can match nothing, once.
    let last = match lx.state {
        S::Initial => t::EOF,
        S::String => t::Q_STRING,
        S::AccTitle => t::ACC_TITLE_VALUE,
        S::AccDescr => t::ACC_DESCR_VALUE,
        S::AccDescrMultiline => t::ACC_DESCR_MULTILINE_VALUE,
        S::Style => -1,
    };
    if last != -1 {
        lx.emit(last, n, n);
    }
    lx.emit(t::END, n, n);
    Tokens { types: lx.types, texts: lx.texts, starts: lx.starts }
}
