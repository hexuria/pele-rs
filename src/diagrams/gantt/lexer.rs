// Port of src/diagrams/gantt/lexer.ts and tokens.ts.

use crate::js;
use crate::util::chars::is_space;

pub const END: u32 = 0;
pub const ERROR: u32 = 1;
pub const OPEN_DIRECTIVE: u32 = 2;
pub const ACC_TITLE: u32 = 3;
pub const ACC_TITLE_VALUE: u32 = 4;
pub const ACC_DESCR: u32 = 5;
pub const ACC_DESCR_VALUE: u32 = 6;
pub const ACC_DESCR_MULTILINE_VALUE: u32 = 7;
pub const NL: u32 = 8;
pub const HREF: u32 = 9;
pub const CALLBACKNAME: u32 = 10;
pub const CALLBACKARGS: u32 = 11;
pub const CLICK: u32 = 12;
pub const GANTT: u32 = 13;
pub const DATE_FORMAT: u32 = 14;
pub const INCLUSIVE_END_DATES: u32 = 15;
pub const TOP_AXIS: u32 = 16;
pub const AXIS_FORMAT: u32 = 17;
pub const TICK_INTERVAL: u32 = 18;
pub const INCLUDES: u32 = 19;
pub const EXCLUDES: u32 = 20;
pub const TODAY_MARKER: u32 = 21;
pub const WEEKDAY_MONDAY: u32 = 22;
pub const WEEKEND_FRIDAY: u32 = 29;
pub const WEEKEND_SATURDAY: u32 = 30;
pub const DATE: u32 = 31;
pub const TITLE: u32 = 32;
pub const ACC_DESCRIPTION: u32 = 33;
pub const SECTION: u32 = 34;
pub const TASK_TXT: u32 = 35;
pub const TASK_DATA: u32 = 36;
pub const COLON: u32 = 37;
pub const EOF: u32 = 38;

pub const TOKEN_NAMES: [&str; 39] = [
    "$end",
    "INVALID",
    "open_directive",
    "acc_title",
    "acc_title_value",
    "acc_descr",
    "acc_descr_value",
    "acc_descr_multiline_value",
    "NL",
    "href",
    "callbackname",
    "callbackargs",
    "click",
    "gantt",
    "dateFormat",
    "inclusiveEndDates",
    "topAxis",
    "axisFormat",
    "tickInterval",
    "includes",
    "excludes",
    "todayMarker",
    "weekday_monday",
    "weekday_tuesday",
    "weekday_wednesday",
    "weekday_thursday",
    "weekday_friday",
    "weekday_saturday",
    "weekday_sunday",
    "weekend_friday",
    "weekend_saturday",
    "date",
    "title",
    "accDescription",
    "section",
    "taskTxt",
    "taskData",
    ":",
    "EOF",
];

pub struct Tokens {
    pub types: Vec<u32>,
    pub texts: Vec<String>,
    pub starts: Vec<usize>,
}

const S_INITIAL: u8 = 0;
const S_ACC_TITLE: u8 = 1;
const S_ACC_DESCR: u8 = 2;
const S_ACC_DESCR_MULTILINE: u8 = 3;
const S_HREF: u8 = 4;
const S_CALLBACKNAME: u8 = 5;
const S_CALLBACKARGS: u8 = 6;
const S_CLICK: u8 = 7;

const DAYS: [&str; 7] = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];

fn at(src: &[u16], i: usize) -> u32 {
    src.get(i).map_or(u32::MAX, |&c| c as u32)
}

// Case-insensitive for ASCII letters, as Jison's `i` flag.
fn word(src: &[u16], p: usize, w: &str) -> bool {
    w.bytes().enumerate().all(|(k, b)| {
        let c = at(src, p + k);
        c < 128 && (c as u8).eq_ignore_ascii_case(&b)
    })
}

fn is_w(c: u32) -> bool {
    c < 128 && js::is_word(c as u8 as char)
}

fn spaces(src: &[u16], mut p: usize) -> usize {
    while p < src.len() && is_space(src[p] as u32) {
        p += 1;
    }
    p
}

// `word\s[^…]+`: the keyword, one whitespace character, then at least one character not in `stop`.
fn statement(src: &[u16], p: usize, w: &str, stop: &[u16]) -> Option<usize> {
    if !word(src, p, w) {
        return None;
    }
    let q = p + w.len();
    if !is_space(at(src, q)) {
        return None;
    }
    let mut e = q + 1;
    while e < src.len() && !stop.contains(&src[e]) {
        e += 1;
    }
    (e > q + 1).then_some(e)
}

// `word\b`
fn bare(src: &[u16], p: usize, w: &str) -> Option<usize> {
    (word(src, p, w) && !is_w(at(src, p + w.len()))).then_some(p + w.len())
}

