// Port of src/diagrams/block/lexer.ts and tokens.ts.

use crate::js;
use crate::util::chars::{is_space, is_word};

pub const END: u32 = 0;
pub const ERROR: u32 = 1;
pub const KEY: u32 = 2;
pub const ID_BLOCK: u32 = 3;
pub const COLUMNS: u32 = 4;
pub const MD_STR: u32 = 5;
pub const STR: u32 = 6;
pub const SPACE_BLOCK: u32 = 7;
pub const DEFAULT: u32 = 8;
pub const LINKSTYLE: u32 = 9;
pub const INTERPOLATE: u32 = 10;
pub const CLASSDEF: u32 = 11;
pub const DEFAULT_CLASSDEF_ID: u32 = 12;
pub const CLASSDEF_ID: u32 = 13;
pub const CLASSDEF_STYLEOPTS: u32 = 14;
pub const CLASS: u32 = 15;
pub const CLASSENTITY_IDS: u32 = 16;
pub const STYLECLASS: u32 = 17;
pub const STYLE: u32 = 18;
pub const STYLE_ENTITY_IDS: u32 = 19;
pub const STYLE_DEFINITION_DATA: u32 = 20;
pub const ACC_TITLE: u32 = 21;
pub const ACC_TITLE_VALUE: u32 = 22;
pub const ACC_DESCR: u32 = 23;
pub const ACC_DESCR_VALUE: u32 = 24;
pub const ACC_DESCR_MULTILINE_VALUE: u32 = 25;
pub const END_KW: u32 = 26;
pub const NODE_DSTART: u32 = 27;
pub const NODE_DEND: u32 = 28;
pub const BLOCK_ARROW_START: u32 = 29;
pub const BLOCK_ARROW_END: u32 = 30;
pub const DIR: u32 = 31;
pub const NODE_ID: u32 = 32;
pub const LINK: u32 = 33;
pub const START_LINK: u32 = 34;
pub const LINK_LABEL: u32 = 35;
pub const SIZE: u32 = 36;
pub const EOF: u32 = 37;

pub const TOKEN_NAMES: [&str; 38] = [
    "$end",
    "INVALID",
    "BLOCK_DIAGRAM_KEY",
    "id-block",
    "COLUMNS",
    "MD_STR",
    "STR",
    "SPACE_BLOCK",
    "DEFAULT",
    "LINKSTYLE",
    "INTERPOLATE",
    "classDef",
    "DEFAULT_CLASSDEF_ID",
    "CLASSDEF_ID",
    "CLASSDEF_STYLEOPTS",
    "class",
    "CLASSENTITY_IDS",
    "STYLECLASS",
    "style",
    "STYLE_ENTITY_IDS",
    "STYLE_DEFINITION_DATA",
    "acc_title",
    "acc_title_value",
    "acc_descr",
    "acc_descr_value",
    "acc_descr_multiline_value",
    "end",
    "NODE_DSTART",
    "NODE_DEND",
    "BLOCK_ARROW_START",
    "BLOCK_ARROW_END",
    "DIR",
    "NODE_ID",
    "LINK",
    "START_LINK",
    "LINK_LABEL",
    "SIZE",
    "EOF",
];

pub struct Tokens {
    pub types: Vec<u32>,
    pub texts: Vec<String>,
    pub starts: Vec<usize>,
}

const S_INITIAL: u8 = 0;
const S_STRING: u8 = 1;
const S_MD_STRING: u8 = 2;
const S_NODE: u8 = 3;
const S_BLOCK_ARROW: u8 = 4;
const S_ARROW_DIR: u8 = 5;
const S_LLABEL: u8 = 6;
const S_CLASSDEF: u8 = 7;
const S_CLASSDEFID: u8 = 8;
const S_CLASS: u8 = 9;
const S_CLASS_STYLE: u8 = 10;
const S_STYLE_STMNT: u8 = 11;
const S_STYLE_DEFINITION: u8 = 12;
const S_ACC_TITLE: u8 = 13;
const S_ACC_DESCR: u8 = 14;
const S_ACC_DESCR_MULTILINE: u8 = 15;

