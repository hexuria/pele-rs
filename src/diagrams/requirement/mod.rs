// Port of src/diagrams/requirement (index.ts, types.ts, db.ts, parser.ts, graph.ts).

pub mod lexer;
pub mod render;

use crate::diagrams::cynefin::collapse_indents;
use crate::diagrams::Diagram;
use crate::errors::{syntax_error, PeleError, PeleErrorCode};
use crate::js;
use lexer::{t, tokenize, TOKEN_NAMES};

pub struct Requirement {
    pub name: String,
    pub type_: String,
    pub requirement_id: String,
    pub text: String,
    pub risk: String,
    pub verify_method: String,
    pub css_styles: Vec<String>,
    pub classes: Vec<String>,
}

pub struct RequirementElement {
    pub name: String,
    pub type_: String,
    pub doc_ref: String,
    pub css_styles: Vec<String>,
    pub classes: Vec<String>,
}

pub struct Relation {
    pub type_: String,
    pub src: String,
    pub dst: String,
}

pub struct RequirementClass {
    pub id: String,
    pub styles: Vec<String>,
}

// Applying a class copies its declarations onto each node, so a short text can ask for
// hundreds of millions of copies. Past this many the diagram is refused.
const MAX_STYLE_COPIES: usize = 1_000_000;

pub struct RequirementDb {
    pub direction: String,
    pub requirements: Vec<Requirement>,
    pub elements: Vec<RequirementElement>,
    pub relations: Vec<Relation>,
    pub classes: Vec<RequirementClass>,
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    // The grammar reports a body's fields before the requirement or element they belong to.
    new_id: String,
    new_text: String,
    new_risk: String,
    new_verify_method: String,
    new_element_type: String,
    new_doc_ref: String,
    style_copies: usize,
}

type R<T> = Result<T, PeleError>;

enum NodeRef {
    Req(usize),
    El(usize),
}

impl RequirementDb {
    fn new() -> Self {
        RequirementDb {
            direction: "TB".into(),
            requirements: Vec::new(),
            elements: Vec::new(),
            relations: Vec::new(),
            classes: Vec::new(),
            title: None,
            acc_title: None,
            acc_descr: None,
            new_id: String::new(),
            new_text: String::new(),
            new_risk: String::new(),
            new_verify_method: String::new(),
            new_element_type: String::new(),
            new_doc_ref: String::new(),
            style_copies: 0,
        }
    }

    fn count(&mut self, n: usize) -> R<()> {
        self.style_copies += n;
        if self.style_copies > MAX_STYLE_COPIES {
            return Err(PeleError::typed("Requirement diagram applies too many style declarations.", PeleErrorCode::Limit, "requirement"));
        }
        Ok(())
    }

    fn styles_of(&mut self, node: &NodeRef) -> &mut Vec<String> {
        match *node {
            NodeRef::Req(i) => &mut self.requirements[i].css_styles,
            NodeRef::El(i) => &mut self.elements[i].css_styles,
        }
    }

    fn copy(&mut self, node: &NodeRef, styles: &[String]) -> R<()> {
        self.count(styles.len())?;
        self.styles_of(node).extend(styles.iter().cloned());
        Ok(())
    }

    fn add_requirement(&mut self, name: &str, type_: &str) {
        if !self.requirements.iter().any(|r| r.name == name) {
            self.requirements.push(Requirement {
                name: name.into(),
                type_: type_.into(),
                requirement_id: self.new_id.clone(),
                text: self.new_text.clone(),
                risk: self.new_risk.clone(),
                verify_method: self.new_verify_method.clone(),
                css_styles: Vec::new(),
                classes: vec!["default".into()],
            });
        }
        self.new_id.clear();
        self.new_text.clear();
        self.new_risk.clear();
        self.new_verify_method.clear();
    }

    fn add_element(&mut self, name: &str) {
        if !self.elements.iter().any(|e| e.name == name) {
            self.elements.push(RequirementElement {
                name: name.into(),
                type_: self.new_element_type.clone(),
                doc_ref: self.new_doc_ref.clone(),
                css_styles: Vec::new(),
                classes: vec!["default".into()],
            });
        }
        self.new_element_type.clear();
        self.new_doc_ref.clear();
    }

    fn node(&self, id: &str) -> Option<NodeRef> {
        if let Some(i) = self.requirements.iter().position(|r| r.name == id) {
            return Some(NodeRef::Req(i));
        }
        self.elements.iter().position(|e| e.name == id).map(NodeRef::El)
    }

