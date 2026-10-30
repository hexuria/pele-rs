// Port of src/diagrams/common/outline.ts: the indented outline grammar that Mermaid's mindmap
// and kanban diagrams share. Kanban adds `@{ ... }` metadata after a node and keeps `@` out of
// node ids.

use crate::errors::{syntax_error, PeleError};
use crate::js;
use crate::util::chars::is_word;

pub mod t {
    pub const END: u32 = 0;
    pub const ERROR: u32 = 1;
    pub const SPACELINE: u32 = 2;
    pub const NL: u32 = 3;
    pub const EOF: u32 = 4;
    pub const KEYWORD: u32 = 5;
    pub const SPACELIST: u32 = 6;
    pub const ICON: u32 = 7;
    pub const CLASS: u32 = 8;
    pub const NODE_DSTART: u32 = 9;
    pub const NODE_ID: u32 = 10;
    pub const NODE_DESCR: u32 = 11;
    pub const NODE_DEND: u32 = 12;
    pub const SHAPE_DATA: u32 = 13;
}

// As the generated parser prints them. It has no name for the end of input and prints its number.
const NAMES: [&str; 14] = ["1", "INVALID", "SPACELINE", "NL", "EOF", "", "SPACELIST", "ICON", "CLASS", "NODE_DSTART", "NODE_ID", "NODE_DESCR", "NODE_DEND", "SHAPE_DATA"];

pub fn token_name(ty: u32, kanban: bool) -> &'static str {
    if ty == t::KEYWORD {
        if kanban {
            "KANBAN"
        } else {
            "MINDMAP"
        }
    } else {
        NAMES[ty as usize]
    }
}

#[derive(Default, Debug)]
pub struct Tokens {
    pub types: Vec<u32>,
    pub texts: Vec<String>,
    pub starts: Vec<usize>,
}

pub mod node_type {
    pub const DEFAULT: u32 = 0;
    pub const NO_BORDER: u32 = 0;
    pub const ROUNDED_RECT: u32 = 1;
    pub const RECT: u32 = 2;
    pub const CIRCLE: u32 = 3;
    pub const CLOUD: u32 = 4;
    pub const BANG: u32 = 5;
    pub const HEXAGON: u32 = 6;
}

/// Any opening delimiter pairs with any closing one, as in Mermaid.
pub fn get_type(start: &str, end: &str) -> u32 {
    match start {
        "[" => node_type::RECT,
        "(" => {
            if end == ")" {
                node_type::ROUNDED_RECT
            } else {
                node_type::CLOUD
            }
        }
        "((" => node_type::CIRCLE,
        ")" => node_type::CLOUD,
        "))" => node_type::BANG,
        "{{" => node_type::HEXAGON,
        _ => node_type::DEFAULT,
    }
}

pub trait OutlineDb {
    fn get_type(&mut self, start: &str, end: &str) -> u32 {
        get_type(start, end)
    }
    fn add_node(&mut self, level: usize, id: &str, descr: &str, ty: u32, shape_data: Option<&str>) -> Result<(), PeleError>;
    /// `{ icon }` or `{ class }`: exactly one is set.
    fn decorate_node(&mut self, icon: Option<&str>, class: Option<&str>) -> Result<(), PeleError>;
}

#[derive(Clone, Copy, PartialEq)]
enum S {
    Initial,
    Class,
    Icon,
    Node,
    Nstr,
    Nstr2,
    Data,
    DataStr,
}

fn at(src: &[u16], i: usize) -> u32 {
    src.get(i).map_or(u32::MAX, |&c| c as u32)
}

fn is_lt(c: u16) -> bool {
    matches!(c, 10 | 13 | 0x2028 | 0x2029)
}

// Jison's case-insensitive flag folds ASCII letters only.
fn letters(src: &[u16], p: usize, word: &str) -> bool {
    word.bytes().enumerate().all(|(k, w)| at(src, p + k) != u32::MAX && (at(src, p + k) | 32) == w as u32)
}

// Sticky matchers: the end of a non-empty match, or None.
fn run(src: &[u16], from: usize, ok: impl Fn(u16) -> bool) -> Option<usize> {
    let mut p = from;
    while p < src.len() && ok(src[p]) {
        p += 1;
    }
    (p > from).then_some(p)
}

