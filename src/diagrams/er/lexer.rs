// Port of src/diagrams/er/lexer.ts.

use super::tokens as t;
use crate::js;
use crate::util::chars::{is_digit, is_line_end, is_space, is_word};

#[derive(Default, Debug)]
pub struct Tokens {
    pub types: Vec<u32>,
    pub texts: Vec<String>,
    pub starts: Vec<usize>,
}

#[derive(Clone, Copy, PartialEq)]
enum S {
    Initial,
    Block,
    BlockBq,
    AccTitle,
    AccDescr,
    AccDescrMultiline,
    Style,
}

const TEXT: u8 = 1;
const STYLE_TEXT: u8 = 2;
const ATTR_START: u8 = 4;
const ATTR_REST: u8 = 8;

const fn classes() -> [u8; 128] {
    let mut class = [0u8; 128];
    let mut c = 0;
    while c < 128 {
        let word = (c >= 48 && c <= 57) || (c >= 65 && c <= 90) || (c >= 97 && c <= 122) || c == 95;
        let letter = word && (c < 48 || c > 57);
        if word {
            class[c] |= TEXT | STYLE_TEXT | ATTR_REST;
        }
        if letter {
            class[c] |= ATTR_START;
        }
        c += 1;
    }
    class[b'-' as usize] |= TEXT | STYLE_TEXT | ATTR_REST;
    class[b'*' as usize] |= TEXT | STYLE_TEXT | ATTR_REST;
    class[46] |= TEXT | ATTR_REST;
    class[42] |= ATTR_START;
    class[b'[' as usize] |= ATTR_REST;
    class[b']' as usize] |= ATTR_REST;
    class[b'(' as usize] |= ATTR_REST;
    class[b')' as usize] |= ATTR_REST;
    class[b',' as usize] |= ATTR_REST;
    class
}

const CLASS: [u8; 128] = classes();

fn class_has(c: u32, kind: u8) -> bool {
    c < 128 && CLASS[c as usize] & kind != 0
}

// Mermaid's attribute pattern is case-insensitive, which lets the micro sign into its À-￿ range.
fn is_attr(c: u32, kind: u8) -> bool {
    if c == u32::MAX {
        return false;
    }
    if c < 128 {
        CLASS[c as usize] & kind != 0
    } else {
        c >= 0xc0 || c == 0xb5
    }
}

// A run of `TEXT` (or `STYLE_TEXT`) characters: anything past ASCII counts.
fn text_char(c: u32, kind: u8) -> bool {
    c != u32::MAX && (c >= 128 || class_has(c, kind))
}

struct Lx<'a> {
    src: &'a [u16],
    n: usize,
    out: Tokens,
    state: S,
    p: usize,
    dir_next: isize,
    dir_failed: usize,
    line_end: isize,
    tilde_next: isize,
    tilde_line: isize,
    tilde_last: isize,
    ws_next: isize,
}

