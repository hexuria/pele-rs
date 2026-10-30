// Port of src/diagrams/treemap/parser.ts.

use crate::diagrams::common::tokens::*;
use crate::errors::PeleError;
use crate::js;

const ACC_DESCR: i32 = 0;
const ACC_TITLE: i32 = 1;
const TITLE: i32 = 2;
const KEYWORD: i32 = 3;
const CLASS_DEF: i32 = 4;
const STYLE_SEPARATOR: i32 = 5;
const SEPARATOR: i32 = 6;
const COMMA: i32 = 7;
const INDENTATION: i32 = 8;
const ID: i32 = 12;
const NUMBER: i32 = 13;
const STRING: i32 = 14;

fn at(src: &[u16], i: usize) -> u32 {
    src.get(i).map_or(u32::MAX, |&c| c as u32)
}

fn starts(src: &[u16], p: usize, word: &str) -> bool {
    word.encode_utf16().enumerate().all(|(i, u)| src.get(p + i) == Some(&u))
}

fn is_w(c: u32) -> bool {
    c < 128 && js::is_word(c as u8 as char)
}

fn is_alpha_(c: u32) -> bool {
    c < 128 && ((c as u8).is_ascii_alphabetic() || c == 95)
}

// `classDef\s+([a-zA-Z_][a-zA-Z0-9_]+)(?:\s+([^;\r\n]*))?(?:;)?`; the end of the match, and
// the name and style text in it.
fn class_def(src: &[u16], p: usize) -> Option<(usize, String, Option<String>)> {
    if !starts(src, p, "classDef") {
        return None;
    }
    let mut q = p + 8;
    let s = q;
    while q < src.len() && js::is_space_u(src[q] as u32) {
        q += 1;
    }
    if q == s || !is_alpha_(at(src, q)) || !is_w(at(src, q + 1)) {
        return None;
    }
    let name_from = q;
    q += 2;
    while is_w(at(src, q)) {
        q += 1;
    }
    let name = js::from16(&src[name_from..q]);
    let mut style = None;
    let mut r = q;
    while r < src.len() && js::is_space_u(src[r] as u32) {
        r += 1;
    }
    if r > q {
        let from = r;
        while r < src.len() && !matches!(src[r], 59 | 13 | 10) {
            r += 1;
        }
        style = Some(js::from16(&src[from..r]));
        q = r;
    }
    if at(src, q) == 59 {
        q += 1;
    }
    Some((q, name, style))
}

pub fn treemap_tokens() -> Vec<TokenType> {
    vec![
        acc_descr(),
        acc_title(),
        title_before(|src, q| is_w(at(src, q))),
        TokenType::re("TREEMAP_KEYWORD", None, |src, p| {
            if starts(src, p, "treemap-beta") {
                Some(p + 12)
            } else if starts(src, p, "treemap") {
                Some(p + 7)
            } else {
                None
            }
        }),
        TokenType::re("CLASS_DEF", None, |src, p| class_def(src, p).map(|m| m.0)),
        TokenType::re("STYLE_SEPARATOR", None, |src, p| starts(src, p, ":::").then_some(p + 3)),
        TokenType::re("SEPARATOR", None, |src, p| (at(src, p) == 58).then_some(p + 1)),
        TokenType::re("COMMA", None, |src, p| (at(src, p) == 44).then_some(p + 1)),
        TokenType::re("INDENTATION", None, |src, p| {
            let q = blanks(src, p);
            (q > p).then_some(q)
        }),
        TokenType::re("WS", None, |src, p| {
            let q = blanks(src, p);
            (q > p).then_some(q)
        })
        .hidden(),
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
        TokenType::re("ID2", None, |src, p| {
            if !is_alpha_(at(src, p)) {
                return None;
            }
            let mut q = p + 1;
            while is_w(at(src, q)) {
                q += 1;
            }
            Some(q)
        }),
        TokenType::re("NUMBER2", None, |src, p| {
            let mut q = p;
            while q < src.len() && matches!(src[q], 48..=57 | 95 | 46 | 44) {
                q += 1;
            }
            (q > p).then_some(q)
        }),
        TokenType::re("STRING2", None, |src, p| {
            let quote = at(src, p);
            if quote != 34 && quote != 39 {
                return None;
            }
            src[p + 1..].iter().position(|&c| c as u32 == quote).map(|k| p + 1 + k + 1)
        }),
    ]
}

pub enum Row {
    Node { indent: Option<usize>, name: String, leaf: bool, value: f64, class_selector: Option<String> },
    ClassDef { indent: Option<usize>, class_name: String, style_text: Option<String> },
}

#[derive(Default)]
pub struct TreemapAst {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub rows: Vec<Row>,
}

pub fn parse_treemap(src: &[u16]) -> Result<TreemapAst, PeleError> {
    let types = treemap_tokens();
    let tokens = tokenize(src, &types, "treemap")?;
    let mut r = Reader::new(&tokens, &types, "treemap");
    let kinds = &tokens.kinds;
    let mut ast = TreemapAst::default();

    r.expect(KEYWORD)?;
    while r.kind() != -1 {
        let kind = r.kind();
        if kind == ACC_DESCR {
            ast.acc_descr = Some(acc_descr_value(&r.take()));
            continue;
        }
        if kind == ACC_TITLE {
            ast.acc_title = Some(acc_title_value(&r.take()));
            continue;
        }
        if kind == TITLE {
            ast.title = Some(title_value(&r.take()));
            continue;
        }
        if kind != INDENTATION && kind != STRING && kind != CLASS_DEF {
            return Err(r.fail("a node, a class definition, or a title"));
        }
        let indent = if kind == INDENTATION { Some(js::len16(&r.take())) } else { None };
        if r.kind() == CLASS_DEF {
            let text = js::u16s(&r.take());
            let (_, name, style) = class_def(&text, 0).unwrap();
            ast.rows.push(Row::ClassDef { indent, class_name: name, style_text: style.filter(|s| !s.is_empty()) });
            continue;
        }
        let text = r.expect(STRING)?;
        let chars: Vec<char> = text.chars().collect();
        let name: String = chars[1..chars.len() - 1].iter().collect();
        // A leaf is a name followed by a colon or comma, with or without whitespace before it.
        let after = if kinds[r.i] == INDENTATION { r.i + 1 } else { r.i };
        let (leaf, value) = if kinds[after] == SEPARATOR || kinds[after] == COMMA {
            r.i = after + 1;
            r.accept(INDENTATION);
            (true, js::parse_float(&r.expect(NUMBER)?.replace(',', "")))
        } else {
            (false, 0.0)
        };
        let class_selector = if r.accept(STYLE_SEPARATOR) { Some(r.expect(ID)?) } else { None };
        ast.rows.push(Row::Node { indent, name, leaf, value, class_selector });
    }
    Ok(ast)
}
