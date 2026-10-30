// Port of src/diagrams/usecase/parser.ts. Mermaid parses use case diagrams with a Chevrotain
// parser, which decides each choice by looking at most three tokens ahead and reports the first
// token that fits nowhere. This parser makes the same decisions from the same lookahead. The
// statement list upstream keeps for editors is not built: nothing drawn depends on it.

use super::db::{resolve_usecase, DraftBoundary, DraftElement, DraftEndpoint, DraftJson, DraftLabel, DraftProperty, DraftRelationship, Drafts, PropertyValue, Span};
use super::json::parse_ordered_json;
use super::lexer::{line_column, location_text, t, token_index, token_name, tokenize};
use super::{arrow, UsecaseModel};
use crate::errors::{PeleError, PeleErrorCode};
use crate::js;

const LINE: u8 = 1;
const RELATION: u8 = 2;
const STYLE_PART: u8 = 4;
const FOLLOWS_NAME: u8 = 8;
const BOUNDARY_LINE: u8 = 16;
const CLOSES_LABEL: u8 = 32;
const METADATA_FIRST: u8 = 64;

const ARROWS: &str = "DEPENDENCY_ARROW|GENERALIZATION|FORWARD_SOLID|BACKWARD_SOLID|MARKERLESS_SOLID|FORWARD_CIRCLE|BACKWARD_CIRCLE|FORWARD_CROSS|BACKWARD_CROSS";
const NAMES: &str = "IDENTIFIER|PLAIN_STRING|MARKDOWN_STRING";
const LABELS: &str = "PLAIN_STRING|MARKDOWN_STRING|LABEL_TEXT";
const STATEMENTS: &str = "ACC_TITLE_LINE|ACC_DESCR_LINE|ACC_DESCR_BLOCK|DIRECTION|ACTOR|SYSTEM_BOUNDARY|NOTE|JSON_DECLARATION_START|CLASS_DEF|CLASS|STYLE";
const STYLE_PARTS: &str =
    "WORD|PLAIN_STRING|NUMBER|HASH_COLOR|CSS_IDENTIFIER|CSS_ESCAPED_COMMA|DASH|DOT|PERCENT|CSS_PUNCTUATION|COLON|LPAREN|RPAREN|LBRACKET|RBRACKET|LBRACE|RBRACE|AT|MARKERLESS_SOLID";

fn flags() -> [u8; 67] {
    let mut f = [0u8; 67];
    let mut flag = |names: &str, bits: u8| {
        for name in names.split('|') {
            // WORD is a category: every keyword and IDENTIFIER.
            if name == "WORD" {
                for k in t::USECASE..=t::IDENTIFIER {
                    f[k as usize] |= bits;
                }
            } else {
                f[token_index(name) as usize] |= bits;
            }
        }
    };
    flag(&format!("NEWLINE|COMMENT|{}|{}", STATEMENTS, NAMES), LINE);
    flag(&format!("IDENTIFIER|{}", ARROWS), RELATION);
    flag(STYLE_PARTS, STYLE_PART);
    flag(&format!("STEREOTYPE_START|CLASS_SEPARATOR|IDENTIFIER|{}|NEWLINE|EOF", ARROWS), FOLLOWS_NAME);
    flag(&format!("NEWLINE|COMMENT|ACTOR|{}", NAMES), BOUNDARY_LINE);
    flag("FORWARD_SOLID|MARKERLESS_SOLID|FORWARD_CIRCLE|FORWARD_CROSS", CLOSES_LABEL);
    flag("NEWLINE|IDENTIFIER|PLAIN_STRING|RBRACE", METADATA_FIRST);
    f
}

const FORBIDDEN: [&str; 5] = ["allowmixing", "newpage", "package", "rectangle", "skinparam"];

#[derive(Clone)]
struct Item {
    id: String,
    label: DraftLabel,
    span: Span,
    generated: bool,
    shape: Option<&'static str>,
    metadata: Option<Vec<DraftProperty>>,
    stereotype: Option<String>,
    classes: Vec<String>,
    // Whether this mention says something about the element, and so declares it.
    explicit: bool,
}

struct Tail {
    explicit_id: Option<String>,
    explicit_id_span: Option<Span>,
    type_: &'static str,
    arrow_type: u8,
    label: Option<DraftLabel>,
    minlen: f64,
    target: Item,
    span: Span,
}

type R<T> = Result<T, PeleError>;

