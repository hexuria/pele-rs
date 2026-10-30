// Port of src/diagrams/er/parser.ts.

use super::db::ErDb;
use super::lexer::tokenize;
use super::tokens::{self as t, TOKEN_NAMES};
use super::types::{DocItem, ParsedAttribute, RelSpec};
use crate::errors::{syntax_error, PeleError};

fn cardinality(ty: u32) -> Option<&'static str> {
    match ty {
        t::ZERO_OR_ONE => Some("ZERO_OR_ONE"),
        t::ZERO_OR_MORE => Some("ZERO_OR_MORE"),
        t::ONE_OR_MORE => Some("ONE_OR_MORE"),
        t::ONLY_ONE => Some("ONLY_ONE"),
        t::MD_PARENT => Some("MD_PARENT"),
        _ => None,
    }
}

const DIRECTIONS: [&str; 4] = ["TB", "BT", "RL", "LR"];

const NAME: &str = "'UNICODE_TEXT', 'NUM', 'ENTITY_NAME', 'DECIMAL_NUM', 'ENTITY_ONE'";
const CARDS: &str = "'ZERO_OR_ONE', 'ZERO_OR_MORE', 'ONE_OR_MORE', 'ONLY_ONE', 'MD_PARENT'";

fn line() -> String {
    format!("'EOF', 'NEWLINE', 'title', 'acc_title', 'acc_descr', 'acc_descr_multiline_value', 'SUBGRAPH', 'direction_tb', 'direction_bt', 'direction_rl', 'direction_lr', 'CLASSDEF', 'CLASS', 'STYLE', {}", NAME)
}

struct Frame {
    id: String,
    title: String,
    // What Mermaid's grammar hands to addSubGraph: the value of every statement inside. Besides
    // entity names that includes accessibility text and the keywords `style`, `class` and `classDef`.
    list: Vec<DocItem>,
}

struct P<'a> {
    src: &'a [u16],
    types: Vec<u32>,
    texts: Vec<String>,
    starts: Vec<usize>,
    i: usize,
}

// String.slice(1, -1) on a token that starts and ends with a quote.
fn unquote(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    if c.len() < 2 {
        return String::new();
    }
    c[1..c.len() - 1].iter().collect()
}

fn is_name(ty: u32) -> bool {
    matches!(ty, t::UNICODE_TEXT | t::ENTITY_NAME | t::NUM | t::DECIMAL_NUM | t::ENTITY_ONE)
}

fn is_style_component(ty: u32) -> bool {
    matches!(ty, t::STYLE_TEXT | t::NUM | t::COLON | t::BRKT)
}

