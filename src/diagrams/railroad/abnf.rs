// Port of src/diagrams/railroad/abnf.ts: `railroad-abnf-beta`, ABNF as in RFC 5234, with each
// rule closed by a semicolon.

use super::common::{acc_descr, acc_title, hidden_tokens, literal, opening, parse_grammar, title, to_rules, Grammar, Nodes, Parts};
use crate::diagrams::common::tokens::{Reader, TokenType};
use crate::errors::PeleError;
use crate::js;

mod t {
    pub const EQUALS: i32 = 1;
    pub const SEMICOLON: i32 = 2;
    pub const SLASH: i32 = 3;
    pub const GROUP: i32 = 4;
    pub const OPTION: i32 = 6;
    pub const TITLE: i32 = 8;
    pub const NAME: i32 = 11;
    pub const STRING: i32 = 12;
    pub const NUMBER: i32 = 13;
    pub const REPEAT: i32 = 14;
    pub const EXACT: i32 = 15;
}

// Part tags.
const ALTERNATION: u8 = 0;
const CONCATENATION: u8 = 1;
const ELEMENT: u8 = 2;
const STRING_LITERAL: u8 = 3;
const NUM_VAL: u8 = 4;
const RULE_NAME: u8 = 5;
const GROUP: u8 = 6;
const OPTIONAL_GROUP: u8 = 7;

fn at(src: &[u16], i: usize) -> u32 {
    src.get(i).map_or(u32::MAX, |&c| c as u32)
}

fn is_digit(c: u32) -> bool {
    (48..=57).contains(&c)
}

fn is_hex(c: u32) -> bool {
    is_digit(c) || (65..=70).contains(&c) || (97..=102).contains(&c)
}

fn is_letter(c: u32) -> bool {
    (65..=90).contains(&c) || (97..=122).contains(&c)
}

fn hex_run(src: &[u16], mut i: usize) -> usize {
    while is_hex(at(src, i)) {
        i += 1;
    }
    i
}

fn tokens() -> Vec<TokenType> {
    const NAME: usize = 11;
    const COMMENT: usize = 20;
    let name = TokenType::re("ABNF_RULENAME", Some(super::common::LETTERS), |src, p| {
        if !is_letter(at(src, p)) {
            return None;
        }
        let mut e = p + 1;
        while is_letter(at(src, e)) || is_digit(at(src, e)) || at(src, e) == 45 {
            e += 1;
        }
        Some(e)
    });
    let mut out = vec![opening("railroad-abnf-beta", NAME)];
    out.push(literal("=", &[]));
    // A semicolon closes a rule only at the end of its line; with anything after it, it starts a comment.
    out.push(literal(";", &[COMMENT]));
    for s in ["/", "(", ")", "[", "]"] {
        out.push(literal(s, &[]));
    }
    out.push(title(|c| is_letter(c) || is_digit(c) || c == 45));
    out.push(acc_title());
    out.push(acc_descr());
    out.push(name);
    out.push(TokenType::re("ABNF_STRING", Some("\""), |src, p| {
        if at(src, p) != 34 {
            return None;
        }
        (p + 1..src.len()).find(|&k| src[k] == 34).map(|k| k + 1)
    }));
    // /%[xXdDbB][0-9A-Fa-f]+(?:-[0-9A-Fa-f]+|\.[0-9A-Fa-f]+)*/y
    out.push(TokenType::re("ABNF_NUMVAL", Some("%"), |src, p| {
        if at(src, p) != 37 || !matches!(at(src, p + 1), 120 | 88 | 100 | 68 | 98 | 66) {
            return None;
        }
        let mut e = hex_run(src, p + 2);
        if e == p + 2 {
            return None;
        }
        while matches!(at(src, e), 45 | 46) && is_hex(at(src, e + 1)) {
            e = hex_run(src, e + 1);
        }
        Some(e)
    }));
    out.push(TokenType::re("ABNF_REPEAT", Some("0123456789*"), |src, p| {
        let mut e = p;
        while is_digit(at(src, e)) {
            e += 1;
        }
        if at(src, e) != 42 {
            return None;
        }
        e += 1;
        while is_digit(at(src, e)) {
            e += 1;
        }
        Some(e)
    }));
    out.push(TokenType::re("ABNF_EXACT_REPEAT", Some("0123456789"), |src, p| {
        let mut e = p;
        while is_digit(at(src, e)) {
            e += 1;
        }
        (e > p).then_some(e)
    }));
    out.extend(hidden_tokens("ABNF"));
    out.push(
        TokenType::re("ABNF_COMMENT", Some(";"), |src, p| {
            if at(src, p) != 59 {
                return None;
            }
            let mut e = p + 1;
            while e < src.len() && src[e] != 10 && src[e] != 13 {
                e += 1;
            }
            Some(e)
        })
        .hidden(),
    );
    out
}