struct Parser<'a> {
    src: &'a [u16],
    types: Vec<u8>,
    starts: Vec<usize>,
    ends: Vec<usize>,
    p: usize,
    flags: [u8; 67],
    drafts: Drafts,
    // Index into drafts.boundaries.
    parent: Option<usize>,
    pending_json: Vec<(usize, usize)>,
}

// `label.replace(/\W/g, "_")`, one underscore per UTF-16 unit.
fn generated_id(label: &str) -> String {
    let mut out = String::new();
    for c in label.chars() {
        if js::is_word(c) {
            out.push(c);
        } else {
            for _ in 0..c.len_utf16() {
                out.push('_');
            }
        }
    }
    out
}

impl<'a> Parser<'a> {
    fn ty(&self, k: usize) -> u8 {
        self.types[k.min(self.types.len() - 1)]
    }

    fn flag(&self, k: usize, bits: u8) -> bool {
        self.flags[self.ty(k) as usize] & bits != 0
    }

    fn image(&self, k: usize) -> String {
        js::from16(&self.src[self.starts[k]..self.ends[k]])
    }

    fn span(&self, k: usize) -> Span {
        (self.starts[k], self.ends[k])
    }

    fn slice(&self, s: Span) -> String {
        js::from16(&self.src[s.0..s.1])
    }

    fn fail(&self, detail: &str) -> PeleError {
        let (line, column) = line_column(self.src, self.starts[self.p]);
        let mut e = PeleError::typed(format!("Error parsing usecase diagram: {} at {}", detail, location_text(self.src, self.starts[self.p], self.ends[self.p])), PeleErrorCode::Syntax, "usecase");
        e.line = line;
        e.column = column;
        e
    }

    fn mismatch(&self, name: &str) -> PeleError {
        self.fail(&format!("Expecting token of type --> {} <-- but found --> '{}' <--", name, self.image(self.p)))
    }

    fn expect(&mut self, k: u8) -> R<usize> {
        if self.ty(self.p) != k {
            return Err(self.mismatch(token_name(k)));
        }
        self.p += 1;
        Ok(self.p - 1)
    }

    // Chevrotain lists every token sequence that would have been accepted.
    fn no_alternative(&self, expected: &str) -> PeleError {
        let list: Vec<String> = expected.split('|').enumerate().map(|(i, s)| format!("  {}. [{}]", i + 1, s)).collect();
        self.fail(&format!("Expecting: one of these possible Token sequences:\n{}\nbut found: '{}'", list.join("\n"), self.image(self.p)))
    }

    fn line_end(&mut self) -> R<()> {
        if self.ty(self.p) == t::NEWLINE {
            self.p += 1;
        } else if self.ty(self.p) != t::EOF {
            return Err(self.no_alternative("NEWLINE|EOF"));
        }
        Ok(())
    }

    // The text of a name or string token, without its quotes.
    fn content(&self, k: usize) -> Span {
        let trim = match self.ty(k) {
            t::MARKDOWN_STRING => 2,
            t::PLAIN_STRING => 1,
            _ => 0,
        };
        (self.starts[k] + trim, self.ends[k] - trim)
    }

    fn token_label(&self, k: usize) -> DraftLabel {
        let at = self.content(k);
        DraftLabel { text: self.slice(at), markdown: self.ty(k) == t::MARKDOWN_STRING, span: at }
    }

    // An unquoted label runs to the next delimiter and is taken from the source as written.
    fn label(&mut self) -> R<DraftLabel> {
        let ty = self.ty(self.p);
        if ty == t::PLAIN_STRING || ty == t::MARKDOWN_STRING {
            self.p += 1;
            return Ok(self.token_label(self.p - 1));
        }
        let is_label_text = |k: u8| (t::USECASE..=t::LABEL_SYMBOL).contains(&k);
        if !is_label_text(ty) {
            return Err(self.no_alternative(LABELS));
        }
        let first = self.p;
        while is_label_text(self.ty(self.p)) {
            self.p += 1;
        }
        let at = (self.starts[first], self.ends[self.p - 1]);
        Ok(DraftLabel { text: self.slice(at), markdown: false, span: at })
    }

    fn is_key(k: u8) -> bool {
        k == t::IDENTIFIER || k == t::PLAIN_STRING
    }

