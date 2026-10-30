// Port of src/diagrams/railroad/ebnf.ts: `railroad-ebnf-beta`, EBNF in the W3C and ISO 14977 styles.

use super::common::{
    acc_descr, acc_title, block_comment, hidden_tokens, identifier, is_space, literal, opening, parse_grammar, quoted, title, to_rules, unescape, Grammar, Nodes, Parts, RailroadNode,
};
use crate::diagrams::common::tokens::{Reader, TokenType};
use crate::errors::PeleError;
use crate::js;

mod t {
    pub const DEFINES: i32 = 1;
    pub const EQUALS: i32 = 2;
    pub const SEMICOLON: i32 = 3;
    pub const BAR: i32 = 4;
    pub const COMMA: i32 = 5;
    pub const GROUP: i32 = 6;
    pub const OPTION: i32 = 8;
    pub const REPEAT: i32 = 10;
    pub const QUESTION: i32 = 12;
    pub const STAR: i32 = 13;
    pub const PLUS: i32 = 14;
    pub const MINUS: i32 = 15;
    pub const TITLE: i32 = 16;
    pub const ID: i32 = 19;
    pub const STRING: i32 = 20;
    pub const SPECIAL: i32 = 21;
}

// Part tags.
const CHOICE: u8 = 0;
const SEQUENCE: u8 = 1;
const TERM: u8 = 2;
const TERMINAL: u8 = 3;
const NONTERMINAL: u8 = 4;
const SPECIAL: u8 = 5;
const GROUP: u8 = 6;
const OPTIONAL: u8 = 7;
const REPETITION: u8 = 8;

// A question mark, text with something other than blanks in it and no `?` or `;`, and a question mark.
fn special_sequence(src: &[u16], p: usize) -> Option<usize> {
    if src.get(p) != Some(&63) {
        return None;
    }
    let mut solid = false;
    for i in p + 1..src.len() {
        let c = src[i] as u32;
        if c == 63 {
            return solid.then_some(i + 1);
        }
        if c == 59 {
            return None;
        }
        solid |= !is_space(c);
    }
    None
}

fn tokens() -> Vec<TokenType> {
    const ID: usize = 19;
    const SPECIAL_T: usize = 21;
    const YAML: usize = 23;
    const ISO: usize = 27;
    let mut out = vec![opening("railroad-ebnf-beta", ID)];
    out.push(literal("::=", &[]));
    out.push(literal("=", &[]));
    out.push(literal(";", &[]));
    out.push(literal("|", &[]));
    out.push(literal(",", &[]));
    out.push(literal("(", &[ISO]));
    out.push(literal(")", &[]));
    out.push(literal("[", &[]));
    out.push(literal("]", &[]));
    out.push(literal("{", &[]));
    out.push(literal("}", &[]));
    out.push(literal("?", &[SPECIAL_T]));
    out.push(literal("*", &[]));
    out.push(literal("+", &[]));
    out.push(literal("-", &[YAML]));
    out.push(title(|c| super::common::is_word(c) || c == 45));
    out.push(acc_title());
    out.push(acc_descr());
    out.push(identifier("EBNF_ID"));
    out.push(quoted("EBNF_STRING"));
    out.push(TokenType::re("EBNF_SPECIAL_SEQUENCE", Some("?"), special_sequence));
    out.extend(hidden_tokens("EBNF"));
    out.push(block_comment("EBNF_BLOCK_COMMENT", "/*", "*/", "/"));
    out.push(block_comment("EBNF_ISO_COMMENT", "(*", "*)", "("));
    out
}

const TOKEN_NAMES_CLOSE: [&str; 3] = [")", "]", "}"];

// One open bracket, or the rule itself at the bottom of the stack.
struct Frame {
    choice: usize,
    sequence: usize,
    term: Option<usize>,
    // The bracket's node, its closing token, and whether it is the right side of an exception.
    node: Option<usize>,
    close: i32,
    except: bool,
}

fn frame(parts: &mut Parts, node: Option<usize>, choice: usize, close: i32, except: bool) -> Frame {
    let sequence = parts.add(SEQUENCE, String::new());
    parts.0[choice].kids.push(sequence);
    Frame { choice, sequence, term: None, node, close, except }
}

