// Port of src/diagrams/agentflow/lexer.ts. Reads UTF-16 code units, as upstream does, and keeps
// Jison's line and column for every token. The sticky regexes it shares with the flowchart lexer
// are that lexer's hand matchers.

use super::tokens as t;
use crate::diagrams::flowchart::lexer::{is_line_end, node_char, Lx};
use crate::util::chars::{is_space, is_word};
use crate::util::unicode::is_unicode_letter;

pub struct Tokens {
    pub types: Vec<u8>,
    pub texts: Vec<Vec<u16>>,
    pub starts: Vec<usize>,
    // Four numbers per token, counted the way Jison counts them:
    // first line, first column, last line, last column. Lines start at 1, columns at 0.
    pub locs: Vec<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum S {
    Initial,
    String,
    MdString,
    AccTitle,
    AccDescr,
    AccDescrMultiline,
    Dir,
    Text,
    EllipseText,
    TrapText,
    EdgeText,
    Click,
    Callbackname,
    Callbackargs,
    ShapeData,
    ShapeDataStr,
}

struct L<'a> {
    lx: Lx<'a>,
    src: &'a [u16],
    n: usize,
    tk: Tokens,
    stack: Vec<S>,
    state: S,
    p: usize,
    line: usize,
    col: usize,
}

