// Port of src/diagrams/usecase/json.ts, and graph.ts's flattenJsonRows. Upstream calls JSON.parse
// and then walks the text again to recover the order keys were written in; here one walk does
// both, keeping each key where it first appears with the value it was last given, which is that
// order. Error messages follow V8's JSON.parse wording for the common cases.

use crate::errors::{PeleError, PeleErrorCode};
use crate::js;
use crate::text::entities::written_codes;

#[derive(Clone, Debug)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

struct Walk<'a> {
    text: &'a [u16],
    at: usize,
}

// The offset and V8's description of the first thing that is not valid JSON.
type Bad = (usize, String);

fn is_digit(c: u32) -> bool {
    (48..=57).contains(&c)
}

impl<'a> Walk<'a> {
    fn c(&self, i: usize) -> u32 {
        self.text.get(i).map_or(u32::MAX, |&c| c as u32)
    }

    fn skip(&mut self) {
        while matches!(self.c(self.at), 32 | 9 | 10 | 13) {
            self.at += 1;
        }
    }

    fn unexpected(&self, at: usize) -> Bad {
        if at >= self.text.len() {
            return (at, "Unexpected end of JSON input".into());
        }
        let token = js::from16(&self.text[at..at + 1]);
        (at, format!("Unexpected token '{}', \"{}\" is not valid JSON", token, js::from16(self.text)))
    }

    fn string(&mut self) -> Result<String, Bad> {
        self.at += 1;
        let mut value: Vec<u16> = Vec::new();
        loop {
            let start = self.at;
            if start >= self.text.len() {
                return Err((start, format!("Unterminated string in JSON at position {}", start)));
            }
            let ch = self.text[start];
            self.at += 1;
            if ch == 34 {
                return Ok(js::from16(&value));
            }
            if ch < 0x20 {
                return Err((start, format!("Bad control character in string literal in JSON at position {}", start)));
            }
            if ch != 92 {
                value.push(ch);
                continue;
            }
            let escape_at = self.at;
            let escape = self.c(self.at);
            self.at += 1;
            let decoded: u16 = match escape {
                34 | 92 | 47 => escape as u16,
                98 => 8,
                102 => 12,
                110 => 10,
                114 => 13,
                116 => 9,
                117 => {
                    let unit = &self.text[self.at.min(self.text.len())..(self.at + 4).min(self.text.len())];
                    if unit.len() < 4 || !unit.iter().all(|&u| (u as u32) < 128 && (u as u8).is_ascii_hexdigit()) {
                        return Err((self.at, format!("Bad Unicode escape in JSON at position {}", escape_at - 1)));
                    }
                    self.at += 4;
                    u16::from_str_radix(&js::from16(unit), 16).unwrap()
                }
                _ => return Err((escape_at, format!("Bad escaped character in JSON at position {}", escape_at))),
            };
            value.push(decoded);
        }
    }

    fn number(&mut self) -> Result<f64, Bad> {
        let from = self.at;
        if self.c(self.at) == 45 {
            self.at += 1;
        }
        if self.c(self.at) == 48 {
            self.at += 1;
        } else if is_digit(self.c(self.at)) {
            while is_digit(self.c(self.at)) {
                self.at += 1;
            }
        } else {
            return Err((self.at, format!("No number after minus sign in JSON at position {}", self.at)));
        }
        if self.c(self.at) == 46 {
            self.at += 1;
            if !is_digit(self.c(self.at)) {
                return Err((self.at, format!("Unterminated fractional number in JSON at position {}", self.at)));
            }
            while is_digit(self.c(self.at)) {
                self.at += 1;
            }
        }
        if self.c(self.at) | 32 == 101 {
            self.at += 1;
            if matches!(self.c(self.at), 43 | 45) {
                self.at += 1;
            }
            if !is_digit(self.c(self.at)) {
                return Err((self.at, format!("Exponent part is missing a number in JSON at position {}", self.at)));
            }
            while is_digit(self.c(self.at)) {
                self.at += 1;
            }
        }
        Ok(js::from16(&self.text[from..self.at]).parse().unwrap_or(f64::NAN))
    }

    fn literal(&mut self, word: &str, value: Json) -> Result<Json, Bad> {
        for (k, b) in word.bytes().enumerate() {
            if self.c(self.at + k) != b as u32 {
                return Err(self.unexpected(self.at + k));
            }
        }
        self.at += word.len();
        Ok(value)
    }