fn definition(r: &mut Reader, parts: &mut Parts) -> Result<usize, PeleError> {
    let mut stack: Vec<Frame> = Vec::new();
    let root = parts.add(CHOICE, String::new());
    let mut top = frame(parts, None, root, -2, false);
    let mut except = false;
    loop {
        let kind = r.kind();
        let mut primary = match kind {
            t::STRING => {
                let v = unescape(&r.take());
                parts.add(TERMINAL, v)
            }
            t::ID => {
                let v = r.take();
                parts.add(NONTERMINAL, v)
            }
            t::SPECIAL => {
                let text = r.take();
                let mut c = text.chars();
                c.next();
                c.next_back();
                parts.add(SPECIAL, js::trim(c.as_str()).to_string())
            }
            t::GROUP | t::OPTION | t::REPEAT => {
                r.i += 1;
                let element = parts.add(CHOICE, String::new());
                let node = parts.add([GROUP, OPTIONAL, REPETITION][((kind - t::GROUP) >> 1) as usize], String::new());
                parts.0[node].kids.push(element);
                let next = frame(parts, Some(node), element, kind + 1, except);
                stack.push(std::mem::replace(&mut top, next));
                except = false;
                continue;
            }
            _ => return Err(r.fail("a terminal, a name, or an opening bracket")),
        };
        loop {
            if except {
                let term = top.term.unwrap();
                parts.0[term].postfixes.push(b'-');
                parts.0[term].kids.push(primary);
                except = false;
            } else {
                let term = parts.add(TERM, String::new());
                parts.0[term].kids.push(primary);
                top.term = Some(term);
                parts.0[top.sequence].kids.push(term);
            }
            let term = top.term.unwrap();
            loop {
                let k = r.kind();
                let op = match k {
                    t::QUESTION => b'?',
                    t::STAR => b'*',
                    t::PLUS => b'+',
                    _ => break,
                };
                r.i += 1;
                parts.0[term].postfixes.push(op);
            }
            let k = r.kind();
            if k == top.close {
                r.i += 1;
                primary = top.node.unwrap();
                except = top.except;
                top = stack.pop().unwrap();
                continue;
            }
            if k == t::MINUS {
                r.i += 1;
                except = true;
            } else if k == t::COMMA {
                r.i += 1;
            } else if k == t::BAR {
                r.i += 1;
                top.sequence = parts.add(SEQUENCE, String::new());
                parts.0[top.choice].kids.push(top.sequence);
            } else if k != t::STRING && k != t::ID && k != t::SPECIAL && k != t::GROUP && k != t::OPTION && k != t::REPEAT {
                if stack.is_empty() {
                    return Ok(root_of(&top));
                }
                return Err(r.fail(&format!("token of type '{}'", TOKEN_NAMES_CLOSE[((top.close - 7) >> 1) as usize])));
            }
            break;
        }
    }
}

fn root_of(top: &Frame) -> usize {
    top.choice
}

pub fn parse_ebnf(src: &[u16], parts: &mut Parts) -> Result<Grammar, PeleError> {
    parse_grammar(src, &tokens(), t::TITLE, unescape, parts, |r, parts| {
        let name = r.expect(t::ID)?;
        if !r.accept(t::EQUALS) {
            r.expect(t::DEFINES)?;
        }
        let definition = definition(r, parts)?;
        r.expect(t::SEMICOLON)?;
        Ok((name, definition))
    })
}

pub fn ebnf_rules(ast: &Grammar, parts: &Parts, nodes: &mut Nodes) -> Vec<(String, usize)> {
    to_rules(ast, parts, nodes, |part, kids, nodes| match part.tag {
        CHOICE => nodes.choice(kids),
        SEQUENCE => nodes.sequence(kids),
        TERM => {
            // Each postfix operator applies to everything before it in the term.
            let mut result = kids[0];
            let mut next = 1;
            for &op in &part.postfixes {
                result = match op {
                    b'?' => nodes.optional(result),
                    b'-' => {
                        let minus = nodes.terminal("-".into());
                        let n = nodes.add(RailroadNode::Sequence(vec![result, minus, kids[next]]));
                        next += 1;
                        n
                    }
                    _ => nodes.repetition(result, if op == b'+' { 1.0 } else { 0.0 }, f64::INFINITY),
                };
            }
            result
        }
        TERMINAL => nodes.terminal(part.text.clone()),
        NONTERMINAL => nodes.nonterminal(part.text.clone()),
        SPECIAL => nodes.special(part.text.clone()),
        GROUP => kids[0],
        OPTIONAL => nodes.optional(kids[0]),
        _ => nodes.repetition(kids[0], 0.0, f64::INFINITY),
    })
}
