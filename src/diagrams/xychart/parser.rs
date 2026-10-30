// Port of src/diagrams/xychart/parser.ts.

use super::db::{DataPoint, XyChartDb, XyText};
use super::lexer::*;
use crate::errors::{syntax_error, PeleError};
use crate::js;

fn is_word_token(t: u32) -> bool {
    matches!(t, AMP | NUM | ALPHA | PLUS | EQUALS | MULT | DOT | BRKT | MINUS | UNDERSCORE)
}
fn is_eol(t: u32) -> bool {
    matches!(t, NEWLINE | SEMI | EOF)
}
fn is_text(t: u32) -> bool {
    t == STR || is_word_token(t)
}

struct P<'a> {
    src: &'a [u16],
    t: Tokens,
    i: usize,
}

impl P<'_> {
    fn fail(&self, expected: &str) -> PeleError {
        let t = self.t.types[self.i];
        if t == ERROR {
            return syntax_error("xychart", self.src, self.t.starts[self.i], "", true);
        }
        syntax_error("xychart", self.src, self.t.starts[self.i], &format!("Expecting {}, got '{}'", expected, TOKEN_NAMES[t as usize]), false)
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
    fn text(&mut self) -> Result<XyText, PeleError> {
        if !is_text(self.kind()) {
            return Err(self.fail("'STR', 'ALPHA', 'NUM'"));
        }
        let mut s = self.take();
        if self.t.types[self.i - 1] != STR {
            while is_word_token(self.kind()) {
                s.push_str(&self.take());
            }
        }
        Ok(XyText { text: s })
    }
    fn range(&mut self) -> Result<(f64, f64), PeleError> {
        let min = js::to_number(&self.expect(NUMBER_WITH_DECIMAL)?);
        self.expect(ARROW_DELIMITER)?;
        Ok((min, js::to_number(&self.expect(NUMBER_WITH_DECIMAL)?)))
    }
    fn points(&mut self) -> Result<Vec<DataPoint>, PeleError> {
        let mut out = Vec::new();
        self.expect(SQUARE_BRACES_START)?;
        loop {
            let value = js::to_number(&self.expect(NUMBER_WITH_DECIMAL)?);
            let label = if self.kind() == STR { self.take() } else { String::new() };
            out.push(DataPoint { value, label });
            if self.kind() != COMMA {
                break;
            }
            self.i += 1;
        }
        self.expect(SQUARE_BRACES_END)?;
        Ok(out)
    }
}

pub fn parse_xychart(src: &[u16], db: &mut XyChartDb) -> Result<(), PeleError> {
    let mut r = P { src, t: tokenize(src), i: 0 };
    loop {
        if is_eol(r.kind()) {
            r.i += 1;
        } else if r.kind() != XYCHART {
            break;
        } else {
            r.i += 1;
            if r.kind() == CHART_ORIENTATION {
                let o = r.take();
                db.set_orientation(&o);
            }
        }
    }
    while r.kind() != END {
        let t = r.kind();
        r.i += 1;
        match t {
            TITLE => {
                let text = r.text()?;
                db.set_diagram_title(js::trim(&text.text));
            }
            X_AXIS | Y_AXIS => {
                let title = if is_text(r.kind()) { Some(r.text()?) } else { None };
                if t == X_AXIS && r.kind() == SQUARE_BRACES_START {
                    let mut categories = Vec::new();
                    loop {
                        r.i += 1;
                        categories.push(r.text()?);
                        if r.kind() != COMMA {
                            break;
                        }
                    }
                    r.expect(SQUARE_BRACES_END)?;
                    db.set_x_axis_band(&categories);
                } else if r.kind() == NUMBER_WITH_DECIMAL || title.is_none() {
                    let (min, max) = r.range()?;
                    if t == X_AXIS {
                        db.set_x_axis_range_data(min, max);
                    } else {
                        db.set_y_axis_range_data(min, max);
                    }
                }
                let title = title.unwrap_or_default();
                if t == X_AXIS {
                    db.set_x_axis_title(&title);
                } else {
                    db.set_y_axis_title(&title);
                }
            }
            LINE | BAR => {
                let title = if r.kind() == SQUARE_BRACES_START { XyText::default() } else { r.text()? };
                let data = r.points()?;
                if t == LINE {
                    db.set_line_data(&title, &data);
                } else {
                    db.set_bar_data(&title, &data);
                }
            }
            ACC_TITLE => {
                let v = r.expect(ACC_TITLE_VALUE)?;
                db.set_acc_title(js::trim(&v));
            }
            ACC_DESCR => {
                let v = r.expect(ACC_DESCR_VALUE)?;
                db.set_acc_description(js::trim(&v));
            }
            ACC_DESCR_MULTILINE_VALUE => {
                let v = r.t.texts[r.i - 1].clone();
                db.set_acc_description(js::trim(&v));
            }
            _ => {
                r.i -= 1;
                return Err(r.fail("'title', 'X_AXIS', 'Y_AXIS', 'LINE', 'BAR', 'acc_title', 'acc_descr', 'acc_descr_multiline_value'"));
            }
        }
        while is_eol(r.kind()) {
            r.i += 1;
        }
    }
    Ok(())
}