const DIRECTIONS: [&str; 6] = ["right", "left", "x", "y", "up", "down"];

fn id_stop(c: u32) -> bool {
    matches!(c, 9 | 10 | 11 | 12 | 13 | 32 | 40 | 41 | 45 | 58 | 60 | 61 | 62 | 91 | 123 | 125)
}

struct Lexer<'a> {
    src: &'a [u16],
    n: usize,
    out: Tokens,
    stack: Vec<u8>,
    state: u8,
    p: usize,
}

impl Lexer<'_> {
    fn at(&self, i: usize) -> u32 {
        self.src.get(i).map_or(u32::MAX, |&c| c as u32)
    }
    fn starts_with(&self, text: &str, p: usize) -> bool {
        text.encode_utf16().enumerate().all(|(k, u)| self.src.get(p + k) == Some(&u))
    }
    fn put(&mut self, ty: u32, text: String, end: usize) -> bool {
        self.out.types.push(ty);
        self.out.texts.push(text);
        self.out.starts.push(self.p);
        self.p = end;
        true
    }
    fn emit(&mut self, ty: u32, end: usize) -> bool {
        let text = js::from16(&self.src[self.p..end]);
        self.put(ty, text, end)
    }
    fn push(&mut self, s: u8) {
        self.stack.push(self.state);
        self.state = s;
    }
    fn pop(&mut self) {
        if let Some(s) = self.stack.pop() {
            self.state = s;
        }
    }
    fn ws(&self, mut q: usize) -> usize {
        while q < self.n && is_space(self.src[q] as u32) {
            q += 1;
        }
        q
    }
    fn digits(&self, mut q: usize) -> usize {
        while q < self.n && (48..=57).contains(&self.src[q]) {
            q += 1;
        }
        q
    }
    fn word(&self, mut q: usize) -> usize {
        while q < self.n && is_word(self.src[q] as u32) {
            q += 1;
        }
        q
    }
    fn keyword(&self, text: &str) -> bool {
        self.starts_with(text, self.p) && !is_word(self.at(self.p + text.len()))
    }
    fn index_of(&self, c: u16, from: usize) -> Option<usize> {
        self.src[from.min(self.n)..].iter().position(|&x| x == c).map(|k| from + k)
    }
    fn quote(&mut self) -> bool {
        if self.at(self.p + 1) == 96 {
            self.push(S_MD_STRING);
            self.p += 2;
        } else {
            self.push(S_STRING);
            self.p += 1;
        }
        true
    }
    // End of `.+-[xo>]?` at q.
    fn dotted(&self, mut q: usize) -> Option<usize> {
        if self.at(q) != 46 {
            return None;
        }
        loop {
            q += 1;
            if self.at(q) != 46 {
                break;
            }
        }
        if self.at(q) != 45 {
            return None;
        }
        let c = self.at(q + 1);
        Some(if c == 120 || c == 111 || c == 62 { q + 2 } else { q + 1 })
    }
    // End of a complete link at q, before its trailing whitespace.
    fn link_end(&self, mut q: usize) -> Option<usize> {
        let c = self.at(q);
        if c == 120 || c == 111 || c == 60 {
            q += 1;
        }
        let line = self.at(q);
        if line == 46 {
            return self.dotted(q);
        }
        if line != 45 && line != 61 {
            return None;
        }
        let mut k = q + 1;
        while self.at(k) == line {
            k += 1;
        }
        if k - q == 1 {
            return if line == 45 { self.dotted(k) } else { None };
        }
        let head = self.at(k);
        if head == 120 || head == 111 || head == 62 {
            return Some(k + 1);
        }
        (k - q >= 3).then_some(k)
    }
    fn link(&mut self) -> bool {
        if let Some(e) = self.link_end(self.p) {
            let e = self.ws(e);
            return self.emit(LINK, e);
        }
        let q = if self.at(self.p) == 60 { self.p + 1 } else { self.p };
        let (a, b) = (self.at(q), self.at(q + 1));
        if (a == 45 && (b == 45 || b == 46)) || (a == 61 && b == 61) {
            self.push(S_LLABEL);
            let e = self.ws(q + 2);
            return self.emit(START_LINK, e);
        }
        false
    }
    fn line_state(&mut self, ty: u32) -> bool {
        let e = self.index_of(10, self.p).unwrap_or(self.n);
        self.pop();
        self.emit(ty, e)
    }
    // `\w+(,\s*\w+)*`
    fn id_list(&mut self, ty: u32, next: u8) -> bool {
        let mut k = self.word(self.p);
        if k == self.p {
            return false;
        }
        while self.at(k) == 44 {
            let from = self.ws(k + 1);
            let to = self.word(from);
            if to == from {
                break;
            }
            k = to;
        }
        self.state = next;
        self.emit(ty, k)
    }
    fn initial(&mut self, c: u32) -> bool {
        let p = self.p;
        if is_space(c) {
            self.p = self.ws(p + 1);
            return true;
        }
        match c {
            98 => {
                if self.starts_with("block", p) {
                    if self.keyword("block-beta") {
                        return self.emit(KEY, p + 10);
                    }
                    if self.at(p + 5) == 58 {
                        return self.emit(ID_BLOCK, p + 6);
                    }
                    if !is_word(self.at(p + 5)) {
                        return self.emit(KEY, p + 5);
                    }
                }
            }
            99 => {
                if self.starts_with("columns", p) {
                    let q = self.ws(p + 7);
                    if q > p + 7 {
                        if self.starts_with("auto", q) && !is_word(self.at(q + 4)) {
                            return self.put(COLUMNS, "-1".into(), q + 4);
                        }
                        let e = self.digits(q);
                        if e > q {
                            return self.put(COLUMNS, js::from16(&self.src[q..e]), e);
                        }
                    }
                } else if self.starts_with("class", p) {
                    let def = self.starts_with("Def", p + 5);
                    let from = if def { p + 8 } else { p + 5 };
                    let q = self.ws(from);
                    if q > from {
                        self.push(if def { S_CLASSDEF } else { S_CLASS });
                        return self.emit(if def { CLASSDEF } else { CLASS }, q);
                    }
                }
            }
            34 => return self.quote(),
            115 => {
                if self.starts_with("space", p) {
                    if self.at(p + 5) == 58 {
                        let e = self.digits(p + 6);
                        if e > p + 6 {
                            return self.put(SPACE_BLOCK, js::from16(&self.src[p + 6..e]), e);
                        }
                    }
                    if !is_word(self.at(p + 5)) {
                        return self.put(SPACE_BLOCK, "1".into(), p + 5);
                    }
                } else if self.starts_with("style", p) {
                    let q = self.ws(p + 5);
                    if q > p + 5 {
                        self.push(S_STYLE_STMNT);
                        return self.emit(STYLE, q);
                    }
                }
            }
            100 => {
                if self.keyword("default") {
                    return self.emit(DEFAULT, p + 7);
                }
            }
            108 => {
                if self.keyword("linkStyle") {
                    return self.emit(LINKSTYLE, p + 9);
                }
            }
            105 => {
                if self.keyword("interpolate") {
                    return self.emit(INTERPOLATE, p + 11);
                }
            }
            97 => {
                if self.starts_with("accTitle", p) {
                    let q = self.ws(p + 8);
                    if self.at(q) == 58 {
                        self.push(S_ACC_TITLE);
                        let e = self.ws(q + 1);
                        return self.emit(ACC_TITLE, e);
                    }
                } else if self.starts_with("accDescr", p) {
                    let q = self.ws(p + 8);
                    let d = self.at(q);
                    if d == 58 {
                        self.push(S_ACC_DESCR);
                        let e = self.ws(q + 1);
                        return self.emit(ACC_DESCR, e);
                    }
                    if d == 123 {
                        self.push(S_ACC_DESCR_MULTILINE);
                        self.p = self.ws(q + 1);
                        return true;
                    }
                }
            }
            101 => {
                if self.keyword("end") {
                    let e = self.ws(p + 3);
                    return self.emit(END_KW, e);
                }
            }
            45 => {
                if self.at(p + 1) == 41 {
                    self.push(S_NODE);
                    return self.emit(NODE_DSTART, p + 2);
                }
                return self.link();
            }
            61 => return self.link(),
            60 => {
                if self.at(p + 1) == 91 {
                    self.push(S_BLOCK_ARROW);
                    return self.emit(BLOCK_ARROW_START, p + 2);
                }
                return self.link();
            }
            40 => {
                let d = self.at(p + 1);
                let len = if d == 45 || d == 91 {
                    2
                } else if d == 40 {
                    if self.at(p + 2) == 40 {
                        3
                    } else {
                        2
                    }
                } else {
                    1
                };
                self.push(S_NODE);
                return self.emit(NODE_DSTART, p + len);
            }
            41 | 123 => {
                self.push(S_NODE);
                let e = if self.at(p + 1) == c { p + 2 } else { p + 1 };
                return self.emit(NODE_DSTART, e);
            }
            62 => {
                self.push(S_NODE);
                return self.emit(NODE_DSTART, p + 1);
            }
            91 => {
                let d = self.at(p + 1);
                self.push(S_NODE);
                let e = if matches!(d, 91 | 124 | 40 | 92 | 47) { p + 2 } else { p + 1 };
                return self.emit(NODE_DSTART, e);
            }
            58 => {
                let e = self.digits(p + 1);
                if e > p + 1 {
                    return self.put(SIZE, js::from16(&self.src[p + 1..e]), e);
                }
                return false;
            }
            _ => {}
        }
        let mut q = p;
        while q < self.n {
            let ch = self.src[q] as u32;
            if if ch < 128 { id_stop(ch) } else { is_space(ch) } {
                break;
            }
            q += 1;
        }
        q > p && self.emit(NODE_ID, q)
    }
    fn node_end(&mut self, c: u32) -> bool {
        let p = self.p;
        let d = self.at(p + 1);
        let len = match c {
            34 => return self.quote(),
            40 => {
                if d == 45 {
                    2
                } else if d == 40 {
                    if self.at(p + 2) == 40 {
                        3
                    } else {
                        2
                    }
                } else {
                    1
                }
            }
            41 => {
                if d == 41 {
                    if self.at(p + 2) == 41 {
                        3
                    } else {
                        2
                    }
                } else if d == 93 {
                    2
                } else {
                    1
                }
            }
            125 => {
                if d == 125 {
                    2
                } else {
                    1
                }
            }
            45 => {
                if d == 41 {
                    2
                } else {
                    0
                }
            }
            93 => {
                if d == 93 || d == 41 || d == 62 {
                    2
                } else {
                    1
                }
            }
            92 | 47 => {
                if d == 93 {
                    2
                } else {
                    0
                }
            }
            _ => 0,
        };
        if len == 0 {
            return false;
        }
        self.pop();
        self.emit(NODE_DEND, p + len)
    }
    fn arrow_dir(&mut self, c: u32) -> bool {
        let p = self.p;
        if c == 41 {
            self.pop();
            self.pop();
            let e = self.ws(p + 1);
            return self.put(BLOCK_ARROW_END, "]>".into(), e);
        }
        let from = self.ws(if c == 44 { p + 1 } else { p });
        for dir in DIRECTIONS {
            if self.starts_with(dir, from) {
                let e = self.ws(from + dir.len());
                let text = js::from16(&self.src[if c == 44 { from } else { p }..e]);
                return self.put(DIR, text, e);
            }
        }
        false
    }
}

