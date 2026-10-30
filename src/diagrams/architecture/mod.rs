// Port of src/diagrams/architecture (index.ts, db.ts, parser.ts, icons.ts).

pub mod layout;
pub mod render;

use crate::diagrams::common::tokens::{
    acc_descr, acc_descr_value, acc_title, acc_title_value, directive, newline_token, single_line_comment, string, string_value, title_before, title_value, tokenize, whitespace, yaml, Reader,
    TokenType,
};
use crate::diagrams::cynefin::collapse_indents;
use crate::diagrams::Diagram;
use crate::errors::{PeleError, PeleErrorCode};
use crate::js;
use crate::util::chars::is_word;

mod t {
    pub const HEADER: i32 = 0;
    pub const JUNCTION: i32 = 1;
    pub const SERVICE: i32 = 2;
    pub const COLUMN: i32 = 3;
    pub const GROUP: i32 = 4;
    pub const ALIGN: i32 = 5;
    pub const ROW: i32 = 6;
    pub const DASHES: i32 = 7;
    pub const IN: i32 = 8;
    pub const COLON: i32 = 9;
    pub const DASH: i32 = 10;
    pub const DIRECTION: i32 = 11;
    pub const ARROW_GROUP: i32 = 12;
    pub const INTO: i32 = 13;
    pub const ACC_DESCR: i32 = 14;
    pub const ACC_TITLE: i32 = 15;
    pub const TITLE: i32 = 16;
    pub const STRING: i32 = 17;
    pub const ID: i32 = 18;
    pub const NEWLINE: i32 = 19;
    pub const ICON: i32 = 24;
    pub const LABEL: i32 = 25;
}

fn at(src: &[u16], i: usize) -> u32 {
    src.get(i).map_or(u32::MAX, |&c| c as u32)
}

// /[\w]([-\w]*\w)?/y
fn id_match(src: &[u16], p: usize) -> Option<usize> {
    if !is_word(at(src, p)) {
        return None;
    }
    let mut e = p + 1;
    let mut last = p + 1;
    while is_word(at(src, e)) || at(src, e) == 45 {
        if is_word(at(src, e)) {
            last = e + 1;
        }
        e += 1;
    }
    Some(last)
}

// /\[(?:"([^"\\]|\\.)*"|'([^'\\]|\\.)*'|[^\[\]\r\n]+)\]/y
fn label_match(src: &[u16], p: usize) -> Option<usize> {
    if at(src, p) != 91 {
        return None;
    }
    let q = at(src, p + 1);
    if q == 34 || q == 39 {
        let mut i = p + 2;
        while i < src.len() {
            let c = src[i] as u32;
            if c == q {
                if at(src, i + 1) == 93 {
                    return Some(i + 2);
                }
                break;
            }
            if c == 92 {
                // `.` does not take a line terminator.
                if i + 1 >= src.len() || matches!(src[i + 1], 10 | 13 | 0x2028 | 0x2029) {
                    break;
                }
                i += 2;
            } else {
                i += 1;
            }
        }
    }
    let mut e = p + 1;
    while e < src.len() && !matches!(src[e], 91 | 93 | 13 | 10) {
        e += 1;
    }
    (e > p + 1 && at(src, e) == 93).then_some(e + 1)
}

