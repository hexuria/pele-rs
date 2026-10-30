// Port of src/diagrams/cynefin (index.ts, model.ts, parser.ts).

pub mod boundaries;
pub mod render;

use crate::diagrams::common::tokens::{
    acc_descr, acc_descr_value, acc_title, acc_title_value, directive, keyword, newline_token, single_line_comment, string, string_value, title, title_value, tokenize, whitespace, yaml, Reader,
    TokenType,
};
use crate::diagrams::Diagram;
use crate::errors::PeleError;
use crate::js;
use crate::util::chars::is_space;

mod t {
    pub const KEYWORD_COLON: i32 = 0;
    pub const KEYWORD: i32 = 1;
    pub const ARROW: i32 = 2;
    pub const COLON: i32 = 3;
    pub const DOMAIN: i32 = 4;
    pub const ACC_DESCR: i32 = 5;
    pub const ACC_TITLE: i32 = 6;
    pub const TITLE: i32 = 7;
    pub const STRING: i32 = 8;
    pub const NEWLINE: i32 = 9;
}

const DOMAIN_NAMES: [&str; 5] = ["complex", "complicated", "clear", "chaotic", "confusion"];

fn tokens() -> Vec<TokenType> {
    vec![
        TokenType::lit("cynefin-beta:", "cynefin-beta:"),
        keyword("cynefin-beta"),
        TokenType::lit("-->", "-->"),
        TokenType::lit(":", ":"),
        // /complex|complicated|clear|chaotic|confusion/y
        TokenType::re("DOMAIN_NAME", None, |src, p| DOMAIN_NAMES.iter().find(|w| w.encode_utf16().enumerate().all(|(k, u)| src.get(p + k) == Some(&u))).map(|w| p + w.len())),
        acc_descr(),
        acc_title(),
        title(),
        string(),
        newline_token(),
        whitespace(),
        yaml(),
        directive(),
        single_line_comment(),
    ]
}

pub struct CynefinItem {
    pub label: String,
}

pub struct CynefinDomain {
    pub name: String,
    pub items: Vec<CynefinItem>,
}

pub struct CynefinTransition {
    pub from: String,
    pub to: String,
    pub label: Option<String>,
}

pub struct CynefinModel {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    // The domains the source declares, in insertion order. All five are drawn either way.
    pub domains: Vec<CynefinDomain>,
    pub transitions: Vec<CynefinTransition>,
}

impl CynefinModel {
    pub fn domain(&self, name: &str) -> Option<&CynefinDomain> {
        self.domains.iter().find(|d| d.name == name)
    }
}

struct DomainBlock {
    domain: String,
    items: Vec<String>,
}

#[derive(Default)]
struct CynefinAst {
    title: Option<String>,
    acc_title: Option<String>,
    acc_descr: Option<String>,
    domains: Vec<DomainBlock>,
    transitions: Vec<CynefinTransition>,
}

fn parse_cynefin(src: &[u16]) -> Result<CynefinAst, PeleError> {
    let types = tokens();
    let tokens = tokenize(src, &types, "cynefin")?;
    let mut r = Reader::new(&tokens, &types, "cynefin");
    let mut ast = CynefinAst::default();

    fn skip_lines(r: &mut Reader) {
        while r.kind() == t::NEWLINE {
            r.i += 1;
        }
    }
    fn end_of_line(r: &mut Reader) -> Result<(), PeleError> {
        if r.kind() == -1 {
            return Ok(());
        }
        if r.kind() != t::NEWLINE {
            return Err(r.fail("a line break or the end of input"));
        }
        skip_lines(r);
        Ok(())
    }

    skip_lines(&mut r);
    if r.kind() != t::KEYWORD && r.kind() != t::KEYWORD_COLON {
        return Err(r.fail("'cynefin-beta'"));
    }
    r.i += 1;
    while r.kind() != -1 {
        match r.kind() {
            t::NEWLINE => r.i += 1,
            t::ACC_DESCR => {
                ast.acc_descr = Some(acc_descr_value(&r.take()));
                end_of_line(&mut r)?;
            }
            t::ACC_TITLE => {
                ast.acc_title = Some(acc_title_value(&r.take()));
                end_of_line(&mut r)?;
            }
            t::TITLE => {
                ast.title = Some(title_value(&r.take()));
                end_of_line(&mut r)?;
            }
            t::DOMAIN => {
                let name = r.take();
                if r.accept(t::ARROW) {
                    let to = r.expect(t::DOMAIN)?;
                    let mut transition = CynefinTransition { from: name, to, label: None };
                    if r.accept(t::COLON) {
                        transition.label = Some(string_value(&r.expect(t::STRING)?));
                    }
                    end_of_line(&mut r)?;
                    ast.transitions.push(transition);
                    continue;
                }
                let mut block = DomainBlock { domain: name, items: Vec::new() };
                skip_lines(&mut r);
                while r.kind() == t::STRING {
                    block.items.push(string_value(&r.take()));
                    skip_lines(&mut r);
                }
                ast.domains.push(block);
            }
            _ => return Err(r.fail("a domain, a transition, a title, or a line break")),
        }
    }
    Ok(ast)
}

// `.replace(/\n\s+/g, '\n')`
pub fn collapse_indents(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '\n' && chars.peek().is_some_and(|&n| is_space(n as u32)) {
            while chars.peek().is_some_and(|&n| is_space(n as u32)) {
                chars.next();
            }
        }
    }
    out
}

fn build_cynefin(ast: CynefinAst, title: Option<&str>) -> CynefinModel {
    let mut domains: Vec<CynefinDomain> = Vec::new();
    // A later block for a domain replaces an earlier one.
    for block in ast.domains {
        let domain = CynefinDomain { name: block.domain.clone(), items: block.items.into_iter().map(|label| CynefinItem { label }).collect() };
        match domains.iter_mut().find(|d| d.name == block.domain) {
            Some(slot) => *slot = domain,
            None => domains.push(domain),
        }
    }
    let transitions = ast.transitions.into_iter().filter(|t| t.from != t.to).map(|t| CynefinTransition { label: t.label.filter(|l| !l.is_empty()), ..t }).collect();
    CynefinModel {
        title: ast.title.filter(|t| !t.is_empty()).or(title.map(str::to_string)),
        acc_title: ast.acc_title.filter(|t| !t.is_empty()).map(|t| js::trim_start(&t).to_string()),
        acc_descr: ast.acc_descr.filter(|t| !t.is_empty()).map(|t| collapse_indents(&t)),
        domains,
        transitions,
    }
}

pub static DIAGRAM: Diagram = Diagram {
    type_: "cynefin",
    section: None,
    keep_comments: false,
    parse: |source, _config, title, _line_offset| Ok(Box::new(build_cynefin(parse_cynefin(source)?, title))),
    render: |model, config, options| Ok(render::render_cynefin(model.downcast_ref::<CynefinModel>().unwrap(), config, options)),
};