impl P<'_> {
    fn ty(&self) -> u32 {
        self.types.get(self.i).copied().unwrap_or(t::END)
    }

    fn take(&mut self) -> String {
        let s = self.texts.get(self.i).cloned().unwrap_or_default();
        self.i += 1;
        s
    }

    fn fail(&self, expected: &str) -> PeleError {
        let ty = self.ty();
        let at = self.starts.get(self.i).copied().unwrap_or(self.src.len());
        if ty == t::ERROR {
            return syntax_error("er", self.src, at, "", true);
        }
        let got = if ty == t::CHAR { self.texts[self.i].clone() } else { TOKEN_NAMES[ty as usize].to_string() };
        syntax_error("er", self.src, at, &format!("Expecting {}, got '{}'", expected, got), false)
    }

    fn expect(&mut self, ty: u32) -> Result<String, PeleError> {
        if self.ty() != ty {
            return Err(self.fail(&format!("'{}'", TOKEN_NAMES[ty as usize])));
        }
        Ok(self.take())
    }

    fn separator(&mut self) -> Result<(), PeleError> {
        if !matches!(self.ty(), t::NEWLINE | t::SEMI | t::EOF) {
            return Err(self.fail("'EOF', 'NEWLINE', 'SEMI'"));
        }
        self.i += 1;
        Ok(())
    }

    fn entity_name(&mut self) -> Result<String, PeleError> {
        let ty = self.ty();
        if !is_name(ty) {
            return Err(self.fail(NAME));
        }
        let text = self.take();
        Ok(if ty == t::ENTITY_NAME { unquote(&text) } else { text })
    }

    fn id_list(&mut self) -> Result<Vec<String>, PeleError> {
        let mut ids = Vec::new();
        loop {
            if !matches!(self.ty(), t::UNICODE_TEXT | t::STYLE_TEXT) {
                return Err(self.fail("'UNICODE_TEXT', 'STYLE_TEXT'"));
            }
            ids.push(self.take());
            if self.ty() != t::COMMA {
                return Ok(ids);
            }
            self.i += 1;
        }
    }

    fn styles_opt(&mut self) -> Result<Vec<String>, PeleError> {
        let mut styles = Vec::new();
        loop {
            if !is_style_component(self.ty()) {
                return Err(self.fail("'STYLE_TEXT', 'COLON', 'NUM', 'BRKT'"));
            }
            let mut style = self.take();
            while is_style_component(self.ty()) {
                style.push_str(&self.take());
            }
            styles.push(style);
            if self.ty() != t::COMMA {
                return Ok(styles);
            }
            self.i += 1;
        }
    }

    fn rel_spec(&mut self) -> Result<RelSpec, PeleError> {
        let card_b = cardinality(self.ty()).unwrap();
        self.i += 1;
        let r = self.ty();
        if r != t::IDENTIFYING && r != t::NON_IDENTIFYING {
            return Err(self.fail("'NON_IDENTIFYING', 'IDENTIFYING'"));
        }
        self.i += 1;
        let Some(card_a) = cardinality(self.ty()) else { return Err(self.fail(CARDS)) };
        self.i += 1;
        Ok(RelSpec { card_a, identifying: r == t::IDENTIFYING, card_b })
    }

    fn role(&mut self) -> Result<String, PeleError> {
        let ty = self.ty();
        if ty != t::WORD && ty != t::ENTITY_NAME && ty != t::UNICODE_TEXT {
            return Err(self.fail("'UNICODE_TEXT', 'ENTITY_NAME', 'WORD'"));
        }
        let text = self.take();
        Ok(if ty == t::UNICODE_TEXT { text } else { unquote(&text) })
    }

    fn is_char(&self, ch: &str) -> bool {
        self.ty() == t::CHAR && self.texts[self.i] == ch
    }

    // Returns the attributes last to first, as Mermaid's right-recursive rule builds them.
    fn attributes(&mut self) -> Result<Vec<ParsedAttribute>, PeleError> {
        let mut list = Vec::new();
        loop {
            let mut type_ = self.expect(t::ATTRIBUTE_WORD)?;
            if self.is_char("?") {
                type_.push_str(&self.take());
            }
            if self.ty() != t::ATTRIBUTE_WORD {
                return Err(self.fail(if type_.ends_with('?') { "'ATTRIBUTE_WORD'" } else { "'ATTRIBUTE_WORD', '?'" }));
            }
            let name = self.take();
            let mut attribute = ParsedAttribute { type_, name, keys: None, comment: None };
            if self.ty() == t::ATTRIBUTE_KEY {
                let mut keys = vec![self.take()];
                while self.is_char(",") {
                    self.i += 1;
                    keys.push(self.expect(t::ATTRIBUTE_KEY)?);
                }
                attribute.keys = Some(keys);
            }
            if self.ty() == t::COMMENT {
                attribute.comment = Some(unquote(&self.take()));
            }
            list.push(attribute);
            if self.ty() == t::BLOCK_STOP {
                break;
            }
        }
        list.reverse();
        Ok(list)
    }

    // Parses an optional `{ ... }` block; None when there is none or it is empty.
    fn block(&mut self) -> Result<Option<Vec<ParsedAttribute>>, PeleError> {
        if self.ty() != t::BLOCK_START {
            return Ok(None);
        }
        self.i += 1;
        if self.ty() == t::BLOCK_STOP {
            self.i += 1;
            return Ok(None);
        }
        let list = self.attributes()?;
        self.i += 1;
        Ok(Some(list))
    }

    fn entity_statement(&mut self, db: &mut ErDb) -> Result<Vec<String>, PeleError> {
        let a = self.entity_name()?;
        let mut ty = self.ty();
        if ty == t::SQS {
            self.i += 1;
            let alias = self.entity_name()?;
            self.expect(t::SQE)?;
            let mut classes = None;
            if self.ty() == t::STYLE_SEPARATOR {
                self.i += 1;
                classes = Some(self.id_list()?);
            }
            let attrs = self.block()?;
            db.add_entity(&a, &alias);
            if let Some(attrs) = attrs {
                db.add_attributes(&a, attrs);
            }
            if let Some(classes) = classes {
                db.set_class(std::slice::from_ref(&a), &classes);
            }
            return Ok(vec![a]);
        }
        let mut classes = None;
        if ty == t::STYLE_SEPARATOR {
            self.i += 1;
            classes = Some(self.id_list()?);
            ty = self.ty();
        }
        if cardinality(ty).is_some() {
            let spec = self.rel_spec()?;
            let b = self.entity_name()?;
            let mut classes_b = None;
            if self.ty() == t::STYLE_SEPARATOR {
                self.i += 1;
                classes_b = Some(self.id_list()?);
            }
            self.expect(t::COLON)?;
            let label = self.role()?;
            db.add_entity(&a, "");
            db.add_entity(&b, "");
            db.add_relationship(&a, &label, &b, spec);
            if let Some(c) = classes {
                db.set_class(std::slice::from_ref(&a), &c);
            }
            if let Some(c) = classes_b {
                db.set_class(std::slice::from_ref(&b), &c);
            }
            return Ok(vec![a, b]);
        }
        let attrs = self.block()?;
        db.add_entity(&a, "");
        if let Some(attrs) = attrs {
            db.add_attributes(&a, attrs);
        }
        if let Some(c) = classes {
            db.set_class(std::slice::from_ref(&a), &c);
        }
        Ok(vec![a])
    }
}

