// Port of src/diagrams/class/members.ts. Offsets are UTF-16, as upstream's string indexes.

use crate::diagrams::common::generics::parse_generic_types;
use crate::js;
use crate::util::chars::is_line_end;

fn is_visibility(c: u16) -> bool {
    matches!(c, 35 | 43 | 126 | 45)
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClassMember {
    pub id: String,
    pub method: bool,
    pub visibility: String,
    pub classifier: String,
    pub parameters: String,
    pub return_type: String,
}

impl ClassMember {
    pub fn new(input: &str, method: bool) -> Self {
        let mut m = ClassMember { id: String::new(), method, visibility: String::new(), classifier: String::new(), parameters: String::new(), return_type: String::new() };
        let units = js::u16s(input);
        // Mermaid throws a TypeError for a method with no opening parenthesis, such as `run)`.
        // It is kept here as an attribute, so it is shown as written.
        if !method || !m.parse_method(&units) {
            m.parse_attribute(&units);
        }
        m.id = if m.id.starts_with(' ') { format!(" {}", js::trim(&m.id)) } else { js::trim(&m.id).to_string() };
        m
    }

    // The same split as Mermaid's /([#+~-])?(.+)\((.*)\)([\s$*])?(.*)([$*])?/, found in one pass:
    // the first line with an opening parenthesis after its first character and a closing one later.
    fn parse_method(&mut self, input: &[u16]) -> bool {
        let n = input.len();
        let mut start = 0;
        while start < n {
            let mut end = start;
            while end < n && !is_line_end(input[end] as u32) {
                end += 1;
            }
            let mut close = end as isize - 1;
            while close > start as isize && input[close as usize] != 41 {
                close -= 1;
            }
            let mut open = close - 1;
            while open > start as isize && input[open as usize] != 40 {
                open -= 1;
            }
            if open > start as isize {
                let (open, close) = (open as usize, close as usize);
                let first = input[start];
                let visible = is_visibility(first) && open > start + 1;
                if visible {
                    self.visibility = js::from16(&[first]);
                }
                self.id = js::from16(&input[if visible { start + 1 } else { start }..open]);
                self.parameters = js::trim(&js::from16(&input[open + 1..close])).to_string();
                let mut rest = close + 1;
                let mut classifier = String::new();
                if rest < n {
                    let next = js::from16(&input[rest..rest + 1]);
                    if next == "$" || next == "*" || js::trim(&next).is_empty() {
                        classifier = js::trim(&next).to_string();
                        rest += 1;
                    }
                }
                let mut stop = rest;
                while stop < n && !is_line_end(input[stop] as u32) {
                    stop += 1;
                }
                self.return_type = js::trim(&js::from16(&input[rest.min(stop)..stop])).to_string();
                if classifier.is_empty() {
                    let rt = js::u16s(&self.return_type);
                    if let Some(&last) = rt.last() {
                        if last == 36 || last == 42 {
                            classifier = js::from16(&[last]);
                            self.return_type = js::from16(&rt[..rt.len() - 1]);
                        }
                    }
                }
                self.classifier = classifier;
                return true;
            }
            start = end + 1;
        }
        self.method = false;
        false
    }

    fn parse_attribute(&mut self, input: &[u16]) {
        let n = input.len();
        if let Some(&first) = input.first() {
            if is_visibility(first) {
                self.visibility = js::from16(&[first]);
            }
        }
        if let Some(&last) = input.last() {
            if last == 36 || last == 42 {
                self.classifier = js::from16(&[last]);
            }
        }
        // String.prototype.substring swaps its ends when the start is past the end.
        let a = if self.visibility.is_empty() { 0 } else { 1 };
        let b = if self.classifier.is_empty() { n } else { n - 1 };
        self.id = js::from16(&input[a.min(b)..a.max(b)]);
    }

    pub fn display_text(&self) -> String {
        let mut text = format!("{}{}", self.visibility, parse_generic_types(&self.id));
        if self.method {
            text.push_str(&format!("({})", parse_generic_types(js::trim(&self.parameters))));
            if !self.return_type.is_empty() {
                text.push_str(" : ");
                text.push_str(&parse_generic_types(&self.return_type));
            }
        }
        js::trim(&text).to_string()
    }
}