    fn set_css_style(&mut self, ids: &[String], styles: &[String]) -> R<()> {
        for id in ids {
            // Mermaid stops at the first unknown id, leaving the rest of the list unstyled.
            let Some(node) = self.node(id) else { return Ok(()) };
            for s in styles {
                let parts: Vec<String> = if s.contains(',') { s.split(',').map(str::to_string).collect() } else { vec![s.clone()] };
                self.copy(&node, &parts)?;
            }
        }
        Ok(())
    }

    fn set_class(&mut self, ids: &[String], class_names: &[String]) -> R<()> {
        for id in ids {
            let Some(node) = self.node(id) else { continue };
            for name in class_names {
                match node {
                    NodeRef::Req(i) => self.requirements[i].classes.push(name.clone()),
                    NodeRef::El(i) => self.elements[i].classes.push(name.clone()),
                }
                if let Some(styles) = self.classes.iter().find(|c| c.id == *name).map(|c| c.styles.clone()) {
                    self.copy(&node, &styles)?;
                }
            }
        }
        Ok(())
    }

    fn define_class(&mut self, ids: &[String], style: &[String]) -> R<()> {
        for id in ids {
            let k = match self.classes.iter().position(|c| c.id == *id) {
                Some(k) => k,
                None => {
                    self.classes.push(RequirementClass { id: id.clone(), styles: Vec::new() });
                    self.classes.len() - 1
                }
            };
            self.classes[k].styles.extend(style.iter().cloned());
            let split: Vec<String> = style.iter().flat_map(|s| s.split(',').map(str::to_string)).collect();
            for i in 0..self.requirements.len() {
                if self.requirements[i].classes.contains(id) {
                    self.copy(&NodeRef::Req(i), &split)?;
                }
            }
            for i in 0..self.elements.len() {
                if self.elements[i].classes.contains(id) {
                    self.copy(&NodeRef::El(i), &split)?;
                }
            }
        }
        Ok(())
    }
}

// What the keyword tokens stand for, from REQUIREMENT to TRACES in token order.
const VALUES: [&str; 21] = [
    "Requirement",
    "Functional Requirement",
    "Interface Requirement",
    "Performance Requirement",
    "Physical Requirement",
    "Design Constraint",
    "Low",
    "Medium",
    "High",
    "Analysis",
    "Demonstration",
    "Inspection",
    "Test",
    "",
    "contains",
    "copies",
    "derives",
    "satisfies",
    "verifies",
    "refines",
    "traces",
];

fn is_style_part(k: i32) -> bool {
    k == t::ALPHA || k == t::COLON || k == t::BRKT || k == t::MINUS || k == t::SEMICOLON
}

struct Parser<'a> {
    src: &'a [u16],
    types: Vec<i32>,
    texts: Vec<String>,
    starts: Vec<usize>,
    i: usize,
}

impl<'a> Parser<'a> {
    // Fails with Jison's wording, naming the token types `from` to `to` and any others as expected.
    fn fail(&self, from: i32, to: i32, others: &[i32]) -> PeleError {
        let k = self.types[self.i];
        if k == t::ERROR {
            return syntax_error("requirement", self.src, self.starts[self.i], "", true);
        }
        let mut list: Vec<i32> = (from..=to).collect();
        list.extend_from_slice(others);
        let expected: Vec<String> = list.iter().map(|&k| format!("'{}'", TOKEN_NAMES[k as usize])).collect();
        syntax_error("requirement", self.src, self.starts[self.i], &format!("Expecting {}, got '{}'", expected.join(", "), TOKEN_NAMES[k as usize]), false)
    }

    fn kind(&self) -> i32 {
        self.types[self.i]
    }

    fn take(&mut self) -> String {
        let s = self.texts[self.i].clone();
        self.i += 1;
        s
    }

    fn expect(&mut self, k: i32) -> R<String> {
        if self.kind() != k {
            return Err(self.fail(k, k, &[]));
        }
        Ok(self.take())
    }

    // The value of the next token if it is a keyword in the given range of token types.
    fn keyword(&mut self, from: i32, to: i32) -> R<String> {
        let k = self.kind();
        if k < from || k > to {
            return Err(self.fail(from, to, &[]));
        }
        self.i += 1;
        Ok(VALUES[(k - t::REQUIREMENT) as usize].to_string())
    }

    fn string(&mut self) -> R<String> {
        let k = self.kind();
        if k != t::UNQ_STRING && k != t::Q_STRING {
            return Err(self.fail(t::Q_STRING, t::UNQ_STRING, &[]));
        }
        Ok(self.take())
    }

