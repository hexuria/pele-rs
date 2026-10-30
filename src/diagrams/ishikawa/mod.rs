// Port of src/diagrams/ishikawa (index.ts, db.ts, lexer.ts, parser.ts, types.ts).

pub mod render;

use crate::diagrams::Diagram;
use crate::errors::{syntax_error, PeleError};
use crate::js;
use crate::util::chars::{is_space, is_word};

pub struct IshikawaNode {
    pub text: String,
    pub children: Vec<IshikawaNode>,
}

// The root is the effect; its children are the categories of causes, theirs the causes, and so on.
#[derive(Default)]
pub struct IshikawaDb {
    pub title: Option<String>,
    pub root: Option<IshikawaNode>,
    // The line of ancestors of the last node, each with its level and its path of child indexes.
    stack: Vec<(i64, Vec<usize>)>,
    base_level: Option<i64>,
}

impl IshikawaDb {
    fn node_at(&mut self, path: &[usize]) -> &mut IshikawaNode {
        let mut node = self.root.as_mut().unwrap();
        for &i in path {
            node = &mut node.children[i];
        }
        node
    }

    // The first line is the effect. Every later line is a cause, nested by its indentation relative
    // to the first cause, so the effect itself may be indented more than the causes.
    pub fn add_node(&mut self, raw_level: i64, text: String) {
        let node = IshikawaNode { text, children: Vec::new() };
        if self.root.is_none() {
            self.root = Some(node);
            self.stack = vec![(0, Vec::new())];
            return;
        }
        let base = *self.base_level.get_or_insert(raw_level);
        let level = (raw_level - base + 1).max(1);
        while self.stack.len() > 1 && self.stack.last().unwrap().0 >= level {
            self.stack.pop();
        }
        let mut path = self.stack.last().unwrap().1.clone();
        let parent = self.node_at(&path);
        parent.children.push(node);
        path.push(parent.children.len() - 1);
        self.stack.push((level, path));
    }
}

mod t {
    pub const END: u8 = 0;
    pub const EOF: u8 = 1;
    pub const SPACELINE: u8 = 2;
    pub const NL: u8 = 3;
    pub const ISHIKAWA: u8 = 4;
    pub const SPACELIST: u8 = 5;
    pub const TEXT: u8 = 6;
}

const TOKEN_NAMES: [&str; 7] = ["$end", "EOF", "SPACELINE", "NL", "ISHIKAWA", "SPACELIST", "TEXT"];

fn at(src: &[u16], i: usize) -> u32 {
    src.get(i).map_or(u32::MAX, |&c| c as u32)
}

fn keyword(src: &[u16], from: usize) -> usize {
    let word = b"ishikawa-beta";
    let mut k = 0;
    while k < 13 {
        let c = at(src, from + k);
        let w = word[k] as u32;
        if c != w && (c.wrapping_add(32) != w || !(65..=90).contains(&c)) {
            break;
        }
        k += 1;
    }
    // `ishikawa-beta`, or failing that `ishikawa`, either one ending at a word boundary.
    if k == 13 && !is_word(at(src, from + 13)) {
        return 13;
    }
    if k >= 8 && !is_word(at(src, from + 8)) {
        8
    } else {
        0
    }
}

