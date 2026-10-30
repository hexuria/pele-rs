// Port of src/diagrams/venn (index.ts, types.ts, db.ts, parser.ts).

pub mod layout;
pub mod lexer;
pub mod render;

use crate::diagrams::Diagram;
use crate::errors::{syntax_error, PeleError, PeleErrorCode};
use crate::js;
use lexer::{t, VennLexer, TOKEN_NAMES};

// A set (one id) or an overlap of sets (two or more ids, sorted), with its relative area.
#[derive(Clone)]
pub struct VennSubset {
    pub sets: Vec<String>,
    pub size: f64,
    pub label: Option<String>,
}

// A line of text shown inside the region of the given sets.
pub struct VennText {
    pub sets: Vec<String>,
    pub id: String,
    pub label: Option<String>,
}

// Targets are the sorted ids of a set or overlap, or the id of a text.
pub struct VennStyle {
    pub targets: Vec<String>,
    pub styles: Vec<(String, String)>,
}

#[derive(Default)]
pub struct VennDb {
    pub title: Option<String>,
    pub subsets: Vec<VennSubset>,
    pub text_nodes: Vec<VennText>,
    pub styles: Vec<VennStyle>,
    known: Vec<String>,
    current: Option<Vec<String>>,
    // Whether an indented `text` line belongs to the set or union before it. The lexer reads this.
    pub indent: bool,
}

// Mermaid takes one pair of quotes off an id, label or style value.
pub fn normalize_text(text: &str) -> String {
    let trimmed = js::trim(text);
    if js::len16(trimmed) >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        trimmed[1..trimmed.len() - 1].to_string()
    } else {
        trimmed.to_string()
    }
}

/// `Array.prototype.sort()` on strings: by UTF-16 code units.
pub fn sort16(ids: &mut [String]) {
    ids.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
}

fn normalize_ids(ids: &[String]) -> Vec<String> {
    let mut out: Vec<String> = ids.iter().map(|s| normalize_text(s)).collect();
    sort16(&mut out);
    out
}

fn semantic(message: &str) -> PeleError {
    PeleError::typed(message, PeleErrorCode::Semantic, "venn")
}

impl VennDb {
    // A set without a size gets 10, and an overlap of n sets 10 / n².
    fn add_subset_data(&mut self, ids: &[String], label: Option<String>, size: Option<f64>) {
        let sets = normalize_ids(ids);
        self.current = Some(sets.clone());
        if sets.len() == 1 && !self.known.contains(&sets[0]) {
            self.known.push(sets[0].clone());
        }
        let n = ids.len() as f64;
        self.subsets.push(VennSubset { sets, size: size.unwrap_or(10.0 / (n * n)), label: label.filter(|l| !l.is_empty()).map(|l| normalize_text(&l)) });
    }

    fn add_text_data(&mut self, ids: &[String], id: &str, label: Option<String>) {
        self.text_nodes.push(VennText { sets: normalize_ids(ids), id: normalize_text(id), label: label.filter(|l| !l.is_empty()).map(|l| normalize_text(&l)) });
    }

    fn add_style_data(&mut self, ids: &[String], data: Vec<(String, String)>) {
        let mut styles: Vec<(String, String)> = Vec::new();
        for (key, value) in data {
            let value = if value.is_empty() { value } else { normalize_text(&value) };
            match styles.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = value,
                None => styles.push((key, value)),
            }
        }
        self.styles.push(VennStyle { targets: normalize_ids(ids), styles });
    }

    fn validate_union_identifiers(&self, ids: &[String]) -> Result<(), PeleError> {
        let unknown: Vec<String> = ids.iter().map(|s| normalize_text(s)).filter(|id| !self.known.contains(id)).collect();
        if !unknown.is_empty() {
            return Err(semantic(&format!("unknown set identifier: {}", unknown.join(", "))));
        }
        Ok(())
    }
}

const STATEMENT: &str = "'EOF', 'NEWLINE', 'TITLE', 'SET', 'UNION', 'TEXT', 'INDENT_TEXT', 'STYLE'";
const VALUE: &str = "'NUMERIC', 'IDENTIFIER', 'STRING', 'HEXCOLOR', 'RGBCOLOR', 'RGBACOLOR'";

struct Parser<'a> {
    src: &'a [u16],
    lexer: VennLexer<'a>,
    type_: i32,
}

impl<'a> Parser<'a> {
    fn peek(&mut self, db: &mut VennDb) -> i32 {
        if self.type_ == -1 {
            self.type_ = self.lexer.next(&mut db.indent);
        }
        self.type_
    }

    fn fail(&mut self, db: &mut VennDb, expected: &str) -> PeleError {
        if self.peek(db) == t::ERROR {
            return syntax_error("venn", self.src, self.lexer.start, "", true);
        }
        syntax_error("venn", self.src, self.lexer.start, &format!("Expecting {}, got '{}'", expected, TOKEN_NAMES[self.type_ as usize]), false)
    }

    fn take(&mut self) -> String {
        self.type_ = -1;
        self.lexer.text.clone()
    }