#[allow(unused_assignments)]
pub fn tokenize(src: &[u16], kanban: bool) -> Tokens {
    let n = src.len();
    let mut out = Tokens::default();
    let keyword = if kanban { "kanban" } else { "mindmap" };
    let re_id = |c: u16| !matches!(c, 40 | 91 | 10 | 41 | 123 | 125) && !(kanban && c == 64);
    let re_space = |c: u16| js::is_space_u(c as u32);
    let re_line = |c: u16| !is_lt(c);
    let re_descr = |c: u16| !matches!(c, 41 | 93 | 40 | 125);
    let re_md = |c: u16| c != 96 && c != 34;
    let re_data = |c: u16| !matches!(c, 125 | 94 | 34);
    let mut state = S::Initial;
    let mut p = 0;
    let mut next_break: isize = -1;
    let up_to = |p: usize, ch: u16| src[p..].iter().position(|&c| c == ch).map_or(n, |k| p + k);

    macro_rules! put {
        ($ty:expr, $text:expr, $end:expr) => {{
            let end = $end;
            out.types.push($ty);
            out.texts.push($text);
            out.starts.push(p);
            p = end;
        }};
    }
    macro_rules! emit {
        ($ty:expr, $end:expr, $next:expr) => {{
            let end = $end;
            put!($ty, js::from16(&src[p..end]), end);
            state = $next;
        }};
    }

    while p < n {
        let c = src[p] as u32;
        let d = at(src, p + 1);
        match state {
            S::Initial => {
                let w = if c <= 32 || c >= 160 { run(src, p, re_space).unwrap_or(p).max(p) } else { p };
                if kanban && c == 64 && d == 123 {
                    put!(t::SHAPE_DATA, String::new(), p + 2);
                    state = S::Data;
                } else if at(src, w) == 37 && at(src, w + 1) == 37 {
                    emit!(t::SPACELINE, run(src, w, re_line).unwrap_or(w), S::Initial);
                } else if w > p {
                    if next_break < p as isize {
                        next_break = up_to(p, 10) as isize;
                    }
                    let last_nl = src[..w].iter().rposition(|&c| c == 10);
                    if next_break >= w as isize {
                        emit!(t::SPACELIST, w, S::Initial);
                    } else if let Some(e) = last_nl.filter(|&e| e > p) {
                        emit!(t::SPACELINE, e + 1, S::Initial);
                    } else {
                        emit!(t::NL, p + 1, S::Initial);
                    }
                } else if letters(src, p, keyword) && !is_word(at(src, p + keyword.len())) {
                    emit!(t::KEYWORD, p + keyword.len(), S::Initial);
                } else if c == 58 && d == 58 && at(src, p + 2) == 58 {
                    p += 3;
                    state = S::Class;
                } else if c == 58 && d == 58 && letters(src, p + 2, "icon") && at(src, p + 6) == 40 {
                    p += 7;
                    state = S::Icon;
                } else if (c == 45 && d == 41) || (c == 40 && (d == 45 || d == 40)) || ((c == 41 || c == 123) && d == c) {
                    emit!(t::NODE_DSTART, p + 2, S::Node);
                } else if c == 41 || c == 40 || c == 91 {
                    emit!(t::NODE_DSTART, p + 1, S::Node);
                } else if let Some(e) = run(src, p, re_id) {
                    emit!(t::NODE_ID, e, S::Initial);
                } else {
                    break;
                }
            }
            S::Node => {
                if c == 34 {
                    state = if d == 96 { S::Nstr2 } else { S::Nstr };
                    p += if d == 96 { 2 } else { 1 };
                } else if c == 41 || c == 40 {
                    emit!(t::NODE_DEND, if d == c || (c == 40 && d == 45) { p + 2 } else { p + 1 }, S::Initial);
                } else if c == 93 {
                    emit!(t::NODE_DEND, p + 1, S::Initial);
                } else if (c == 125 && d == 125) || (c == 45 && d == 41) {
                    emit!(t::NODE_DEND, p + 2, S::Initial);
                } else {
                    // A lone closing brace takes the rest of its line as text. The regular
                    // expression fails on a line break, and lastIndex then reads as -1.
                    let e = if c == 125 { run(src, p, re_line) } else { run(src, p, re_descr) };
                    match e {
                        Some(e) => emit!(t::NODE_DESCR, e, S::Node),
                        None => break,
                    }
                }
            }
            S::Nstr => {
                if c == 34 {
                    p += 1;
                    state = S::Node;
                } else {
                    emit!(t::NODE_DESCR, up_to(p, 34), S::Nstr);
                }
            }
            S::Nstr2 => {
                if c == 96 && d == 34 {
                    p += 2;
                    state = S::Node;
                } else if let Some(e) = run(src, p, re_md) {
                    emit!(t::NODE_DESCR, e, S::Nstr2);
                } else {
                    break;
                }
            }
            S::Class => {
                if let Some(e) = run(src, p, re_line) {
                    emit!(t::CLASS, e, S::Initial);
                } else if c == 10 {
                    p += 1;
                    state = S::Initial;
                } else {
                    break;
                }
            }
            S::Icon => {
                if c == 41 {
                    p += 1;
                    state = S::Initial;
                } else {
                    emit!(t::ICON, up_to(p, 41), S::Icon);
                }
            }
            S::Data => {
                if c == 34 {
                    emit!(t::SHAPE_DATA, p + 1, S::DataStr);
                } else if c == 125 {
                    p += 1;
                    state = S::Initial;
                } else if let Some(e) = run(src, p, re_data) {
                    emit!(t::SHAPE_DATA, e, S::Data);
                } else {
                    break;
                }
            }
            S::DataStr => {
                if c == 34 {
                    emit!(t::SHAPE_DATA, p + 1, S::Data);
                } else {
                    let e = up_to(p, 34);
                    // .replace(/\n\s*/g, '<br/>')
                    let mut text = Vec::new();
                    let mut i = p;
                    while i < e {
                        if src[i] == 10 {
                            i += 1;
                            while i < e && js::is_space_u(src[i] as u32) {
                                i += 1;
                            }
                            text.extend("<br/>".encode_utf16());
                        } else {
                            text.push(src[i]);
                            i += 1;
                        }
                    }
                    put!(t::SHAPE_DATA, js::from16(&text), e);
                }
            }
        }
    }

    if p < n {
        put!(t::ERROR, String::new(), p);
    } else {
        if state == S::Initial {
            put!(t::EOF, String::new(), n);
        }
        put!(t::END, String::new(), n);
    }
    out
}