    fn property(&mut self) -> R<DraftProperty> {
        let key = self.p;
        if !Self::is_key(self.ty(self.p)) {
            return Err(self.no_alternative("IDENTIFIER|PLAIN_STRING"));
        }
        self.p += 1;
        self.expect(t::COLON)?;
        let value = self.p;
        let ty = self.ty(self.p);
        if !Self::is_key(ty) && ty != t::TRUE && ty != t::FALSE {
            return Err(self.no_alternative("IDENTIFIER|PLAIN_STRING|TRUE|FALSE"));
        }
        self.p += 1;
        let key_span = self.content(key);
        let value_span = self.content(value);
        Ok(DraftProperty {
            key: self.slice(key_span),
            value: match ty {
                t::TRUE => PropertyValue::Bool(true),
                t::FALSE => PropertyValue::Bool(false),
                _ => PropertyValue::Str(self.slice(value_span)),
            },
            key_span,
        })
    }

    // Whether another property follows. A comma or line breaks before the closing brace do not count,
    // but Chevrotain looks only three tokens ahead, so a comma and two line breaks, or three line
    // breaks, are taken for a separator and the brace after them is then an error.
    fn separator_ahead(&self) -> bool {
        let (a, b, c) = (self.ty(self.p), self.ty(self.p + 1), self.ty(self.p + 2));
        if a == t::COMMA {
            return Self::is_key(b) || (b == t::NEWLINE && (c == t::NEWLINE || Self::is_key(c)));
        }
        if a != t::NEWLINE {
            return false;
        }
        b == t::COMMA || Self::is_key(b) || (b == t::NEWLINE && (c == t::NEWLINE || c == t::COMMA || Self::is_key(c)))
    }

    fn metadata(&mut self) -> R<Vec<DraftProperty>> {
        self.expect(t::METADATA_START)?;
        let mut properties = Vec::new();
        while self.ty(self.p) == t::NEWLINE {
            self.p += 1;
        }
        if Self::is_key(self.ty(self.p)) {
            properties.push(self.property()?);
            while self.separator_ahead() {
                if self.ty(self.p) == t::COMMA {
                    self.p += 1;
                } else {
                    while self.ty(self.p) == t::NEWLINE {
                        self.p += 1;
                    }
                    if self.ty(self.p) == t::COMMA {
                        self.p += 1;
                    }
                }
                while self.ty(self.p) == t::NEWLINE {
                    self.p += 1;
                }
                properties.push(self.property()?);
            }
            if self.ty(self.p) == t::COMMA {
                self.p += 1;
            }
            while self.ty(self.p) == t::NEWLINE {
                self.p += 1;
            }
        }
        self.expect(t::RBRACE)?;
        Ok(properties)
    }

    fn class_suffix(&mut self, classes: &mut Vec<String>) -> R<()> {
        loop {
            self.p += 1;
            let id = self.expect(t::IDENTIFIER)?;
            classes.push(self.image(id));
            if self.ty(self.p) != t::COMMA {
                return Ok(());
            }
        }
    }

    // An id with an optional label in parentheses or brackets, or a string that stands for both.
    fn name(&mut self, brackets: bool) -> R<Item> {
        let k = self.p;
        let ty = self.ty(self.p);
        let mut item = Item {
            id: String::new(),
            label: DraftLabel { text: String::new(), markdown: false, span: (0, 0) },
            span: self.content(k),
            generated: ty != t::IDENTIFIER,
            shape: None,
            metadata: None,
            stereotype: None,
            classes: Vec::new(),
            explicit: false,
        };
        if ty == t::IDENTIFIER {
            self.p += 1;
            item.id = self.image(k);
            let open = self.ty(self.p);
            if open == t::LPAREN || (brackets && open == t::LBRACKET) {
                self.p += 1;
                item.label = self.label()?;
                self.expect(if open == t::LPAREN { t::RPAREN } else { t::RBRACKET })?;
                item.shape = Some(if open == t::LPAREN { "ellipse" } else { "rect" });
            } else {
                item.label = self.token_label(k);
            }
        } else if ty == t::PLAIN_STRING || ty == t::MARKDOWN_STRING {
            self.p += 1;
            item.label = self.token_label(k);
            item.id = generated_id(&item.label.text);
        } else {
            return Err(self.no_alternative(NAMES));
        }
        Ok(item)
    }