#[allow(unused_assignments)]
pub fn tokenize(src: &[u16]) -> Tokens {
    let n = src.len();
    let mut out = Tokens { types: Vec::new(), texts: Vec::new(), starts: Vec::new() };
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
    let until = |ch: u16, from: usize| src[from.min(n)..].iter().position(|&c| c == ch).map_or(n, |k| from + k);
    const HASH_NL_SEMI: [u16; 3] = [35, 10, 59];
    const NL_SEMI: [u16; 2] = [10, 59];
    const NL_ONLY: [u16; 1] = [10];

    loop {
        if p >= n {
            let ty = match state {
                S_ACC_TITLE => ACC_TITLE_VALUE,
                S_ACC_DESCR => ACC_DESCR_VALUE,
                S_ACC_DESCR_MULTILINE => ACC_DESCR_MULTILINE_VALUE,
                S_HREF => HREF,
                S_CALLBACKNAME => CALLBACKNAME,
                S_CALLBACKARGS => CALLBACKARGS,
                S_CLICK => CLICK,
                _ => EOF,
            };
            emit!(ty, n, n);
            break;
        }
        let c = src[p] as u32;
        match state {
            S_INITIAL => {
                let d = at(src, p + 1);
                if c == 37 {
                    if d != 37 {
                        let mut e = until(10, p + 1);
                        while at(src, e) == 10 {
                            e += 1;
                        }
                        p = e;
                    } else if at(src, p + 2) == 123 {
                        // The grammar enters a lexer state it never defines, so nothing can follow.
                        emit!(OPEN_DIRECTIVE, p, p + 3);
                        out.types.push(ERROR);
                        out.texts.push(String::new());
                        out.starts.push(p);
                        return out;
                    } else {
                        p = until(10, p + 2);
                    }
                } else if c == 10 {
                    let mut e = p + 1;
                    while at(src, e) == 10 {
                        e += 1;
                    }
                    emit!(NL, p, e);
                } else if is_space(c) {
                    p = spaces(src, p + 1);
                } else if let Some((ty, e, skip)) = keyword(src, p, c, &mut state) {
                    if skip {
                        p = e;
                    } else {
                        emit!(ty, p, e);
                    }
                } else if (48..=57).contains(&c) && date_at(src, p) {
                    emit!(DATE, p, p + 10);
                } else if c != 58 {
                    let mut e = p + 1;
                    while e < n && src[e] != 58 && src[e] != 10 {
                        e += 1;
                    }
                    emit!(TASK_TXT, p, e);
                } else {
                    let mut e = p + 1;
                    while e < n && !matches!(src[e], 35 | 10 | 59) {
                        e += 1;
                    }
                    emit!(if e > p + 1 { TASK_DATA } else { COLON }, p, e);
                }
            }
            S_ACC_TITLE | S_ACC_DESCR => {
                let ty = if state == S_ACC_TITLE { ACC_TITLE_VALUE } else { ACC_DESCR_VALUE };
                state = S_INITIAL;
                emit!(ty, p, until(10, p));
            }
            S_ACC_DESCR_MULTILINE => {
                if c == 125 {
                    state = S_INITIAL;
                    p += 1;
                } else {
                    emit!(ACC_DESCR_MULTILINE_VALUE, p, until(125, p));
                }
            }
            S_HREF => {
                if c == 34 {
                    state = S_INITIAL;
                    p += 1;
                } else {
                    emit!(HREF, p, until(34, p));
                }
            }
            S_CALLBACKNAME => {
                if c == 40 {
                    // /\(\s*\)/y
                    let q = spaces(src, p + 1);
                    if at(src, q) == 41 {
                        state = S_INITIAL;
                        p = q + 1;
                    } else {
                        state = S_CALLBACKARGS;
                        p += 1;
                    }
                } else {
                    emit!(CALLBACKNAME, p, until(40, p));
                }
            }
            S_CALLBACKARGS => {
                if c == 41 {
                    state = S_INITIAL;
                    p += 1;
                } else {
                    emit!(CALLBACKARGS, p, until(41, p));
                }
            }
            _ => {
                if is_space(c) {
                    state = S_INITIAL;
                    p += 1;
                } else {
                    let mut e = p + 1;
                    while e < n && !is_space(src[e] as u32) {
                        e += 1;
                    }
                    emit!(CLICK, p, e);
                }
            }
        }
    }

    // Statements that start with a keyword, in the grammar's rule order. Returns (type, end,
    // whether the text is skipped rather than emitted).
    fn keyword(src: &[u16], p: usize, c: u32, state: &mut u8) -> Option<(u32, usize, bool)> {
        let ws = |q: usize| spaces(src, q);
        match c | 32 {
            97 => {
                if word(src, p, "accTitle") {
                    let q = ws(p + 8);
                    if at(src, q) == 58 {
                        *state = S_ACC_TITLE;
                        return Some((ACC_TITLE, ws(q + 1), false));
                    }
                }
                if word(src, p, "accDescr") {
                    let q = ws(p + 8);
                    if at(src, q) == 58 {
                        *state = S_ACC_DESCR;
                        return Some((ACC_DESCR, ws(q + 1), false));
                    }
                    if at(src, q) == 123 {
                        *state = S_ACC_DESCR_MULTILINE;
                        return Some((0, ws(q + 1), true));
                    }
                }
            }
            104 => {
                if word(src, p, "href") {
                    let q = p + 4;
                    let r = spaces(src, q);
                    if r > q && at(src, r) == 34 {
                        *state = S_HREF;
                        return Some((0, r + 1, true));
                    }
                }
            }
            99 => {
                if word(src, p, "call") {
                    let r = spaces(src, p + 4);
                    if r > p + 4 {
                        *state = S_CALLBACKNAME;
                        return Some((0, r, true));
                    }
                }
                if word(src, p, "click") {
                    let r = spaces(src, p + 5);
                    if r > p + 5 {
                        *state = S_CLICK;
                        return Some((0, r, true));
                    }
                }
            }
            116 => {
                // /title[^\S\r\n]*(?=[\r\n]|$)/iy
                if word(src, p, "title") {
                    let mut q = p + 5;
                    while q < src.len() && is_space(src[q] as u32) && src[q] != 10 && src[q] != 13 {
                        q += 1;
                    }
                    if q >= src.len() || src[q] == 10 || src[q] == 13 {
                        return Some((0, q, true));
                    }
                }
            }
            119 => {
                if word(src, p, "weekday") {
                    let r = spaces(src, p + 7);
                    if r > p + 7 {
                        for (k, d) in DAYS.iter().enumerate() {
                            if let Some(e) = bare(src, r, d) {
                                return Some((WEEKDAY_MONDAY + k as u32, e, false));
                            }
                        }
                    }
                }
                if word(src, p, "weekend") {
                    let r = spaces(src, p + 7);
                    if r > p + 7 {
                        if let Some(e) = bare(src, r, "friday") {
                            return Some((WEEKEND_FRIDAY, e, false));
                        }
                        if let Some(e) = bare(src, r, "saturday") {
                            return Some((WEEKEND_SATURDAY, e, false));
                        }
                    }
                }
            }
            _ => {}
        }
        let rules: &[(&str, u32, Option<&[u16]>)] = match c | 32 {
            97 => &[("axisFormat", AXIS_FORMAT, Some(&HASH_NL_SEMI)), ("accDescription", ACC_DESCRIPTION, Some(&HASH_NL_SEMI))],
            103 => &[("gantt", GANTT, None)],
            100 => &[("dateFormat", DATE_FORMAT, Some(&HASH_NL_SEMI))],
            105 => &[("inclusiveEndDates", INCLUSIVE_END_DATES, None), ("includes", INCLUDES, Some(&HASH_NL_SEMI))],
            116 => &[("topAxis", TOP_AXIS, None), ("tickInterval", TICK_INTERVAL, Some(&HASH_NL_SEMI)), ("todayMarker", TODAY_MARKER, Some(&NL_SEMI)), ("title", TITLE, Some(&NL_ONLY))],
            101 => &[("excludes", EXCLUDES, Some(&HASH_NL_SEMI))],
            115 => &[("section", SECTION, Some(&NL_ONLY))],
            _ => &[],
        };
        for &(w, ty, stop) in rules {
            let end = match stop {
                None => bare(src, p, w),
                // `title[^\S\r\n][^\n]+`: the separator is whitespace other than a line break.
                Some(stop) if ty == TITLE => {
                    if word(src, p, w) && is_space(at(src, p + 5)) && at(src, p + 5) != 10 && at(src, p + 5) != 13 {
                        let mut e = p + 6;
                        while e < src.len() && !stop.contains(&src[e]) {
                            e += 1;
                        }
                        (e > p + 6).then_some(e)
                    } else {
                        None
                    }
                }
                Some(stop) => statement(src, p, w, stop),
            };
            if let Some(e) = end {
                return Some((ty, e, false));
            }
        }
        None
    }

    // /\d\d\d\d-\d\d-\d\d\b/
    fn date_at(src: &[u16], p: usize) -> bool {
        let d = |k: usize| (48..=57).contains(&at(src, p + k));
        d(0) && d(1) && d(2) && d(3) && at(src, p + 4) == 45 && d(5) && d(6) && at(src, p + 7) == 45 && d(8) && d(9) && !is_w(at(src, p + 10))
    }

    out.types.push(END);
    out.texts.push(String::new());
    out.starts.push(n);
    out
}