impl Lx<'_> {
    fn c(&self, i: usize) -> u32 {
        self.src.get(i).map_or(u32::MAX, |&c| c as u32)
    }

    fn emit(&mut self, ty: u32, s: usize, e: usize) {
        self.out.types.push(ty);
        self.out.texts.push(js::from16(&self.src[s..e]));
        self.out.starts.push(s);
        self.p = e;
    }

    /// `word` (lower case) at `q`, ASCII letters in either case.
    fn ci(&self, q: usize, word: &str) -> bool {
        word.bytes().enumerate().all(|(i, k)| {
            let c = self.c(q + i);
            let k = k as u32;
            c == k || ((97..=122).contains(&k) && c != u32::MAX && (c | 32) == k)
        })
    }

    // Matches a lower-case keyword at p, ignoring case.
    fn kw(&self, word: &str) -> bool {
        self.ci(self.p, word)
    }

    fn kwb(&self, word: &str) -> bool {
        self.kw(word) && !is_word(self.c(self.p + word.len()))
    }

    fn lower(&self, q: usize) -> u32 {
        let c = self.c(q);
        if (65..=90).contains(&c) {
            c + 32
        } else {
            c
        }
    }

    fn spaces(&self, mut q: usize) -> usize {
        while is_space(self.c(q)) {
            q += 1;
        }
        q
    }

    fn digits(&self, mut q: usize) -> usize {
        while is_digit(self.c(q)) {
            q += 1;
        }
        q
    }

    fn index_of(&self, ch: u16, from: usize) -> Option<usize> {
        self.src[from.min(self.n)..].iter().position(|&c| c == ch).map(|k| from + k)
    }

    fn next_line_end(&self, q: usize) -> usize {
        let mut k = q;
        while k < self.n && !is_line_end(self.c(k)) {
            k += 1;
        }
        k
    }

    /// `/accTitle\s*:\s*/iy` and its kin: `word`, spaces, `sep`, spaces.
    fn acc(&self, word: &str, sep: u32) -> Option<usize> {
        if !self.ci(self.p, word) {
            return None;
        }
        let q = self.spaces(self.p + word.len());
        (self.c(q) == sep).then(|| self.spaces(q + 1))
    }

    /// `/.*direction\s+XX[^\n]*/iy` at p.
    fn direction_re(&self, dir: &str) -> Option<usize> {
        let p = self.p;
        let line_end = self.next_line_end(p);
        let mut j = line_end;
        while j > p {
            j -= 1;
            if j + 9 > line_end || !self.ci(j, "direction") {
                continue;
            }
            let q = j + 9;
            let e = self.spaces(q);
            if e == q || !self.ci(e, dir) {
                continue;
            }
            let mut k = e + 2;
            while k < self.n && self.c(k) != 10 {
                k += 1;
            }
            return Some(k);
        }
        None
    }

    // A direction statement swallows its whole line, from wherever the lexer stands on it.
    fn direction(&mut self) -> bool {
        let p = self.p;
        if self.dir_next == -1 {
            return false;
        }
        if self.dir_next < p as isize {
            self.dir_next = (p..self.n).find(|&k| self.ci(k, "direction")).map_or(-1, |k| k as isize);
            if self.dir_next == -1 {
                return false;
            }
        }
        if p < self.dir_failed {
            return false;
        }
        if self.line_end < p as isize {
            self.line_end = self.next_line_end(p) as isize;
        }
        if self.dir_next >= self.line_end {
            return false;
        }
        for (i, d) in ["tb", "bt", "rl", "lr"].iter().enumerate() {
            if let Some(e) = self.direction_re(d) {
                self.emit(t::direction_tb + i as u32, p, e);
                return true;
            }
        }
        self.dir_failed = self.line_end as usize;
        false
    }

    fn quoted(&mut self) {
        let p = self.p;
        let Some(e) = self.index_of(34, p + 1) else {
            self.emit(t::CHAR, p, p + 1);
            return;
        };
        let mut name = e > p + 1;
        let mut q = p + 1;
        while name && q < e {
            let c = self.c(q);
            if c == 37 || c == 92 || c == 13 || c == 10 || c == 11 || c == 8 {
                name = false;
            }
            q += 1;
        }
        self.emit(if name { t::ENTITY_NAME } else { t::WORD }, p, e + 1);
    }

    fn number(&mut self, c: u32) {
        let p = self.p;
        let d = self.c(p + 1);
        if d == 43 && (c == 49 || c == 48) {
            self.emit(if c == 49 { t::ONE_OR_MORE } else { t::ZERO_OR_MORE }, p, p + 2);
            return;
        }
        let q = self.digits(p + 1);
        if self.c(q) == 46 && is_digit(self.c(q + 1)) {
            let e = self.digits(q + 2);
            self.emit(t::DECIMAL_NUM, p, e);
            return;
        }
        if c == 49 {
            if is_space(d) {
                let r = self.spaces(p + 2);
                let f = self.c(r);
                if is_word(f) || f == 34 || f == 39 {
                    self.emit(t::ONLY_ONE, p, p + 1);
                    return;
                }
            } else if d == 45 || d == 46 {
                let f = self.c(p + 2);
                if f == 45 || f == 46 {
                    self.emit(t::ONLY_ONE, p, p + 1);
                    return;
                }
            }
            if !is_word(d) {
                self.emit(t::ENTITY_ONE, p, p + 1);
                return;
            }
        }
        self.emit(t::NUM, p, q);
    }

    fn keyword(&mut self, lc: u32) -> bool {
        let p = self.p;
        let d = self.c(p + 1);
        match lc {
            101 => {
                if self.kwb("erdiagram") {
                    self.emit(t::ER_DIAGRAM, p, p + 9);
                } else if self.kwb("end") {
                    let e = self.spaces(p + 3);
                    self.emit(t::END_KW, p, e);
                } else {
                    return false;
                }
            }
            115 => {
                if self.kwb("style") {
                    self.state = S::Style;
                    self.emit(t::STYLE, p, p + 5);
                } else if self.kwb("subgraph") {
                    self.emit(t::SUBGRAPH, p, p + 8);
                } else {
                    return false;
                }
            }
            99 => {
                if self.kwb("classdef") {
                    self.state = S::Style;
                    self.emit(t::CLASSDEF, p, p + 8);
                } else if self.kwb("class") {
                    self.emit(t::CLASS, p, p + 5);
                } else {
                    return false;
                }
            }
            111 => {
                if self.kwb("one or zero") {
                    self.emit(t::ZERO_OR_ONE, p, p + 11);
                } else if self.kwb("one or more") || self.kwb("one or many") {
                    self.emit(t::ONE_OR_MORE, p, p + 11);
                } else if self.kwb("one") {
                    self.emit(t::ONLY_ONE, p, p + 3);
                } else if self.kwb("only one") {
                    self.emit(t::ONLY_ONE, p, p + 8);
                } else if d == 124 {
                    self.emit(t::ZERO_OR_ONE, p, p + 2);
                } else if d == 123 {
                    self.emit(t::ZERO_OR_MORE, p, p + 2);
                } else if self.kwb("optionally to") {
                    self.emit(t::NON_IDENTIFYING, p, p + 13);
                } else {
                    return false;
                }
            }
            122 => {
                if self.kwb("zero or one") {
                    self.emit(t::ZERO_OR_ONE, p, p + 11);
                } else if self.kwb("zero or more") || self.kwb("zero or many") {
                    self.emit(t::ZERO_OR_MORE, p, p + 12);
                } else {
                    return false;
                }
            }
            109 => {
                if self.kw("many(0)") {
                    self.emit(t::ZERO_OR_MORE, p, p + 7);
                } else if self.kw("many(1)") {
                    self.emit(t::ONE_OR_MORE, p, p + 7);
                } else if self.kwb("many") {
                    self.emit(t::ZERO_OR_MORE, p, p + 4);
                } else {
                    return false;
                }
            }
            116 => {
                if !self.kwb("to") {
                    return false;
                }
                self.emit(t::IDENTIFYING, p, p + 2);
            }
            117 => {
                if d != 46 && d != 45 && d != 124 {
                    return false;
                }
                self.emit(t::MD_PARENT, p, p + 1);
            }
            124 => {
                if self.lower(p + 1) == 111 && !is_word(self.c(p + 2)) {
                    self.emit(t::ZERO_OR_ONE, p, p + 2);
                } else if d == 124 {
                    self.emit(t::ONLY_ONE, p, p + 2);
                } else if d == 123 {
                    self.emit(t::ONE_OR_MORE, p, p + 2);
                } else {
                    return false;
                }
            }
            125 => {
                if self.lower(p + 1) == 111 && !is_word(self.c(p + 2)) {
                    self.emit(t::ZERO_OR_MORE, p, p + 2);
                } else if d == 124 {
                    self.emit(t::ONE_OR_MORE, p, p + 2);
                } else {
                    return false;
                }
            }
            46 => {
                if d != 46 && d != 45 {
                    return false;
                }
                self.emit(t::NON_IDENTIFYING, p, p + 2);
            }
            45 => {
                if d == 45 {
                    self.emit(t::IDENTIFYING, p, p + 2);
                } else if d == 46 {
                    self.emit(t::NON_IDENTIFYING, p, p + 2);
                } else {
                    return false;
                }
            }
            _ => return false,
        }
        true
    }

    fn initial(&mut self, c: u32) {
        let p = self.p;
        if (c == 97 || c == 65) && self.kw("acc") {
            if let Some(e) = self.acc("acctitle", 58) {
                self.state = S::AccTitle;
                self.emit(t::acc_title, p, e);
                return;
            }
            if let Some(e) = self.acc("accdescr", 58) {
                self.state = S::AccDescr;
                self.emit(t::acc_descr, p, e);
                return;
            }
            if let Some(e) = self.acc("accdescr", 123) {
                self.state = S::AccDescrMultiline;
                self.p = e;
                return;
            }
        }
        if self.direction() {
            return;
        }
        match c {
            32 | 9 | 13 => {
                let mut e = p + 1;
                while matches!(self.c(e), 32 | 9 | 13) {
                    e += 1;
                }
                self.p = e;
                return;
            }
            10 => {
                let mut e = p + 1;
                while self.c(e) == 10 {
                    e += 1;
                }
                self.emit(t::NEWLINE, p, e);
                return;
            }
            34 => return self.quoted(),
            123 => {
                self.state = S::Block;
                return self.emit(t::BLOCK_START, p, p + 1);
            }
            35 => return self.emit(t::BRKT, p, p + 1),
            44 => return self.emit(t::COMMA, p, p + 1),
            58 => {
                if self.c(p + 1) == 58 && self.c(p + 2) == 58 {
                    return self.emit(t::STYLE_SEPARATOR, p, p + 3);
                }
                return self.emit(t::COLON, p, p + 1);
            }
            91 => return self.emit(t::SQS, p, p + 1),
            93 => return self.emit(t::SQE, p, p + 1),
            _ => {}
        }
        if is_digit(c) {
            return self.number(c);
        }
        if self.keyword(if (65..=90).contains(&c) { c + 32 } else { c }) {
            return;
        }
        if text_char(c, TEXT) {
            let mut e = p + 1;
            while text_char(self.c(e), TEXT) {
                e += 1;
            }
            return self.emit(t::UNICODE_TEXT, p, e);
        }
        self.emit(t::CHAR, p, p + 1);
    }

    // Mermaid's generic type rule: a run of non-space characters holding a `~`, through the last `~` on the line.
    fn generic(&mut self) -> bool {
        let p = self.p;
        if self.tilde_next == -1 {
            return false;
        }
        if self.tilde_next < p as isize {
            self.tilde_next = self.index_of(126, p).map_or(-1, |k| k as isize);
            if self.tilde_next == -1 {
                return false;
            }
        }
        if self.ws_next < p as isize {
            let mut q = p;
            while q < self.n && !is_space(self.c(q)) {
                q += 1;
            }
            self.ws_next = q as isize;
        }
        if self.tilde_next >= self.ws_next {
            return false;
        }
        if self.tilde_line < p as isize {
            self.tilde_line = self.next_line_end(p) as isize;
            // lastIndexOf('~', tildeLine - 1), the position clamped to the text.
            let from = (self.tilde_line - 1).clamp(0, self.n as isize - 1);
            self.tilde_last = (0..=from as usize).rev().find(|&k| self.src[k] == 126).map_or(-1, |k| k as isize);
        }
        if self.tilde_last <= self.tilde_next {
            return false;
        }
        let mut e = self.tilde_last as usize + 1;
        while e < self.n && !is_space(self.c(e)) {
            e += 1;
        }
        self.emit(t::ATTRIBUTE_WORD, p, e);
        true
    }

    fn block(&mut self, c: u32) {
        let p = self.p;
        if is_space(c) {
            self.p = self.spaces(p + 1);
            return;
        }
        let lc = c | 32;
        if (lc == 112 || lc == 102 || lc == 117) && self.c(p + 1) != u32::MAX && (self.c(p + 1) | 32) == 107 && !is_word(self.c(p + 2)) {
            return self.emit(t::ATTRIBUTE_KEY, p, p + 2);
        }
        if self.generic() {
            return;
        }
        if is_attr(c, ATTR_START) {
            let mut e = p + 1;
            while e < self.n && is_attr(self.c(e), ATTR_REST) {
                e += 1;
            }
            return self.emit(t::ATTRIBUTE_WORD, p, e);
        }
        if c == 96 {
            self.state = S::BlockBq;
            self.p += 1;
            return;
        }
        if c == 34 {
            if let Some(e) = self.index_of(34, p + 1) {
                return self.emit(t::COMMENT, p, e + 1);
            }
        } else if c == 125 {
            self.state = S::Initial;
            return self.emit(t::BLOCK_STOP, p, p + 1);
        }
        self.emit(t::CHAR, p, p + 1);
    }
}