    fn element(&mut self, actor: bool) -> R<Item> {
        let mut item = self.name(!actor)?;
        if actor {
            item.shape = None;
        }
        if self.ty(self.p) == t::METADATA_START {
            item.metadata = Some(self.metadata()?);
        }
        if self.ty(self.p) == t::STEREOTYPE_START {
            self.p += 1;
            let text = self.expect(t::STEREOTYPE_TEXT)?;
            self.expect(t::STEREOTYPE_END)?;
            item.stereotype = Some(js::trim(&self.image(text)).to_string());
        }
        if self.ty(self.p) == t::CLASS_SEPARATOR {
            let mut classes = std::mem::take(&mut item.classes);
            self.class_suffix(&mut classes)?;
            item.classes = classes;
        }
        item.explicit = item.shape.is_some() || item.metadata.is_some() || item.stereotype.as_deref().is_some_and(|s| !s.is_empty());
        Ok(item)
    }

    fn draft(&self, item: &Item, kind: &'static str) -> DraftElement {
        DraftElement {
            id: item.id.clone(),
            kind,
            label: item.label.clone(),
            span: item.span,
            generated: item.generated,
            parent: self.parent,
            shape: item.shape,
            metadata: item.metadata.clone(),
            stereotype: item.stereotype.clone(),
            classes: item.classes.clone(),
        }
    }

    fn endpoint(item: &Item, declares: bool) -> DraftEndpoint {
        DraftEndpoint { id: item.id.clone(), label: item.label.clone(), span: item.span, generated: item.generated, declares, classed: !item.classes.is_empty() }
    }

    // Whether a label and then one of the operators that can close it come next.
    fn labeled_right(&self, closers: u8) -> bool {
        let in_label = |k: u8| (t::USECASE..=t::MARKDOWN_STRING).contains(&k);
        if !in_label(self.ty(self.p)) {
            return false;
        }
        let mut k = self.p + 1;
        loop {
            let ty = self.ty(k);
            if ty == t::MARKERLESS_SOLID || self.flags[ty as usize] & closers != 0 {
                return true;
            }
            if !in_label(ty) {
                return false;
            }
            k += 1;
        }
    }

    // Each dash beyond the second asks for one more rank between the two ends.
    fn minimum_length(&self, k: usize) -> f64 {
        let dashes = (self.ends[k] - self.starts[k]) as f64 - if self.ty(k) == t::MARKERLESS_SOLID { 0.0 } else { 1.0 };
        (dashes - 1.0).max(1.0)
    }

    fn relation_tail(&mut self) -> R<Tail> {
        let first = self.p;
        let mut explicit_id = None;
        let mut explicit_id_span = None;
        let mut type_ = "association";
        let mut arrow_type = arrow::SOLID_ARROW;
        let mut minlen = 1.0;
        let mut label = None;
        if self.ty(self.p) == t::IDENTIFIER && self.ty(self.p + 1) == t::AT {
            explicit_id = Some(self.image(self.p));
            explicit_id_span = Some(self.span(self.p));
            self.p += 2;
        }
        let k = self.p;
        let ty = self.ty(self.p);
        self.p += 1;
        match ty {
            t::DEPENDENCY_ARROW => {
                self.expect(t::COLON)?;
                if self.ty(self.p) != t::INCLUDE && self.ty(self.p) != t::EXTEND {
                    return Err(self.no_alternative("INCLUDE|EXTEND"));
                }
                type_ = if self.ty(self.p) == t::INCLUDE { "include" } else { "extend" };
                label = Some(DraftLabel { text: type_.into(), markdown: false, span: self.span(self.p) });
                self.p += 1;
            }
            t::GENERALIZATION => type_ = "generalization",
            t::FORWARD_SOLID => minlen = self.minimum_length(k),
            t::BACKWARD_SOLID => {
                arrow_type = arrow::BACK_ARROW;
                // Only the shortest operator can open a label: `A <-- label -- B`.
                if self.ends[k] - self.starts[k] == 3 && self.labeled_right(0) {
                    label = Some(self.label()?);
                    let close = self.expect(t::MARKERLESS_SOLID)?;
                    minlen = self.minimum_length(close);
                } else {
                    minlen = self.minimum_length(k);
                }
            }
            t::MARKERLESS_SOLID => {
                arrow_type = arrow::LINE_SOLID;
                if self.ends[k] - self.starts[k] == 2 && self.labeled_right(CLOSES_LABEL) {
                    label = Some(self.label()?);
                    let close = self.ty(self.p);
                    if close == t::FORWARD_SOLID {
                        arrow_type = arrow::SOLID_ARROW;
                    } else if close == t::FORWARD_CIRCLE {
                        arrow_type = arrow::CIRCLE_ARROW;
                    } else if close == t::FORWARD_CROSS {
                        arrow_type = arrow::CROSS_ARROW;
                    } else if close != t::MARKERLESS_SOLID {
                        return Err(self.no_alternative("FORWARD_SOLID|MARKERLESS_SOLID|FORWARD_CIRCLE|FORWARD_CROSS"));
                    }
                    if arrow_type == arrow::SOLID_ARROW || arrow_type == arrow::LINE_SOLID {
                        minlen = self.minimum_length(self.p);
                    }
                    self.p += 1;
                } else {
                    minlen = self.minimum_length(k);
                }
            }
            t::FORWARD_CIRCLE => arrow_type = arrow::CIRCLE_ARROW,
            t::FORWARD_CROSS => arrow_type = arrow::CROSS_ARROW,
            t::BACKWARD_CIRCLE | t::BACKWARD_CROSS => {
                arrow_type = if ty == t::BACKWARD_CIRCLE { arrow::CIRCLE_ARROW_REVERSED } else { arrow::CROSS_ARROW_REVERSED };
                if self.labeled_right(0) {
                    label = Some(self.label()?);
                    self.expect(t::MARKERLESS_SOLID)?;
                }
            }
            _ => {
                self.p -= 1;
                return Err(self.no_alternative(ARROWS));
            }
        }
        let target = self.element(false)?;
        let span = (self.starts[first], self.ends[self.p - 1]);
        Ok(Tail { explicit_id, explicit_id_span, type_, arrow_type, label, minlen, target, span })
    }