impl L<'_> {
    fn c(&self, i: usize) -> u32 {
        self.lx.c(i)
    }

    // Moves over one match. Jison counts \r\n, \r and \n as one line break each, and after a
    // break its column stops at the first U+2028 or U+2029.
    fn advance(&mut self, e: usize) {
        let mut brk: Option<usize> = None;
        let mut k = self.p;
        while k < e {
            let ch = self.c(k);
            if ch == 10 {
                self.line += 1;
                brk = Some(k);
            } else if ch == 13 {
                if k + 1 < e && self.c(k + 1) == 10 {
                    k += 1;
                }
                self.line += 1;
                brk = Some(k);
            }
            k += 1;
        }
        match brk {
            None => self.col += e - self.p,
            Some(b) => {
                let mut k = b + 1;
                while k < e {
                    let ch = self.c(k);
                    if ch == 0x2028 || ch == 0x2029 {
                        break;
                    }
                    k += 1;
                }
                self.col = k - b - 1;
            }
        }
        self.p = e;
    }

    fn start(&mut self, ty: u8, text: Vec<u16>) {
        self.tk.types.push(ty);
        self.tk.texts.push(text);
        self.tk.starts.push(self.p);
        self.tk.locs.push(self.line);
        self.tk.locs.push(self.col);
    }

    fn finish(&mut self) {
        self.tk.locs.push(self.line);
        self.tk.locs.push(self.col);
    }

    fn emit(&mut self, ty: u8, e: usize) {
        self.start(ty, self.src[self.p..e].to_vec());
        self.advance(e);
        self.finish();
    }

    // For a token that cannot hold a line break.
    fn emit_flat(&mut self, ty: u8, e: usize) {
        self.start(ty, self.src[self.p..e].to_vec());
        self.col += e - self.p;
        self.p = e;
        self.finish();
    }

    // For a run that Jison matches one character at a time, where \r\n counts as two line breaks.
    fn emit_chars(&mut self, ty: u8, e: usize) {
        self.start(ty, self.src[self.p..e].to_vec());
        for k in self.p..e {
            let ch = self.c(k);
            if ch == 10 || ch == 13 {
                self.line += 1;
                self.col = 0;
            } else {
                self.col += 1;
            }
        }
        self.p = e;
        self.finish();
    }

    fn emit_text(&mut self, ty: u8, text: Vec<u16>, e: usize) {
        self.start(ty, text);
        self.advance(e);
        self.finish();
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

    fn kw(&self, word: &str) -> bool {
        self.lx.starts_with(word, self.p) && !is_word(self.c(self.p + word.len()))
    }

    fn blanks(&self, from: usize) -> usize {
        let mut e = from;
        while e < self.n && (self.c(e) == 32 || self.c(e) == 9) {
            e += 1;
        }
        e
    }

    // End of `\s*--+[x>]\s*` starting at q.
    fn arrow_end(&self, q: usize) -> Option<usize> {
        let j = self.lx.ws(q);
        if self.c(j) != 45 || self.c(j + 1) != 45 {
            return None;
        }
        let mut k = j + 2;
        while self.c(k) == 45 {
            k += 1;
        }
        let t = self.c(k);
        if t == 120 || t == 62 {
            Some(self.lx.ws(k + 1))
        } else {
            None
        }
    }

    fn link(&mut self) -> bool {
        let j = self.lx.ws(self.p);
        if self.c(j) != 45 {
            return false;
        }
        let d = self.c(j + 1);
        let mut k = j + 2;
        if d == 45 {
            while self.c(k) == 45 {
                k += 1;
            }
            let t = self.c(k);
            if t == 120 || t == 62 {
                let e = self.lx.ws(k + 1);
                self.emit(t::LINK, e);
            } else {
                self.push(S::EdgeText);
                let e = self.lx.ws(j + 2);
                self.emit(t::START_LINK, e);
            }
            return true;
        }
        if d == 46 {
            while self.c(k) == 46 {
                k += 1;
            }
            if self.c(k) == 45 {
                let e = self.lx.ws(k + 1);
                self.emit(t::LINK, e);
                return true;
            }
        }
        false
    }

    // `%%` not followed by `{`, through the end of the line.
    fn comment_end(&self, q: usize) -> Option<usize> {
        if self.c(q) != 37 || self.c(q + 1) != 37 || self.c(q + 2) == 123 {
            return None;
        }
        Some(self.lx.index_of(10, q + 2).unwrap_or(self.n))
    }

    fn quote(&mut self) {
        if self.c(self.p + 1) == 96 {
            self.push(S::MdString);
            self.advance(self.p + 2);
        } else {
            self.push(S::String);
            self.advance(self.p + 1);
        }
    }

    fn opener(&mut self, c: u32) -> bool {
        let p = self.p;
        let d = self.c(p + 1);
        if c == 40 {
            if d == 45 {
                self.push(S::EllipseText);
                self.emit_flat(t::ELLIPSE_START, p + 2);
            } else if d == 91 {
                self.push(S::Text);
                self.emit_flat(t::STADIUMSTART, p + 2);
            } else if d == 40 && self.c(p + 2) == 40 {
                self.push(S::Text);
                self.emit_flat(t::DOUBLECIRCLESTART, p + 3);
            } else {
                self.push(S::Text);
                self.emit_flat(t::PS, p + 1);
            }
            return true;
        }
        if c == 91 {
            if d == 91 {
                self.push(S::Text);
                self.emit_flat(t::SUBROUTINESTART, p + 2);
            } else if d == 40 {
                self.push(S::Text);
                self.emit_flat(t::CYLINDERSTART, p + 2);
            } else if d == 47 {
                self.push(S::TrapText);
                self.emit_flat(t::TRAPSTART, p + 2);
            } else if d == 92 {
                self.push(S::TrapText);
                self.emit_flat(t::INVTRAPSTART, p + 2);
            } else {
                self.push(S::Text);
                self.emit_flat(t::SQS, p + 1);
            }
            return true;
        }
        if c == 124 {
            self.push(S::Text);
            self.emit_flat(t::PIPE, p + 1);
            return true;
        }
        if c == 123 {
            self.push(S::Text);
            self.emit_flat(t::DIAMOND_START, p + 1);
            return true;
        }
        false
    }

    fn node_string(&mut self) -> bool {
        let n = self.n;
        let mut q = self.p;
        while q < n {
            let ch = self.c(q);
            if node_char(ch) {
                q += 1;
            } else if ch == 45 && q + 1 < n {
                let d = self.c(q + 1);
                if d == 62 || d == 45 || d == 46 {
                    break;
                }
                q += 1;
            } else {
                break;
            }
        }
        if q == self.p {
            return false;
        }
        self.emit_flat(t::NODE_STRING, q);
        true
    }
}

