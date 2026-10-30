// Port of src/diagrams/railroad/ir.ts: `railroad-beta`, Mermaid's own notation, one constructor
// call per node.

use super::common::{acc_descr, acc_title, block_comment, hidden_tokens, identifier, literal, opening, parse_grammar, quoted, title, to_rules, unescape, Grammar, Nodes, Parts};
use crate::diagrams::common::tokens::{Reader, TokenType};
use crate::errors::PeleError;

mod t {
    pub const NONTERMINAL: i32 = 1;
    pub const ZERO_OR_MORE: i32 = 2;
    pub const ONE_OR_MORE: i32 = 3;
    pub const SEQUENCE: i32 = 4;
    pub const OPTIONAL: i32 = 5;
    pub const TERMINAL: i32 = 6;
    pub const SPECIAL: i32 = 7;
    pub const CHOICE: i32 = 8;
    pub const EQUALS: i32 = 9;
    pub const SEMICOLON: i32 = 10;
    pub const OPEN: i32 = 11;
    pub const COMMA: i32 = 12;
    pub const CLOSE: i32 = 13;
    pub const TITLE: i32 = 14;
    pub const ID: i32 = 17;
    pub const STRING: i32 = 18;
}

// Part tags.
const TERMINAL: u8 = 0;
const NONTERMINAL: u8 = 1;
const SPECIAL: u8 = 2;
const SEQUENCE: u8 = 3;
const CHOICE: u8 = 4;
const OPTIONAL: u8 = 5;
const ONE_OR_MORE: u8 = 6;
const ZERO_OR_MORE: u8 = 7;

fn tokens() -> Vec<TokenType> {
    const ID: usize = 17;
    let mut out = vec![opening("railroad-beta", ID)];
    for w in ["nonterminal", "zeroOrMore", "oneOrMore", "sequence", "optional", "terminal", "special", "choice"] {
        out.push(literal(w, &[ID]));
    }
    for s in ["=", ";", "(", ",", ")"] {
        out.push(literal(s, &[]));
    }
    out.push(title(|c| super::common::is_word(c) || c == 45));
    out.push(acc_title());
    out.push(acc_descr());
    out.push(identifier("RR_ID"));
    out.push(quoted("RR_STRING"));
    out.extend(hidden_tokens("RR"));
    out.push(block_comment("RR_BLOCK_COMMENT", "/*", "*/", "/"));
    out
}

fn argument(r: &mut Reader) -> Result<String, PeleError> {
    r.i += 1;
    r.expect(t::OPEN)?;
    let value = unescape(&r.expect(t::STRING)?);
    r.expect(t::CLOSE)?;
    Ok(value)
}

fn expression(r: &mut Reader, parts: &mut Parts) -> Result<usize, PeleError> {
    let mut open: Vec<usize> = Vec::new();
    loop {
        let (tag, text) = match r.kind() {
            t::TERMINAL => (TERMINAL, argument(r)?),
            t::NONTERMINAL => (NONTERMINAL, argument(r)?),
            t::SPECIAL => (SPECIAL, argument(r)?),
            t::SEQUENCE => (SEQUENCE, String::new()),
            t::CHOICE => (CHOICE, String::new()),
            t::OPTIONAL => (OPTIONAL, String::new()),
            t::ONE_OR_MORE => (ONE_OR_MORE, String::new()),
            t::ZERO_OR_MORE => (ZERO_OR_MORE, String::new()),
            _ => return Err(r.fail("an expression")),
        };
        let mut value = parts.add(tag, text);
        if tag >= SEQUENCE {
            r.i += 1;
            r.expect(t::OPEN)?;
            open.push(value);
            continue;
        }
        loop {
            let Some(&parent) = open.last() else { return Ok(value) };
            parts.0[parent].kids.push(value);
            if parts.0[parent].tag == SEQUENCE || parts.0[parent].tag == CHOICE {
                if r.accept(t::COMMA) {
                    break;
                }
            }
            r.expect(t::CLOSE)?;
            open.pop();
            value = parent;
        }
    }
}

pub fn parse_ir(src: &[u16], parts: &mut Parts) -> Result<Grammar, PeleError> {
    parse_grammar(src, &tokens(), t::TITLE, unescape, parts, |r, parts| {
        let name = r.expect(t::ID)?;
        r.expect(t::EQUALS)?;
        let definition = expression(r, parts)?;
        r.expect(t::SEMICOLON)?;
        Ok((name, definition))
    })
}

pub fn ir_rules(ast: &Grammar, parts: &Parts, nodes: &mut Nodes) -> Vec<(String, usize)> {
    to_rules(ast, parts, nodes, |part, kids, nodes| match part.tag {
        TERMINAL => nodes.terminal(part.text.clone()),
        NONTERMINAL => nodes.nonterminal(part.text.clone()),
        SPECIAL => nodes.special(part.text.clone()),
        SEQUENCE => nodes.sequence(kids),
        CHOICE => nodes.choice(kids),
        OPTIONAL => nodes.optional(kids[0]),
        _ => nodes.repetition(kids[0], if part.tag == ONE_OR_MORE { 1.0 } else { 0.0 }, f64::INFINITY),
    })
}
