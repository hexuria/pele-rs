// Port of src/diagrams/treeview/parser.ts.

use crate::diagrams::common::acc_descr::acc_descr;
use crate::diagrams::common::tokens::*;
use crate::errors::PeleError;
use crate::js;
use std::rc::Rc;

const KEYWORD: i32 = 0;
const ACC_DESCR: i32 = 1;
const ACC_TITLE: i32 = 2;
const TITLE: i32 = 3;
const CLASS_ANNOTATION: i32 = 4;
const ICON_ANNOTATION: i32 = 5;
const DESC_ANNOTATION: i32 = 6;
const INDENTATION: i32 = 7;
const QUOTED_NAME: i32 = 8;
const BARE_NAME: i32 = 12;

fn at(src: &[u16], i: usize) -> u32 {
    src.get(i).map_or(u32::MAX, |&c| c as u32)
}

fn is_blank(c: u32) -> bool {
    c == 32 || c == 9
}

fn starts(src: &[u16], p: usize, word: &str) -> bool {
    word.encode_utf16().enumerate().all(|(i, u)| src.get(p + i) == Some(&u))
}

fn is_w_or_dash(c: u32) -> bool {
    c < 128 && (js::is_word(c as u8 as char) || c == 45)
}

fn is_name_start(c: u32) -> bool {
    (65..=90).contains(&c) || (97..=122).contains(&c) || c == 95
}

fn annotation_at(src: &[u16], a: usize) -> bool {
    if starts(src, a, "##") || starts(src, a, "icon(") {
        return true;
    }
    if !starts(src, a, ":::") {
        return false;
    }
    let mut i = a + 3;
    while is_blank(at(src, i)) {
        i += 1;
    }
    is_name_start(at(src, i))
}

// The pattern looks ahead from every blank for an annotation, which rescans long runs of
// blanks. This looks once per run.
fn match_bare_name(src: &[u16], a: usize, _previous: i32) -> Option<usize> {
    let first = at(src, a);
    if is_blank(first) || first == 10 || first == 13 || first == 34 || first == 39 || first == u32::MAX {
        return None;
    }
    if starts(src, a, ":::") || starts(src, a, "icon(") || starts(src, a, "##") {
        return None;
    }
    let n = src.len();
    let mut i = a + 1;
    while i < n {
        let c = src[i] as u32;
        if c == 10 || c == 13 {
            break;
        }
        if is_blank(c) {
            let mut j = i + 1;
            while is_blank(at(src, j)) {
                j += 1;
            }
            if annotation_at(src, j) {
                break;
            }
            i = j;
        } else {
            i += 1;
        }
    }
    Some(i)
}

fn blanks1(src: &[u16], p: usize) -> Option<usize> {
    let q = blanks(src, p);
    (q > p).then_some(q)
}

pub fn treeview_tokens() -> Vec<TokenType> {
    let bare = || {
        let mut t = TokenType::re("BARE_NAME", None, |_, _| None);
        t.match_ = Some(Rc::new(match_bare_name));
        t
    };
    let mut kw = keyword("treeView-beta");
    kw.longer = vec![BARE_NAME as usize];
    vec![
        kw,
        acc_descr("ACC_DESCR"),
        acc_title(),
        title_before(|src, q| !starts(src, q, "%%") && q < src.len() && src[q] != 10 && src[q] != 13),
        TokenType::re("CLASS_ANNOTATION", None, |src, p| {
            let q = blanks1(src, p)?;
            if !starts(src, q, ":::") {
                return None;
            }
            let q = blanks(src, q + 3);
            if !is_name_start(at(src, q)) {
                return None;
            }
            let mut q = q + 1;
            while is_w_or_dash(at(src, q)) {
                q += 1;
            }
            Some(q)
        }),
        TokenType::re("ICON_ANNOTATION", None, |src, p| {
            let q = blanks1(src, p)?;
            if !starts(src, q, "icon(") {
                return None;
            }
            let mut q = q + 5;
            while is_w_or_dash(at(src, q)) {
                q += 1;
            }
            if at(src, q) == 58 && is_w_or_dash(at(src, q + 1)) {
                q += 1;
                while is_w_or_dash(at(src, q)) {
                    q += 1;
                }
            }
            (at(src, q) == 41).then_some(q + 1)
        }),
        TokenType::re("DESC_ANNOTATION", None, |src, p| {
            let q = blanks1(src, p)?;
            starts(src, q, "##").then(|| rest_of_line(src, q + 2))
        }),
        TokenType::re("INDENTATION", None, blanks1),
        TokenType::re("QUOTED_NAME", None, |src, p| {
            let quote = at(src, p);
            if quote != 34 && quote != 39 {
                return None;
            }
            src[p + 1..].iter().position(|&c| c as u32 == quote).map(|k| p + 1 + k + 1)
        }),
        TokenType::re("WS", None, blanks1).hidden(),
        TokenType::re("ML_COMMENT", None, |src, p| {
            if !starts(src, p, "%%") {
                return None;
            }
            let mut q = p + 2;
            while q < src.len() && src[q] != 10 {
                q += 1;
            }
            Some(q)
        })
        .hidden(),
        TokenType::re("NL", None, newline).hidden(),
        bare(),
    ]
}

#[derive(Default)]
pub struct TreeNodeAst {
    pub indent: Option<usize>,
    pub name: String,
    pub class_annotation: Option<String>,
    pub icon_annotation: Option<String>,
    pub desc_annotation: Option<String>,
}

#[derive(Default)]
pub struct TreeViewAst {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub nodes: Vec<TreeNodeAst>,
}

fn trim_blanks_end(text: &str) -> String {
    text.trim_end_matches([' ', '\t']).to_string()
}

// `substring(k)` / `substring(k, len - 1)` on ASCII prefixes.
fn skip(text: &str, k: usize) -> &str {
    &text[k.min(text.len())..]
}

pub fn parse_tree_view(src: &[u16]) -> Result<TreeViewAst, PeleError> {
    let types = treeview_tokens();
    let tokens = tokenize(src, &types, "treeView")?;
    let mut r = Reader::new(&tokens, &types, "treeView");
    let mut ast = TreeViewAst::default();
    r.expect(KEYWORD)?;
    loop {
        match r.kind() {
            ACC_DESCR => ast.acc_descr = Some(acc_descr_value(&r.take())),
            ACC_TITLE => ast.acc_title = Some(acc_title_value(&r.take())),
            TITLE => ast.title = Some(title_value(&r.take())),
            _ => break,
        }
    }
    while r.kind() != -1 {
        let mut node = TreeNodeAst::default();
        if r.kind() == INDENTATION {
            node.indent = Some(js::len16(&r.take()));
        }
        if r.kind() == QUOTED_NAME {
            let text = r.take();
            let chars: Vec<char> = text.chars().collect();
            node.name = chars[1..chars.len() - 1].iter().collect();
        } else if r.kind() == BARE_NAME {
            node.name = trim_blanks_end(&r.take());
        } else {
            return Err(r.fail("a name"));
        }
        loop {
            match r.kind() {
                CLASS_ANNOTATION => node.class_annotation = Some(js::trim(skip(js::trim(&r.take()), 3)).to_string()),
                ICON_ANNOTATION => {
                    let t = r.take();
                    let trimmed = js::trim(&t);
                    node.icon_annotation = Some(trimmed[5..trimmed.len() - 1].to_string());
                }
                DESC_ANNOTATION => node.desc_annotation = Some(js::trim(skip(js::trim(&r.take()), 2)).to_string()),
                _ => break,
            }
        }
        ast.nodes.push(node);
    }
    Ok(ast)
}
