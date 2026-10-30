// Port of src/diagrams/class/parser.ts.

use super::db::ClassDb;
use super::lexer::tokenize;
use super::tokens::{self as t, TOKEN_NAMES};
use super::types::{ClassRelation, RelationEnd};
use crate::errors::{syntax_error, PeleError};
use crate::js;

fn alphanum(ty: u32) -> bool {
    matches!(ty, t::UNICODE_TEXT | t::ALPHA | t::MINUS)
}

fn style_part(ty: u32) -> bool {
    matches!(ty, t::ALPHA | t::COLON | t::BRKT | t::STYLE | t::PCT | t::LABEL)
}

// Mermaid's relationType numbers, plus one so that zero means "not an end marker".
fn end_type(ty: u32) -> u32 {
    match ty {
        t::AGGREGATION => 1,
        t::EXTENSION => 2,
        t::COMPOSITION => 3,
        t::DEPENDENCY => 4,
        t::LOLLIPOP => 5,
        _ => 0,
    }
}

const NAME_TOKENS: &str = "'UNICODE_TEXT', 'NUM', 'ALPHA', 'MINUS', 'BQUOTE_STR'";

struct Frame {
    id: String,
    classes: Vec<String>,
    notes: Vec<String>,
}

struct P<'a> {
    src: &'a [u16],
    types: Vec<u32>,
    texts: Vec<String>,
    starts: Vec<usize>,
    i: usize,
}

type R<T> = Result<T, PeleError>;

