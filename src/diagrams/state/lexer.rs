// Port of src/diagrams/state/lexer.ts.

use super::tokens as t;
use crate::js;
use crate::util::chars::{is_line_end, is_space, is_word};

#[derive(Default, Debug)]
pub struct Tokens {
    pub types: Vec<u32>,
    pub texts: Vec<String>,
    pub starts: Vec<usize>,
    // Set when lexing stopped on the grammar's own error rather than on unrecognized text.
    pub error: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum S {
    Initial,
    Struct,
    State,
    StateString,
    StateId,
    Scale,
    AccTitle,
    AccDescr,
    AccDescrMultiline,
    ClassDef,
    ClassDefId,
    Class,
    ClassStyle,
    Style,
    StyleDefStyles,
    Note,
    NoteId,
    NoteText,
    FloatingNote,
    FloatingNoteId,
}

#[derive(Clone, Copy)]
enum Pat {
    Direction,
    Stereotype,
    LineEnd,
    Percent,
    Brace,
}

// Finds the next match of a pattern at or after a position, remembering the answer so that
// asking at each token costs one scan of the text in total.
struct Finder {
    pat: Pat,
    from: usize,
    next: isize,
}

struct Lx<'a> {
    src: &'a [u16],
    n: usize,
    out: Tokens,
    stack: Vec<S>,
    state: S,
    p: usize,
    // The last `direction <dir>` of each kind on the line being lexed, and where its keyword ends.
    dir_from: isize,
    dir_to: isize,
    dir_at: [isize; 4],
    dir_end: [usize; 4],
    // The same for <<fork>>, <<join>>, <<choice>> and their [[ ]] spellings.
    stereo_from: isize,
    stereo_to: isize,
    stereo_at: [isize; 6],
    f_direction: Finder,
    f_stereotype: Finder,
    f_line_end: Finder,
    f_percent: Finder,
    f_brace: Finder,
}