    fn relationship(&mut self, source: &Item, declaration: bool, tail: Tail) {
        let target = &tail.target;
        if target.explicit {
            let d = self.draft(target, "usecase");
            self.drafts.elements.push(d);
        }
        self.drafts.relationships.push(DraftRelationship {
            source: Self::endpoint(source, declaration),
            target: Self::endpoint(target, target.explicit),
            span: tail.span,
            id: tail.explicit_id,
            id_span: tail.explicit_id_span,
            type_: tail.type_,
            arrow_type: tail.arrow_type,
            label: tail.label,
            minlen: tail.minlen,
        });
    }

    // `actor A, B` declares several actors; at the top level `actor A --> B` declares one and relates it.
    fn actors(&mut self, top: bool) -> R<()> {
        self.p += 1;
        let mut items = vec![self.element(true)?];
        let mut tail = None;
        if self.ty(self.p) == t::COMMA {
            while self.ty(self.p) == t::COMMA {
                self.p += 1;
                items.push(self.element(true)?);
            }
        } else if top && self.flag(self.p, RELATION) {
            tail = Some(self.relation_tail()?);
        }
        self.line_end()?;
        for item in &items {
            let d = self.draft(item, "actor");
            self.drafts.elements.push(d);
        }
        if let Some(tail) = tail {
            self.relationship(&items[0], true, tail);
        }
        Ok(())
    }

    fn entity(&mut self, top: bool) -> R<()> {
        let mut source = self.element(false)?;
        let tail = if top && self.flag(self.p, RELATION) { Some(self.relation_tail()?) } else { None };
        self.line_end()?;
        if tail.is_none() {
            source.explicit = true;
            source.shape.get_or_insert("ellipse");
        }
        if source.explicit {
            let d = self.draft(&source, "usecase");
            self.drafts.elements.push(d);
        }
        if let Some(tail) = tail {
            let explicit = source.explicit;
            self.relationship(&source, explicit, tail);
        }
        Ok(())
    }

    fn boundary(&mut self) -> R<()> {
        self.p += 1;
        let named = self.name(true)?;
        let mut classes = named.classes.clone();
        let inline = if self.ty(self.p) == t::METADATA_START { Some(self.metadata()?) } else { None };
        if self.ty(self.p) == t::CLASS_SEPARATOR {
            self.class_suffix(&mut classes)?;
        }
        self.line_end()?;
        self.drafts.boundaries.push(DraftBoundary { id: named.id.clone(), label: named.label.clone(), span: named.span, generated: named.generated, classes });
        self.parent = Some(self.drafts.boundaries.len() - 1);
        while self.flag(self.p, BOUNDARY_LINE) {
            self.line(false)?;
        }
        self.parent = None;
        self.expect(t::END)?;
        self.line_end()?;
        if let Some(inline) = inline {
            self.drafts.metadata.push((named.id.clone(), named.span, inline));
        }
        Ok(())
    }

