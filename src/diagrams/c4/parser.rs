// Port of src/diagrams/c4/parser.ts: accepts what the parser Jison generates from
// c4Diagram.jison accepts, and calls the model builder in the order its semantic actions do.

use super::db::{Attr, C4Db, Named};
use super::lexer::*;
use crate::errors::{syntax_error, PeleError};
use crate::js;

// Mermaid's typeC4Shape for each shape keyword, indexed from PERSON.
const KINDS: [&str; 20] = [
    "person",
    "external_person",
    "system",
    "system_db",
    "system_queue",
    "external_system",
    "external_system_db",
    "external_system_queue",
    "container",
    "container_db",
    "container_queue",
    "external_container",
    "external_container_db",
    "external_container_queue",
    "component",
    "component_db",
    "component_queue",
    "external_component",
    "external_component_db",
    "external_component_queue",
];
const REL_TYPES: [&str; 8] = ["rel", "birel", "rel_u", "rel_d", "rel_l", "rel_r", "rel_b", "rel"];
const NODE_TYPES: [&str; 3] = ["node", "nodeL", "nodeR"];

const HEADERS: &str = "'direction_tb', 'direction_bt', 'direction_rl', 'direction_lr', 'C4_CONTEXT', 'C4_CONTAINER', 'C4_COMPONENT', 'C4_DYNAMIC', 'C4_DEPLOYMENT'";
const ATTRIBUTES: &str = "'STR', 'STR_KEY', 'ATTRIBUTE', 'ATTRIBUTE_EMPTY'";

const P_ITEM: u8 = 0;
// Only a diagram statement may come next.
const P_DIAGRAM: u8 = 1;
const P_ITEM_OR_END: u8 = 2;
const P_AFTER_OTHER: u8 = 3;
const P_AFTER_DIAGRAM: u8 = 4;

fn is_other(t: u32) -> bool {
    matches!(t, TITLE | ACC_DESCRIPTION | ACC_TITLE | ACC_DESCR | ACC_DESCR_MULTILINE_VALUE)
}

fn is_diagram(t: u32) -> bool {
    (PERSON..=UPDATE_LAYOUT_CONFIG).contains(&t)
}

struct P<'a> {
    src: &'a [u16],
    t: Tokens,
    i: usize,
}

