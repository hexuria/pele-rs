// Port of src/diagrams/packet/parser.ts.

use crate::diagrams::common::tokens::*;
use crate::errors::PeleError;
use crate::js;

const PACKET_BETA: i32 = 0;
const PACKET: i32 = 1;
const DASH: i32 = 2;
const PLUS: i32 = 3;
const COLON: i32 = 4;
const ACC_DESCR: i32 = 5;
const ACC_TITLE: i32 = 6;
const TITLE: i32 = 7;
const INT: i32 = 8;
const STRING: i32 = 9;
const NEWLINE: i32 = 10;
const YAML_INDEX: usize = 12;

// `/0|[1-9][0-9]*(?!\.)/y`
pub fn int_end(src: &[u16], p: usize) -> Option<usize> {
    let c = *src.get(p)?;
    if c == 48 {
        return Some(p + 1);
    }
    if !(49..=57).contains(&c) {
        return None;
    }
    let mut e = p + 1;
    while e < src.len() && (48..=57).contains(&src[e]) {
        e += 1;
    }
    if src.get(e) != Some(&46) {
        return Some(e);
    }
    (e - 1 > p).then_some(e - 1)
}

pub fn packet_tokens() -> Vec<TokenType> {
    // A front matter block also starts with dashes, so Langium lets it win over a dash.
    let mut dash = TokenType::lit("-", "-");
    dash.longer = vec![YAML_INDEX];
    vec![
        TokenType::lit("packet-beta", "packet-beta"),
        keyword("packet"),
        dash,
        TokenType::lit("+", "+"),
        TokenType::lit(":", ":"),
        acc_descr(),
        acc_title(),
        title(),
        TokenType::re("INT", None, int_end),
        string(),
        newline_token(),
        whitespace(),
        yaml(),
        directive(),
        single_line_comment(),
    ]
}

pub struct PacketBlockAst {
    pub start: Option<f64>,
    pub end: Option<f64>,
    pub bits: Option<f64>,
    pub label: String,
}

#[derive(Default)]
pub struct PacketAst {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub blocks: Vec<PacketBlockAst>,
}

pub fn parse_packet(src: &[u16]) -> Result<PacketAst, PeleError> {
    let types = packet_tokens();
    let tokens = tokenize(src, &types, "packet")?;
    let mut r = Reader::new(&tokens, &types, "packet");
    let mut ast = PacketAst::default();
    let end_of_line = |r: &mut Reader| -> Result<(), PeleError> {
        if r.kind() == -1 {
            return Ok(());
        }
        if r.kind() != NEWLINE {
            return Err(r.fail("a line break or the end of input"));
        }
        while r.kind() == NEWLINE {
            r.i += 1;
        }
        Ok(())
    };
    while r.kind() == NEWLINE {
        r.i += 1;
    }
    if !r.accept(PACKET_BETA) {
        r.expect(PACKET)?;
    }
    while r.kind() != -1 {
        match r.kind() {
            NEWLINE => r.i += 1,
            ACC_DESCR => {
                ast.acc_descr = Some(acc_descr_value(&r.take()));
                end_of_line(&mut r)?;
            }
            ACC_TITLE => {
                ast.acc_title = Some(acc_title_value(&r.take()));
                end_of_line(&mut r)?;
            }
            TITLE => {
                ast.title = Some(title_value(&r.take()));
                end_of_line(&mut r)?;
            }
            INT | PLUS => {
                let mut block = PacketBlockAst { start: None, end: None, bits: None, label: String::new() };
                if r.accept(PLUS) {
                    block.bits = Some(js::to_number(&r.expect(INT)?));
                } else {
                    block.start = Some(js::to_number(&r.take()));
                    if r.accept(DASH) {
                        block.end = Some(js::to_number(&r.expect(INT)?));
                    }
                }
                r.expect(COLON)?;
                block.label = string_value(&r.expect(STRING)?);
                ast.blocks.push(block);
                end_of_line(&mut r)?;
            }
            _ => return Err(r.fail("a field, a title, or a line break")),
        }
    }
    Ok(ast)
}