fn tokens() -> Vec<TokenType> {
    const ID: usize = 18;
    const YAML: usize = 21;
    let word = |text: &'static str| {
        let mut t = TokenType::lit(text, text);
        t.longer = vec![ID];
        t
    };
    let mut dashes = TokenType::lit("--", "--");
    dashes.longer = vec![YAML];
    let mut dash = TokenType::lit("-", "-");
    dash.longer = vec![YAML];
    vec![
        word("architecture-beta"),
        word("junction"),
        word("service"),
        word("column"),
        word("group"),
        word("align"),
        word("row"),
        dashes,
        word("in"),
        TokenType::lit(":", ":"),
        dash,
        TokenType::re("ARROW_DIRECTION", None, |src, p| matches!(at(src, p), 76 | 82 | 84 | 66).then_some(p + 1)),
        TokenType::re("ARROW_GROUP", None, |src, p| "{group}".bytes().enumerate().all(|(k, b)| at(src, p + k) == b as u32).then_some(p + 7)),
        TokenType::re("ARROW_INTO", None, |src, p| matches!(at(src, p), 60 | 62).then_some(p + 1)),
        acc_descr(),
        acc_title(),
        // titleBefore(String.raw`[-\w]*\w`)
        title_before(|src, q| {
            let mut e = q;
            while is_word(at(src, e)) || at(src, e) == 45 {
                if is_word(at(src, e)) {
                    return true;
                }
                e += 1;
            }
            false
        }),
        string(),
        TokenType::re("ID", None, id_match),
        newline_token(),
        whitespace(),
        yaml(),
        directive(),
        single_line_comment(),
        // /\([\w-:]+\)/y: next to a class escape, `-` is itself, so the class is \w, `-` and `:`.
        TokenType::re("ARCH_ICON", None, |src, p| {
            if at(src, p) != 40 {
                return None;
            }
            let mut e = p + 1;
            while is_word(at(src, e)) || at(src, e) == 45 || at(src, e) == 58 {
                e += 1;
            }
            (e > p + 1 && at(src, e) == 41).then_some(e + 1)
        }),
        TokenType::re("ARCH_TITLE", None, label_match),
    ]
}

#[derive(Clone)]
pub struct ArchNode {
    pub id: String,
    pub junction: bool,
    pub icon: Option<String>,
    pub icon_text: Option<String>,
    pub title: Option<String>,
    pub in_: Option<String>,
}

#[derive(Clone)]
pub struct ArchGroup {
    pub id: String,
    pub icon: Option<String>,
    pub title: Option<String>,
    pub in_: Option<String>,
}

#[derive(Clone)]
pub struct ArchEdge {
    pub lhs_id: String,
    pub lhs_dir: char,
    pub lhs_into: bool,
    pub lhs_group: bool,
    pub rhs_id: String,
    pub rhs_dir: char,
    pub rhs_into: bool,
    pub rhs_group: bool,
    pub title: Option<String>,
}

pub struct ArchLayoutHint {
    pub row: bool,
    pub members: Vec<String>,
}

#[derive(Default)]
pub struct ArchitectureDb {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub nodes: Vec<ArchNode>,
    pub groups: Vec<ArchGroup>,
    pub edges: Vec<ArchEdge>,
    pub layout_hints: Vec<ArchLayoutHint>,
    // Ids in use: true for a group, false for a node.
    registered: Vec<(String, bool)>,
}

fn fail(message: String) -> PeleError {
    PeleError::typed(message, PeleErrorCode::Semantic, "architecture")
}

impl ArchitectureDb {
    fn registered(&self, id: &str) -> Option<bool> {
        self.registered.iter().find(|(k, _)| k == id).map(|(_, g)| *g)
    }

    fn register(&mut self, kind: &str, id: &str, parent: Option<&str>) -> Result<(), PeleError> {
        if let Some(group) = self.registered(id) {
            return Err(fail(format!("The {} id [{}] is already in use by another {}", kind, id, if group { "group" } else { "node" })));
        }
        if let Some(parent) = parent {
            if id == parent {
                return Err(fail(format!("The {} [{}] cannot be placed within itself", kind, id)));
            }
            match self.registered(parent) {
                None => return Err(fail(format!("The {} [{}]'s parent does not exist. Please make sure the parent is created before this {}", kind, id, kind))),
                Some(false) => return Err(fail(format!("The {} [{}]'s parent is not a group", kind, id))),
                _ => {}
            }
        }
        self.registered.push((id.to_string(), kind == "group"));
        Ok(())
    }

    fn node(&self, id: &str) -> Option<&ArchNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    fn add_edge(&mut self, edge: ArchEdge) -> Result<(), PeleError> {
        let ends = [("left", &edge.lhs_id, edge.lhs_group), ("right", &edge.rhs_id, edge.rhs_group)];
        for (side, id, _) in ends {
            if self.registered(id).is_none() {
                return Err(fail(format!("The {}-hand id [{}] does not yet exist. Please create the service/group before declaring an edge to it.", side, id)));
            }
        }
        // Mermaid lets a group id through the check above and then fails with a TypeError.
        for (side, id, _) in ends {
            if self.node(id).is_none() {
                return Err(fail(format!("The {}-hand id [{}] is a group. Add {{group}} to a service inside it instead.", side, id)));
            }
        }
        let parent = self.node(&edge.lhs_id).unwrap().in_.clone();
        if parent.is_some() && parent == self.node(&edge.rhs_id).unwrap().in_ {
            for (side, id, crosses) in ends {
                if crosses {
                    return Err(fail(format!("The {}-hand id [{}] is modified to traverse the group boundary, but the edge does not pass through two groups.", side, id)));
                }
            }
        }
        self.edges.push(edge);
        Ok(())
    }