pub fn tokenize(src: &[u16]) -> Tokens {
    let n = src.len();
    let mut lx = Lx { src, n, out: Tokens::default(), state: S::Initial, p: 0, dir_next: -2, dir_failed: 0, line_end: -1, tilde_next: -2, tilde_line: -1, tilde_last: -1, ws_next: -1 };
    while lx.p < n {
        let p = lx.p;
        let c = lx.c(p);
        match lx.state {
            S::Initial => lx.initial(c),
            S::Block => lx.block(c),
            S::BlockBq => {
                if c == 96 {
                    lx.state = S::Block;
                    lx.p += 1;
                } else {
                    let e = lx.index_of(96, p).unwrap_or(n);
                    lx.emit(t::ATTRIBUTE_WORD, p, e);
                }
            }
            S::AccTitle | S::AccDescr => {
                let e = lx.index_of(10, p).unwrap_or(n);
                let ty = if lx.state == S::AccTitle { t::acc_title_value } else { t::acc_descr_value };
                lx.emit(ty, p, e);
                lx.state = S::Initial;
            }
            S::AccDescrMultiline => {
                if c == 125 {
                    lx.state = S::Initial;
                    lx.p += 1;
                } else {
                    let e = lx.index_of(125, p).unwrap_or(n);
                    lx.emit(t::acc_descr_multiline_value, p, e);
                }
            }
            S::Style => {
                if c == 10 {
                    let mut e = p + 1;
                    while lx.c(e) == 10 {
                        e += 1;
                    }
                    lx.state = S::Initial;
                    lx.emit(t::NEWLINE, p, e);
                } else if is_space(c) {
                    lx.p = lx.spaces(p + 1);
                } else if c == 58 {
                    lx.emit(t::COLON, p, p + 1);
                } else if c == 44 {
                    lx.emit(t::COMMA, p, p + 1);
                } else if c == 35 {
                    lx.emit(t::BRKT, p, p + 1);
                } else if c == 59 {
                    lx.emit(t::SEMI, p, p + 1);
                } else if text_char(c, STYLE_TEXT) {
                    let mut e = p + 1;
                    while text_char(lx.c(e), STYLE_TEXT) {
                        e += 1;
                    }
                    lx.emit(t::STYLE_TEXT, p, e);
                } else {
                    lx.out.types.push(t::ERROR);
                    lx.out.texts.push(String::new());
                    lx.out.starts.push(p);
                    return lx.out;
                }
            }
        }
    }

    // At the end of input Jison's INITIAL state yields EOF, the title and description states yield
    // an empty value, and the block and style states yield nothing.
    let last = match lx.state {
        S::Initial => Some(t::EOF),
        S::AccTitle => Some(t::acc_title_value),
        S::AccDescr => Some(t::acc_descr_value),
        S::AccDescrMultiline => Some(t::acc_descr_multiline_value),
        _ => None,
    };
    let mut out = lx.out;
    if let Some(last) = last {
        out.types.push(last);
        out.texts.push(String::new());
        out.starts.push(n);
    }
    out.types.push(t::END);
    out.texts.push(String::new());
    out.starts.push(n);
    out
}
