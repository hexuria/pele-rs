// Port of src/diagrams/sequence/lexer.ts.

use super::tokens as t;
use crate::js;
use crate::util::chars::{is_digit, is_space, is_word};

#[derive(Default, Debug)]
pub struct Tokens {
    pub types: Vec<u32>,
    pub texts: Vec<String>,
    pub starts: Vec<usize>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum S {
    Initial,
    Id,
    Alias,
    Line,
    Config,
    AccTitle,
    AccDescr,
    AccDescrMultiline,
}

const ACTOR_START: u8 = 1;
const ACTOR_REST: u8 = 2;
const ID: u8 = 4;
const ID_WIDE: u8 = 8;
const LINE: u8 = 16;
const MENU: u8 = 32;

// Character classes of Mermaid's lexer rules, for ASCII. Other characters are tested by isSpace.
fn classes() -> [u8; 128] {
    let mut class = [0u8; 128];
    for (c, slot) in class.iter_mut().enumerate() {
        let ch = c as u8 as char;
        let ws = c == 32 || (9..=13).contains(&c);
        let mut bits = 0;
        if !"/\\+()<->:\n,;".contains(ch) {
            bits |= ACTOR_START;
        }
        if !"+<->:\n,;".contains(ch) {
            bits |= ACTOR_REST;
        }
        if !"<>:\n,;@".contains(ch) {
            bits |= ID_WIDE | if ws { 0 } else { ID };
        }
        if !"#\n;".contains(ch) {
            bits |= LINE;
        }
        if !"/\\+()<>:\n,;-".contains(ch) && !ws {
            bits |= MENU;
        }
        *slot = bits;
    }
    class
}

struct Lx<'a> {
    src: &'a [u16],
    n: usize,
    class: [u8; 128],
    out: Tokens,
    stack: Vec<S>,
    state: S,
    p: usize,
    next_nl: isize,
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