    fn add_layout_hint(&mut self, hint: ArchLayoutHint) -> Result<(), PeleError> {
        let direction = if hint.row { "row" } else { "column" };
        if hint.members.len() < 2 {
            return Err(fail(format!("An align directive requires at least two members; got {}", hint.members.len())));
        }
        let mut seen: Vec<&str> = Vec::new();
        for id in &hint.members {
            if self.registered(id) != Some(false) {
                return Err(fail(format!("align {} references [{}], which is not a service or junction", direction, id)));
            }
            if seen.contains(&id.as_str()) {
                return Err(fail(format!("align {} lists [{}] more than once", direction, id)));
            }
            seen.push(id);
        }
        self.layout_hints.push(hint);
        Ok(())
    }
}

// Mermaid's ArchitectureValueConverter for ARCH_TITLE.
fn label_value(text: &str) -> String {
    let inner: String = {
        let mut c = text.chars();
        c.next();
        c.next_back();
        c.as_str().to_string()
    };
    let mut result = js::trim(&inner).to_string();
    if result.chars().count() >= 1 && ((result.starts_with('"') && result.ends_with('"')) || (result.starts_with('\'') && result.ends_with('\''))) {
        let mut c = result.chars();
        c.next();
        c.next_back();
        result = c.as_str().replace("\\\"", "\"").replace("\\'", "'");
    }
    js::trim(&result).to_string()
}

#[derive(Default)]
struct Ast {
    title: Option<String>,
    acc_title: Option<String>,
    acc_descr: Option<String>,
    groups: Vec<ArchGroup>,
    services: Vec<ArchNode>,
    junctions: Vec<ArchNode>,
    edges: Vec<ArchEdge>,
    alignments: Vec<ArchLayoutHint>,
}