impl P<'_> {
    fn kind(&self) -> u32 {
        self.t.types[self.i]
    }
    fn text(&self, at: usize) -> String {
        js::from16(&self.src[self.t.starts[at]..self.t.ends[at]])
    }
    fn fail(&self, expected: &str) -> PeleError {
        let t = self.kind();
        if t == ERROR {
            return syntax_error("c4", self.src, self.t.starts[self.i], "", true);
        }
        syntax_error("c4", self.src, self.t.starts[self.i], &format!("Expecting {}, got '{}'", expected, TOKEN_NAMES[t as usize]), false)
    }
    fn expect(&mut self, ty: u32) -> Result<(), PeleError> {
        if self.kind() != ty {
            return Err(self.fail(&format!("'{}'", TOKEN_NAMES[ty as usize])));
        }
        self.i += 1;
        Ok(())
    }
    fn attributes(&mut self) -> Result<Vec<Attr>, PeleError> {
        let mut attrs = Vec::new();
        loop {
            match self.kind() {
                STR => {
                    attrs.push(Attr::Str(js::trim(&self.text(self.i)).to_string()));
                    self.i += 1;
                }
                ATTRIBUTE_EMPTY => {
                    attrs.push(Attr::Str(String::new()));
                    self.i += 1;
                }
                STR_KEY => {
                    self.i += 1;
                    if self.kind() != STR_VALUE {
                        return Err(self.fail("'STR_VALUE'"));
                    }
                    attrs.push(Attr::Named(Named { key: js::trim(&self.text(self.i - 1)).to_string(), value: js::trim(&self.text(self.i)).to_string() }));
                    self.i += 1;
                }
                _ => break,
            }
        }
        if attrs.is_empty() {
            return Err(self.fail(ATTRIBUTES));
        }
        Ok(attrs)
    }
    fn other(&mut self, t: u32, db: &mut C4Db) -> Result<(), PeleError> {
        let skip = |s: String, k: usize| -> String { s.chars().skip(k).collect() };
        if t == TITLE {
            db.set_title(&skip(self.text(self.i), 6));
            self.i += 1;
        } else if t == ACC_DESCRIPTION {
            db.set_acc_description(&skip(self.text(self.i), 15));
            self.i += 1;
        } else if t == ACC_DESCR_MULTILINE_VALUE {
            db.set_acc_description(js::trim(&self.text(self.i)));
            self.i += 1;
        } else {
            self.i += 1;
            if t == ACC_TITLE {
                if self.kind() != ACC_TITLE_VALUE {
                    return Err(self.fail("'acc_title_value'"));
                }
                // The grammar hands `accTitle:` to setTitle, so it becomes the visible title.
                db.set_title(js::trim(&self.text(self.i)));
            } else {
                if self.kind() != ACC_DESCR_VALUE {
                    return Err(self.fail("'acc_descr_value'"));
                }
                db.set_acc_description(js::trim(&self.text(self.i)));
            }
            self.i += 1;
        }
        Ok(())
    }
    // Returns true when the statement opens a boundary.
    fn diagram(&mut self, t: u32, db: &mut C4Db) -> Result<bool, PeleError> {
        self.i += 1;
        let mut attrs = self.attributes()?;
        let insert = |attrs: &mut Vec<Attr>, s: &str| {
            let at = 2.min(attrs.len());
            attrs.insert(at, Attr::Str(s.to_string()));
        };
        if t <= SYSTEM_EXT_QUEUE {
            db.add_person_or_system(KINDS[(t - PERSON) as usize], &attrs);
        } else if t <= COMPONENT_EXT_QUEUE {
            db.add_container(KINDS[(t - PERSON) as usize], &attrs);
        } else if t <= NODE_R {
            if t == ENTERPRISE_BOUNDARY {
                insert(&mut attrs, "ENTERPRISE");
            } else if t == SYSTEM_BOUNDARY {
                insert(&mut attrs, "SYSTEM");
            } else if t == CONTAINER_BOUNDARY {
                insert(&mut attrs, "CONTAINER");
            }
            if t >= NODE {
                db.add_deployment_node(NODE_TYPES[(t - NODE) as usize], &attrs);
            } else if t == CONTAINER_BOUNDARY {
                db.add_person_or_system_boundary(&attrs, "container");
            } else {
                db.add_person_or_system_boundary(&attrs, "system");
            }
            if self.kind() == LBRACE {
                self.i += 1;
                self.expect(NEWLINE)?;
            } else if self.kind() == NEWLINE {
                self.i += 1;
                self.expect(LBRACE)?;
                if self.kind() == NEWLINE {
                    self.i += 1;
                }
            } else {
                return Err(self.fail("'NEWLINE', 'LBRACE'"));
            }
            return Ok(true);
        } else if t <= REL_INDEX {
            // RelIndex takes a sequence number first, which Mermaid ignores.
            if t == REL_INDEX {
                attrs.remove(0);
            }
            db.add_rel(REL_TYPES[(t - REL) as usize], &attrs);
        } else if t == UPDATE_EL_STYLE {
            db.update_el_style(&attrs);
        } else if t == UPDATE_REL_STYLE {
            db.update_rel_style(&attrs);
        } else {
            db.update_layout_config(&attrs);
        }
        Ok(false)
    }
}

pub fn parse_c4(src: &[u16], db: &mut C4Db) -> Result<(), PeleError> {
    let mut r = P { src, t: tokenize(src), i: 0 };
    let header = r.kind();
    if !(C4_CONTEXT..=C4_DEPLOYMENT).contains(&header) {
        return Err(r.fail(HEADERS));
    }
    r.i += 1;
    r.expect(NEWLINE)?;
    let mut depth = 0;
    let mut state = P_ITEM;
    loop {
        let t = r.kind();
        if state == P_AFTER_OTHER || state == P_AFTER_DIAGRAM {
            if t == NEWLINE {
                r.i += 1;
                state = P_ITEM_OR_END;
                continue;
            }
            if state == P_AFTER_OTHER && is_diagram(t) {
                state = P_DIAGRAM;
                continue;
            }
        } else if is_diagram(t) {
            if r.diagram(t, db)? {
                depth += 1;
                state = P_DIAGRAM;
            } else {
                state = P_AFTER_DIAGRAM;
            }
            continue;
        } else if is_other(t) && state != P_DIAGRAM {
            r.other(t, db)?;
            state = P_AFTER_OTHER;
            continue;
        } else if state != P_ITEM_OR_END {
            return Err(r.fail("'title', 'accDescription', 'acc_title', 'acc_descr', 'acc_descr_multiline_value', 'PERSON', 'REL'"));
        }
        if depth > 0 {
            if t != RBRACE {
                return Err(r.fail("'NEWLINE', 'RBRACE'"));
            }
            r.i += 1;
            db.pop_boundary_parse_stack();
            depth -= 1;
            state = P_AFTER_DIAGRAM;
        } else {
            if t != EOF {
                return Err(r.fail("'NEWLINE', 'EOF'"));
            }
            r.i += 1;
            r.expect(END)?;
            let first = r.text(0);
            db.set_c4_type(&first);
            return Ok(());
        }
    }
}
