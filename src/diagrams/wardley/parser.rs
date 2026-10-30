// Port of src/diagrams/wardley/parser.ts.

use crate::diagrams::common::acc_descr::acc_descr;
use crate::diagrams::common::tokens::{
    acc_descr_value, acc_title, acc_title_value, directive, newline_token, single_line_comment, string, string_value, title, title_value, tokenize, whitespace, yaml, Reader, TokenType,
};
use crate::errors::PeleError;
use crate::js;
use crate::util::chars::{is_space, is_word};

mod t {
    pub const LBRACKET: i32 = 0;
    pub const COMMA: i32 = 1;
    pub const RBRACKET: i32 = 2;
    pub const AT: i32 = 3;
    pub const SLASH: i32 = 4;
    pub const LPAREN: i32 = 5;
    pub const RPAREN: i32 = 6;
    pub const MINUS: i32 = 7;
    pub const LBRACE: i32 = 8;
    pub const RBRACE: i32 = 9;
    pub const NUMBER: i32 = 10;
    pub const ARROW: i32 = 11;
    pub const LINK_PORT: i32 = 12;
    pub const LINK_ARROW: i32 = 13;
    pub const LINK_LABEL: i32 = 14;
    pub const STRATEGY: i32 = 15;
    pub const WARDLEY: i32 = 16;
    pub const SIZE: i32 = 17;
    pub const EVOLUTION: i32 = 18;
    pub const ANCHOR: i32 = 19;
    pub const COMPONENT: i32 = 20;
    pub const LABEL: i32 = 21;
    pub const INERTIA: i32 = 22;
    pub const EVOLVE: i32 = 23;
    pub const PIPELINE: i32 = 24;
    pub const NOTE: i32 = 25;
    pub const ANNOTATIONS: i32 = 26;
    pub const ANNOTATION: i32 = 27;
    pub const ACCELERATOR: i32 = 28;
    pub const DEACCELERATOR: i32 = 29;
    pub const NAME: i32 = 30;
    pub const ACC_DESCR: i32 = 32;
    pub const ACC_TITLE: i32 = 33;
    pub const TITLE: i32 = 34;
    pub const INT: i32 = 35;
    pub const STRING: i32 = 36;
    pub const ID: i32 = 37;
    pub const NEWLINE: i32 = 38;
}

fn at(src: &[u16], i: usize) -> u32 {
    src.get(i).map_or(u32::MAX, |&c| c as u32)
}

fn is_digit(c: u32) -> bool {
    (48..=57).contains(&c)
}

fn digits(src: &[u16], from: usize) -> usize {
    let mut i = from;
    while is_digit(at(src, i)) {
        i += 1;
    }
    i
}

fn starts(src: &[u16], p: usize, lit: &str) -> bool {
    lit.bytes().enumerate().all(|(k, b)| at(src, p + k) == b as u32)
}

// `\+'[^']*'` and then `<>`, `<` or `>`, as LINK_ARROW tries them.
fn quoted_arrow(src: &[u16], p: usize) -> Option<usize> {
    if !starts(src, p, "+'") {
        return None;
    }
    let close = (p + 2..src.len()).find(|&k| src[k] == 39)?;
    let q = close + 1;
    if starts(src, q, "<>") {
        Some(q + 2)
    } else if at(src, q) == 60 || at(src, q) == 62 {
        Some(q + 1)
    } else {
        None
    }
}

fn is_name_char(src: &[u16], i: usize) -> bool {
    let c = at(src, i);
    (65..=90).contains(&c) || (97..=122).contains(&c) || is_digit(c) || matches!(c, 95 | 40 | 41 | 38) || (c == 45 && at(src, i + 1) != 62)
}

fn is_letter(c: u32) -> bool {
    (65..=90).contains(&c) || (97..=122).contains(&c)
}

// NAME_WITH_SPACES: /(?!title\s|accTitle|accDescr)[A-Za-z](?:[A-Za-z0-9_()&]|-(?!>))*(?:[ \t]+[A-Za-z(](?:…)*)*/y
fn name_with_spaces(src: &[u16], p: usize) -> Option<usize> {
    if (starts(src, p, "title") && is_space(at(src, p + 5))) || starts(src, p, "accTitle") || starts(src, p, "accDescr") {
        return None;
    }
    if !is_letter(at(src, p)) {
        return None;
    }
    let mut e = p + 1;
    while is_name_char(src, e) {
        e += 1;
    }
    loop {
        let mut b = e;
        while matches!(at(src, b), 9 | 32) {
            b += 1;
        }
        let c = at(src, b);
        if b == e || !(is_letter(c) || c == 40) {
            return Some(e);
        }
        e = b + 1;
        while is_name_char(src, e) {
            e += 1;
        }
    }
}

