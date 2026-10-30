// Port of src/diagrams/class/lexer.ts.

use super::tokens as t;
use crate::js;
use crate::util::chars::{is_line_end, is_space, is_word};
use crate::util::unicode::is_unicode_letter;

#[derive(Default, Debug)]
pub struct Tokens {
    pub types: Vec<u32>,
    pub texts: Vec<String>,
    pub starts: Vec<usize>,
}

#[derive(Clone, Copy, PartialEq)]
enum S {
    Initial,
    String,
    Bqstring,
    Generic,
    CallbackName,
    CallbackArgs,
    AccTitle,
    AccDescr,
    AccDescrMultiline,
    Cls,
    ClassBody,
    Namespace,
    NamespaceBody,
}

// The token each state gives at the end of input, before the parser sees the end itself.
fn at_end(s: S) -> u32 {
    match s {
        S::Initial => t::EOF,
        S::String => t::STR,
        S::Bqstring => t::EOF,
        S::Generic => t::GENERICTYPE,
        S::CallbackName => t::CALLBACK_NAME,
        S::CallbackArgs => t::CALLBACK_ARGS,
        S::AccTitle => t::acc_title_value,
        S::AccDescr => t::acc_descr_value,
        S::AccDescrMultiline => t::acc_descr_multiline_value,
        S::Cls => t::EOF,
        S::ClassBody => t::EOF_IN_STRUCT,
        S::Namespace => t::EOF,
        S::NamespaceBody => t::EOF_IN_STRUCT,
    }
}

struct Lx<'a> {
    src: &'a [u16],
    n: usize,
    out: Tokens,
    stack: Vec<S>,
    state: S,
    p: usize,
    dir_next: isize,
    line_end: isize,
    dir_failed: usize,
}