pub fn parse_er(src: &[u16], db: &mut ErDb) -> Result<(), PeleError> {
    let tokens = tokenize(src);
    let mut p = P { src, types: tokens.types, texts: tokens.texts, starts: tokens.starts, i: 0 };
    p.expect(t::ER_DIAGRAM)?;
    // Subgraphs nest on this stack rather than on the call stack; the first frame is the diagram itself.
    let mut frames: Vec<Frame> = vec![Frame { id: String::new(), title: String::new(), list: Vec::new() }];
    let mut nested = false;
    let line = line();

    loop {
        let ty = p.ty();
        match ty {
            t::NEWLINE => p.i += 1,
            t::EOF => {
                p.i += 1;
                if nested {
                    return Err(p.fail(&format!("'END', {}", line)));
                }
                return Ok(());
            }
            t::END_KW => {
                if !nested {
                    return Err(p.fail(&line));
                }
                p.i += 1;
                let frame = frames.pop().unwrap();
                let id = db.add_sub_graph(&frame.id, &frame.list, &frame.title);
                nested = frames.len() > 1;
                if nested {
                    frames.last_mut().unwrap().list.push(DocItem::Name(id));
                }
            }
            t::SUBGRAPH => {
                p.i += 1;
                let id = p.entity_name()?;
                let mut title = id.clone();
                if p.ty() == t::SQS {
                    p.i += 1;
                    title = p.entity_name()?;
                    while is_name(p.ty()) {
                        title.push(' ');
                        title.push_str(&p.entity_name()?);
                    }
                    p.expect(t::SQE)?;
                }
                p.separator()?;
                frames.push(Frame { id, title, list: Vec::new() });
                nested = true;
            }
            t::acc_title => {
                p.i += 1;
                let value = crate::js::trim(&p.expect(t::acc_title_value)?).to_string();
                db.set_acc_title(&value);
                if nested {
                    frames.last_mut().unwrap().list.push(DocItem::Name(value));
                }
            }
            t::acc_descr => {
                p.i += 1;
                let value = crate::js::trim(&p.expect(t::acc_descr_value)?).to_string();
                db.set_acc_description(&value);
                if nested {
                    frames.last_mut().unwrap().list.push(DocItem::Name(value));
                }
            }
            t::acc_descr_multiline_value => {
                let value = crate::js::trim(&p.take()).to_string();
                db.set_acc_description(&value);
                if nested {
                    frames.last_mut().unwrap().list.push(DocItem::Name(value));
                }
            }
            t::direction_tb | t::direction_bt | t::direction_rl | t::direction_lr => {
                p.i += 1;
                let value = DIRECTIONS[(ty - t::direction_tb) as usize];
                if nested {
                    frames.last_mut().unwrap().list.push(DocItem::Dir(value));
                } else {
                    db.set_direction(value);
                }
            }
            t::CLASSDEF => {
                let keyword = p.take();
                let ids = p.id_list()?;
                let styles = p.styles_opt()?;
                p.separator()?;
                db.add_class(&ids, &styles);
                if nested {
                    frames.last_mut().unwrap().list.push(DocItem::Name(keyword));
                }
            }
            t::CLASS => {
                let keyword = p.take();
                let ids = p.id_list()?;
                let names = p.id_list()?;
                db.set_class(&ids, &names);
                if nested {
                    frames.last_mut().unwrap().list.push(DocItem::Name(keyword));
                }
            }
            t::STYLE => {
                let keyword = p.take();
                let ids = p.id_list()?;
                let styles = p.styles_opt()?;
                p.separator()?;
                db.add_css_styles(&ids, &styles);
                if nested {
                    frames.last_mut().unwrap().list.push(DocItem::Name(keyword));
                }
            }
            _ => {
                if !is_name(ty) {
                    return Err(p.fail(&if nested { format!("'END', {}", line) } else { line.clone() }));
                }
                let names = p.entity_statement(db)?;
                if nested {
                    frames.last_mut().unwrap().list.extend(names.into_iter().map(DocItem::Name));
                }
            }
        }
    }
}
