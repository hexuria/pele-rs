// Port of src/diagrams/railroad/peg.ts: `railroad-peg-beta`, parsing expression grammars.

use super::common::{acc_descr, acc_title, hidden_tokens, identifier, literal, opening, parse_grammar, quoted, title, to_rules, unescape, Grammar, Nodes, Parts, RailroadNode};
use crate::diagrams::common::tokens::{Reader, TokenType};
use crate::errors::PeleError;

mod t {
    pub const ARROW: i32 = 1;
    pub const SEMICOLON: i32 = 2;
    pub const SLASH: i32 = 3;
    pub const AND: i32 = 4;
    pub const NOT: i32 = 5;
    pub const QUESTION: i32 = 6;
    pub const STAR: i32 = 7;
    pub const PLUS: i32 = 8;
    pub const GROUP: i32 = 9;
    pub const GROUP_END: i32 = 10;
    pub const DOT: i32 = 11;
    pub const TITLE: i32 = 12;
    pub const ID: i32 = 15;
    pub const STRING: i32 = 16;
}

// Part tags.
const CHOICE: u8 = 0;
const SEQUENCE: u8 = 1;
const PREFIX: u8 = 2;
const SUFFIX: u8 = 3;
const LITERAL: u8 = 4;
const IDENTIFIER: u8 = 5;
const ANY: u8 = 6;
const GROUP: u8 = 7;

fn tokens() -> Vec<TokenType> {
    const ID: usize = 15;
    let mut out = vec![opening("railroad-peg-beta", ID)];
    for s in ["<-", ";", "/", "&", "!", "?", "*", "+", "(", ")", "."] {
        out.push(literal(s, &[]));
    }
    out.push(title(|c| super::common::is_word(c) || c == 45));
    out.push(acc_title());
    out.push(acc_descr());
    out.push(identifier("PEG_ID"));
    out.push(quoted("PEG_STRING"));
    out.extend(hidden_tokens("PEG"));
    out.push(
        TokenType::re("PEG_LINE_COMMENT", Some("#"), |src, p| {
            if src.get(p) != Some(&35) {
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

// One open group, or the rule itself at the bottom of the stack.
struct Frame {
    choice: usize,
    sequence: usize,
    // The group's node and the predicate written before it.
    node: Option<usize>,
    operator: Option<String>,
}

fn frame(parts: &mut Parts, node: Option<usize>, choice: usize, operator: Option<String>) -> Frame {
    let sequence = parts.add(SEQUENCE, String::new());
    parts.0[choice].kids.push(sequence);
    Frame { choice, sequence, node, operator }
}

fn definition(r: &mut Reader, parts: &mut Parts) -> Result<usize, PeleError> {
    let mut stack: Vec<Frame> = Vec::new();
    let root = parts.add(CHOICE, String::new());
    let mut top = frame(parts, None, root, None);
    loop {
        let mut operator = if r.kind() == t::AND || r.kind() == t::NOT { Some(r.take()) } else { None };
        let kind = r.kind();
        let mut primary = match kind {
            t::STRING => {
                let v = unescape(&r.take());
                parts.add(LITERAL, v)
            }
            t::ID => {
                let v = r.take();
                parts.add(IDENTIFIER, v)
            }
            t::DOT => {
                let v = r.take();
                parts.add(ANY, v)
            }
            t::GROUP => {
                r.i += 1;
                let element = parts.add(CHOICE, String::new());
                let node = parts.add(GROUP, String::new());
                parts.0[node].kids.push(element);
                let next = frame(parts, Some(node), element, operator);
                stack.push(std::mem::replace(&mut top, next));
                continue;
            }
            _ => return Err(r.fail("a literal, a name, a dot, or an opening parenthesis")),
        };
        loop {
            let suffix = parts.add(SUFFIX, String::new());
            parts.0[suffix].kids.push(primary);
            if r.kind() == t::QUESTION || r.kind() == t::STAR || r.kind() == t::PLUS {
                parts.0[suffix].op = Some(r.take());
            }
            let prefix = parts.add(PREFIX, String::new());
            parts.0[prefix].op = operator.take();
            parts.0[prefix].kids.push(suffix);
            parts.0[top.sequence].kids.push(prefix);
            let k = r.kind();
            if k == t::GROUP_END && !stack.is_empty() {
                r.i += 1;
                primary = top.node.unwrap();
                operator = top.operator.take();
                top = stack.pop().unwrap();
                continue;
            }
            if k == t::SLASH {
                r.i += 1;
                top.sequence = parts.add(SEQUENCE, String::new());
                parts.0[top.choice].kids.push(top.sequence);
            } else if k != t::AND && k != t::NOT && k != t::STRING && k != t::ID && k != t::DOT && k != t::GROUP {
                if stack.is_empty() {
                    return Ok(top.choice);
                }
                return Err(r.fail("token of type ')'"));
            }
            break;
        }
    }
}

pub fn parse_peg(src: &[u16], parts: &mut Parts) -> Result<Grammar, PeleError> {
    parse_grammar(src, &tokens(), t::TITLE, unescape, parts, |r, parts| {
        let name = r.expect(t::ID)?;
        r.expect(t::ARROW)?;
        let definition = definition(r, parts)?;
        r.expect(t::SEMICOLON)?;
        Ok((name, definition))
    })
}

// What a predicate shows of the expression it looks ahead for.
fn label(node: &RailroadNode) -> String {
    match node {
        RailroadNode::Terminal(v) => format!("\"{}\"", v),
        RailroadNode::NonTerminal(n) => n.clone(),
        RailroadNode::Special(t) => t.clone(),
        _ => "(...)".into(),
    }
}

pub fn peg_rules(ast: &Grammar, parts: &Parts, nodes: &mut Nodes) -> Vec<(String, usize)> {
    to_rules(ast, parts, nodes, |part, kids, nodes| {
        let kid = kids.first().copied();
        match part.tag {
            CHOICE => nodes.choice(kids),
            SEQUENCE => nodes.sequence(kids),
            PREFIX => match part.op.as_deref().filter(|o| !o.is_empty()) {
                Some(op) => {
                    let text = format!("{}{}", op, label(&nodes.0[kid.unwrap()]));
                    nodes.special(text)
                }
                None => kid.unwrap(),
            },
            SUFFIX => match part.op.as_deref().filter(|o| !o.is_empty()) {
                None => kid.unwrap(),
                Some("?") => nodes.optional(kid.unwrap()),
                Some(op) => nodes.repetition(kid.unwrap(), if op == "+" { 1.0 } else { 0.0 }, f64::INFINITY),
            },
            LITERAL => nodes.terminal(part.text.clone()),
            IDENTIFIER => nodes.nonterminal(part.text.clone()),
            ANY => nodes.special(part.text.clone()),
            _ => kid.unwrap(),
        }
    })
}