fn word(name: &str, text: &'static str) -> TokenType {
    TokenType::re(name, Some(&text[..1]), move |src, p| starts(src, p, text).then(|| p + text.len()))
}

pub fn tokens() -> Vec<TokenType> {
    let mut minus = TokenType::lit("-", "-");
    minus.longer = vec![11, 13, 40];
    vec![
        TokenType::lit("[", "["),
        TokenType::lit(",", ","),
        TokenType::lit("]", "]"),
        TokenType::lit("@", "@"),
        TokenType::lit("/", "/"),
        TokenType::lit("(", "("),
        TokenType::lit(")", ")"),
        minus,
        TokenType::lit("{", "{"),
        TokenType::lit("}", "}"),
        // /[0-9]+\.[0-9]+/y
        TokenType::re("WARDLEY_NUMBER", Some("0123456789"), |src, p| {
            let end = digits(src, p);
            if end == p || at(src, end) != 46 {
                return None;
            }
            let stop = digits(src, end + 1);
            (stop > end + 1).then_some(stop)
        }),
        TokenType::re("ARROW", Some("-"), |src, p| starts(src, p, "->").then_some(p + 2)),
        TokenType::re("LINK_PORT", Some("+"), |src, p| {
            if starts(src, p, "+<>") {
                Some(p + 3)
            } else if starts(src, p, "+>") || starts(src, p, "+<") {
                Some(p + 2)
            } else {
                None
            }
        }),
        // /-->|-\.->|>|\+'[^']*'<>|\+'[^']*'<|\+'[^']*'>/y
        TokenType::re("LINK_ARROW", Some("->+"), |src, p| {
            if starts(src, p, "-->") {
                Some(p + 3)
            } else if starts(src, p, "-.->") {
                Some(p + 4)
            } else if at(src, p) == 62 {
                Some(p + 1)
            } else {
                quoted_arrow(src, p)
            }
        }),
        TokenType::re("LINK_LABEL", Some(";"), |src, p| {
            if at(src, p) != 59 {
                return None;
            }
            let mut e = p + 1;
            while e < src.len() && src[e] != 10 && src[e] != 13 {
                e += 1;
            }
            (e > p + 1).then_some(e)
        }),
        TokenType::re("STRATEGY", Some("bom"), |src, p| ["build", "buy", "outsource", "market"].iter().find(|w| starts(src, p, w)).map(|w| p + w.len())),
        word("KW_WARDLEY", "wardley-beta"),
        word("KW_SIZE", "size"),
        word("KW_EVOLUTION", "evolution"),
        word("KW_ANCHOR", "anchor"),
        word("KW_COMPONENT", "component"),
        word("KW_LABEL", "label"),
        word("KW_INERTIA", "inertia"),
        word("KW_EVOLVE", "evolve"),
        word("KW_PIPELINE", "pipeline"),
        word("KW_NOTE", "note"),
        word("KW_ANNOTATIONS", "annotations"),
        word("KW_ANNOTATION", "annotation"),
        word("KW_ACCELERATOR", "accelerator"),
        word("KW_DEACCELERATOR", "deaccelerator"),
        TokenType::re("NAME_WITH_SPACES", Some("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"), name_with_spaces),
        whitespace().named("WS"),
        acc_descr("ACC_DESCR"),
        acc_title(),
        title(),
        // /0|[1-9][0-9]*(?!\.)/y: the run of digits backs off one when a dot follows it.
        TokenType::re("INT", Some("0123456789"), |src, p| {
            let c = at(src, p);
            if c == 48 {
                return Some(p + 1);
            }
            if !(49..=57).contains(&c) {
                return None;
            }
            let end = digits(src, p + 1);
            if at(src, end) != 46 {
                Some(end)
            } else if end > p + 1 {
                Some(end - 1)
            } else {
                None
            }
        }),
        string(),
        // /[\w]([-\w]*\w)?/y
        TokenType::re("ID", Some("_0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"), |src, p| {
            if !is_word(at(src, p)) {
                return None;
            }
            let mut e = p + 1;
            let mut last = p + 1;
            while is_word(at(src, e)) || at(src, e) == 45 {
                if is_word(at(src, e)) {
                    last = e + 1;
                }
                e += 1;
            }
            Some(last)
        }),
        newline_token(),
        whitespace(),
        yaml(),
        directive(),
        single_line_comment(),
    ]
}

pub struct LabelAst {
    pub neg_x: bool,
    pub offset_x: f64,
    pub neg_y: bool,
    pub offset_y: f64,
}

pub struct StageAst {
    pub name: String,
    pub boundary: Option<f64>,
    pub second_name: Option<String>,
}