pub fn tokenize(src: &[u16]) -> Tokens {
    let n = src.len();
    let mut l = Lexer { src, n, out: Tokens { types: Vec::new(), texts: Vec::new(), starts: Vec::new() }, stack: Vec::new(), state: S_INITIAL, p: 0 };
    loop {
        if l.p >= n {
            let ty = match l.state {
                S_INITIAL => Some(EOF),
                S_STRING => Some(STR),
                S_CLASSDEFID => Some(CLASSDEF_STYLEOPTS),
                S_CLASS_STYLE => Some(STYLECLASS),
                S_STYLE_DEFINITION => Some(STYLE_DEFINITION_DATA),
                S_ACC_TITLE => Some(ACC_TITLE_VALUE),
                S_ACC_DESCR => Some(ACC_DESCR_VALUE),
                S_ACC_DESCR_MULTILINE => Some(ACC_DESCR_MULTILINE_VALUE),
                _ => None,
            };
            l.p = n;
            if let Some(ty) = ty {
                l.put(ty, String::new(), n);
            }
            break;
        }
        let c = src[l.p] as u32;
        let ok = match l.state {
            S_INITIAL => l.initial(c),
            S_STRING => {
                if c == 34 {
                    l.pop();
                    l.p += 1;
                } else {
                    let e = l.index_of(34, l.p).unwrap_or(n);
                    l.emit(STR, e);
                }
                true
            }
            S_MD_STRING => {
                if c == 96 {
                    let ok = l.at(l.p + 1) == 34;
                    if ok {
                        l.pop();
                        l.p += 2;
                    }
                    ok
                } else if c == 34 {
                    false
                } else {
                    let mut q = l.p + 1;
                    while q < n && src[q] != 96 && src[q] != 34 {
                        q += 1;
                    }
                    l.emit(MD_STR, q)
                }
            }
            S_NODE => l.node_end(c),
            S_BLOCK_ARROW => {
                if c == 34 {
                    l.quote()
                } else if c == 93 && l.at(l.p + 1) == 62 {
                    let q = l.ws(l.p + 2);
                    let ok = l.at(q) == 40;
                    if ok {
                        l.push(S_ARROW_DIR);
                        l.p = q + 1;
                    }
                    ok
                } else {
                    false
                }
            }
            S_ARROW_DIR => l.arrow_dir(c),
            S_LLABEL => {
                if c == 34 {
                    if l.at(l.p + 1) == 96 {
                        l.quote();
                    } else {
                        let e = l.p + 1;
                        l.emit(LINK_LABEL, e);
                        l.push(S_STRING);
                    }
                    true
                } else {
                    match l.link_end(l.ws(l.p)) {
                        Some(e) => {
                            l.pop();
                            let e = l.ws(e);
                            l.emit(LINK, e)
                        }
                        None => false,
                    }
                }
            }
            S_CLASSDEF => {
                let p = l.p;
                let mut ty = DEFAULT_CLASSDEF_ID;
                let mut k = if l.starts_with("DEFAULT", p) { p + 7 } else { p };
                let mut q = l.ws(k);
                if k == p || q == k {
                    ty = CLASSDEF_ID;
                    k = l.word(p);
                    q = l.ws(k);
                }
                let ok = k > p && q > k;
                if ok {
                    l.state = S_CLASSDEFID;
                    l.emit(ty, q);
                }
                ok
            }
            S_CLASSDEFID => l.line_state(CLASSDEF_STYLEOPTS),
            S_CLASS => l.id_list(CLASSENTITY_IDS, S_CLASS_STYLE),
            S_CLASS_STYLE => l.line_state(STYLECLASS),
            S_STYLE_STMNT => l.id_list(STYLE_ENTITY_IDS, S_STYLE_DEFINITION),
            S_STYLE_DEFINITION => l.line_state(STYLE_DEFINITION_DATA),
            S_ACC_TITLE => l.line_state(ACC_TITLE_VALUE),
            S_ACC_DESCR => l.line_state(ACC_DESCR_VALUE),
            _ => {
                if c == 125 {
                    l.pop();
                    l.p += 1;
                } else {
                    let e = l.index_of(125, l.p).unwrap_or(n);
                    l.emit(ACC_DESCR_MULTILINE_VALUE, e);
                }
                true
            }
        };
        if !ok {
            l.out.types.push(ERROR);
            l.out.texts.push(String::new());
            l.out.starts.push(l.p);
            return l.out;
        }
    }
    l.out.types.push(END);
    l.out.texts.push(String::new());
    l.out.starts.push(n);
    l.out
}
