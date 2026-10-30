// Port of src/diagrams/quadrant/parser.ts.

use super::db::{QuadrantDb, QuadrantText};
use super::lexer::*;
use crate::errors::{syntax_error, PeleError};
use crate::js;

fn is_word_token(t: u32) -> bool {
    matches!(t, PUNCTUATION | AMP | NUM | ALPHA | COMMA | PLUS | EQUALS | MULT | DOT | BRKT | UNDERSCORE | UNICODE_TEXT)
}
fn is_id(t: u32) -> bool {
    matches!(t, ALPHA | NUM | MINUS | COMMA | COLON | AMP | BRKT | MULT | UNICODE_TEXT)
}
fn is_style(t: u32) -> bool {
    matches!(t, ALPHA | NUM | COLON | SPACE | BRKT | MINUS)
}
fn is_eol(t: u32) -> bool {
    matches!(t, NEWLINE | SEMI | EOF)
}
fn is_text(t: u32) -> bool {
    t == STR || t == MD_STR || is_word_token(t)
}

const TEXT_TOKENS: &str = "'STR', 'MD_STR', 'ALPHA', 'NUM', 'UNICODE_TEXT', 'PUNCTUATION'";

struct P<'a> {
    src: &'a [u16],
    t: Tokens,
    i: usize,
}

impl P<'_> {
    fn fail(&self, expected: &str) -> PeleError {
        let t = self.t.types[self.i];
        if t == ERROR {
            return syntax_error("quadrantChart", self.src, self.t.starts[self.i], "", true);
        }
        syntax_error("quadrantChart", self.src, self.t.starts[self.i], &format!("Expecting {}, got '{}'", expected, TOKEN_NAMES[t as usize]), false)
    }
    fn kind(&self) -> u32 {
        self.t.types[self.i]
    }
    fn take(&mut self) -> String {
        self.i += 1;
        self.t.texts[self.i - 1].clone()
    }
    fn expect(&mut self, ty: u32) -> Result<String, PeleError> {
        if self.kind() != ty {
            return Err(self.fail(&format!("'{}'", TOKEN_NAMES[ty as usize])));
        }
        Ok(self.take())
    }
    fn text(&mut self) -> Result<QuadrantText, PeleError> {
        let first = self.kind();
        if !is_text(first) {
            return Err(self.fail(TEXT_TOKENS));
        }
        let mut s = self.take();
        while is_word_token(self.kind()) || self.kind() == SPACE || self.kind() == MINUS {
            s.push_str(&self.take());
        }
        Ok(QuadrantText { text: s, markdown: first == MD_STR })
    }
    fn styles(&mut self) -> Result<Vec<String>, PeleError> {
        let mut out = Vec::new();
        loop {
            if !is_style(self.kind()) {
                return Err(self.fail("'ALPHA', 'NUM', 'COLON', 'SPACE', 'BRKT', 'MINUS'"));
            }
            let mut s = self.take();
            while is_style(self.kind()) {
                s.push_str(&self.take());
            }
            out.push(js::trim(&s).to_string());
            if self.kind() != COMMA {
                return Ok(out);
            }
            self.i += 1;
        }
    }
}

pub fn parse_quadrant(src: &[u16], db: &mut QuadrantDb) -> Result<(), PeleError> {
    let mut r = P { src, t: tokenize(src), i: 0 };
    while is_eol(r.kind()) || r.kind() == SPACE {
        r.i += 1;
    }
    r.expect(QUADRANT)?;
    while r.kind() != END {
        while r.kind() == SPACE {
            r.i += 1;
        }
        let t = r.kind();
        match t {
            CLASSDEF => {
                r.i += 1;
                r.expect(SPACE)?;
                if !is_id(r.kind()) {
                    return Err(r.fail("'ALPHA', 'NUM', 'UNICODE_TEXT'"));
                }
                let mut id = r.take();
                while is_id(r.kind()) {
                    id.push_str(&r.take());
                }
                r.expect(SPACE)?;
                let styles = r.styles()?;
                db.add_class(id, &styles)?;
            }
            X_AXIS | Y_AXIS => {
                r.i += 1;
                let mut from = r.text()?;
                let mut to = None;
                if r.kind() == DELIMITER {
                    r.i += 1;
                    if is_text(r.kind()) {
                        to = Some(r.text()?);
                    } else {
                        from.text.push_str(" ⟶ ");
                    }
                }
                if t == X_AXIS {
                    db.set_x_axis_left_text(from);
                    if let Some(to) = to {
                        db.set_x_axis_right_text(to);
                    }
                } else {
                    db.set_y_axis_bottom_text(from);
                    if let Some(to) = to {
                        db.set_y_axis_top_text(to);
                    }
                }
            }
            13..=16 => {
                r.i += 1;
                let text = r.text()?;
                db.set_quadrant_text((t - QUADRANT_1) as usize, text);
            }
            TITLE => {
                r.i += 1;
                let v = r.expect(TITLE_VALUE)?;
                db.set_diagram_title(js::trim(&v));
            }
            ACC_TITLE => {
                r.i += 1;
                let v = r.expect(ACC_TITLE_VALUE)?;
                db.set_acc_title(js::trim(&v));
            }
            ACC_DESCR => {
                r.i += 1;
                let v = r.expect(ACC_DESCR_VALUE)?;
                db.set_acc_description(js::trim(&v));
            }
            ACC_DESCR_MULTILINE_VALUE => {
                let v = r.take();
                db.set_acc_description(js::trim(&v));
            }
            _ => {
                if is_text(t) {
                    let label = r.text()?;
                    let class_name = if r.kind() == CLASS_NAME { r.take() } else { String::new() };
                    r.expect(POINT_START)?;
                    let x = r.expect(POINT_X)?;
                    let y = r.expect(POINT_Y)?;
                    let styles = if is_style(r.kind()) { r.styles()? } else { Vec::new() };
                    db.add_point(label, class_name, &x, &y, &styles)?;
                }
            }
        }
        if !is_eol(r.kind()) {
            return Err(r.fail("'NEWLINE', 'SEMI', 'EOF'"));
        }
        r.i += 1;
    }
    Ok(())
}
