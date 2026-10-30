// Port of src/diagrams/railroad/common.ts and types.ts: what the four railroad grammars share,
// and the rule model. Trees live in arenas and are walked without recursion, as upstream's
// `fold` does, so nesting depth is bounded by memory, not by the stack.

use crate::diagrams::common::tokens::{
    acc_descr_value, acc_title_value, directive, keyword, rest_before_comment, single_line_comment, string, title_value, tokenize, yaml, Reader, Scanner, TokenType,
};
use crate::errors::PeleError;
use crate::js;
use std::rc::Rc;

pub enum RailroadNode {
    Terminal(String),
    NonTerminal(String),
    Special(String),
    Sequence(Vec<usize>),
    Choice(Vec<usize>),
    Optional(usize),
    // max is Infinity when the count has no upper bound.
    Repetition(usize, f64, f64),
}

impl RailroadNode {
    pub fn parts(&self) -> &[usize] {
        match self {
            RailroadNode::Sequence(k) | RailroadNode::Choice(k) => k,
            RailroadNode::Optional(k) | RailroadNode::Repetition(k, _, _) => std::slice::from_ref(k),
            _ => &[],
        }
    }
}

/// The nodes of every rule; a rule's definition is an index into it.
#[derive(Default)]
pub struct Nodes(pub Vec<RailroadNode>);

impl Nodes {
    pub fn add(&mut self, node: RailroadNode) -> usize {
        self.0.push(node);
        self.0.len() - 1
    }
    pub fn terminal(&mut self, value: String) -> usize {
        self.add(RailroadNode::Terminal(value))
    }
    pub fn nonterminal(&mut self, name: String) -> usize {
        self.add(RailroadNode::NonTerminal(name))
    }
    pub fn special(&mut self, text: String) -> usize {
        self.add(RailroadNode::Special(text))
    }
    pub fn optional(&mut self, element: usize) -> usize {
        self.add(RailroadNode::Optional(element))
    }
    pub fn repetition(&mut self, element: usize, min: f64, max: f64) -> usize {
        self.add(RailroadNode::Repetition(element, min, max))
    }
    // A sequence or a choice of one is that one, in every notation.
    pub fn sequence(&mut self, elements: Vec<usize>) -> usize {
        if elements.len() == 1 {
            elements[0]
        } else {
            self.add(RailroadNode::Sequence(elements))
        }
    }
    pub fn choice(&mut self, alternatives: Vec<usize>) -> usize {
        if alternatives.len() == 1 {
            alternatives[0]
        } else {
            self.add(RailroadNode::Choice(alternatives))
        }
    }
}

/// A syntax tree node of any of the four notations: a tag, its text, and its parts in the order
/// upstream's `parts()` lists them.
pub struct Part {
    pub tag: u8,
    pub text: String,
    pub op: Option<String>,
    pub kids: Vec<usize>,
    // EBNF terms: the postfix operators, in order (b'?', b'*', b'+', or b'-' for an exception).
    pub postfixes: Vec<u8>,
}

#[derive(Default)]
pub struct Parts(pub Vec<Part>);

impl Parts {
    pub fn add(&mut self, tag: u8, text: String) -> usize {
        self.0.push(Part { tag, text, op: None, kids: Vec::new(), postfixes: Vec::new() });
        self.0.len() - 1
    }
}

/// Rebuilds a tree bottom-up without recursion.
pub fn fold<R>(root: usize, children: impl Fn(usize) -> Vec<usize>, mut build: impl FnMut(usize, Vec<R>) -> R) -> R {
    let mut nodes = vec![root];
    let mut lists = vec![children(root)];
    let mut done: Vec<Vec<R>> = vec![Vec::new()];
    loop {
        let top = nodes.len() - 1;
        if done[top].len() < lists[top].len() {
            let child = lists[top][done[top].len()];
            nodes.push(child);
            lists.push(children(child));
            done.push(Vec::new());
        } else {
            let results = done.pop().unwrap();
            let value = build(nodes[top], results);
            if top == 0 {
                return value;
            }
            nodes.pop();
            lists.pop();
            done[top - 1].push(value);
        }
    }
}