impl P<'_> {
    fn ty_at(&self, i: usize) -> u32 {
        self.types.get(i).copied().unwrap_or(u32::MAX)
    }

    fn ty(&self) -> u32 {
        self.ty_at(self.i)
    }

    fn take(&mut self) -> String {
        let s = self.texts[self.i].clone();
        self.i += 1;
        s
    }

    fn fail(&self, expected: &str) -> PeleError {
        let ty = self.ty();
        if ty == t::ERROR {
            return syntax_error("class", self.src, self.starts[self.i], "", true);
        }
        syntax_error("class", self.src, self.starts[self.i], &format!("Expecting {}, got '{}'", expected, TOKEN_NAMES[ty as usize]), false)
    }

    fn expect(&mut self, ty: u32) -> R<String> {
        if self.ty() != ty {
            return Err(self.fail(&format!("'{}'", TOKEN_NAMES[ty as usize])));
        }
        Ok(self.take())
    }

    fn alpha_num(&mut self) -> R<String> {
        if !alphanum(self.ty()) {
            return Err(self.fail("'UNICODE_TEXT', 'NUM', 'ALPHA', 'MINUS'"));
        }
        Ok(self.take())
    }

    fn name(&mut self, generic: bool) -> R<String> {
        let mut out = String::new();
        loop {
            if self.ty() == t::BQUOTE_STR {
                out.push_str(&self.take());
                break;
            }
            if !alphanum(self.ty()) {
                return Err(self.fail(NAME_TOKENS));
            }
            out.push_str(&self.take());
            let ty = self.ty();
            if ty == t::DOT {
                out.push('.');
                self.i += 1;
            } else if !alphanum(ty) && ty != t::BQUOTE_STR {
                break;
            }
        }
        if generic && self.ty() == t::GENERICTYPE {
            out = format!("{}~{}~", out, self.take());
        }
        Ok(out)
    }

    fn class_label(&mut self) -> R<String> {
        self.i += 1;
        let label = self.expect(t::STR)?;
        self.expect(t::SQE)?;
        Ok(label)
    }

    fn members(&mut self) -> R<Vec<String>> {
        if self.ty() != t::MEMBER {
            return Err(self.fail("'STRUCT_STOP', 'MEMBER'"));
        }
        let mut list = Vec::new();
        while self.ty() == t::MEMBER {
            list.push(self.take());
        }
        self.expect(t::STRUCT_STOP)?;
        // The grammar collects members last to first, and addMembers turns them back.
        list.reverse();
        Ok(list)
    }

    fn class_statement(&mut self, db: &mut ClassDb) -> R<String> {
        self.i += 1;
        let id = self.name(true)?;
        db.add_class(&id);
        if self.ty() == t::SQS {
            let label = self.class_label()?;
            db.set_class_label(&id, &label);
        }
        match self.ty() {
            t::STYLE_SEPARATOR => {
                self.i += 1;
                let css = self.alpha_num()?;
                db.set_css_class(&id, &css);
                if self.ty() == t::STRUCT_START {
                    self.i += 1;
                    let m = self.members()?;
                    db.add_members(&id, m);
                }
            }
            t::STRUCT_START => {
                self.i += 1;
                if self.ty() == t::STRUCT_STOP {
                    self.i += 1;
                } else {
                    let m = self.members()?;
                    db.add_members(&id, m);
                }
            }
            t::ANNOTATION_START => {
                self.i += 1;
                let annotation = self.alpha_num()?;
                self.expect(t::ANNOTATION_END)?;
                db.add_annotation(&id, &annotation);
                if self.ty() == t::STRUCT_START {
                    self.i += 1;
                    if self.ty() == t::STRUCT_STOP {
                        self.i += 1;
                    } else {
                        let m = self.members()?;
                        db.add_members(&id, m);
                    }
                }
            }
            _ => {}
        }
        Ok(id)
    }

    fn note_statement(&mut self, db: &mut ClassDb) -> R<String> {
        let ty = self.ty();
        self.i += 1;
        if ty == t::NOTE {
            let text = self.expect(t::STR)?;
            return Ok(db.add_note(&text, None));
        }
        let id = self.name(true)?;
        let text = self.expect(t::STR)?;
        Ok(db.add_note(&text, Some(&id)))
    }

    // Namespaces nest, so they are parsed with a stack of open ones.
    fn namespace_statement(&mut self, db: &mut ClassDb) -> R<()> {
        let mut open: Vec<Frame> = Vec::new();
        loop {
            self.i += 1;
            let ns = self.name(false)?;
            let id = if self.ty() == t::SQS {
                let label = self.class_label()?;
                db.add_namespace(&ns, Some(&label))
            } else {
                db.add_namespace(&ns, None)
            };
            self.expect(t::STRUCT_START)?;
            if self.ty() == t::NEWLINE {
                self.i += 1;
            }
            open.push(Frame { id, classes: Vec::new(), notes: Vec::new() });
            let mut item = true;
            while item {
                let ty = self.ty();
                if ty == t::NAMESPACE {
                    break;
                }
                if ty == t::CLASS {
                    let c = self.class_statement(db)?;
                    open.last_mut().unwrap().classes.push(c);
                } else if ty == t::NOTE || ty == t::NOTE_FOR {
                    let n = self.note_statement(db)?;
                    open.last_mut().unwrap().notes.push(n);
                } else {
                    return Err(self.fail("'NAMESPACE', 'CLASS', 'NOTE_FOR', 'NOTE'"));
                }
                loop {
                    if self.ty() == t::NEWLINE {
                        let next = self.ty_at(self.i + 1);
                        if next == t::NAMESPACE || next == t::CLASS || next == t::NOTE || next == t::NOTE_FOR {
                            self.i += 1;
                            break;
                        }
                        self.i += 1;
                    }
                    self.expect(t::STRUCT_STOP)?;
                    let done = open.pop().unwrap();
                    db.add_classes_to_namespace(&done.id, &done.classes, &done.notes);
                    db.pop_namespace();
                    if open.is_empty() {
                        item = false;
                        break;
                    }
                }
            }
            if open.is_empty() {
                return Ok(());
            }
        }
    }

    fn relation_end(&mut self) -> RelationEnd {
        let ty = end_type(self.ty());
        if ty == 0 {
            return None;
        }
        self.i += 1;
        Some(ty - 1)
    }

    fn class_or_relation(&mut self, db: &mut ClassDb) -> R<()> {
        let id1 = self.name(true)?;
        let mut ty = self.ty();
        if ty == t::LABEL {
            let label = db.cleanup_label(&self.take());
            db.add_member(&id1, &label);
            return Ok(());
        }
        let mut title1 = "none".to_string();
        if ty == t::STR {
            title1 = self.take();
            ty = self.ty();
        } else if end_type(ty) == 0 && ty != t::LINE && ty != t::DOTTED_LINE {
            return Ok(());
        }
        let _ = ty;
        let type1 = self.relation_end();
        let ty = self.ty();
        if ty != t::LINE && ty != t::DOTTED_LINE {
            return Err(self.fail("'AGGREGATION', 'EXTENSION', 'COMPOSITION', 'DEPENDENCY', 'LOLLIPOP', 'LINE', 'DOTTED_LINE'"));
        }
        self.i += 1;
        let type2 = self.relation_end();
        let title2 = if self.ty() == t::STR { self.take() } else { "none".to_string() };
        let id2 = self.name(true)?;
        let mut relation = ClassRelation { id1, id2, relation_title1: title1, relation_title2: title2, title: None, type1, type2, line_type: if ty == t::LINE { 0 } else { 1 } };
        if self.ty() == t::LABEL {
            relation.title = Some(db.cleanup_label(&self.take()));
        }
        db.add_relation(relation);
        Ok(())
    }

    fn styles(&mut self) -> R<Vec<String>> {
        let mut out = Vec::new();
        loop {
            if !style_part(self.ty()) {
                return Err(self.fail("'ALPHA', 'NUM', 'COLON', 'UNIT', 'SPACE', 'BRKT', 'STYLE', 'PCT', 'LABEL'"));
            }
            let mut s = self.take();
            while style_part(self.ty()) {
                s.push_str(&self.take());
            }
            out.push(s);
            if self.ty() != t::COMMA {
                return Ok(out);
            }
            self.i += 1;
        }
    }

    fn tooltip(&mut self, db: &mut ClassDb, id: &str) {
        if self.ty() == t::STR {
            let tip = self.take();
            db.set_tooltip(id, Some(&tip));
        }
    }

    fn link_statement(&mut self, db: &mut ClassDb, id: &str) -> R<()> {
        let href = self.expect(t::STR)?;
        let tip = if self.ty() == t::STR { Some(self.take()) } else { None };
        if self.ty() == t::LINK_TARGET {
            let target = self.take();
            db.set_link(id, &href, Some(&target));
        } else {
            db.set_link(id, &href, None);
        }
        if let Some(tip) = tip {
            db.set_tooltip(id, Some(&tip));
        }
        Ok(())
    }

    fn statement(&mut self, db: &mut ClassDb) -> R<()> {
        let ty = self.ty();
        match ty {
            t::UNICODE_TEXT | t::ALPHA | t::MINUS | t::BQUOTE_STR => return self.class_or_relation(db),
            t::NAMESPACE => return self.namespace_statement(db),
            t::CLASS => return self.class_statement(db).map(|_| ()),
            t::MEMBER => {
                self.i += 1;
                return Ok(());
            }
            t::ANNOTATION_START => {
                self.i += 1;
                let annotation = self.alpha_num()?;
                self.expect(t::ANNOTATION_END)?;
                let name = self.name(true)?;
                db.add_annotation(&name, &annotation);
                return Ok(());
            }
            t::CALLBACK => {
                self.i += 1;
                let id = self.name(true)?;
                self.expect(t::STR)?;
                db.set_click_event(&id);
                self.tooltip(db, &id);
                return Ok(());
            }
            t::LINK => {
                self.i += 1;
                let id = self.name(true)?;
                return self.link_statement(db, &id);
            }
            t::CLICK => {
                self.i += 1;
                let id = self.name(true)?;
                if self.ty() == t::HREF {
                    self.i += 1;
                    return self.link_statement(db, &id);
                }
                self.expect(t::CALLBACK_NAME)?;
                if self.ty() == t::CALLBACK_ARGS {
                    self.i += 1;
                }
                db.set_click_event(&id);
                self.tooltip(db, &id);
                return Ok(());
            }
            t::STYLE => {
                self.i += 1;
                let id = self.expect(t::ALPHA)?;
                let styles = self.styles()?;
                db.set_css_style(&id, &styles);
                return Ok(());
            }
            t::CSSCLASS => {
                self.i += 1;
                let ids = self.expect(t::STR)?;
                let class = self.expect(t::ALPHA)?;
                db.set_css_class(&ids, &class);
                return Ok(());
            }
            t::NOTE | t::NOTE_FOR => return self.note_statement(db).map(|_| ()),
            t::CLASSDEF => {
                self.i += 1;
                let mut ids = vec![self.expect(t::ALPHA)?];
                while self.ty() == t::COMMA {
                    self.i += 1;
                    ids.push(self.expect(t::ALPHA)?);
                }
                let styles = self.styles()?;
                db.define_class(&ids, &styles);
                return Ok(());
            }
            t::direction_tb | t::direction_bt | t::direction_rl | t::direction_lr => {
                self.i += 1;
                db.set_direction(["TB", "BT", "RL", "LR"][(ty - t::direction_tb) as usize]);
                return Ok(());
            }
            t::acc_title => {
                self.i += 1;
                let v = self.expect(t::acc_title_value)?;
                db.set_acc_title(js::trim(&v));
                return Ok(());
            }
            t::acc_descr => {
                self.i += 1;
                let v = self.expect(t::acc_descr_value)?;
                db.set_acc_description(js::trim(&v));
                return Ok(());
            }
            t::acc_descr_multiline_value => {
                let v = self.take();
                db.set_acc_description(js::trim(&v));
                return Ok(());
            }
            _ => {}
        }
        Err(self.fail("'acc_title', 'acc_descr', 'acc_descr_multiline_value', 'NAMESPACE', 'ANNOTATION_START', 'CLASS', 'MEMBER', 'NOTE_FOR', 'NOTE', 'CLASSDEF', 'direction_tb', 'CALLBACK', 'LINK', 'CLICK', 'STYLE', 'CSSCLASS', 'ALPHA', 'MINUS', 'BQUOTE_STR'"))
    }
}

pub fn parse_class_diagram(src: &[u16], db: &mut ClassDb) -> R<()> {
    let tokens = tokenize(src);
    let mut p = P { src, types: tokens.types, texts: tokens.texts, starts: tokens.starts, i: 0 };
    // Mermaid's grammar also accepts statements with no `classDiagram` line before them.
    let header = p.ty() == t::CLASS_DIAGRAM;
    if header {
        p.i += 1;
        p.expect(t::NEWLINE)?;
    }
    loop {
        p.statement(db)?;
        if p.ty() != t::NEWLINE {
            break;
        }
        p.i += 1;
        if p.ty() == t::EOF || p.ty() == t::END {
            break;
        }
    }
    if header {
        p.expect(t::EOF)?;
    }
    p.expect(t::END)?;
    Ok(())
}