impl Lx<'_> {
    fn c(&self, i: usize) -> u32 {
        self.src.get(i).map_or(u32::MAX, |&c| c as u32)
    }

    // Case-insensitive match of a lowercase ASCII word, as the grammar's /i flag gives.
    fn word(&self, w: &str, q: usize) -> bool {
        w.bytes().enumerate().all(|(i, a)| {
            let a = a as u32;
            let c = self.c(q + i);
            c == a || ((97..=122).contains(&a) && c == a - 32)
        })
    }

    fn matches_at(&self, pat: Pat, k: usize) -> bool {
        match pat {
            Pat::Direction => self.word("direction", k),
            Pat::Stereotype => {
                let (open, close) = match self.c(k) {
                    60 => (60, 62),
                    91 => (91, 93),
                    _ => return false,
                };
                if self.c(k + 1) != open {
                    return false;
                }
                for w in ["fork", "join", "choice"] {
                    if self.word(w, k + 2) && self.c(k + 2 + w.len()) == close && self.c(k + 3 + w.len()) == close {
                        return true;
                    }
                }
                false
            }
            Pat::LineEnd => is_line_end(self.c(k)),
            Pat::Percent => self.c(k) == 37 && self.c(k + 1) == 37,
            Pat::Brace => self.c(k) == 123,
        }
    }

    fn find(&mut self, which: u8, q: usize) -> isize {
        let f = match which {
            0 => &self.f_direction,
            1 => &self.f_stereotype,
            2 => &self.f_line_end,
            3 => &self.f_percent,
            _ => &self.f_brace,
        };
        let (pat, from, next) = (f.pat, f.from, f.next);
        if next == -2 || q < from || (next != -1 && q as isize > next) {
            let found = (q..self.n).find(|&k| self.matches_at(pat, k)).map_or(-1, |k| k as isize);
            let f = match which {
                0 => &mut self.f_direction,
                1 => &mut self.f_stereotype,
                2 => &mut self.f_line_end,
                3 => &mut self.f_percent,
                _ => &mut self.f_brace,
            };
            f.next = found;
            f.from = q;
            return found;
        }
        next
    }

    fn emit(&mut self, ty: u32, s: usize, e: usize) {
        self.out.types.push(ty);
        self.out.texts.push(js::from16(&self.src[s..e]));
        self.out.starts.push(s);
        self.p = e;
    }

    fn emit_text(&mut self, ty: u32, text: String, s: usize, e: usize) {
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

    fn ws_end(&self, q: usize) -> usize {
        let mut j = q;
        while j < self.n && is_space(self.c(j)) {
            j += 1;
        }
        j
    }

    fn word_end(&self, q: usize) -> usize {
        let mut j = q;
        while j < self.n && is_word(self.c(j)) {
            j += 1;
        }
        j
    }

    fn index_of(&self, ch: u16, from: usize) -> Option<usize> {
        self.src[from.min(self.n)..].iter().position(|&c| c == ch).map(|k| from + k)
    }

    fn eol(&self, q: usize) -> usize {
        self.index_of(10, q).unwrap_or(self.n)
    }

    fn line_end(&mut self, q: usize) -> usize {
        let k = self.find(2, q);
        if k == -1 {
            self.n
        } else {
            k as usize
        }
    }

    // Rules `.*direction\s+TB[^\n]*` and its three siblings: the first kind, in grammar order,
    // found between q and the end of its line.
    fn direction(&mut self, q: usize) -> Option<usize> {
        let qi = q as isize;
        if qi < self.dir_from || qi > self.dir_to {
            let first = self.find(0, q);
            let end = if first == -1 { self.n } else { self.line_end(q) };
            self.dir_from = qi;
            self.dir_to = end as isize;
            self.dir_at = [-1; 4];
            let mut d = first;
            while d != -1 && (d as usize) < end {
                let du = d as usize;
                let w = self.ws_end(du + 9);
                if w != du + 9 {
                    let kind = if self.word("tb", w) {
                        0
                    } else if self.word("bt", w) {
                        1
                    } else if self.word("rl", w) {
                        2
                    } else if self.word("lr", w) {
                        3
                    } else {
                        4
                    };
                    if kind < 4 {
                        self.dir_at[kind] = d;
                        self.dir_end[kind] = w + 2;
                    }
                }
                d = self.find(0, du + 1);
            }
        }
        (0..4).find(|&kind| self.dir_at[kind] >= qi)
    }

    fn stereotype(&mut self, q: usize) -> Option<usize> {
        let qi = q as isize;
        if qi < self.stereo_from || qi > self.stereo_to {
            let first = self.find(1, q);
            let end = if first == -1 { self.n } else { self.line_end(q) };
            self.stereo_from = qi;
            self.stereo_to = end as isize;
            self.stereo_at = [-1; 6];
            let mut d = first;
            while d != -1 && (d as usize) < end {
                let du = d as usize;
                let letter = self.c(du + 2) | 32;
                let k = (if self.c(du) == 60 { 0 } else { 3 })
                    + if letter == 102 {
                        0
                    } else if letter == 106 {
                        1
                    } else {
                        2
                    };
                self.stereo_at[k] = d;
                d = self.find(1, du + 1);
            }
        }
        (0..6).find(|&kind| self.stereo_at[kind] >= qi)
    }

    // The grammar's processId(): an id ends at an inline `%%`, and a text that starts with one is dropped.
    fn emit_id(&mut self, s: usize, e: usize) -> bool {
        let at = self.find(3, s);
        if at == -1 || at as usize + 2 > e {
            self.emit(t::ID, s, e);
            return true;
        }
        if at as usize == s {
            self.p = e;
            return false;
        }
        self.emit(t::ID, s, at as usize);
        true
    }

    fn finish(mut self, ty: u32) -> Tokens {
        self.out.types.push(ty);
        self.out.texts.push(String::new());
        self.out.starts.push(self.p);
        self.out
    }
}