    fn directive(&mut self, db: &mut RequirementDb) -> R<bool> {
        let k = self.kind();
        if k == t::ACC_TITLE {
            self.i += 1;
            db.acc_title = Some(js::trim_start(js::trim(&self.expect(t::ACC_TITLE_VALUE)?)).to_string());
        } else if k == t::ACC_DESCR {
            self.i += 1;
            db.acc_descr = Some(collapse_indents(js::trim(&self.expect(t::ACC_DESCR_VALUE)?)));
        } else if k == t::ACC_DESCR_MULTILINE_VALUE {
            let v = self.take();
            db.acc_descr = Some(collapse_indents(js::trim(&v)));
        } else {
            return Ok(false);
        }
        Ok(true)
    }

    fn id_list(&mut self) -> R<Vec<String>> {
        let mut list = Vec::new();
        loop {
            let k = self.kind();
            if k != t::ALPHA && k != t::UNQ_STRING && k != t::Q_STRING {
                return Err(self.fail(t::Q_STRING, t::UNQ_STRING, &[t::ALPHA]));
            }
            list.push(self.take());
            if self.kind() != t::COMMA {
                return Ok(list);
            }
            self.i += 1;
        }
    }

    fn styles_opt(&mut self) -> R<Vec<String>> {
        let mut styles = Vec::new();
        loop {
            if !is_style_part(self.kind()) {
                return Err(self.fail(t::ALPHA, t::SEMICOLON, &[t::MINUS, t::BRKT]));
            }
            let mut style = self.take();
            while is_style_part(self.kind()) {
                style.push_str(&self.take());
            }
            styles.push(style);
            if self.kind() != t::COMMA {
                return Ok(styles);
            }
            self.i += 1;
        }
    }

    fn class_list(&mut self) -> R<Option<Vec<String>>> {
        let mut classes = None;
        if self.kind() == t::STYLE_SEPARATOR {
            self.i += 1;
            classes = Some(self.id_list()?);
            self.expect(t::STRUCT_START)?;
        } else if self.kind() == t::STRUCT_START {
            self.i += 1;
        } else {
            return Err(self.fail(t::STRUCT_START, t::STRUCT_START, &[t::STYLE_SEPARATOR]));
        }
        self.expect(t::NEWLINE)?;
        Ok(classes)
    }

    // Reads a body up to its closing brace. The grammar is right recursive, so Mermaid applies
    // the fields last to first and the first of two repeated fields wins.
    fn body(&mut self, db: &mut RequirementDb, requirement: bool) -> R<()> {
        let mut fields: Vec<i32> = Vec::new();
        let mut values: Vec<String> = Vec::new();
        loop {
            let k = self.kind();
            if k == t::STRUCT_STOP {
                self.i += 1;
                break;
            }
            if k == t::NEWLINE {
                self.i += 1;
                continue;
            }
            if if requirement { k < t::ID || k > t::VERIFYMTHD } else { k != t::TYPE && k != t::DOCREF } {
                if requirement {
                    return Err(self.fail(t::ID, t::VERIFYMTHD, &[t::NEWLINE, t::STRUCT_STOP]));
                }
                return Err(self.fail(t::TYPE, t::DOCREF, &[t::NEWLINE, t::STRUCT_STOP]));
            }
            self.i += 1;
            self.expect(t::COLONSEP)?;
            let value = if k == t::RISK {
                self.keyword(t::LOW_RISK, t::HIGH_RISK)?
            } else if k == t::VERIFYMTHD {
                self.keyword(t::VERIFY_ANALYSIS, t::VERIFY_TEST)?
            } else {
                self.string()?
            };
            values.push(value);
            fields.push(k);
            self.expect(t::NEWLINE)?;
        }
        for k in (0..fields.len()).rev() {
            let value = values[k].clone();
            match fields[k] {
                t::ID => db.new_id = value,
                t::TEXT => db.new_text = value,
                t::RISK => db.new_risk = value,
                t::VERIFYMTHD => db.new_verify_method = value,
                t::TYPE => db.new_element_type = value,
                _ => db.new_doc_ref = value,
            }
        }
        Ok(())
    }
}