    fn style(&mut self) -> R<String> {
        let first = self.p;
        let ty = self.ty(self.p);
        let is_word = |k: u8| (t::USECASE..=t::IDENTIFIER).contains(&k);
        if is_word(ty) || ty == t::CSS_IDENTIFIER {
            self.p += 1;
        } else if ty == t::MARKERLESS_SOLID {
            self.p += 1;
            if !is_word(self.ty(self.p)) {
                return Err(self.mismatch("WORD"));
            }
            self.p += 1;
        } else {
            return Err(self.no_alternative("WORD|CSS_IDENTIFIER|MARKERLESS_SOLID"));
        }
        self.expect(t::COLON)?;
        if !self.flag(self.p, STYLE_PART) {
            return Err(self.fail(&format!(
                "Expecting: expecting at least one iteration which starts with one of these possible Token sequences::\n  <[{}]>\nbut found: '{}'",
                STYLE_PARTS.replace('|', "] ,["),
                self.image(self.p)
            )));
        }
        while self.flag(self.p, STYLE_PART) {
            self.p += 1;
        }
        // The value is taken from the source, so the spaces inside `1px solid red` survive.
        Ok(self.slice((self.starts[first], self.ends[self.p - 1])).replace("\\,", ","))
    }

    fn styles(&mut self) -> R<Vec<String>> {
        let mut out = vec![self.style()?];
        while self.ty(self.p) == t::COMMA {
            self.p += 1;
            out.push(self.style()?);
        }
        Ok(out)
    }

    fn identifiers(&mut self) -> R<Vec<usize>> {
        let mut out = vec![self.expect(t::IDENTIFIER)?];
        while self.ty(self.p) == t::COMMA {
            self.p += 1;
            out.push(self.expect(t::IDENTIFIER)?);
        }
        Ok(out)
    }

    fn statement_sequences() -> String {
        let names: Vec<&str> = NAMES.split('|').collect();
        let metadata: Vec<String> = names.iter().flat_map(|name| ["NEWLINE", "IDENTIFIER", "PLAIN_STRING", "RBRACE"].iter().map(move |next| format!("{}, METADATA_START, {}", name, next))).collect();
        let follows: Vec<String> = format!("STEREOTYPE_START|CLASS_SEPARATOR|IDENTIFIER|{}|NEWLINE|EOF", ARROWS).split('|').map(str::to_string).collect();
        let mut entities: Vec<String> = Vec::new();
        for (i, name) in names.iter().enumerate() {
            let mut list: Vec<String> = if i == 0 { vec!["LPAREN".into(), "LBRACKET".into()] } else { Vec::new() };
            list.extend(follows.iter().cloned());
            for next in list {
                entities.push(format!("{}, {}", name, next));
            }
        }
        let mut all = vec![STATEMENTS.to_string()];
        all.extend(metadata.iter().cloned());
        all.extend(entities);
        all.extend(metadata);
        all.join("|")
    }