pub fn tokenize(src: &[u16]) -> Tokens {
    let n = src.len();
    let finder = |pat| Finder { pat, from: 0, next: -2 };
    let mut lx = Lx {
        src,
        n,
        out: Tokens::default(),
        stack: Vec::new(),
        state: S::Initial,
        p: 0,
        dir_from: -1,
        dir_to: -1,
        dir_at: [-1; 4],
        dir_end: [0; 4],
        stereo_from: -1,
        stereo_to: -1,
        stereo_at: [-1; 6],
        f_direction: finder(Pat::Direction),
        f_stereotype: finder(Pat::Stereotype),
        f_line_end: finder(Pat::LineEnd),
        f_percent: finder(Pat::Percent),
        f_brace: finder(Pat::Brace),
    };

    loop {
        let p = lx.p;
        if p >= n {
            // Jison tries the current state's rules once more on the empty remainder.
            let ty = match lx.state {
                S::Initial => Some(t::NL),
                S::AccTitle => Some(t::acc_title_value),
                S::AccDescr => Some(t::acc_descr_value),
                S::AccDescrMultiline => Some(t::acc_descr_multiline_value),
                S::ClassDefId => Some(t::CLASSDEF_STYLEOPTS),
                S::ClassStyle => Some(t::STYLECLASS),
                S::StyleDefStyles => Some(t::STYLEDEF_STYLEOPTS),
                S::StateString => Some(t::STATE_DESCR),
                S::FloatingNote => Some(t::NOTE_TEXT),
                S::StateId | S::FloatingNoteId => Some(t::ID),
                _ => None,
            };
            if let Some(ty) = ty {
                lx.emit(ty, n, n);
            }
            return lx.finish(t::END);
        }

        let c = lx.c(p);
        match lx.state {
            S::Initial | S::Struct => {
                let root = lx.state == S::Initial;
                let lc = c | 32;
                if root {
                    if lc == 99 && lx.word("click", p) && !is_word(lx.c(p + 5)) {
                        lx.emit(t::CLICK, p, p + 5);
                        continue;
                    }
                    if lc == 104 && lx.word("href", p) && !is_word(lx.c(p + 4)) {
                        lx.emit(t::HREF, p, p + 4);
                        continue;
                    }
                    if c == 34 {
                        if let Some(close) = lx.index_of(34, p + 1) {
                            lx.emit(t::STRING, p, close + 1);
                            continue;
                        }
                    }
                    if lc == 100 && lx.word("default", p) && !is_word(lx.c(p + 7)) {
                        lx.emit(t::DEFAULT, p, p + 7);
                        continue;
                    }
                    if let Some(kind) = lx.direction(p) {
                        let e = lx.eol(lx.dir_end[kind]);
                        lx.emit(t::direction_tb + kind as u32, p, e);
                        continue;
                    }
                    if c == 10 {
                        let mut j = p + 1;
                        while lx.c(j) == 10 {
                            j += 1;
                        }
                        lx.emit(t::NL, p, j);
                        continue;
                    }
                    if is_space(c) {
                        lx.p = lx.ws_end(p);
                        continue;
                    }
                } else if c != 10 && is_space(c) {
                    let mut j = p + 1;
                    while j < n && lx.c(j) != 10 && is_space(lx.c(j)) {
                        j += 1;
                    }
                    lx.p = j;
                    continue;
                }
                if c == 35 || (c == 37 && lx.c(p + 1) == 37 && lx.c(p + 2) != 123) {
                    lx.p = lx.eol(p);
                    continue;
                }
                if lc == 115 {
                    if root && lx.word("scale", p) {
                        let e = lx.ws_end(p + 5);
                        if e > p + 5 {
                            lx.emit(t::scale, p, e);
                            lx.push(S::Scale);
                            continue;
                        }
                    }
                    if lx.word("style", p) {
                        let e = lx.ws_end(p + 5);
                        if e > p + 5 {
                            lx.emit(t::style, p, e);
                            lx.push(S::Style);
                            continue;
                        }
                    }
                    if lx.word("state", p) {
                        let e = lx.ws_end(p + 5);
                        if e > p + 5 {
                            lx.p = e;
                            lx.push(S::State);
                            continue;
                        }
                        if root && lx.word("diagram", p + 5) {
                            let from = if lx.word("-v2", p + 12) { p + 15 } else { p + 12 };
                            let e = lx.ws_end(from);
                            if e > from {
                                lx.emit(t::SD, p, e);
                                continue;
                            }
                        }
                    }
                } else if lc == 99 {
                    if lx.word("class", p) {
                        let def = lx.word("def", p + 5);
                        let from = if def { p + 8 } else { p + 5 };
                        let e = lx.ws_end(from);
                        if e > from {
                            lx.emit(if def { t::classDef } else { t::class_ }, p, e);
                            lx.push(if def { S::ClassDef } else { S::Class });
                            continue;
                        }
                    }
                } else if lc == 97 && root && (lx.word("acctitle", p) || lx.word("accdescr", p)) {
                    let title = (lx.c(p + 3) | 32) == 116;
                    let q = lx.ws_end(p + 8);
                    let d = lx.c(q);
                    if d == 58 {
                        let e = lx.ws_end(q + 1);
                        lx.emit(if title { t::acc_title } else { t::acc_descr }, p, e);
                        lx.push(if title { S::AccTitle } else { S::AccDescr });
                        continue;
                    }
                    if d == 123 && !title {
                        lx.p = lx.ws_end(q + 1);
                        lx.push(S::AccDescrMultiline);
                        continue;
                    }
                }
                if root {
                    if c == 123 {
                        lx.emit(t::STRUCT_START, p, p + 1);
                        lx.push(S::Struct);
                        continue;
                    }
                } else {
                    if let Some(kind) = lx.direction(p) {
                        let e = lx.eol(lx.dir_end[kind]);
                        lx.emit(t::direction_tb + kind as u32, p, e);
                        continue;
                    }
                    if c == 125 {
                        lx.emit(t::STRUCT_STOP, p, p + 1);
                        lx.pop();
                        continue;
                    }
                    if c == 10 {
                        lx.p += 1;
                        continue;
                    }
                }
                if lc == 110 && lx.word("note", p) {
                    let e = lx.ws_end(p + 4);
                    if e > p + 4 {
                        lx.emit(t::note, p, e);
                        lx.push(S::Note);
                        continue;
                    }
                }
                if root && lc == 104 && lx.word("hide empty description", p) && !is_word(lx.c(p + 22)) {
                    lx.emit(t::HIDE_EMPTY, p, p + 22);
                    continue;
                }
                if c == 91 && lx.c(p + 1) == 42 && lx.c(p + 2) == 93 {
                    lx.emit(t::EDGE_STATE, p, p + 3);
                    continue;
                }
                let mut j = p;
                while j < n {
                    let d = lx.c(j);
                    if d == 58 || d == 45 || d == 123 || is_space(d) {
                        break;
                    }
                    j += 1;
                }
                if j > p {
                    lx.emit_id(p, j);
                    continue;
                }
                if c == 58 {
                    let mut j = p + 1;
                    while j < n {
                        let d = lx.c(j);
                        if d == 10 || d == 59 {
                            break;
                        }
                        if d == 58 {
                            let after = lx.c(j + 1);
                            if j + 1 >= n || after == 58 || after == 10 || after == 59 {
                                break;
                            }
                            j += 1;
                        }
                        j += 1;
                    }
                    if j > p + 1 {
                        let text = js::trim(&js::from16(&src[p..j])).to_string();
                        lx.emit_text(t::DESCR, text, p, j);
                        continue;
                    }
                    if lx.c(p + 1) == 58 && lx.c(p + 2) == 58 {
                        lx.emit(t::STYLE_SEPARATOR, p, p + 3);
                        continue;
                    }
                } else if c == 45 && lx.c(p + 1) == 45 {
                    if lx.c(p + 2) == 62 {
                        lx.emit(t::ARROW, p, p + 3);
                        continue;
                    }
                    if !root {
                        lx.emit(t::CONCURRENT, p, p + 2);
                        continue;
                    }
                }
                if !root {
                    return lx.finish(t::ERROR);
                }
                lx.emit(t::INVALID, p, p + 1);
            }

            S::State => {
                if c != 10 && is_space(c) {
                    let mut j = p + 1;
                    while j < n && lx.c(j) != 10 && is_space(lx.c(j)) {
                        j += 1;
                    }
                    lx.p = j;
                    continue;
                }
                if c == 35 || (c == 37 && lx.c(p + 1) == 37 && lx.c(p + 2) != 123) {
                    lx.p = lx.eol(p);
                    continue;
                }
                if let Some(kind) = lx.stereotype(p) {
                    let at = lx.stereo_at[kind] as usize;
                    let choice = kind == 2 || kind == 5;
                    let ty = if choice {
                        t::CHOICE
                    } else if kind % 3 == 0 {
                        t::FORK
                    } else {
                        t::JOIN
                    };
                    let text = js::trim(&js::from16(&src[p..at])).to_string();
                    lx.emit_text(ty, text, p, at + if choice { 10 } else { 8 });
                    lx.pop();
                    continue;
                }
                if c == 34 {
                    lx.p += 1;
                    lx.push(S::StateString);
                    continue;
                }
                let q = if c == 10 { lx.ws_end(p) } else { p };
                if lx.word("as", q) {
                    let e = lx.ws_end(q + 2);
                    if e > q + 2 {
                        lx.emit(t::AS, p, e);
                        lx.push(S::StateId);
                        continue;
                    }
                }
                if c == 10 {
                    lx.p += 1;
                    lx.pop();
                    continue;
                }
                if c == 123 {
                    lx.emit(t::STRUCT_START, p, p + 1);
                    lx.pop();
                    lx.push(S::Struct);
                    continue;
                }
                if is_word(c) {
                    let first = lx.word_end(p);
                    let second = lx.ws_end(first);
                    if second > first && is_word(lx.c(second)) {
                        let brace = lx.find(4, second + 1);
                        if brace != -1 && (brace as usize) < lx.line_end(second) {
                            let found = js::trim(&js::from16(&src[p..brace as usize + 1])).to_string();
                            lx.out.error = Some(format!("Error: State name must be a single word. Found: \"{}\"", found));
                            return lx.finish(t::ERROR);
                        }
                    }
                }
                let mut j = p + 1;
                while j < n {
                    let d = lx.c(j);
                    if d == 123 || is_space(d) {
                        break;
                    }
                    j += 1;
                }
                lx.emit(t::COMPOSIT_STATE, p, j);
            }

            S::StateString => {
                if c == 34 {
                    lx.p += 1;
                    lx.pop();
                    continue;
                }
                let close = lx.index_of(34, p).unwrap_or(n);
                lx.emit(t::STATE_DESCR, p, close);
            }

            S::StateId => {
                let mut j = p;
                while j < n {
                    let d = lx.c(j);
                    if d == 10 || d == 123 {
                        break;
                    }
                    j += 1;
                }
                if lx.emit_id(p, j) {
                    lx.pop();
                }
            }

            S::Scale => {
                if (48..=57).contains(&c) {
                    let mut j = p + 1;
                    while j < n && (48..=57).contains(&lx.c(j)) {
                        j += 1;
                    }
                    lx.emit(t::WIDTH, p, j);
                    continue;
                }
                let q = lx.ws_end(p);
                if q > p && lx.word("width", q) && !is_word(lx.c(q + 5)) {
                    lx.p = q + 5;
                    lx.pop();
                    continue;
                }
                return lx.finish(t::ERROR);
            }

            S::AccTitle | S::AccDescr => {
                let ty = if lx.state == S::AccTitle { t::acc_title_value } else { t::acc_descr_value };
                let e = lx.eol(p);
                lx.emit(ty, p, e);
                lx.pop();
            }

            S::AccDescrMultiline => {
                if c == 125 {
                    lx.p += 1;
                    lx.pop();
                    continue;
                }
                let close = lx.index_of(125, p).unwrap_or(n);
                lx.emit(t::acc_descr_multiline_value, p, close);
            }

            S::ClassDef => {
                let is_default = lx.word("default", p) && lx.ws_end(p + 7) > p + 7;
                let j = if is_default { p + 7 } else { lx.word_end(p) };
                let e = lx.ws_end(j);
                if j == p || e == j {
                    return lx.finish(t::ERROR);
                }
                lx.emit(if is_default { t::DEFAULT_CLASSDEF_ID } else { t::CLASSDEF_ID }, p, e);
                lx.pop();
                lx.push(S::ClassDefId);
            }

            S::ClassDefId => {
                let e = lx.eol(p);
                lx.emit(t::CLASSDEF_STYLEOPTS, p, e);
                lx.pop();
            }

            S::Class => {
                let mut j = lx.word_end(p);
                if j == p {
                    return lx.finish(t::ERROR);
                }
                while lx.c(j) == 44 {
                    let k = lx.ws_end(j + 1);
                    let e = lx.word_end(k);
                    if e == k {
                        break;
                    }
                    j = e;
                }
                lx.emit(t::CLASSENTITY_IDS, p, j);
                lx.pop();
                lx.push(S::ClassStyle);
            }

            S::ClassStyle => {
                let e = lx.eol(p);
                lx.emit(t::STYLECLASS, p, e);
                lx.pop();
            }

            S::Style => {
                let mut j = p;
                while j < n && (is_word(lx.c(j)) || lx.c(j) == 44) {
                    j += 1;
                }
                let e = lx.ws_end(j);
                if j == p || e == j {
                    return lx.finish(t::ERROR);
                }
                lx.emit(t::STYLE_IDS, p, e);
                lx.pop();
                lx.push(S::StyleDefStyles);
            }

            S::StyleDefStyles => {
                let e = lx.eol(p);
                lx.emit(t::STYLEDEF_STYLEOPTS, p, e);
                lx.pop();
            }

            S::Note => {
                if lx.word("left of", p) && !is_word(lx.c(p + 7)) {
                    lx.emit(t::left_of, p, p + 7);
                    lx.pop();
                    lx.push(S::NoteId);
                    continue;
                }
                if lx.word("right of", p) && !is_word(lx.c(p + 8)) {
                    lx.emit(t::right_of, p, p + 8);
                    lx.pop();
                    lx.push(S::NoteId);
                    continue;
                }
                if c == 34 {
                    lx.p += 1;
                    lx.pop();
                    lx.push(S::FloatingNote);
                    continue;
                }
                return lx.finish(t::ERROR);
            }

            S::NoteId => {
                let q = lx.ws_end(p);
                let mut j = q;
                while j < n {
                    let d = lx.c(j);
                    if d == 58 || d == 45 || is_space(d) {
                        break;
                    }
                    j += 1;
                }
                if j == q {
                    return lx.finish(t::ERROR);
                }
                if lx.emit_id(p, j) {
                    lx.pop();
                    lx.push(S::NoteText);
                }
            }

            S::NoteText => {
                let q = lx.ws_end(p);
                if lx.c(q) == 58 {
                    let mut j = q + 1;
                    while j < n {
                        let d = lx.c(j);
                        if d == 58 || d == 10 || d == 59 {
                            break;
                        }
                        j += 1;
                    }
                    if j > q + 1 {
                        // Mermaid drops two characters here, expecting " :"; with no space before the
                        // colon the first character of the note goes too.
                        let text = if p + 2 <= j { js::trim(&js::from16(&src[p + 2..j])).to_string() } else { String::new() };
                        lx.emit_text(t::NOTE_TEXT, text, p, j);
                        lx.pop();
                        continue;
                    }
                }
                let mut k = lx.index_of(10, p);
                let mut e = 0;
                while let Some(kk) = k {
                    e = lx.ws_end(kk);
                    if lx.word("end note", e) && !is_word(lx.c(e + 8)) {
                        break;
                    }
                    k = lx.index_of(10, e);
                }
                if k.is_none() {
                    return lx.finish(t::ERROR);
                }
                let text = js::trim(&js::from16(&src[p..e])).to_string();
                lx.emit_text(t::NOTE_TEXT, text, p, e + 8);
                lx.pop();
            }

            S::FloatingNote => {
                let q = lx.ws_end(p);
                if lx.word("as", q) {
                    let e = lx.ws_end(q + 2);
                    lx.emit(t::AS, p, e);
                    lx.pop();
                    lx.push(S::FloatingNoteId);
                    continue;
                }
                if c == 34 {
                    lx.p += 1;
                    continue;
                }
                let close = lx.index_of(34, p).unwrap_or(n);
                lx.emit(t::NOTE_TEXT, p, close);
            }

            S::FloatingNoteId => {
                let e = lx.eol(p);
                if lx.emit_id(p, e) {
                    lx.pop();
                }
            }
        }
    }
}