// Produces the tokens of Mermaid's Jison lexer for Ishikawa diagrams, where the first rule that
// matches wins: white space up to a comment or up to the last line break in it is a blank line,
// a single line break before text is a new line, other white space is indentation, and the rest
// of a line is text.
fn tokenize(src: &[u16]) -> (Vec<u8>, Vec<usize>, Vec<usize>) {
    let (mut types, mut starts, mut ends) = (Vec::new(), Vec::new(), Vec::new());
    let n = src.len();
    let mut i = 0;
    while i < n {
        let mut j = i;
        let mut last_break: isize = -1;
        while j < n && is_space(src[j] as u32) {
            if src[j] == 10 {
                last_break = j as isize;
            }
            j += 1;
        }
        let (type_, end);
        if at(src, j) == 37 && at(src, j + 1) == 37 {
            type_ = t::SPACELINE;
            let mut e = j + 2;
            while e < n {
                let c = src[e];
                if c == 10 || c == 13 || c == 0x2028 || c == 0x2029 {
                    break;
                }
                e += 1;
            }
            end = e;
        } else if j == i {
            let length = keyword(src, i);
            if length > 0 {
                type_ = t::ISHIKAWA;
                end = i + length;
            } else {
                type_ = t::TEXT;
                end = src[i..].iter().position(|&c| c == 10).map_or(n, |p| p + i);
            }
        } else if last_break > i as isize {
            type_ = t::SPACELINE;
            end = last_break as usize + 1;
        } else if last_break == i as isize {
            type_ = t::NL;
            end = i + 1;
        } else {
            type_ = t::SPACELIST;
            end = j;
        }
        types.push(type_);
        starts.push(i);
        ends.push(end);
        i = end;
    }
    types.extend([t::EOF, t::END]);
    starts.extend([n, n]);
    ends.extend([n, n]);
    (types, starts, ends)
}

const STOP: &str = "'SPACELINE', 'NL', 'EOF'";

// Follows the parser Jison builds from Mermaid's grammar: blank lines and comments may come first,
// then the keyword and at most one line break, then statements. A statement is a line of text,
// with or without indentation, or a blank line; each is ended by a blank line, a line break or
// the end of the text, and any number of line breaks after that.
pub fn parse_ishikawa(src: &[u16], db: &mut IshikawaDb) -> Result<(), PeleError> {
    let (types, starts, ends) = tokenize(src);
    let mut i = 0;
    let fail = |i: usize, expected: &str| syntax_error("ishikawa", src, starts[i], &format!("Expecting {}, got '{}'", expected, TOKEN_NAMES[types[i] as usize]), false);

    if types[i] == t::SPACELINE {
        i += 1;
        while types[i] == t::SPACELINE || types[i] == t::NL {
            i += 1;
        }
        if types[i] != t::ISHIKAWA {
            return Err(fail(i, "'SPACELINE', 'NL', 'ISHIKAWA'"));
        }
    } else if types[i] != t::ISHIKAWA {
        return Err(fail(i, "'SPACELINE', 'ISHIKAWA'"));
    }
    i += 1;
    if types[i] == t::NL {
        i += 1;
    }
    let stops = |k: u8| k == t::SPACELINE || k == t::NL || k == t::EOF;
    loop {
        let type_ = types[i];
        if type_ == t::TEXT || (type_ == t::SPACELIST && types[i + 1] == t::TEXT) {
            let text = if type_ == t::TEXT { i } else { i + 1 };
            i = text + 1;
            if !stops(types[i]) {
                return Err(fail(i, STOP));
            }
            let level = if type_ == t::TEXT { 0 } else { (ends[text - 1] - starts[text - 1]) as i64 };
            db.add_node(level, js::trim(&js::from16(&src[starts[text]..ends[text]])).to_string());
        } else if type_ == t::SPACELINE || type_ == t::SPACELIST {
            i += 1;
            if !stops(types[i]) {
                return Err(fail(i, if type_ == t::SPACELIST { "'SPACELINE', 'NL', 'EOF', 'TEXT'" } else { STOP }));
            }
        } else {
            return Err(fail(i, "'SPACELINE', 'SPACELIST', 'TEXT'"));
        }
        i += 1;
        while types[i] == t::NL || types[i] == t::EOF {
            i += 1;
        }
        if types[i] == t::END {
            return Ok(());
        }
    }
}

pub static DIAGRAM: Diagram = Diagram {
    type_: "ishikawa",
    section: None,
    keep_comments: false,
    parse: |source, _config, title, _line_offset| {
        let mut db = IshikawaDb { title: title.map(str::to_string), ..Default::default() };
        parse_ishikawa(source, &mut db)?;
        Ok(Box::new(db))
    },
    render: |model, config, options| Ok(render::render_ishikawa(model.downcast_ref::<IshikawaDb>().unwrap(), config, options)),
};