    fn expect(&mut self, db: &mut VennDb, wanted: i32) -> Result<String, PeleError> {
        if self.peek(db) != wanted {
            return Err(self.fail(db, &format!("'{}'", TOKEN_NAMES[wanted as usize])));
        }
        Ok(self.take())
    }

    fn identifier(&mut self, db: &mut VennDb) -> Result<String, PeleError> {
        if self.peek(db) != t::IDENTIFIER && self.type_ != t::STRING {
            return Err(self.fail(db, "'IDENTIFIER', 'STRING'"));
        }
        Ok(self.take())
    }

    fn identifiers(&mut self, db: &mut VennDb) -> Result<Vec<String>, PeleError> {
        let mut list = vec![self.identifier(db)?];
        while self.peek(db) == t::COMMA {
            self.take();
            list.push(self.identifier(db)?);
        }
        Ok(list)
    }

    fn subset(&mut self, db: &mut VennDb, ids: Vec<String>, union: bool) -> Result<(), PeleError> {
        let label = if self.peek(db) == t::BRACKET_LABEL { Some(self.take()) } else { None };
        let mut size = None;
        if self.peek(db) == t::COLON {
            self.take();
            size = Some(js::parse_float(&self.expect(db, t::NUMERIC)?));
        }
        if union {
            if ids.len() < 2 {
                return Err(semantic("union requires multiple identifiers"));
            }
            db.validate_union_identifiers(&ids)?;
        }
        db.add_subset_data(&ids, label, size);
        db.indent = true;
        Ok(())
    }

    // The id of a text, and the label in brackets that an id which is not a number may carry.
    fn text(&mut self, db: &mut VennDb, ids: Option<Vec<String>>) -> Result<(), PeleError> {
        let kind = self.peek(db);
        if kind != t::IDENTIFIER && kind != t::STRING && kind != t::NUMERIC {
            return Err(self.fail(db, "'NUMERIC', 'IDENTIFIER', 'STRING'"));
        }
        let id = self.take();
        let label = if kind != t::NUMERIC && self.peek(db) == t::BRACKET_LABEL { Some(self.take()) } else { None };
        let sets = match ids {
            Some(ids) => ids,
            None => db.current.clone().ok_or_else(|| semantic("text requires set"))?,
        };
        db.add_text_data(&sets, &id, label);
        Ok(())
    }

    fn style(&mut self, db: &mut VennDb) -> Result<(), PeleError> {
        let ids = self.identifiers(db)?;
        let mut styles: Vec<(String, String)> = Vec::new();
        loop {
            let key = self.expect(db, t::IDENTIFIER)?;
            self.expect(db, t::COLON)?;
            let value;
            if self.peek(db) == t::STRING {
                value = self.take();
            } else {
                let mut parts: Vec<String> = Vec::new();
                while self.peek(db) == t::IDENTIFIER || (self.type_ >= t::NUMERIC && self.type_ <= t::RGBACOLOR) {
                    parts.push(self.take());
                }
                if parts.is_empty() {
                    return Err(self.fail(db, VALUE));
                }
                value = parts.join(" ");
            }
            styles.push((key, value));
            if self.peek(db) != t::COMMA {
                break;
            }
            self.take();
        }
        db.add_style_data(&ids, styles);
        Ok(())
    }
}

// Ports Mermaid's venn grammar. A token is read only when the grammar has to look at one to
// decide, because the lexer asks the model whether an indented `text` continues a set, and the
// statements are what tell the model so.
pub fn parse_venn(src: &[u16], db: &mut VennDb) -> Result<(), PeleError> {
    let mut p = Parser { src, lexer: VennLexer::new(src), type_: -1 };
    while p.peek(db) == t::NEWLINE {
        p.take();
    }
    p.expect(db, t::VENN)?;
    loop {
        let statement = p.peek(db);
        if statement == t::EOF {
            return Ok(());
        }
        if !(t::NEWLINE..=t::STYLE).contains(&statement) || statement == t::VENN {
            return Err(p.fail(db, STATEMENT));
        }
        let text = p.take();
        match statement {
            t::TITLE => db.title = Some(text.chars().skip(6).collect()),
            t::SET => {
                let id = p.identifier(db)?;
                p.subset(db, vec![id], false)?;
            }
            t::UNION => {
                let ids = p.identifiers(db)?;
                p.subset(db, ids, true)?;
            }
            t::TEXT => {
                let ids = p.identifiers(db)?;
                p.text(db, Some(ids))?;
            }
            t::INDENT_TEXT => p.text(db, None)?,
            t::STYLE => p.style(db)?,
            _ => {}
        }
    }
}

pub static DIAGRAM: Diagram = Diagram {
    type_: "venn",
    section: None,
    keep_comments: false,
    parse: |source, _config, title, _line_offset| {
        let mut db = VennDb::default();
        if let Some(t) = title.filter(|t| !t.is_empty()) {
            db.title = Some(t.to_string());
        }
        parse_venn(source, &mut db)?;
        Ok(Box::new(db))
    },
    render: |model, config, options| Ok(render::render_venn(model.downcast_ref::<VennDb>().unwrap(), config, options)),
};