pub fn parse_requirement(src: &[u16], db: &mut RequirementDb) -> R<()> {
    let tokens = tokenize(src);
    let mut p = Parser { src, types: tokens.types, texts: tokens.texts, starts: tokens.starts, i: 0 };

    while p.directive(db)? {
        if p.kind() == t::NEWLINE {
            p.i += 1;
        }
    }
    if p.kind() != t::RD {
        return Err(p.fail(t::RD, t::RD, &[t::ACC_TITLE, t::ACC_DESCR, t::ACC_DESCR_MULTILINE_VALUE]));
    }
    p.i += 1;
    p.expect(t::NEWLINE)?;

    loop {
        let k = p.kind();
        if k == t::EOF {
            return Ok(());
        }
        if k == t::NEWLINE {
            p.i += 1;
        } else if p.directive(db)? {
            continue;
        } else if (t::DIRECTION_TB..=t::DIRECTION_LR).contains(&k) {
            p.i += 1;
            db.direction = ["TB", "BT", "RL", "LR"][(k - t::DIRECTION_TB) as usize].to_string();
        } else if (t::REQUIREMENT..=t::DESIGN_CONSTRAINT).contains(&k) {
            let type_ = p.keyword(t::REQUIREMENT, t::DESIGN_CONSTRAINT)?;
            let name = p.string()?;
            let classes = p.class_list()?;
            p.body(db, true)?;
            db.add_requirement(&name, &type_);
            if let Some(classes) = classes {
                db.set_class(&[name], &classes)?;
            }
        } else if k == t::ELEMENT {
            p.i += 1;
            let name = p.string()?;
            let classes = p.class_list()?;
            p.body(db, false)?;
            db.add_element(&name);
            if let Some(classes) = classes {
                db.set_class(&[name], &classes)?;
            }
        } else if k == t::STYLE {
            p.i += 1;
            let ids = p.id_list()?;
            let styles = p.styles_opt()?;
            db.set_css_style(&ids, &styles)?;
        } else if k == t::CLASSDEF {
            p.i += 1;
            let ids = p.id_list()?;
            let styles = p.styles_opt()?;
            db.define_class(&ids, &styles)?;
        } else if k == t::CLASS {
            p.i += 1;
            let ids = p.id_list()?;
            let names = p.id_list()?;
            db.set_class(&ids, &names)?;
        } else if k == t::UNQ_STRING || k == t::Q_STRING {
            let id = p.take();
            let next = p.kind();
            if next == t::STYLE_SEPARATOR {
                p.i += 1;
                let names = p.id_list()?;
                db.set_class(&[id], &names)?;
            } else if next == t::END_ARROW_L || next == t::LINE {
                p.i += 1;
                let relationship = p.keyword(t::CONTAINS, t::TRACES)?;
                p.expect(if next == t::LINE { t::END_ARROW_R } else { t::LINE })?;
                let other = p.string()?;
                if next == t::LINE {
                    db.relations.push(Relation { type_: relationship, src: id, dst: other });
                } else {
                    db.relations.push(Relation { type_: relationship, src: other, dst: id });
                }
            } else {
                return Err(p.fail(t::STYLE_SEPARATOR, t::STYLE_SEPARATOR, &[t::END_ARROW_L, t::LINE]));
            }
        } else {
            return Err(p.fail(
                t::NEWLINE,
                t::EOF,
                &[
                    t::ACC_TITLE,
                    t::ACC_DESCR,
                    t::ACC_DESCR_MULTILINE_VALUE,
                    t::DIRECTION_TB,
                    t::DIRECTION_BT,
                    t::DIRECTION_RL,
                    t::DIRECTION_LR,
                    t::REQUIREMENT,
                    t::FUNCTIONAL_REQUIREMENT,
                    t::INTERFACE_REQUIREMENT,
                    t::PERFORMANCE_REQUIREMENT,
                    t::PHYSICAL_REQUIREMENT,
                    t::DESIGN_CONSTRAINT,
                    t::ELEMENT,
                    t::CLASSDEF,
                    t::CLASS,
                    t::STYLE,
                    t::UNQ_STRING,
                    t::Q_STRING,
                ],
            ));
        }
    }
}

pub static DIAGRAM: Diagram = Diagram {
    type_: "requirement",
    section: None,
    keep_comments: false,
    parse: |source, _config, title, _line_offset| {
        let mut db = RequirementDb::new();
        if let Some(t) = title.filter(|t| !t.is_empty()) {
            db.title = Some(t.to_string());
        }
        parse_requirement(source, &mut db)?;
        Ok(Box::new(db))
    },
    render: |model, config, options| Ok(render::render_requirement(model.downcast_ref::<RequirementDb>().unwrap(), config, options)),
};