    fn top_statement(&mut self) -> R<()> {
        let k = self.p;
        let ty = self.ty(self.p);
        if ty == t::ACC_TITLE_LINE || ty == t::ACC_DESCR_LINE || ty == t::ACC_DESCR_BLOCK {
            self.p += 1;
            let text = self.image(k);
            if ty == t::ACC_DESCR_BLOCK {
                let (open, close) = (text.find('{').unwrap(), text.rfind('}').unwrap());
                self.drafts.acc_description = js::trim(&text[open + 1..close]).to_string();
            } else if ty == t::ACC_DESCR_LINE {
                self.drafts.acc_description = js::trim(&text[text.find(':').unwrap() + 1..]).to_string();
            } else {
                self.drafts.acc_title = js::trim(&text[text.find(':').unwrap() + 1..]).to_string();
            }
        } else if ty == t::DIRECTION {
            self.p += 1;
            let value = self.ty(self.p);
            if !(t::TD..=t::RL).contains(&value) {
                return Err(self.no_alternative("TD|TB|BT|LR|RL"));
            }
            self.drafts.direction = Some(if value == t::TD { "TB".to_string() } else { self.image(self.p) });
            self.p += 1;
        } else if ty == t::ACTOR {
            return self.actors(true);
        } else if ty == t::SYSTEM_BOUNDARY {
            return self.boundary();
        } else if ty == t::NOTE {
            self.p += 1;
            self.expect(t::FOR)?;
            let target = self.expect(t::IDENTIFIER)?;
            let text = self.label()?;
            let note = (self.image(target), self.span(target), text);
            self.drafts.notes.push(note);
        } else if ty == t::JSON_DECLARATION_START {
            self.p += 1;
            let literal = self.expect(t::JSON_OBJECT_LITERAL)?;
            let mut classes = Vec::new();
            if self.ty(self.p) == t::CLASS_SEPARATOR {
                self.class_suffix(&mut classes)?;
            }
            let mut from = self.starts[k] + 4;
            while matches!(self.src.get(from), Some(&32) | Some(&9)) {
                from += 1;
            }
            let mut to = from;
            while to < self.src.len() && crate::util::chars::is_word(self.src[to] as u32) {
                to += 1;
            }
            self.drafts.jsons.push(DraftJson { id: self.slice((from, to)), value: Vec::new(), span: (from, to), classes });
            self.pending_json.push((self.drafts.jsons.len() - 1, literal));
        } else if ty == t::CLASS_DEF {
            self.p += 1;
            let ids = self.identifiers()?;
            let ids: Vec<String> = ids.iter().map(|&k| self.image(k)).collect();
            let styles = self.styles()?;
            self.drafts.class_defs.push((ids, styles));
        } else if ty == t::CLASS {
            self.p += 1;
            let targets = self.identifiers()?;
            let classes = self.identifiers()?;
            let targets = targets.iter().map(|&k| (self.image(k), self.span(k))).collect();
            let classes = classes.iter().map(|&k| self.image(k)).collect();
            self.drafts.classes.push((targets, classes));
        } else if ty == t::STYLE {
            self.p += 1;
            let target = self.expect(t::IDENTIFIER)?;
            let styles = self.styles()?;
            self.drafts.styles.push((self.image(target), self.span(target), styles));
        } else {
            let next = self.ty(self.p + 1);
            if next == t::METADATA_START {
                if !self.flag(self.p + 2, METADATA_FIRST) {
                    return Err(self.no_alternative(&Self::statement_sequences()));
                }
                self.p += 1;
                let at = self.content(k);
                let id = if ty != t::IDENTIFIER { generated_id(&self.slice(at)) } else { self.image(k) };
                let assigned = self.metadata()?;
                self.drafts.metadata.push((id, at, assigned));
            } else {
                let follows = self.flags[next as usize] & FOLLOWS_NAME != 0 || (ty == t::IDENTIFIER && (next == t::LPAREN || next == t::LBRACKET));
                // Mermaid turns away the PlantUML statements people are most likely to try.
                if !follows || (ty == t::IDENTIFIER && FORBIDDEN.contains(&self.image(k).to_lowercase().as_str())) {
                    return Err(self.no_alternative(&Self::statement_sequences()));
                }
                return self.entity(true);
            }
        }
        self.line_end()
    }

    fn line(&mut self, top: bool) -> R<()> {
        let ty = self.ty(self.p);
        if ty == t::NEWLINE {
            self.p += 1;
            return Ok(());
        }
        if ty == t::COMMENT {
            self.p += 1;
            return self.line_end();
        }
        if top {
            self.top_statement()
        } else if ty == t::ACTOR {
            self.actors(false)
        } else {
            self.entity(false)
        }
    }
}

pub fn parse_usecase(source: &[u16], title: Option<&str>) -> R<UsecaseModel> {
    let tokens = tokenize(source)?;
    let mut p = Parser { src: source, types: tokens.types, starts: tokens.starts, ends: tokens.ends, p: 0, flags: flags(), drafts: Drafts::default(), parent: None, pending_json: Vec::new() };
    p.expect(t::USECASE)?;
    p.line_end()?;
    while p.flag(p.p, LINE) {
        p.line(true)?;
    }
    if p.ty(p.p) != t::EOF {
        return Err(p.fail(&format!("Redundant input, expecting EOF but found: {}", p.image(p.p))));
    }
    // The whole text has to be well formed before the JSON in it is looked at, as in Mermaid.
    for (json, literal) in std::mem::take(&mut p.pending_json) {
        let text = p.image(literal);
        let start = p.starts[literal];
        let parsed = parse_ordered_json(&text, || line_column(source, start))?;
        p.drafts.jsons[json].value = parsed;
    }
    resolve_usecase(source, p.drafts, title)
}