fn at(src: &[u16], i: usize) -> u32 {
    src.get(i).map_or(u32::MAX, |&c| c as u32)
}

fn starts(src: &[u16], p: usize, lit: &str) -> bool {
    lit.bytes().enumerate().all(|(k, b)| at(src, p + k) == b as u32)
}

pub fn is_space(c: u32) -> bool {
    c == 32 || (9..=13).contains(&c) || (c > 127 && js::is_space_u(c))
}

pub fn is_word(c: u32) -> bool {
    crate::util::chars::is_word(c)
}

// The braced form scans to the next closing brace. Once none is found, none exists further on
// either, so later braced descriptions fail without scanning again.
fn acc_descr_scanner() -> Scanner {
    let mut unclosed = usize::MAX;
    Box::new(move |src, p, _| {
        if !starts(src, p, "accDescr") {
            return None;
        }
        let n = src.len();
        let mut i = p + 8;
        while i < n && (src[i] == 32 || src[i] == 9) {
            i += 1;
        }
        if at(src, i) == 58 {
            i += 1;
            while i < n {
                let c = src[i];
                if c == 10 || c == 13 || (c == 37 && at(src, i + 1) == 37) {
                    break;
                }
                i += 1;
            }
            return Some(i);
        }
        while i < n && is_space(src[i] as u32) {
            i += 1;
        }
        if at(src, i) != 123 || i >= unclosed {
            return None;
        }
        match src[i + 1..].iter().position(|&c| c == 125) {
            Some(k) => Some(i + 1 + k + 1),
            None => {
                unclosed = i;
                None
            }
        }
    })
}

pub const LETTERS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

// `rest` matches a name suffix so `title` does not split longer names.
pub fn title(rest: fn(u32) -> bool) -> TokenType {
    TokenType::re("TITLE", Some("t"), move |src, p| {
        if !starts(src, p, "title") {
            return None;
        }
        let q = p + 5;
        if matches!(at(src, q), 9 | 32) {
            return Some(rest_before_comment(src, q + 1));
        }
        (!rest(at(src, q))).then_some(q)
    })
}

pub fn acc_title() -> TokenType {
    TokenType::re("ACC_TITLE", Some("a"), |src, p| {
        if !starts(src, p, "accTitle") {
            return None;
        }
        let mut q = p + 8;
        while matches!(at(src, q), 9 | 32) {
            q += 1;
        }
        (at(src, q) == 58).then(|| rest_before_comment(src, q + 1))
    })
}

pub fn acc_descr() -> TokenType {
    let mut t = TokenType::re("ACC_DESCR", Some("a"), |_, _| None);
    t.scanner = Some(Rc::new(acc_descr_scanner));
    t
}

pub fn literal(name: &'static str, longer: &[usize]) -> TokenType {
    let mut t = TokenType::lit(name, name);
    t.longer = longer.to_vec();
    t
}

// The diagram's keyword. A name that starts with it and goes on is a name.
pub fn opening(word: &'static str, name: usize) -> TokenType {
    let mut t = keyword(word);
    t.longer = vec![name];
    t
}

pub fn identifier(name: &str) -> TokenType {
    TokenType::re(name, Some("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_"), |src, p| {
        let c = at(src, p);
        if !((65..=90).contains(&c) || (97..=122).contains(&c) || c == 95) {
            return None;
        }
        let mut e = p + 1;
        while is_word(at(src, e)) || at(src, e) == 45 {
            e += 1;
        }
        Some(e)
    })
}

pub fn quoted(name: &str) -> TokenType {
    string().named(name)
}