const STOP: &str = "'SPACELINE', 'NL', 'EOF'";

fn is_start(ty: u32) -> bool {
    matches!(ty, t::SPACELINE | t::SPACELIST | t::ICON | t::CLASS | t::NODE_DSTART | t::NODE_ID)
}

fn is_stop(ty: u32) -> bool {
    ty == t::SPACELINE || ty == t::NL || ty == t::EOF
}

/// Makes the calls on `db` that the grammar's actions make, in the same order, and reports
/// errors in the state where the generated parser would.
pub fn parse_outline(src: &[u16], db: &mut dyn OutlineDb, kanban: bool) -> Result<(), PeleError> {
    let diagram = if kanban { "kanban" } else { "mindmap" };
    let Tokens { types, texts, starts } = tokenize(src, kanban);
    let data = if kanban { ", 'SHAPE_DATA'" } else { "" };
    let mut i = 0;
    let fail = |i: usize, expected: &str| -> PeleError {
        let ty = types[i];
        if ty == t::ERROR {
            return syntax_error(diagram, src, starts[i], "", true);
        }
        syntax_error(diagram, src, starts[i], &format!("Expecting {}, got '{}'", expected, token_name(ty, kanban)), false)
    };

    let keyword = format!("'{}'", token_name(t::KEYWORD, kanban));
    if types[i] == t::SPACELINE {
        i += 1;
        while types[i] == t::SPACELINE || types[i] == t::NL {
            i += 1;
        }
        if types[i] != t::KEYWORD {
            return Err(fail(i, &format!("'SPACELINE', 'NL', {}", keyword)));
        }
    } else if types[i] != t::KEYWORD {
        return Err(fail(i, &format!("'SPACELINE', {}", keyword)));
    }
    i += 1;
    let mut first = "'SPACELINE', 'NL', 'SPACELIST', 'ICON', 'CLASS', 'NODE_DSTART', 'NODE_ID'";
    if types[i] == t::NL {
        i += 1;
        first = "'SPACELINE', 'SPACELIST', 'ICON', 'CLASS', 'NODE_DSTART', 'NODE_ID'";
    }

    loop {
        let mut ty = types[i];
        let mut level = 0;
        let indented = ty == t::SPACELIST;
        if indented {
            level = js::len16(&texts[i]);
            i += 1;
            ty = types[i];
        }
        if ty == t::NODE_ID || ty == t::NODE_DSTART {
            let mut id: Option<String> = None;
            let descr: String;
            let mut node_type = 0;
            if ty == t::NODE_ID {
                id = Some(texts[i].clone());
                i += 1;
            }
            if types[i] == t::NODE_DSTART {
                let start = texts[i].clone();
                i += 1;
                if types[i] != t::NODE_DESCR {
                    return Err(fail(i, "'NODE_DESCR'"));
                }
                descr = texts[i].clone();
                i += 1;
                if types[i] != t::NODE_DEND {
                    return Err(fail(i, "'NODE_DEND'"));
                }
                let end = texts[i].clone();
                i += 1;
                if !is_stop(types[i]) && types[i] != t::SHAPE_DATA {
                    return Err(fail(i, &format!("{}{}", STOP, data)));
                }
                node_type = db.get_type(&start, &end);
            } else {
                if !is_stop(types[i]) && types[i] != t::SHAPE_DATA {
                    return Err(fail(i, &format!("{}, 'NODE_DSTART'{}", STOP, data)));
                }
                descr = id.clone().unwrap();
            }
            let id = id.unwrap_or_else(|| descr.clone());
            if types[i] == t::SHAPE_DATA {
                let mut shape_data = texts[i].clone();
                i += 1;
                while types[i] == t::SHAPE_DATA {
                    shape_data.push_str(&texts[i]);
                    i += 1;
                }
                if !is_stop(types[i]) {
                    return Err(fail(i, &format!("{}{}", STOP, data)));
                }
                db.add_node(level, &id, &descr, node_type, Some(&shape_data))?;
            } else {
                db.add_node(level, &id, &descr, node_type, None)?;
            }
        } else if ty == t::ICON || ty == t::CLASS {
            let text = texts[i].clone();
            i += 1;
            if !is_stop(types[i]) {
                return Err(fail(i, STOP));
            }
            if ty == t::ICON {
                db.decorate_node(Some(&text), None)?;
            } else {
                db.decorate_node(None, Some(&text))?;
            }
        } else if indented {
            if !is_stop(ty) {
                return Err(fail(i, &format!("{}, 'ICON', 'CLASS', 'NODE_DSTART', 'NODE_ID'", STOP)));
            }
        } else if ty == t::SPACELINE {
            i += 1;
            if !is_stop(types[i]) {
                return Err(fail(i, STOP));
            }
        } else {
            return Err(fail(i, first));
        }

        i += 1;
        while types[i] == t::NL || types[i] == t::EOF {
            i += 1;
        }
        if types[i] == t::END {
            return Ok(());
        }
        if !is_start(types[i]) {
            return Err(fail(i, &format!("{}, 'SPACELIST', 'ICON', 'CLASS', 'NODE_DSTART', 'NODE_ID'", STOP)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Db(Vec<String>);
    impl OutlineDb for Db {
        fn add_node(&mut self, level: usize, id: &str, descr: &str, ty: u32, shape_data: Option<&str>) -> Result<(), PeleError> {
            self.0.push(format!("{level}:{id}:{descr}:{ty}:{}", shape_data.unwrap_or("")));
            Ok(())
        }
        fn decorate_node(&mut self, icon: Option<&str>, class: Option<&str>) -> Result<(), PeleError> {
            self.0.push(format!("deco {:?} {:?}", icon, class));
            Ok(())
        }
    }

    #[test]
    fn an_outline_makes_the_grammars_calls() {
        let mut db = Db::default();
        parse_outline(&js::u16s("mindmap\n  root((Root))\n    A[One]\n    ::icon(fa fa-book)\n    B\n"), &mut db, false).unwrap();
        assert_eq!(db.0, vec!["2:root:Root:3:", "4:A:One:2:", "deco Some(\"fa fa-book\") None", "4:B:B:0:"]);
        let mut db = Db::default();
        parse_outline(&js::u16s("kanban\n  todo[Todo]\n    t1[Task]@{ priority: 'High' }\n"), &mut db, true).unwrap();
        assert_eq!(db.0, vec!["2:todo:Todo:2:", "4:t1:Task:2: priority: 'High' "]);
        let e = parse_outline(&js::u16s("graph\n"), &mut Db::default(), false).unwrap_err();
        assert!(e.message.contains("Expecting 'SPACELINE', 'MINDMAP'"), "{}", e.message);
    }
}