pub struct ComponentAst {
    pub name: String,
    pub visibility: f64,
    pub evolution: f64,
    pub label: Option<LabelAst>,
    pub strategy: Option<String>,
    pub inertia: bool,
}

pub struct PipelineComponentAst {
    pub name: String,
    pub evolution: f64,
    pub label: Option<LabelAst>,
}

pub struct LinkAst {
    pub from: String,
    pub from_port: Option<String>,
    pub arrow: Option<String>,
    pub to: String,
    pub to_port: Option<String>,
    pub link_label: Option<String>,
}

pub struct Placed {
    pub name: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Default)]
pub struct WardleyAst {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub size: Option<(f64, f64)>,
    pub evolution: Option<Vec<StageAst>>,
    pub anchors: Vec<(String, f64, f64)>,
    pub components: Vec<ComponentAst>,
    pub links: Vec<LinkAst>,
    pub evolves: Vec<(String, f64)>,
    pub pipelines: Vec<(String, Vec<PipelineComponentAst>)>,
    pub notes: Vec<(String, f64, f64)>,
    pub annotations: Vec<(f64, f64)>,
    // number, x, y, text
    pub annotation: Vec<(f64, f64, f64, String)>,
    pub accelerators: Vec<Placed>,
    pub deaccelerators: Vec<Placed>,
}

type R<T> = Result<T, PeleError>;

fn name(r: &mut Reader) -> R<String> {
    if r.kind() == t::STRING {
        return Ok(string_value(&r.take()));
    }
    if r.kind() == t::ID || r.kind() == t::NAME {
        return Ok(r.take());
    }
    Err(r.fail("a name"))
}

fn number(r: &mut Reader) -> R<f64> {
    Ok(js::to_number(&r.expect(t::NUMBER)?))
}

fn int(r: &mut Reader) -> R<f64> {
    Ok(js::parse_int(&r.expect(t::INT)?, 10))
}

fn coordinate(r: &mut Reader) -> R<f64> {
    if r.kind() == t::NUMBER {
        Ok(js::to_number(&r.take()))
    } else {
        Ok(js::parse_int(&r.expect(t::INT)?, 10))
    }
}

fn end_of_line(r: &mut Reader) -> R<()> {
    if r.kind() == -1 {
        return Ok(());
    }
    if r.kind() != t::NEWLINE {
        return Err(r.fail("a line break or the end of input"));
    }
    while r.kind() == t::NEWLINE {
        r.i += 1;
    }
    Ok(())
}

fn pair(r: &mut Reader, read: fn(&mut Reader) -> R<f64>) -> R<(f64, f64)> {
    r.expect(t::LBRACKET)?;
    let first = read(r)?;
    r.expect(t::COMMA)?;
    let second = read(r)?;
    r.expect(t::RBRACKET)?;
    Ok((first, second))
}

fn label(r: &mut Reader) -> R<LabelAst> {
    r.expect(t::LABEL)?;
    r.expect(t::LBRACKET)?;
    let neg_x = r.accept(t::MINUS);
    let offset_x = int(r)?;
    r.expect(t::COMMA)?;
    let neg_y = r.accept(t::MINUS);
    let offset_y = int(r)?;
    r.expect(t::RBRACKET)?;
    Ok(LabelAst { neg_x, offset_x, neg_y, offset_y })
}

fn stage(r: &mut Reader) -> R<StageAst> {
    let mut out = StageAst { name: name(r)?, boundary: None, second_name: None };
    if r.accept(t::AT) {
        out.boundary = Some(number(r)?);
    }
    if r.accept(t::SLASH) {
        out.second_name = Some(name(r)?);
    }
    Ok(out)
}