const CLOSE_NAMES: [&str; 2] = [")", "]"];

// One open bracket, or the rule itself at the bottom of the stack.
struct Frame {
    alternation: usize,
    concatenation: usize,
    // The bracket's node, its closing token, and the repeat written before it.
    node: Option<usize>,
    close: i32,
    repeat: Option<String>,
}

fn frame(parts: &mut Parts, node: Option<usize>, alternation: usize, close: i32, repeat: Option<String>) -> Frame {
    let concatenation = parts.add(CONCATENATION, String::new());
    parts.0[alternation].kids.push(concatenation);
    Frame { alternation, concatenation, node, close, repeat }
}

fn alternation(r: &mut Reader, parts: &mut Parts) -> Result<usize, PeleError> {
    let mut stack: Vec<Frame> = Vec::new();
    let root = parts.add(ALTERNATION, String::new());
    let mut top = frame(parts, None, root, -2, None);
    loop {
        let mut repeat = if r.kind() == t::REPEAT || r.kind() == t::EXACT { Some(r.take()) } else { None };
        let kind = r.kind();
        let mut primary = match kind {
            t::STRING => {
                let text = r.take();
                let mut c = text.chars();
                c.next();
                c.next_back();
                parts.add(STRING_LITERAL, c.as_str().to_string())
            }
            t::NUMBER => {
                let v = r.take();
                parts.add(NUM_VAL, v)
            }
            t::NAME => {
                let v = r.take();
                parts.add(RULE_NAME, v)
            }
            t::GROUP | t::OPTION => {
                r.i += 1;
                let element = parts.add(ALTERNATION, String::new());
                let node = parts.add(if kind == t::GROUP { GROUP } else { OPTIONAL_GROUP }, String::new());
                parts.0[node].kids.push(element);
                let next = frame(parts, Some(node), element, kind + 1, repeat);
                stack.push(std::mem::replace(&mut top, next));
                continue;
            }
            _ => return Err(r.fail("a string, a number, a name, or an opening bracket")),
        };
        loop {
            let element = parts.add(ELEMENT, String::new());
            parts.0[element].op = repeat.take();
            parts.0[element].kids.push(primary);
            parts.0[top.concatenation].kids.push(element);
            let k = r.kind();
            if k == top.close {
                r.i += 1;
                primary = top.node.unwrap();
                repeat = top.repeat.take();
                top = stack.pop().unwrap();
                continue;
            }
            if k == t::SLASH {
                r.i += 1;
                top.concatenation = parts.add(CONCATENATION, String::new());
                parts.0[top.alternation].kids.push(top.concatenation);
            } else if k != t::STRING && k != t::NUMBER && k != t::NAME && k != t::GROUP && k != t::OPTION && k != t::REPEAT && k != t::EXACT {
                if stack.is_empty() {
                    return Ok(top.alternation);
                }
                return Err(r.fail(&format!("token of type '{}'", CLOSE_NAMES[((top.close - 5) >> 1) as usize])));
            }
            break;
        }
    }
}

fn strip(text: &str) -> String {
    let mut c = text.chars();
    c.next();
    c.next_back();
    c.as_str().to_string()
}

pub fn parse_abnf(src: &[u16], parts: &mut Parts) -> Result<Grammar, PeleError> {
    parse_grammar(src, &tokens(), t::TITLE, strip, parts, |r, parts| {
        let name = r.expect(t::NAME)?;
        r.expect(t::EQUALS)?;
        let definition = alternation(r, parts)?;
        r.expect(t::SEMICOLON)?;
        Ok((name, definition))
    })
}

pub fn abnf_rules(ast: &Grammar, parts: &Parts, nodes: &mut Nodes) -> Vec<(String, usize)> {
    to_rules(ast, parts, nodes, |part, kids, nodes| match part.tag {
        ALTERNATION => nodes.choice(kids),
        CONCATENATION => nodes.sequence(kids),
        ELEMENT => {
            let Some(repeat) = part.op.as_deref().filter(|r| !r.is_empty()) else { return kids[0] };
            let star = repeat.find('*');
            let exact = star.is_none();
            let min = if exact || star > Some(0) { js::parse_int(repeat, 10) } else { 0.0 };
            let max = match star {
                None => min,
                Some(s) if s < repeat.len() - 1 => js::parse_int(&repeat[s + 1..], 10),
                _ => f64::INFINITY,
            };
            if min == 0.0 && max == 1.0 {
                nodes.optional(kids[0])
            } else {
                nodes.repetition(kids[0], min, max)
            }
        }
        STRING_LITERAL | NUM_VAL => nodes.terminal(part.text.clone()),
        RULE_NAME => nodes.nonterminal(part.text.clone()),
        GROUP => kids[0],
        _ => nodes.optional(kids[0]),
    })
}