impl Lx<'_> {
    fn c(&self, i: usize) -> u32 {
        self.src.get(i).map_or(u32::MAX, |&c| c as u32)
    }

    fn emit(&mut self, ty: u32, s: usize, e: usize) -> bool {
        self.out.types.push(ty);
        self.out.texts.push(js::from16(&self.src[s..e]));
        self.out.starts.push(s);
        self.p = e;
        true
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

    fn starts_with(&self, word: &str, at: usize) -> bool {
        word.bytes().enumerate().all(|(i, b)| self.c(at + i) == b as u32)
    }

    fn kw(&self, word: &str) -> bool {
        self.starts_with(word, self.p) && !is_word(self.c(self.p + word.len()))
    }

    fn skip_ws(&self, mut q: usize) -> usize {
        while q < self.n && is_space(self.c(q)) {
            q += 1;
        }
        q
    }

    fn index_of(&self, ch: u16, from: usize) -> Option<usize> {
        self.src[from.min(self.n)..].iter().position(|&c| c == ch).map(|k| from + k)
    }

    fn until(&self, ch: u16) -> usize {
        self.index_of(ch, self.p).unwrap_or(self.n)
    }

    // A run of whitespace is a NEWLINE up to its last line feed; the rest is skipped.
    fn space(&mut self, leaves: bool) {
        let mut q = self.p;
        let mut k: isize = -1;
        while q < self.n && is_space(self.c(q)) {
            if self.c(q) == 10 {
                k = q as isize;
            }
            q += 1;
        }
        if k < 0 {
            self.p = q;
            return;
        }
        if leaves {
            self.pop();
        }
        self.emit(t::NEWLINE, self.p, k as usize + 1);
    }

    // The keywords that every state recognizes, even inside generics and backtick names.
    fn keyword(&mut self, c: u32) -> bool {
        let p = self.p;
        match c {
            99 => {
                if self.kw("cssClass") {
                    return self.emit(t::CSSCLASS, p, p + 8);
                }
                if self.kw("callback") {
                    return self.emit(t::CALLBACK, p, p + 8);
                }
                if self.kw("click") {
                    return self.emit(t::CLICK, p, p + 5);
                }
            }
            108 => {
                if self.kw("link") {
                    return self.emit(t::LINK, p, p + 4);
                }
            }
            110 => {
                if self.kw("note for") {
                    return self.emit(t::NOTE_FOR, p, p + 8);
                }
                if self.kw("note") {
                    return self.emit(t::NOTE, p, p + 4);
                }
            }
            60 => {
                if self.c(p + 1) == 60 {
                    return self.emit(t::ANNOTATION_START, p, p + 2);
                }
            }
            62 => {
                if self.c(p + 1) == 62 {
                    return self.emit(t::ANNOTATION_END, p, p + 2);
                }
            }
            104 => {
                if self.kw("href") {
                    return self.emit(t::HREF, p, p + 4);
                }
            }
            _ => {}
        }
        false
    }

    fn common(&mut self, c: u32) -> bool {
        if self.keyword(c) {
            return true;
        }
        let p = self.p;
        let d = self.c(p + 1);
        match c {
            34 => {
                self.push(S::String);
                self.p += 1;
                return true;
            }
            126 => {
                self.push(S::Generic);
                self.p += 1;
                return true;
            }
            96 => {
                self.push(S::Bqstring);
                self.p += 1;
                return true;
            }
            95 => {
                for (w, len) in [("_self", 5), ("_blank", 6), ("_parent", 7), ("_top", 4)] {
                    if self.kw(w) {
                        return self.emit(t::LINK_TARGET, p, p + len);
                    }
                }
            }
            60 => return if d == 124 { self.emit(t::EXTENSION, p, p + 2) } else { self.emit(t::DEPENDENCY, p, p + 1) },
            124 => return d == 62 && self.emit(t::EXTENSION, p, p + 2),
            62 => return self.emit(t::DEPENDENCY, p, p + 1),
            42 => return self.emit(t::COMPOSITION, p, p + 1),
            111 => {
                if !is_word(d) {
                    return self.emit(t::AGGREGATION, p, p + 1);
                }
            }
            40 => return d == 41 && self.emit(t::LOLLIPOP, p, p + 2),
            45 => return if d == 45 { self.emit(t::LINE, p, p + 2) } else { self.emit(t::MINUS, p, p + 1) },
            46 => return if d == 46 { self.emit(t::DOTTED_LINE, p, p + 2) } else { self.emit(t::DOT, p, p + 1) },
            58 => {
                let mut q = p + 1;
                while q < self.n {
                    let ch = self.c(q);
                    if ch == 58 || ch == 10 || ch == 59 {
                        break;
                    }
                    q += 1;
                }
                if q > p + 1 {
                    return self.emit(t::LABEL, p, q);
                }
                return d == 58 && self.c(p + 2) == 58 && self.emit(t::STYLE_SEPARATOR, p, p + 3);
            }
            43 => return self.emit(t::PLUS, p, p + 1),
            37 => return self.emit(t::PCT, p, p + 1),
            61 => return self.emit(t::EQUALS, p, p + 1),
            91 => return self.emit(t::SQS, p, p + 1),
            93 => return self.emit(t::SQE, p, p + 1),
            33 | 35 | 36 | 38 | 39 | 44 | 63 | 92 | 47 => return self.emit(t::PUNCTUATION, p, p + 1),
            _ => {}
        }
        if is_word(c) {
            let mut q = p + 1;
            while is_word(self.c(q)) {
                q += 1;
            }
            return self.emit(t::ALPHA, p, q);
        }
        c != u32::MAX && is_unicode_letter(c) && self.emit(t::UNICODE_TEXT, p, p + 1)
    }

    /// `/.*direction\s+XX[^\n]*/y` at p.
    fn direction_re(&self, dir: &str) -> Option<usize> {
        let p = self.p;
        let mut line_end = p;
        while line_end < self.n && !is_line_end(self.c(line_end)) {
            line_end += 1;
        }
        let mut j = line_end;
        while j > p {
            j -= 1;
            if j + 9 > line_end || !self.starts_with("direction", j) {
                continue;
            }
            let q = j + 9;
            let e = self.skip_ws(q);
            if e == q || !self.starts_with(dir, e) {
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

    fn find_direction(&self, from: usize) -> isize {
        (from..self.n).find(|&k| self.starts_with("direction", k)).map_or(-1, |k| k as isize)
    }

    fn direction(&mut self) -> bool {
        let p = self.p;
        if self.dir_next == -1 || p < self.dir_failed {
            return false;
        }
        if self.dir_next < p as isize {
            self.dir_next = self.find_direction(p);
        }
        if self.dir_next == -1 {
            return false;
        }
        if self.line_end < p as isize {
            let mut k = p;
            while k < self.n && !is_line_end(self.c(k)) {
                k += 1;
            }
            self.line_end = k as isize;
        }
        if self.dir_next >= self.line_end {
            return false;
        }
        for (i, d) in ["TB", "BT", "RL", "LR"].iter().enumerate() {
            if let Some(e) = self.direction_re(d) {
                return self.emit(t::direction_tb + i as u32, p, e);
            }
        }
        // No direction statement starts anywhere in the rest of this line either.
        self.dir_failed = self.line_end as usize;
        false
    }

    fn initial(&mut self, c: u32) -> bool {
        if self.direction() {
            return true;
        }
        if is_space(c) {
            self.space(false);
            return true;
        }
        let p = self.p;
        match c {
            37 => {
                if self.c(p + 1) == 37 {
                    let mut e = self.until(10);
                    while e < self.n && (self.c(e) == 10 || self.c(e) == 13) {
                        e += 1;
                    }
                    self.p = e;
                    return true;
                }
            }
            97 => {
                let title = self.starts_with("accTitle", p);
                if title || self.starts_with("accDescr", p) {
                    let q = self.skip_ws(p + 8);
                    let d = self.c(q);
                    if d == 58 {
                        let e = self.skip_ws(q + 1);
                        self.emit(if title { t::acc_title } else { t::acc_descr }, p, e);
                        self.push(if title { S::AccTitle } else { S::AccDescr });
                        return true;
                    }
                    if d == 123 && !title {
                        self.p = self.skip_ws(q + 1);
                        self.push(S::AccDescrMultiline);
                        return true;
                    }
                }
            }
            99 => {
                if self.kw("classDiagram-v2") {
                    return self.emit(t::CLASS_DIAGRAM, p, p + 15);
                }
                if self.kw("classDiagram") {
                    return self.emit(t::CLASS_DIAGRAM, p, p + 12);
                }
                if self.starts_with("call", p) && is_space(self.c(p + 4)) {
                    self.p = self.skip_ws(p + 4);
                    self.push(S::CallbackName);
                    return true;
                }
                if self.kw("classDef") {
                    return self.emit(t::CLASSDEF, p, p + 8);
                }
                if self.kw("class") {
                    self.push(S::Cls);
                    return self.emit(t::CLASS, p, p + 5);
                }
            }
            91 => {
                if self.starts_with("*]", p + 1) {
                    return self.emit(t::EDGE_STATE, p, p + 3);
                }
            }
            115 => {
                if self.kw("style") {
                    return self.emit(t::STYLE, p, p + 5);
                }
            }
            110 => {
                if self.kw("namespace") {
                    self.push(S::Namespace);
                    return self.emit(t::NAMESPACE, p, p + 9);
                }
            }
            44 => return self.emit(t::COMMA, p, p + 1),
            35 => return self.emit(t::BRKT, p, p + 1),
            _ => {}
        }
        self.common(c) || (c == 58 && self.emit(t::COLON, self.p, self.p + 1))
    }
}

pub fn tokenize(src: &[u16]) -> Tokens {
    let n = src.len();
    let mut lx = Lx { src, n, out: Tokens::default(), stack: Vec::new(), state: S::Initial, p: 0, dir_next: 0, line_end: -1, dir_failed: 0 };
    lx.dir_next = lx.find_direction(0);
    loop {
        if lx.p >= n {
            let ty = at_end(lx.state);
            lx.out.types.push(ty);
            lx.out.texts.push(String::new());
            lx.out.starts.push(n);
            break;
        }
        let p = lx.p;
        let c = lx.c(p);
        let mut ok = true;
        match lx.state {
            S::Initial => ok = lx.initial(c),
            S::Cls => {
                if is_space(c) {
                    lx.space(true);
                } else if c == 125 {
                    lx.pop();
                    lx.pop();
                    lx.emit(t::STRUCT_STOP, p, p + 1);
                } else if c == 123 {
                    lx.push(S::ClassBody);
                    lx.emit(t::STRUCT_START, p, p + 1);
                } else {
                    ok = lx.common(c);
                }
            }
            S::ClassBody => {
                if c == 34 {
                    lx.push(S::String);
                    lx.p += 1;
                } else if c == 125 {
                    lx.pop();
                    lx.emit(t::STRUCT_STOP, p, p + 1);
                } else if c == 91 && lx.starts_with("*]", p + 1) {
                    lx.emit(t::EDGE_STATE, p, p + 3);
                } else if c == 123 {
                    lx.emit(t::OPEN_IN_STRUCT, p, p + 1);
                } else if c == 10 {
                    lx.p += 1;
                } else {
                    let mut q = p + 1;
                    while q < n {
                        let ch = lx.c(q);
                        if ch == 123 || ch == 125 || ch == 10 {
                            break;
                        }
                        q += 1;
                    }
                    lx.emit(t::MEMBER, p, q);
                }
            }
            S::Namespace => {
                if is_space(c) {
                    lx.space(true);
                } else if c == 123 {
                    lx.push(S::NamespaceBody);
                    lx.emit(t::STRUCT_START, p, p + 1);
                } else if c == 125 {
                    // The brace is read again in the state the namespace was opened from.
                    lx.pop();
                } else if c == 110 && lx.kw("namespace") {
                    lx.push(S::Namespace);
                    lx.emit(t::NAMESPACE, p, p + 9);
                } else {
                    ok = lx.common(c);
                }
            }
            S::NamespaceBody => {
                if is_space(c) {
                    lx.space(false);
                } else if c == 125 {
                    lx.pop();
                    lx.emit(t::STRUCT_STOP, p, p + 1);
                } else if c == 110 && lx.kw("namespace") {
                    lx.push(S::Namespace);
                    lx.emit(t::NAMESPACE, p, p + 9);
                } else if c == 91 && lx.starts_with("*]", p + 1) {
                    lx.emit(t::EDGE_STATE, p, p + 3);
                } else if c == 99 && lx.kw("class") {
                    lx.push(S::Cls);
                    lx.emit(t::CLASS, p, p + 5);
                } else {
                    ok = lx.common(c);
                }
            }
            S::String => {
                if c == 34 {
                    lx.pop();
                    lx.p += 1;
                } else {
                    let e = lx.until(34);
                    lx.emit(t::STR, p, e);
                }
            }
            S::Generic => {
                if c == 34 {
                    lx.push(S::String);
                    lx.p += 1;
                } else if lx.keyword(c) {
                } else if c == 126 {
                    lx.pop();
                    lx.p += 1;
                } else {
                    let e = lx.until(126);
                    lx.emit(t::GENERICTYPE, p, e);
                }
            }
            S::Bqstring => {
                if c == 34 {
                    lx.push(S::String);
                    lx.p += 1;
                } else if lx.keyword(c) {
                } else if c == 126 {
                    lx.push(S::Generic);
                    lx.p += 1;
                } else if c == 96 {
                    lx.pop();
                    lx.p += 1;
                } else {
                    let e = lx.until(96);
                    lx.emit(t::BQUOTE_STR, p, e);
                }
            }
            S::CallbackName => {
                if c == 40 {
                    let q = lx.skip_ws(p + 1);
                    lx.pop();
                    if lx.c(q) == 41 {
                        lx.p = q + 1;
                    } else {
                        lx.push(S::CallbackArgs);
                        lx.p += 1;
                    }
                } else {
                    let e = lx.until(40);
                    lx.emit(t::CALLBACK_NAME, p, e);
                }
            }
            S::CallbackArgs => {
                if c == 41 {
                    lx.pop();
                    lx.p += 1;
                } else {
                    let e = lx.until(41);
                    lx.emit(t::CALLBACK_ARGS, p, e);
                }
            }
            S::AccTitle | S::AccDescr => {
                let ty = if lx.state == S::AccTitle { t::acc_title_value } else { t::acc_descr_value };
                let e = lx.until(10);
                lx.emit(ty, p, e);
                lx.pop();
            }
            S::AccDescrMultiline => {
                if c == 125 {
                    lx.pop();
                    lx.p += 1;
                } else {
                    let e = lx.until(125);
                    lx.emit(t::acc_descr_multiline_value, p, e);
                }
            }
        }
        if !ok {
            lx.out.types.push(t::ERROR);
            lx.out.texts.push(String::new());
            lx.out.starts.push(lx.p);
            return lx.out;
        }
    }
    let mut out = lx.out;
    out.types.push(t::END);
    out.texts.push(String::new());
    out.starts.push(n);
    out
}