pub fn parse_wardley(src: &[u16]) -> R<WardleyAst> {
    let types = tokens();
    let tokens = tokenize(src, &types, "wardley")?;
    let mut r = Reader::new(&tokens, &types, "wardley");
    let r = &mut r;
    let mut ast = WardleyAst::default();

    while r.kind() == t::NEWLINE {
        r.i += 1;
    }
    r.expect(t::WARDLEY)?;
    while r.kind() != -1 {
        match r.kind() {
            t::NEWLINE => r.i += 1,
            t::ACC_DESCR => {
                ast.acc_descr = Some(acc_descr_value(&r.take()));
                end_of_line(r)?;
            }
            t::ACC_TITLE => {
                ast.acc_title = Some(acc_title_value(&r.take()));
                end_of_line(r)?;
            }
            t::TITLE => {
                ast.title = Some(title_value(&r.take()));
                end_of_line(r)?;
            }
            t::SIZE => {
                r.i += 1;
                let size = pair(r, int)?;
                end_of_line(r)?;
                ast.size = Some(size);
            }
            t::EVOLUTION => {
                r.i += 1;
                let mut stages = vec![stage(r)?];
                r.expect(t::ARROW)?;
                stages.push(stage(r)?);
                while r.accept(t::ARROW) {
                    stages.push(stage(r)?);
                }
                end_of_line(r)?;
                ast.evolution = Some(stages);
            }
            t::ANCHOR => {
                r.i += 1;
                let anchor = name(r)?;
                let (visibility, evolution) = pair(r, number)?;
                end_of_line(r)?;
                ast.anchors.push((anchor, visibility, evolution));
            }
            t::COMPONENT => {
                r.i += 1;
                let n = name(r)?;
                let (visibility, evolution) = pair(r, number)?;
                let mut component = ComponentAst { name: n, visibility, evolution, label: None, strategy: None, inertia: false };
                if r.kind() == t::LABEL {
                    component.label = Some(label(r)?);
                }
                if r.kind() == t::LPAREN && r.tokens.kinds[r.i + 1] == t::STRATEGY {
                    r.i += 1;
                    component.strategy = Some(r.take());
                    r.expect(t::RPAREN)?;
                }
                if r.accept(t::INERTIA) {
                    component.inertia = true;
                } else if r.accept(t::LPAREN) {
                    r.expect(t::INERTIA)?;
                    r.expect(t::RPAREN)?;
                    component.inertia = true;
                }
                end_of_line(r)?;
                ast.components.push(component);
            }
            t::EVOLVE => {
                r.i += 1;
                let component = name(r)?;
                let target = number(r)?;
                end_of_line(r)?;
                ast.evolves.push((component, target));
            }
            t::PIPELINE => {
                r.i += 1;
                let parent = name(r)?;
                r.expect(t::LBRACE)?;
                r.expect(t::NEWLINE)?;
                while r.kind() == t::NEWLINE {
                    r.i += 1;
                }
                let mut components = Vec::new();
                loop {
                    r.expect(t::COMPONENT)?;
                    let n = name(r)?;
                    r.expect(t::LBRACKET)?;
                    let evolution = number(r)?;
                    r.expect(t::RBRACKET)?;
                    let mut component = PipelineComponentAst { name: n, evolution, label: None };
                    if r.kind() == t::LABEL {
                        component.label = Some(label(r)?);
                    }
                    end_of_line(r)?;
                    components.push(component);
                    if r.kind() != t::COMPONENT {
                        break;
                    }
                }
                r.expect(t::RBRACE)?;
                end_of_line(r)?;
                ast.pipelines.push((parent, components));
            }
            t::NOTE => {
                r.i += 1;
                let text = string_value(&r.expect(t::STRING)?);
                let (visibility, evolution) = pair(r, number)?;
                end_of_line(r)?;
                ast.notes.push((text, visibility, evolution));
            }
            t::ANNOTATIONS => {
                r.i += 1;
                let (x, y) = pair(r, coordinate)?;
                end_of_line(r)?;
                ast.annotations.push((x, y));
            }
            t::ANNOTATION => {
                r.i += 1;
                let n = int(r)?;
                r.expect(t::COMMA)?;
                let (x, y) = pair(r, coordinate)?;
                let text = string_value(&r.expect(t::STRING)?);
                end_of_line(r)?;
                ast.annotation.push((n, x, y, text));
            }
            t::ACCELERATOR | t::DEACCELERATOR => {
                let slow = r.kind() == t::DEACCELERATOR;
                r.i += 1;
                let force = name(r)?;
                let (x, y) = pair(r, number)?;
                end_of_line(r)?;
                let placed = Placed { name: force, x, y };
                if slow {
                    ast.deaccelerators.push(placed);
                } else {
                    ast.accelerators.push(placed);
                }
            }
            _ => {
                if !(r.kind() == t::STRING || r.kind() == t::ID || r.kind() == t::NAME) {
                    return Err(r.fail("a statement"));
                }
                let from = name(r)?;
                let mut link = LinkAst { from, from_port: None, arrow: None, to: String::new(), to_port: None, link_label: None };
                if r.kind() == t::LINK_PORT {
                    link.from_port = Some(r.take());
                }
                if r.kind() == t::LINK_ARROW || r.kind() == t::ARROW {
                    link.arrow = Some(r.take());
                }
                link.to = name(r)?;
                if r.kind() == t::LINK_PORT {
                    link.to_port = Some(r.take());
                }
                if r.kind() == t::LINK_LABEL {
                    let text = r.take();
                    link.link_label = Some(js::trim(&text[1..]).to_string());
                }
                end_of_line(r)?;
                ast.links.push(link);
            }
        }
    }
    Ok(ast)
}