    fn emit_text(&mut self, ty: u32, s: usize, e: usize, text: String) {
        self.out.types.push(ty);
        self.out.texts.push(text);
        self.out.starts.push(s);
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

    fn eol(&mut self, q: usize) -> usize {
        if self.next_nl < q as isize {
            self.next_nl = self.src[q.min(self.n)..].iter().position(|&c| c == 10).map_or(self.n, |k| q + k) as isize;
        }
        self.next_nl as usize
    }

    fn has(&self, q: usize, bit: u8) -> bool {
        let c = self.c(q);
        // Past the end charCodeAt is NaN, which is not below 128 and not a space: every class has it.
        if c < 128 {
            return self.class[c as usize] & bit != 0;
        }
        if bit == ID || bit == MENU {
            !is_space(c)
        } else {
            true
        }
    }

    // Matches a lowercase literal without regard to the case of the source.
    fn ci(&self, q: usize, word: &str) -> bool {
        word.bytes().enumerate().all(|(k, w)| {
            let w = w as u32;
            let c = self.c(q + k);
            if (97..=122).contains(&w) {
                c != u32::MAX && (c | 32) == w
            } else {
                c == w
            }
        })
    }

    fn word(&self, lit: &str) -> bool {
        self.ci(self.p, lit) && !is_word(self.c(self.p + lit.len()))
    }

    fn skip_ws(&self, mut q: usize) -> usize {
        while q < self.n && is_space(self.c(q)) {
            q += 1;
        }
        q
    }

    fn skip_same_line(&mut self) -> bool {
        let mut q = self.p;
        while q < self.n {
            let c = self.c(q);
            if c == 10 || !is_space(c) {
                break;
            }
            q += 1;
        }
        if q == self.p {
            return false;
        }
        self.p = q;
        true
    }

    fn line_run(&self, mut q: usize) -> usize {
        while q < self.n && self.has(q, LINE) {
            q += 1;
        }
        q
    }

    fn begin(&mut self, ty: u32, len: usize, next: S) {
        self.emit(ty, self.p, self.p + len);
        self.push(next);
    }

    fn keyword(&mut self, lit: &str, ty: u32, next: Option<S>) -> bool {
        if !self.word(lit) {
            return false;
        }
        self.emit(ty, self.p, self.p + lit.len());
        if let Some(s) = next {
            self.push(s);
        }
        true
    }

    // The actor menu keywords count only when spaces and the start of a name follow.
    fn menu(&mut self, lit: &str, ty: u32) -> bool {
        if !self.ci(self.p, lit) {
            return false;
        }
        let mut q = self.p + lit.len();
        let mut c = self.c(q);
        if c != 32 && c != 9 {
            return false;
        }
        while c == 32 || c == 9 {
            q += 1;
            c = self.c(q);
        }
        if q >= self.n || !self.has(q, MENU) {
            return false;
        }
        self.emit(ty, self.p, self.p + lit.len());
        true
    }

    fn arrow_ahead(&self, q: usize) -> bool {
        let c = self.c(q);
        let d = self.c(q + 1);
        if c == 45 {
            if matches!(d, 120 | 88 | 41 | 92 | 47 | 45) {
                return true;
            }
            let e = self.c(q + 2);
            return d == 124 && (e == 92 || e == 47);
        }
        if c == 47 || c == 92 {
            return (d == 124 || d == c) && self.c(q + 2) == 45;
        }
        c == 40 && d == 41
    }

    fn actor(&mut self) {
        let p = self.p;
        let mut q = p + 1;
        while q < self.n && self.has(q, ACTOR_START) {
            q += 1;
        }
        while q < self.n && !self.arrow_ahead(q) {
            let mut r = q;
            while self.c(r) == 45 {
                r += 1;
            }
            if r >= self.n || !self.has(r, ACTOR_REST) {
                break;
            }
            r += 1;
            while r < self.n && self.has(r, ACTOR_REST) {
                r += 1;
            }
            q = r;
        }
        let text = js::trim(&js::from16(&self.src[p..q])).to_string();
        self.emit_text(t::ACTOR, p, q, text);
    }

    fn number(&mut self) -> bool {
        let p = self.p;
        let mut q = p;
        while is_digit(self.c(q)) {
            q += 1;
        }
        let whole = q > p;
        if self.c(q) == 46 && is_digit(self.c(q + 1)) {
            let r = if is_digit(self.c(q + 2)) { q + 3 } else { q + 2 };
            let after = self.c(r);
            if after == 32 || after == 10 {
                self.emit(t::NUM, p, r);
                return true;
            }
        }
        let after = self.c(q);
        if whole && (after == 32 || after == 10) {
            self.emit(t::NUM, p, q);
            return true;
        }
        false
    }

    fn dash(&mut self) {
        let p = self.p;
        let d = self.c(p + 1);
        let e = self.c(p + 2);
        if d == 62 {
            if e == 62 {
                self.emit(t::SOLID_ARROW, p, p + 3);
            } else {
                self.emit(t::SOLID_OPEN_ARROW, p, p + 2);
            }
        } else if d == 120 || d == 88 {
            self.emit(t::SOLID_CROSS, p, p + 2);
        } else if d == 41 {
            self.emit(t::SOLID_POINT, p, p + 2);
        } else if d == 124 && e == 92 {
            self.emit(t::SOLID_ARROW_TOP, p, p + 3);
        } else if d == 124 && e == 47 {
            self.emit(t::SOLID_ARROW_BOTTOM, p, p + 3);
        } else if d == 92 && e == 92 {
            self.emit(t::STICK_ARROW_TOP, p, p + 3);
        } else if d == 47 && e == 47 {
            self.emit(t::STICK_ARROW_BOTTOM, p, p + 3);
        } else if d == 45 {
            let f = self.c(p + 3);
            if e == 62 {
                if f == 62 {
                    self.emit(t::DOTTED_ARROW, p, p + 4);
                } else {
                    self.emit(t::DOTTED_OPEN_ARROW, p, p + 3);
                }
            } else if e == 120 || e == 88 {
                self.emit(t::DOTTED_CROSS, p, p + 3);
            } else if e == 41 {
                self.emit(t::DOTTED_POINT, p, p + 3);
            } else if e == 124 && f == 92 {
                self.emit(t::SOLID_ARROW_TOP_DOTTED, p, p + 4);
            } else if e == 124 && f == 47 {
                self.emit(t::SOLID_ARROW_BOTTOM_DOTTED, p, p + 4);
            } else if e == 92 && f == 92 {
                self.emit(t::STICK_ARROW_TOP_DOTTED, p, p + 4);
            } else if e == 47 && f == 47 {
                self.emit(t::STICK_ARROW_BOTTOM_DOTTED, p, p + 4);
            } else {
                self.emit(t::MINUS, p, p + 1);
            }
        } else {
            self.emit(t::MINUS, p, p + 1);
        }
    }

    fn reverse(&mut self, c: u32) -> bool {
        let p = self.p;
        let d = self.c(p + 1);
        if (d != 124 && d != c) || self.c(p + 2) != 45 {
            return false;
        }
        let dotted = self.c(p + 3) == 45;
        let top = c == 47;
        let ty = if d == 124 {
            match (dotted, top) {
                (true, true) => t::SOLID_ARROW_TOP_REVERSE_DOTTED,
                (true, false) => t::SOLID_ARROW_BOTTOM_REVERSE_DOTTED,
                (false, true) => t::SOLID_ARROW_TOP_REVERSE,
                (false, false) => t::SOLID_ARROW_BOTTOM_REVERSE,
            }
        } else {
            match (dotted, top) {
                (true, true) => t::STICK_ARROW_TOP_REVERSE_DOTTED,
                (true, false) => t::STICK_ARROW_BOTTOM_REVERSE_DOTTED,
                (false, true) => t::STICK_ARROW_TOP_REVERSE,
                (false, false) => t::STICK_ARROW_BOTTOM_REVERSE,
            }
        };
        self.emit(ty, p, p + if dotted { 4 } else { 3 });
        true
    }

    fn title(&mut self) -> bool {
        let p = self.p;
        if !self.ci(p, "title") {
            return false;
        }
        let mut q = p + 5;
        let legacy = self.c(q) == 58;
        if legacy {
            q += 1;
        }
        let mut e = q;
        while e < self.n && self.c(e) != 10 && is_space(self.c(e)) {
            e += 1;
        }
        if e == self.n || self.c(e) == 10 {
            self.p = e;
            return true;
        }
        let sep = self.c(q);
        if !is_space(sep) || sep == 13 || !self.has(q + 1, LINE) {
            return false;
        }
        let end = self.line_run(q + 1);
        self.emit(if legacy { t::legacy_title } else { t::title }, p, end);
        true
    }

    fn acc(&mut self) -> bool {
        let p = self.p;
        let descr = self.ci(p, "accdescr");
        if !descr && !self.ci(p, "acctitle") {
            return false;
        }
        let q = self.skip_ws(p + 8);
        let c = self.c(q);
        if c == 58 {
            let e = self.skip_ws(q + 1);
            self.emit(if descr { t::acc_descr } else { t::acc_title }, p, e);
            self.push(if descr { S::AccDescr } else { S::AccTitle });
            return true;
        }
        if descr && c == 123 {
            self.p = self.skip_ws(q + 1);
            self.push(S::AccDescrMultiline);
            return true;
        }
        false
    }

    fn keywords(&mut self, c: u32) -> bool {
        match c | 32 {
            97 => {
                self.keyword("actor", t::participant_actor, Some(S::Id))
                    || self.keyword("alt", t::alt, Some(S::Line))
                    || self.keyword("and", t::and, Some(S::Line))
                    || self.keyword("activate", t::activate, Some(S::Id))
                    || self.acc()
                    || self.keyword("autonumber", t::autonumber, None)
            }
            98 => self.keyword("box", t::box_, Some(S::Line)) || self.keyword("break", t::break_, Some(S::Line)),
            99 => self.keyword("create", t::create, None) || self.keyword("critical", t::critical, Some(S::Line)),
            100 => self.keyword("destroy", t::destroy, Some(S::Id)) || self.menu("details", t::details) || self.keyword("deactivate", t::deactivate, Some(S::Id)),
            101 => self.keyword("else", t::else_, Some(S::Line)) || self.keyword("end", t::end, None),
            108 => self.keyword("loop", t::loop_, Some(S::Line)) || self.keyword("left of", t::left_of, None) || self.menu("links", t::links) || self.menu("link", t::link),
            110 => self.keyword("note", t::note, None),
            111 => self.keyword("opt", t::opt, Some(S::Line)) || self.keyword("option", t::option, Some(S::Line)) || self.keyword("over", t::over, None) || self.keyword("off", t::off, None),
            112 => {
                self.keyword("participant", t::participant, Some(S::Id))
                    || self.keyword("par", t::par, Some(S::Line))
                    || self.keyword("par_over", t::par_over, Some(S::Line))
                    || self.menu("properties", t::properties)
            }
            114 => self.keyword("rect", t::rect, Some(S::Line)) || self.keyword("right of", t::right_of, None),
            115 => self.keyword("sequencediagram", t::SD, None),
            116 => self.title(),
            _ => false,
        }
    }

    fn starts_with(&self, q: usize, lit: &str) -> bool {
        lit.bytes().enumerate().all(|(k, b)| self.c(q + k) == b as u32)
    }

    fn initial(&mut self) {
        let p = self.p;
        let c = self.c(p);
        if c == 10 {
            let mut q = p + 1;
            while self.c(q) == 10 {
                q += 1;
            }
            return self.emit(t::NEWLINE, p, q);
        }
        if is_space(c) {
            self.p = self.skip_ws(p + 1);
            return;
        }
        if c == 35 || (c == 37 && self.c(p + 1) != 123) {
            self.p = self.eol(p);
            return;
        }
        if (is_digit(c) || c == 46) && self.number() {
            return;
        }
        if c < 128 && (c | 32) >= 97 && (c | 32) <= 122 && self.keywords(c) {
            return;
        }
        if c == 44 {
            self.emit(t::COMMA, p, p + 1);
        } else if c == 59 {
            self.emit(t::NEWLINE, p, p + 1);
        } else if self.has(p, ACTOR_START) {
            self.actor();
        } else if c == 45 {
            self.dash();
        } else if c == 60 && self.starts_with(p, "<<->>") {
            self.emit(t::BIDIRECTIONAL_SOLID_ARROW, p, p + 5);
        } else if c == 60 && self.starts_with(p, "<<-->>") {
            self.emit(t::BIDIRECTIONAL_DOTTED_ARROW, p, p + 6);
        } else if (c == 47 || c == 92) && self.reverse(c) {
        } else if c == 58 {
            let e = self.line_run(p + 1);
            self.emit(t::TXT, p, e);
        } else if c == 43 {
            self.emit(t::PLUS, p, p + 1);
        } else if c == 40 && self.c(p + 1) == 41 {
            self.emit(t::CENTRAL, p, p + 2);
        } else {
            self.emit(t::INVALID, p, p + 1);
        }
    }

    // A participant name, up to an alias, a config object, or the end of the statement.
    fn id(&mut self) -> bool {
        if self.skip_same_line() {
            return true;
        }
        let p = self.p;
        let c = self.c(p);
        if c == 35 {
            self.p = self.eol(p);
            return true;
        }
        if c == 64 && self.c(p + 1) == 123 {
            self.begin(t::CONFIG_START, 2, S::Config);
            return true;
        }
        let mut a = p;
        while a < self.n && self.has(a, ID) {
            a += 1;
        }
        if a > p {
            let w = self.skip_ws(a);
            if self.c(w) == 64 && self.c(w + 1) == 123 {
                self.emit(t::ACTOR, p, a);
                return true;
            }
            if w > a && self.ci(w, "as") && is_space(self.c(w + 2)) {
                self.emit(t::ACTOR, p, a);
                self.push(S::Alias);
                return true;
            }
        }
        let mut b = a;
        while b < self.n && self.has(b, ID_WIDE) {
            b += 1;
        }
        if b > p {
            let end = self.c(b);
            // Before any other delimiter the name must be followed by a comment, so it ends at the last one.
            let mut cut = b;
            if b < self.n && end != 10 && end != 59 {
                cut -= 1;
                while cut > p && self.c(cut) != 35 {
                    cut -= 1;
                }
            }
            if cut > p {
                let text = js::trim(&js::from16(&self.src[p..cut])).to_string();
                self.emit_text(t::ACTOR, p, cut, text);
                self.pop();
                return true;
            }
        }
        if self.c(b) == 60 {
            let e = self.eol(p);
            self.emit(t::INVALID, p, e);
            self.pop();
            return true;
        }
        if c != 10 {
            let e = self.eol(p);
            let text = js::trim(&js::from16(&self.src[p..e])).to_string();
            self.emit_text(t::INVALID, p, e, text);
            self.pop();
            return true;
        }
        false
    }

    fn step(&mut self) -> bool {
        match self.state {
            S::Initial => {
                self.initial();
                true
            }
            S::Id => self.id(),
            S::Alias => {
                if self.skip_same_line() {
                    return true;
                }
                if self.c(self.p) == 35 {
                    self.p = self.eol(self.p);
                    return true;
                }
                self.pop();
                self.pop();
                if self.word("as") {
                    self.begin(t::AS, 2, S::Line);
                } else {
                    self.emit(t::NEWLINE, self.p, self.p);
                }
                true
            }
            S::Line => {
                if self.skip_same_line() {
                    return true;
                }
                if self.c(self.p) == 35 {
                    self.p = self.eol(self.p);
                    return true;
                }
                let e = self.line_run(self.p);
                self.emit(t::restOfLine, self.p, e);
                self.pop();
                true
            }
            S::Config => {
                let p = self.p;
                if self.c(p) != 125 {
                    let q = self.src[p..].iter().position(|&c| c == 125).map_or(self.n, |k| p + k);
                    self.emit(t::CONFIG_CONTENT, p, q);
                } else {
                    let w = self.skip_ws(p + 1);
                    let alias = w > p + 1 && self.ci(w, "as") && is_space(self.c(w + 2));
                    self.emit(t::CONFIG_END, p, p + 1);
                    self.pop();
                    if alias {
                        self.push(S::Alias);
                    } else {
                        self.pop();
                    }
                }
                true
            }
            S::AccTitle | S::AccDescr => {
                let ty = if self.state == S::AccTitle { t::acc_title_value } else { t::acc_descr_value };
                let e = self.eol(self.p);
                self.emit(ty, self.p, e);
                self.pop();
                true
            }
            S::AccDescrMultiline => {
                let p = self.p;
                if self.c(p) == 125 {
                    self.p += 1;
                    self.pop();
                } else {
                    let q = self.src[p..].iter().position(|&c| c == 125).map_or(self.n, |k| p + k);
                    self.emit(t::acc_descr_multiline_value, p, q);
                }
                true
            }
        }
    }
}

pub fn tokenize(src: &[u16]) -> Tokens {
    let n = src.len();
    let mut lx = Lx { src, n, class: classes(), out: Tokens::default(), stack: Vec::new(), state: S::Initial, p: 0, next_nl: -1 };
    while lx.p < n {
        if !lx.step() {
            lx.out.types.push(t::ERROR);
            lx.out.texts.push(String::new());
            lx.out.starts.push(lx.p);
            return lx.out;
        }
    }
    // Mermaid's lexer tries its rules once more on the empty remainder before reporting the end.
    match lx.state {
        S::Initial | S::Alias => lx.emit(t::NEWLINE, n, n),
        S::Line => lx.emit(t::restOfLine, n, n),
        S::AccTitle => lx.emit(t::acc_title_value, n, n),
        S::AccDescr => lx.emit(t::acc_descr_value, n, n),
        S::AccDescrMultiline => lx.emit(t::acc_descr_multiline_value, n, n),
        _ => {}
    }
    let mut out = lx.out;
    out.types.push(t::END);
    out.texts.push(String::new());
    out.starts.push(n);
    out
}