pub fn tokenize(src: &[u16]) -> Tokens {
    let n = src.len();
    let mut l = L { lx: Lx { src, n }, src, n, tk: Tokens { types: Vec::new(), texts: Vec::new(), starts: Vec::new(), locs: Vec::new() }, stack: Vec::new(), state: S::Initial, p: 0, line: 1, col: 0 };
    let find = |word: &str, from: usize| -> Option<usize> {
        let w: Vec<u16> = word.encode_utf16().collect();
        if from >= n || w.len() > n {
            return None;
        }
        (from..=n - w.len()).find(|&i| src[i..i + w.len()] == w[..])
    };
    let mut first_graph = true;
    let mut dir_next = find("direction", 0);
    let mut at_next = l.lx.index_of(64, 0);
    let mut line_end: isize = -1;
    let mut dir_failed: usize = 0;
    let mut run_end: usize = 0;
    let mut run_at: isize = -1;

    loop {
        if l.p >= n {
            let ty = match l.state {
                S::Initial => Some(t::EOF),
                S::AccTitle => Some(t::acc_title_value),
                S::AccDescr => Some(t::acc_descr_value),
                S::AccDescrMultiline => Some(t::acc_descr_multiline_value),
                S::Click => Some(t::CLICK),
                S::Callbackname => Some(t::CALLBACKNAME),
                S::Callbackargs => Some(t::CALLBACKARGS),
                _ => None,
            };
            if let Some(ty) = ty {
                l.emit_flat(ty, n);
            }
            break;
        }

        let p = l.p;
        let c = l.c(p);
        let mut ok = true;

        match l.state {
            S::Initial => {
                ok = 'initial: {
                    match c {
                        97 => {
                            if l.lx.starts_with("acc", p) {
                                if let Some(e) = l.lx.word_sep(p, "accTitle", 58) {
                                    l.push(S::AccTitle);
                                    l.emit(t::acc_title, e);
                                    break 'initial true;
                                }
                                if let Some(e) = l.lx.word_sep(p, "accDescr", 58) {
                                    l.push(S::AccDescr);
                                    l.emit(t::acc_descr, e);
                                    break 'initial true;
                                }
                                if let Some(e) = l.lx.word_sep(p, "accDescr", 123) {
                                    l.push(S::AccDescrMultiline);
                                    l.advance(e);
                                    break 'initial true;
                                }
                            }
                            if l.kw("agentflow-beta") {
                                if first_graph {
                                    first_graph = false;
                                    l.push(S::Dir);
                                }
                                l.emit_flat(t::GRAPH, p + 14);
                                break 'initial true;
                            }
                        }
                        64 => {
                            if l.c(p + 1) == 123 {
                                l.push(S::ShapeData);
                                l.emit_text(t::SHAPE_DATA, Vec::new(), p + 2);
                                break 'initial true;
                            }
                        }
                        99 => {
                            if let Some(e) = l.lx.word_space(p, "call") {
                                l.push(S::Callbackname);
                                l.advance(e);
                                break 'initial true;
                            }
                            if l.kw("classDef") {
                                l.emit_flat(t::CLASSDEF, p + 8);
                                break 'initial true;
                            }
                            if l.kw("class") {
                                l.emit_flat(t::CLASS, p + 5);
                                break 'initial true;
                            }
                            if let Some(e) = l.lx.word_space(p, "click") {
                                l.push(S::Click);
                                l.advance(e);
                                break 'initial true;
                            }
                            if l.kw("connector") {
                                l.emit_flat(t::connector, p + 9);
                                break 'initial true;
                            }
                        }
                        34 => {
                            l.quote();
                            break 'initial true;
                        }
                        115 => {
                            if l.kw("style") {
                                l.emit_flat(t::STYLE, p + 5);
                                break 'initial true;
                            }
                        }
                        100 => {
                            if l.kw("default") {
                                l.emit_flat(t::DEFAULT, p + 7);
                                break 'initial true;
                            }
                        }
                        108 => {
                            if l.kw("linkStyle") {
                                l.emit_flat(t::LINKSTYLE, p + 9);
                                break 'initial true;
                            }
                        }
                        105 => {
                            if l.kw("interpolate") {
                                l.emit_flat(t::INTERPOLATE, p + 11);
                                break 'initial true;
                            }
                        }
                        104 => {
                            if l.lx.starts_with("href", p) && is_space(l.c(p + 4)) {
                                l.emit(t::HREF, p + 5);
                                break 'initial true;
                            }
                        }
                        102 => {
                            if l.kw("flow") {
                                l.emit_flat(t::flow, p + 4);
                                break 'initial true;
                            }
                        }
                        103 => {
                            if l.kw("global") {
                                let e = l.blanks(p + 6);
                                l.emit_flat(t::global, e);
                                break 'initial true;
                            }
                        }
                        101 => {
                            if l.kw("end") {
                                let e = l.blanks(p + 3);
                                l.emit_flat(t::end, e);
                                break 'initial true;
                            }
                        }
                        95 => {
                            for (word, len) in [("_self", 5), ("_blank", 6), ("_parent", 7), ("_top", 4)] {
                                if l.kw(word) {
                                    l.emit_flat(t::LINK_TARGET, p + len);
                                    break 'initial true;
                                }
                            }
                        }
                        _ => {}
                    }

                    if dir_next.is_some() && p >= dir_failed {
                        if dir_next.is_some_and(|d| d < p) {
                            dir_next = find("direction", p);
                        }
                        if let Some(dn) = dir_next {
                            if line_end < p as isize {
                                let mut k = p;
                                while k < n && !is_line_end(l.c(k)) {
                                    k += 1;
                                }
                                line_end = k as isize;
                            }
                            if (dn as isize) < line_end {
                                for (i, d) in ["TB", "BT", "RL", "LR", "TD"].iter().enumerate() {
                                    if let Some(e) = l.lx.direction(p, d) {
                                        l.emit(t::direction_tb + i as u8, e);
                                        break 'initial true;
                                    }
                                }
                                // No direction statement starts anywhere in the rest of this line either.
                                dir_failed = line_end as usize;
                            }
                        }
                    }

                    // A link id is a run of non-space characters up to its last `@` that is not followed by `{` or `"`.
                    if at_next.is_some() && c != 34 && !is_space(c) {
                        if p >= run_end {
                            if at_next.is_some_and(|a| a < p) {
                                at_next = l.lx.index_of(64, p);
                            }
                            let mut q = p;
                            while q < n {
                                let ch = l.c(q);
                                if ch == 34 || is_space(ch) {
                                    break;
                                }
                                q += 1;
                            }
                            run_end = q;
                            run_at = -1;
                            if let Some(an) = at_next {
                                if an < q {
                                    let mut k = q.min(n - 1) as isize - 1;
                                    while k >= an as isize {
                                        if l.c(k as usize) == 64 {
                                            let after = l.c(k as usize + 1);
                                            if after != 123 && after != 34 {
                                                run_at = k;
                                                break;
                                            }
                                        }
                                        k -= 1;
                                    }
                                }
                            }
                        }
                        if run_at > p as isize {
                            l.emit_flat(t::LINK_ID, run_at as usize + 1);
                            break 'initial true;
                        }
                    }

                    if (48..=57).contains(&c) {
                        let mut q = p + 1;
                        while q < n && (48..=57).contains(&l.c(q)) {
                            q += 1;
                        }
                        l.emit_flat(t::NUM, q);
                        break 'initial true;
                    }

                    match c {
                        35 => {
                            l.emit_flat(t::BRKT, p + 1);
                            true
                        }
                        58 => {
                            if l.c(p + 1) == 58 && l.c(p + 2) == 58 {
                                l.emit_flat(t::STYLE_SEPARATOR, p + 3);
                            } else {
                                l.emit_flat(t::COLON, p + 1);
                            }
                            true
                        }
                        38 => {
                            l.emit_flat(t::AMP, p + 1);
                            true
                        }
                        59 => {
                            l.emit_flat(t::SEMI, p + 1);
                            true
                        }
                        44 => {
                            l.emit_flat(t::COMMA, p + 1);
                            true
                        }
                        42 => {
                            l.emit_flat(t::MULT, p + 1);
                            true
                        }
                        60 => {
                            l.emit_flat(t::TAGSTART, p + 1);
                            true
                        }
                        45 => {
                            if !(l.link() || l.node_string()) {
                                l.emit_flat(t::MINUS, p + 1);
                            }
                            true
                        }
                        40 => l.opener(c),
                        91 => {
                            if l.c(p + 1) == 124 {
                                l.emit_flat(t::VERTEX_WITH_PROPS_START, p + 2);
                                true
                            } else {
                                l.opener(c)
                            }
                        }
                        124 | 123 => l.opener(c),
                        62 => {
                            l.push(S::Text);
                            l.emit_flat(t::TAGEND, p + 1);
                            true
                        }
                        94 => {
                            l.emit_flat(t::UP, p + 1);
                            true
                        }
                        92 => {
                            if l.c(p + 1) == 124 {
                                l.emit_flat(t::SEP, p + 2);
                                true
                            } else {
                                l.node_string()
                            }
                        }
                        118 => {
                            if !is_word(l.c(p + 1)) {
                                l.emit_flat(t::DOWN, p + 1);
                                true
                            } else {
                                l.node_string()
                            }
                        }
                        37 => {
                            if let Some(e) = l.comment_end(p) {
                                l.emit(t::COMMENT, e);
                                true
                            } else {
                                l.node_string()
                            }
                        }
                        _ => {
                            if is_space(c) {
                                if l.link() {
                                    break 'initial true;
                                }
                                if c == 32 || c == 9 {
                                    if let Some(e) = l.comment_end(l.blanks(p)) {
                                        l.emit(t::COMMENT, e);
                                        break 'initial true;
                                    }
                                }
                                let mut e = p;
                                loop {
                                    let ch = l.c(e);
                                    if ch == 10 {
                                        e += 1;
                                    } else if ch == 13 && l.c(e + 1) == 10 {
                                        e += 2;
                                    } else {
                                        break;
                                    }
                                }
                                if e > p {
                                    l.emit(t::NEWLINE, e);
                                } else {
                                    l.emit(t::SPACE, p + 1);
                                }
                                true
                            } else if c < 128 {
                                l.node_string()
                            } else if is_unicode_letter(c) {
                                l.emit_flat(t::UNICODE_TEXT, p + 1);
                                true
                            } else {
                                false
                            }
                        }
                    }
                };
            }

            S::String => {
                if c == 34 {
                    if l.c(p + 1) == 96 {
                        l.push(S::MdString);
                        l.advance(p + 2);
                    } else {
                        l.pop();
                        l.advance(p + 1);
                    }
                } else {
                    let e = l.lx.index_of(34, p).unwrap_or(n);
                    l.emit(t::STR, e);
                }
            }

            S::MdString => {
                if c == 96 {
                    if l.c(p + 1) == 34 {
                        l.pop();
                        l.advance(p + 2);
                    } else {
                        ok = false;
                    }
                } else if c == 34 {
                    l.quote();
                } else {
                    let mut q = p + 1;
                    while q < n && l.c(q) != 96 && l.c(q) != 34 {
                        q += 1;
                    }
                    l.emit(t::MD_STR, q);
                }
            }

            S::AccTitle | S::AccDescr => {
                let e = l.lx.index_of(10, p).unwrap_or(n);
                let ty = if l.state == S::AccTitle { t::acc_title_value } else { t::acc_descr_value };
                l.pop();
                l.emit(ty, e);
            }

            S::AccDescrMultiline => {
                if c == 125 {
                    l.pop();
                    l.advance(p + 1);
                } else {
                    let e = l.lx.index_of(125, p).unwrap_or(n);
                    l.emit(t::acc_descr_multiline_value, e);
                }
            }

            S::Dir => {
                if c == 34 {
                    l.quote();
                } else if let Some(e) = l.lx.nodir(p) {
                    l.pop();
                    l.emit(t::NODIR, e);
                } else if let Some(e) = l.lx.dir(p).or_else(|| l.lx.dir_sym(p)).or_else(|| l.lx.dir_v(p)) {
                    l.pop();
                    l.emit(t::DIR, e);
                } else {
                    ok = l.opener(c);
                }
            }

            S::Text => {
                let d = l.c(p + 1);
                match c {
                    34 => l.quote(),
                    40 | 91 | 123 => {
                        l.opener(c);
                    }
                    93 => {
                        l.pop();
                        if d == 41 {
                            l.emit_flat(t::STADIUMEND, p + 2);
                        } else if d == 93 {
                            l.emit_flat(t::SUBROUTINEEND, p + 2);
                        } else {
                            l.emit_flat(t::SQE, p + 1);
                        }
                    }
                    41 => {
                        l.pop();
                        if d == 93 {
                            l.emit_flat(t::CYLINDEREND, p + 2);
                        } else if d == 41 && l.c(p + 2) == 41 {
                            l.emit_flat(t::DOUBLECIRCLEEND, p + 3);
                        } else {
                            l.emit_flat(t::PE, p + 1);
                        }
                    }
                    124 => {
                        l.pop();
                        l.emit_flat(t::PIPE, p + 1);
                    }
                    125 => {
                        l.pop();
                        l.emit_flat(t::DIAMOND_STOP, p + 1);
                    }
                    37 => {
                        // Jison drops the lone `%` from Mermaid's text pattern, so only a comment may start with one.
                        match l.comment_end(p) {
                            Some(e) => l.advance(e),
                            None => ok = false,
                        }
                    }
                    _ => {
                        let mut q = p + 1;
                        while q < n && !matches!(l.c(q), 91 | 93 | 40 | 41 | 123 | 125 | 124 | 34 | 37) {
                            q += 1;
                        }
                        l.emit(t::TEXT, q);
                    }
                }
            }

            S::EllipseText => {
                let d = l.c(p + 1);
                if c == 34 {
                    l.quote();
                } else if let Some(e) = if c == 37 { l.comment_end(p) } else { None } {
                    l.advance(e);
                } else if (c == 45 || c == 47 || c == 41) && d == 41 {
                    l.pop();
                    l.emit_flat(t::ELLIPSE_END, p + 2);
                } else if c == 40 || c == 91 || c == 123 {
                    l.opener(c);
                } else if c == 41 || c == 93 || c == 125 {
                    ok = false;
                } else {
                    let mut q = p + 1;
                    while q < n {
                        let ch = l.c(q);
                        if matches!(ch, 34 | 40 | 41 | 91 | 93 | 123 | 125) {
                            break;
                        }
                        if (ch == 45 || ch == 47) && l.c(q + 1) == 41 {
                            break;
                        }
                        if ch == 37 && l.c(q + 1) == 37 && l.c(q + 2) != 123 {
                            break;
                        }
                        q += 1;
                    }
                    l.emit_chars(t::TEXT, q);
                }
            }

            S::TrapText => {
                let d = l.c(p + 1);
                if c == 34 {
                    l.quote();
                } else {
                    match c {
                        40 => {
                            if d == 45 || d == 91 || (d == 40 && l.c(p + 2) == 40) {
                                l.opener(c);
                            } else if d == 93 {
                                l.pop();
                                l.emit_flat(t::TRAPEND, p + 2);
                            } else {
                                l.opener(c);
                            }
                        }
                        91 | 123 => {
                            l.opener(c);
                        }
                        92 => {
                            if d == 93 {
                                l.pop();
                                l.emit_flat(t::TRAPEND, p + 2);
                            } else {
                                l.emit_flat(t::TEXT, p + 1);
                            }
                        }
                        47 => {
                            if d == 93 {
                                l.pop();
                                l.emit_flat(t::INVTRAPEND, p + 2);
                            } else {
                                l.emit_flat(t::TEXT, p + 1);
                            }
                        }
                        93 | 41 => {
                            if d == 93 {
                                l.pop();
                                l.emit_flat(t::TRAPEND, p + 2);
                            } else {
                                ok = false;
                            }
                        }
                        125 => ok = false,
                        37 => match l.comment_end(p) {
                            Some(e) => l.advance(e),
                            None => ok = false,
                        },
                        _ => {
                            if (c == 63 || c == 61) && d == 93 {
                                l.pop();
                                l.emit_flat(t::TRAPEND, p + 2);
                            } else {
                                let mut q = p + 1;
                                while q < n && !matches!(l.c(q), 92 | 91 | 93 | 40 | 41 | 123 | 125 | 47 | 37) {
                                    q += 1;
                                }
                                l.emit(t::TEXT, q);
                            }
                        }
                    }
                }
            }

            S::EdgeText => {
                if c == 34 {
                    l.quote();
                } else if let Some(e) = l.arrow_end(p) {
                    l.pop();
                    l.emit(t::LINK, e);
                } else if c == 45 && l.c(p + 1) == 45 {
                    ok = false;
                } else {
                    let mut q = p + 1;
                    while q < n {
                        let ch = l.c(q);
                        if ch == 34 {
                            break;
                        }
                        if ch == 45 {
                            if l.c(q + 1) == 45 {
                                break;
                            }
                        } else if is_space(ch) && l.arrow_end(q).is_some() {
                            break;
                        }
                        q += 1;
                    }
                    l.emit_chars(t::EDGE_TEXT, q);
                }
            }

            S::Click => {
                if c == 34 {
                    l.quote();
                } else if is_space(c) {
                    l.pop();
                    l.advance(p + 1);
                } else {
                    let mut q = p + 1;
                    while q < n && !is_space(l.c(q)) {
                        q += 1;
                    }
                    l.emit_flat(t::CLICK, q);
                }
            }

            S::Callbackname => {
                if c == 40 {
                    // /\(\s*\)/y
                    let q = l.lx.ws(p + 1);
                    let empty_call = if l.c(q) == 41 { Some(q + 1) } else { None };
                    l.pop();
                    match empty_call {
                        Some(e) => l.advance(e),
                        None => {
                            l.push(S::Callbackargs);
                            l.advance(p + 1);
                        }
                    }
                } else {
                    let e = l.lx.index_of(40, p).unwrap_or(n);
                    l.emit(t::CALLBACKNAME, e);
                }
            }

            S::Callbackargs => {
                if c == 41 {
                    l.pop();
                    l.advance(p + 1);
                } else {
                    let e = l.lx.index_of(41, p).unwrap_or(n);
                    l.emit(t::CALLBACKARGS, e);
                }
            }

            S::ShapeData => {
                if c == 34 {
                    l.push(S::ShapeDataStr);
                    l.emit_flat(t::SHAPE_DATA, p + 1);
                } else if c == 125 {
                    l.pop();
                    l.advance(p + 1);
                } else if c == 94 {
                    ok = false;
                } else {
                    let mut q = p + 1;
                    while q < n && !matches!(l.c(q), 125 | 94 | 34) {
                        q += 1;
                    }
                    l.emit(t::SHAPE_DATA, q);
                }
            }

            S::ShapeDataStr => {
                if c == 34 {
                    l.pop();
                    l.emit_flat(t::SHAPE_DATA, p + 1);
                } else {
                    let e = l.lx.index_of(34, p).unwrap_or(n);
                    // .replace(/\n\s*/g, '<br/>')
                    let mut text: Vec<u16> = Vec::new();
                    let mut i = p;
                    while i < e {
                        if src[i] == 10 {
                            text.extend("<br/>".encode_utf16());
                            i += 1;
                            while i < e && is_space(src[i] as u32) {
                                i += 1;
                            }
                            continue;
                        }
                        text.push(src[i]);
                        i += 1;
                    }
                    l.emit_text(t::SHAPE_DATA, text, e);
                }
            }
        }

        if !ok {
            let (line, col) = (l.line, l.col);
            l.tk.types.push(t::ERROR);
            l.tk.texts.push(Vec::new());
            l.tk.starts.push(l.p);
            l.tk.locs.extend([line, col, line, col]);
            return l.tk;
        }
    }

    let (line, col) = (l.line, l.col);
    l.tk.types.push(t::END);
    l.tk.texts.push(Vec::new());
    l.tk.starts.push(n);
    l.tk.locs.extend([line, col, line, col]);
    l.tk
}