    // Nesting is kept on a stack of its own, so depth is limited by memory and not by the call stack.
    fn value(&mut self) -> Result<Json, Bad> {
        enum Open {
            Arr(Vec<Json>),
            Obj(Vec<(String, Json)>, String),
        }
        let mut stack: Vec<Open> = Vec::new();
        loop {
            self.skip();
            let c = self.c(self.at);
            let mut done = match c {
                123 => {
                    self.at += 1;
                    self.skip();
                    if self.c(self.at) == 125 {
                        self.at += 1;
                        Json::Obj(Vec::new())
                    } else {
                        let key = self.key()?;
                        stack.push(Open::Obj(Vec::new(), key));
                        continue;
                    }
                }
                91 => {
                    self.at += 1;
                    self.skip();
                    if self.c(self.at) == 93 {
                        self.at += 1;
                        Json::Arr(Vec::new())
                    } else {
                        stack.push(Open::Arr(Vec::new()));
                        continue;
                    }
                }
                34 => Json::Str(self.string()?),
                116 => self.literal("true", Json::Bool(true))?,
                102 => self.literal("false", Json::Bool(false))?,
                110 => self.literal("null", Json::Null)?,
                45 => Json::Num(self.number()?),
                c if is_digit(c) => Json::Num(self.number()?),
                _ => return Err(self.unexpected(self.at)),
            };
            loop {
                let Some(top) = stack.last_mut() else { return Ok(done) };
                self.skip();
                let c = self.c(self.at);
                match top {
                    Open::Arr(items) => {
                        items.push(done);
                        if c == 44 {
                            self.at += 1;
                            break;
                        }
                        if c != 93 {
                            return Err(if self.at >= self.text.len() {
                                self.unexpected(self.at)
                            } else {
                                (self.at, format!("Expected ',' or ']' after array element in JSON at position {}", self.at))
                            });
                        }
                        self.at += 1;
                    }
                    Open::Obj(entries, slot) => {
                        let key = std::mem::take(slot);
                        match entries.iter_mut().find(|(k, _)| *k == key) {
                            Some(slot) => slot.1 = done,
                            None => entries.push((key, done)),
                        }
                        if c == 44 {
                            self.at += 1;
                            self.skip();
                            *slot = self.key()?;
                            break;
                        }
                        if c != 125 {
                            return Err(if self.at >= self.text.len() {
                                self.unexpected(self.at)
                            } else {
                                (self.at, format!("Expected ',' or '}}' after property value in JSON at position {}", self.at))
                            });
                        }
                        self.at += 1;
                    }
                }
                done = match stack.pop().unwrap() {
                    Open::Arr(items) => Json::Arr(items),
                    Open::Obj(entries, _) => Json::Obj(entries),
                };
            }
        }
    }

    // A property name and its colon.
    fn key(&mut self) -> Result<String, Bad> {
        if self.c(self.at) != 34 {
            return Err(if self.at >= self.text.len() { self.unexpected(self.at) } else { (self.at, format!("Expected property name or '}}' in JSON at position {}", self.at)) });
        }
        let key = self.string()?;
        self.skip();
        if self.c(self.at) != 58 {
            return Err(if self.at >= self.text.len() { self.unexpected(self.at) } else { (self.at, format!("Expected ':' after property name in JSON at position {}", self.at)) });
        }
        self.at += 1;
        Ok(key)
    }
}

fn parse(text: &[u16]) -> Result<Json, Bad> {
    let mut w = Walk { text, at: 0 };
    let value = w.value()?;
    w.skip();
    if w.at != text.len() {
        return Err((w.at, format!("Unexpected non-whitespace character after JSON at position {}", w.at)));
    }
    Ok(value)
}

fn location_at(text: &[u16], offset: usize, mut line: usize, mut column: usize) -> (usize, usize) {
    let end = offset.min(text.len());
    for i in 0..end {
        let c = text[i];
        if c == 13 {
            line += 1;
            column = 1;
        } else if c == 10 {
            if i == 0 || text[i - 1] != 13 {
                line += 1;
                column = 1;
            }
        } else {
            column += 1;
        }
    }
    (line, column)
}

fn json_error(message: String, line: usize, column: usize) -> PeleError {
    let mut e = PeleError::typed(format!("{} (line {}, column {})", message, line, column), PeleErrorCode::Syntax, "usecase");
    e.line = line;
    e.column = column;
    e
}

// Parses the body of a `json` node. `origin` gives the line and column the text starts at in the diagram.
pub fn parse_ordered_json(text: &str, origin: impl Fn() -> (usize, usize)) -> Result<Vec<(String, Json)>, PeleError> {
    match parse(&js::u16s(text)) {
        Ok(Json::Obj(entries)) => Ok(entries),
        Ok(_) => {
            let (line, column) = origin();
            Err(json_error("JSON value must have an object root".into(), line, column))
        }
        Err(_) => {
            // Reparse restored entity codes so error messages and positions match the source.
            let shown = js::u16s(&written_codes(text));
            let (offset, message) = parse(&shown).err().unwrap_or((0, String::new()));
            let (line, column) = origin();
            let location = location_at(&shown, offset, line, column);
            Err(json_error(format!("Invalid JSON: {}", message), location.0, location.1))
        }
    }
}

pub struct JsonRow {
    // Blank on the second and later values of an array.
    pub key: String,
    pub value: String,
}

fn scalar(value: &Json) -> String {
    match value {
        Json::Str(s) => s.clone(),
        Json::Null => "null".into(),
        Json::Bool(b) => b.to_string(),
        Json::Num(n) => js::num_str(*n),
        _ => String::new(),
    }
}

// One row for each leaf of a JSON object, depth first in the order the keys were written.
pub fn flatten_json_rows(root: &[(String, Json)]) -> Vec<JsonRow> {
    let mut rows = Vec::new();
    let root = Json::Obj(root.to_vec());
    let mut stack: Vec<(&Json, String)> = vec![(&root, String::new())];
    while let Some((current, path)) = stack.pop() {
        let mut children: Vec<(&Json, String)> = Vec::new();
        match current {
            Json::Arr(items) => {
                if items.is_empty() {
                    rows.push(JsonRow { key: path.clone(), value: "[]".into() });
                } else if items.iter().all(|i| !matches!(i, Json::Arr(_) | Json::Obj(_))) {
                    for (i, item) in items.iter().enumerate() {
                        rows.push(JsonRow { key: if i == 0 { path.clone() } else { String::new() }, value: scalar(item) });
                    }
                } else {
                    for (i, item) in items.iter().enumerate() {
                        children.push((item, format!("{}[{}]", path, i)));
                    }
                }
            }
            Json::Obj(entries) => {
                if entries.is_empty() {
                    rows.push(JsonRow { key: path.clone(), value: "{}".into() });
                }
                for (key, value) in entries {
                    children.push((value, if path.is_empty() { key.clone() } else { format!("{}.{}", path, key) }));
                }
            }
            other => rows.push(JsonRow { key: path.clone(), value: scalar(other) }),
        }
        while let Some(child) = children.pop() {
            stack.push(child);
        }
    }
    rows
}