// Whitespace, front matter, directive, and `%%` comment, in the order every railroad grammar lists them.
pub fn hidden_tokens(prefix: &str) -> Vec<TokenType> {
    vec![
        TokenType::re(&format!("{}_WHITESPACE", prefix), Some("\t \r\n"), |src, p| {
            let mut e = p;
            while matches!(at(src, e), 9 | 32 | 13 | 10) {
                e += 1;
            }
            (e > p).then_some(e)
        })
        .hidden(),
        yaml().named(&format!("{}_YAML", prefix)),
        directive().named(&format!("{}_DIRECTIVE", prefix)),
        single_line_comment().named(&format!("{}_SINGLE_LINE_COMMENT", prefix)),
    ]
}

/// A hidden comment from `open` to the first `close` after it, never reached again once unclosed.
pub fn block_comment(name: &str, open: &'static str, close: &'static str, first: &'static str) -> TokenType {
    let mut t = TokenType::re(name, Some(first), move |src, p| {
        if !starts(src, p, open) {
            return None;
        }
        (p + open.len()..src.len()).find(|&k| starts(src, k, close)).map(|k| k + close.len())
    })
    .hidden();
    t.opener = Some(Rc::new(move |src: &[u16], p: usize| starts(src, p, open).then_some(p + open.len())));
    t
}

// Mermaid's railroad value converters know three escapes; any other escaped character stands for itself.
pub fn unescape(text: &str) -> String {
    let c: Vec<u16> = text.encode_utf16().collect();
    if !text.contains('\\') {
        return js::from16(&c[1.min(c.len())..c.len().saturating_sub(1).max(1.min(c.len()))]);
    }
    let end = c.len() - 1;
    let mut out: Vec<u16> = Vec::new();
    let mut i = 1;
    while i < end {
        let mut ch = c[i];
        if ch == 92 && i + 1 < end {
            i += 1;
            ch = c[i];
            out.push(match ch {
                110 => 10,
                114 => 13,
                116 => 9,
                other => other,
            });
        } else {
            out.push(ch);
        }
        i += 1;
    }
    js::from16(&out)
}

#[derive(Default)]
pub struct Grammar {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    // (name, root part)
    pub rules: Vec<(String, usize)>,
}

// Parses the keyword, then any titles and accessibility statements, then rules to the end of the text.
// Every table starts with its keyword and has TITLE, ACC_TITLE, and ACC_DESCR in a row from `meta`.
pub fn parse_grammar(
    src: &[u16],
    types: &[TokenType],
    meta: i32,
    unquote: fn(&str) -> String,
    parts: &mut Parts,
    mut rule: impl FnMut(&mut Reader, &mut Parts) -> Result<(String, usize), PeleError>,
) -> Result<Grammar, PeleError> {
    let tokens = tokenize(src, types, "railroad")?;
    let mut r = Reader::new(&tokens, types, "railroad");
    let mut ast = Grammar::default();
    r.expect(0)?;
    loop {
        let kind = r.kind();
        if kind == meta {
            let title = title_value(&r.take());
            let quote = title.chars().next();
            ast.title = Some(match quote {
                Some(q @ ('"' | '\'')) if title.ends_with(q) => unquote(&title),
                _ => title,
            });
        } else if kind == meta + 1 {
            ast.acc_title = Some(acc_title_value(&r.take()));
        } else if kind == meta + 2 {
            ast.acc_descr = Some(acc_descr_value(&r.take()));
        } else {
            break;
        }
    }
    while r.kind() != -1 {
        let rule = rule(&mut r, parts)?;
        ast.rules.push(rule);
    }
    Ok(ast)
}

/// Turns each rule's syntax tree into the common model, given how one node is built from its parts.
pub fn to_rules(ast: &Grammar, parts: &Parts, nodes: &mut Nodes, mut build: impl FnMut(&Part, Vec<usize>, &mut Nodes) -> usize) -> Vec<(String, usize)> {
    ast.rules.iter().map(|(name, root)| (name.clone(), fold(*root, |k| parts.0[k].kids.clone(), |k, kids| build(&parts.0[k], kids, nodes)))).collect()
}