fn parse_architecture(src: &[u16]) -> Result<Ast, PeleError> {
    let types = tokens();
    let tokens = tokenize(src, &types, "architecture")?;
    let mut r = Reader::new(&tokens, &types, "architecture");
    let r = &mut r;
    let mut ast = Ast::default();
    let strip = |s: String| -> String {
        let mut c = s.chars();
        c.next();
        c.next_back();
        c.as_str().to_string()
    };

    while r.kind() == t::NEWLINE {
        r.i += 1;
    }
    r.expect(t::HEADER)?;
    while r.kind() != -1 {
        match r.kind() {
            t::NEWLINE => {
                r.i += 1;
                continue;
            }
            t::ACC_DESCR => ast.acc_descr = Some(acc_descr_value(&r.take())),
            t::ACC_TITLE => ast.acc_title = Some(acc_title_value(&r.take())),
            t::TITLE => ast.title = Some(title_value(&r.take())),
            t::GROUP => {
                r.i += 1;
                let mut group = ArchGroup { id: r.expect(t::ID)?, icon: None, title: None, in_: None };
                if r.kind() == t::ICON {
                    group.icon = Some(strip(r.take()));
                }
                if r.kind() == t::LABEL {
                    group.title = Some(label_value(&r.take()));
                }
                if r.accept(t::IN) {
                    group.in_ = Some(r.expect(t::ID)?);
                }
                ast.groups.push(group);
            }
            t::SERVICE => {
                r.i += 1;
                let mut service = ArchNode { id: r.expect(t::ID)?, junction: false, icon: None, icon_text: None, title: None, in_: None };
                if r.kind() == t::STRING {
                    service.icon_text = Some(string_value(&r.take()));
                } else if r.kind() == t::ICON {
                    service.icon = Some(strip(r.take()));
                }
                if r.kind() == t::LABEL {
                    service.title = Some(label_value(&r.take()));
                }
                if r.accept(t::IN) {
                    service.in_ = Some(r.expect(t::ID)?);
                }
                ast.services.push(service);
            }
            t::JUNCTION => {
                r.i += 1;
                let mut junction = ArchNode { id: r.expect(t::ID)?, junction: true, icon: None, icon_text: None, title: None, in_: None };
                if r.accept(t::IN) {
                    junction.in_ = Some(r.expect(t::ID)?);
                }
                ast.junctions.push(junction);
            }
            t::ID => {
                let lhs_id = r.take();
                let lhs_group = r.accept(t::ARROW_GROUP);
                r.expect(t::COLON)?;
                let lhs_dir = r.expect(t::DIRECTION)?.chars().next().unwrap();
                let lhs_into = r.accept(t::INTO);
                let mut title = None;
                if !r.accept(t::DASHES) {
                    if r.kind() != t::DASH {
                        return Err(r.fail("'--' or '-'"));
                    }
                    r.i += 1;
                    title = Some(label_value(&r.expect(t::LABEL)?));
                    r.expect(t::DASH)?;
                }
                let rhs_into = r.accept(t::INTO);
                let rhs_dir = r.expect(t::DIRECTION)?.chars().next().unwrap();
                r.expect(t::COLON)?;
                let rhs_id = r.expect(t::ID)?;
                let rhs_group = r.accept(t::ARROW_GROUP);
                ast.edges.push(ArchEdge { lhs_id, lhs_dir, lhs_into, lhs_group, rhs_id, rhs_dir, rhs_into, rhs_group, title });
            }
            t::ALIGN => {
                r.i += 1;
                if r.kind() != t::ROW && r.kind() != t::COLUMN {
                    return Err(r.fail("'row' or 'column'"));
                }
                let row = r.take() == "row";
                let mut members = vec![r.expect(t::ID)?, r.expect(t::ID)?];
                while r.kind() == t::ID {
                    members.push(r.take());
                }
                ast.alignments.push(ArchLayoutHint { row, members });
            }
            _ => return Err(r.fail("a group, a service, a junction, an edge, an alignment, a title, or a line break")),
        }
        if r.kind() != -1 {
            if r.kind() != t::NEWLINE {
                return Err(r.fail("a line break or the end of input"));
            }
            while r.kind() == t::NEWLINE {
                r.i += 1;
            }
        }
    }
    Ok(ast)
}

// Mermaid's populateDb: every group, then services, junctions, edges, and alignments.
fn populate(ast: Ast, db: &mut ArchitectureDb) -> Result<(), PeleError> {
    if let Some(d) = ast.acc_descr.filter(|d| !d.is_empty()) {
        db.acc_descr = Some(collapse_indents(&d));
    }
    if let Some(t) = ast.acc_title.filter(|t| !t.is_empty()) {
        db.acc_title = Some(js::trim_start(&t).to_string());
    }
    if let Some(t) = ast.title.filter(|t| !t.is_empty()) {
        db.title = Some(t);
    }
    for group in ast.groups {
        db.register("group", &group.id, group.in_.as_deref())?;
        match db.groups.iter_mut().find(|g| g.id == group.id) {
            Some(slot) => *slot = group,
            None => db.groups.push(group),
        }
    }
    for node in ast.services.into_iter().chain(ast.junctions) {
        db.register(if node.junction { "junction" } else { "service" }, &node.id, node.in_.as_deref())?;
        match db.nodes.iter_mut().find(|n| n.id == node.id) {
            Some(slot) => *slot = node,
            None => db.nodes.push(node),
        }
    }
    for edge in ast.edges {
        db.add_edge(edge)?;
    }
    for hint in ast.alignments {
        db.add_layout_hint(hint)?;
    }
    Ok(())
}

pub static DIAGRAM: Diagram = Diagram {
    type_: "architecture",
    section: None,
    keep_comments: false,
    parse: |source, _config, title, _line_offset| {
        let mut db = ArchitectureDb::default();
        if let Some(t) = title.filter(|t| !t.is_empty()) {
            db.title = Some(t.to_string());
        }
        populate(parse_architecture(source)?, &mut db)?;
        Ok(Box::new(db))
    },
    render: |model, config, options| Ok(render::render_architecture(model.downcast_ref::<ArchitectureDb>().unwrap(), config, options)),
};
