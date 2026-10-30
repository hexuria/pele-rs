// Port of src/diagrams/agentflow/parser.ts. Mappings of statements to source spans are not kept;
// token positions are read only to place metadata errors.

use super::db::AgentflowDb;
use super::lexer::tokenize;
use super::tokens as t;
use super::types::{DocItem, FlowText, LabelType, LinkInfo, Loc};
use crate::errors::{input_offset, syntax_error, Edit, PeleError};
use crate::js;
use crate::util::chars::is_space;

const ID: &[u8] = &[t::NUM, t::NODE_STRING, t::DOWN, t::MINUS, t::DEFAULT, t::COMMA, t::COLON, t::AMP, t::BRKT, t::MULT, t::UNICODE_TEXT];
const ALPHANUM: &[u8] = &[t::NUM, t::UNICODE_TEXT, t::NODE_STRING, t::DIR, t::DOWN, t::MINUS, t::COMMA, t::COLON, t::AMP, t::BRKT, t::MULT];
const NO_TAGS: &[u8] = &[
    t::NUM,
    t::NODE_STRING,
    t::SPACE,
    t::MINUS,
    t::AMP,
    t::UNICODE_TEXT,
    t::COLON,
    t::MULT,
    t::BRKT,
    t::STYLE,
    t::LINKSTYLE,
    t::CLASSDEF,
    t::CLASS,
    t::CLICK,
    t::GRAPH,
    t::DIR,
    t::flow,
    t::connector,
    t::global,
    t::end,
    t::DOWN,
    t::UP,
    t::START_LINK,
];
const STYLE_PART: &[u8] = &[t::NUM, t::NODE_STRING, t::COLON, t::SPACE, t::BRKT, t::STYLE];
const SEPARATOR: &[u8] = &[t::NEWLINE, t::SEMI, t::EOF, t::COMMENT];
const TRAILING: &[u8] = &[t::COMMENT, t::NEWLINE, t::SPACE, t::EOF];

const TEXT_TOKENS: &str = "'UNICODE_TEXT', 'TEXT', 'TAGSTART'";
const SEPARATORS: &str = "'COMMENT', 'SEMI', 'NEWLINE', 'EOF'";
const DIRECTIONS: [&str; 5] = ["TB", "BT", "RL", "LR", "TD"];
const STATEMENT_START: &str = "'COMMENT', 'SEMI', 'NEWLINE', 'SPACE', 'EOF', 'flow', 'global', 'connector', 'acc_title', 'acc_descr', 'acc_descr_multiline_value', 'STYLE', 'LINKSTYLE', 'CLASSDEF', 'CLASS', 'CLICK', 'NODE_STRING'";

struct Frame {
    global: bool,
    id: Option<FlowText>,
    title: Option<FlowText>,
    data: Option<String>,
    data_from: usize,
    list: Vec<DocItem>,
    collect: bool,
}

type R<T> = Result<T, PeleError>;

/// `source.replace(/}[^\S\n]*\n/g, '}\n')` and the edits it makes. Mermaid trims spaces and
/// tabs between a closing brace and the end of its line before parsing.
fn brace_trail(source: &[u16]) -> (Vec<u16>, Vec<Edit>) {
    let mut out = Vec::with_capacity(source.len());
    let mut edits = Vec::new();
    let mut i = 0;
    while i < source.len() {
        if source[i] == b'}' as u16 {
            let mut e = i + 1;
            while e < source.len() && source[e] != 10 && is_space(source[e] as u32) {
                e += 1;
            }
            if e < source.len() && source[e] == 10 {
                edits.push((i, e + 1, 2));
                out.push(b'}' as u16);
                out.push(10);
                i = e + 1;
                continue;
            }
        }
        out.push(source[i]);
        i += 1;
    }
    (out, edits)
}

struct P<'a> {
    source: &'a [u16],
    src: Vec<u16>,
    edits: Vec<Edit>,
    types: Vec<u8>,
    texts: Vec<String>,
    starts: Vec<usize>,
    locs: Vec<usize>,
    i: usize,
    pending_data: Option<String>,
    data_from: usize,
}

impl P<'_> {
    fn ty(&self) -> u8 {
        self.types[self.i]
    }

    fn fail<T>(&self, expected: &str) -> R<T> {
        let ty = self.ty();
        let mut from = self.starts[self.i];
        if ty == t::EOF || ty == t::END {
            // Comments are tokens here, so skip trailing ones.
            let mut k = self.i;
            while k > 0 && TRAILING.contains(&self.types[k - 1]) {
                k -= 1;
            }
            let mut e = self.starts[k];
            while e > 0 && js::is_space_u(self.src[e - 1] as u32) {
                e -= 1;
            }
            from = e;
        }
        let at = if self.src[..] == self.source[..] { from } else { input_offset(std::slice::from_ref(&self.edits), from) };
        if ty == t::ERROR {
            return Err(syntax_error("agentflow", self.source, at, "", true));
        }
        Err(syntax_error("agentflow", self.source, at, &format!("Expecting {}, got '{}'", expected, t::TOKEN_NAMES[ty as usize]), false))
    }

    fn loc(&self, a: usize) -> Loc {
        Loc { first_line: self.locs[4 * a], first_column: self.locs[4 * a + 1] }
    }

    fn expect(&mut self, ty: u8, also: &str) -> R<String> {
        if self.ty() != ty {
            let name = t::TOKEN_NAMES[ty as usize];
            return self.fail(&if also.is_empty() { format!("'{}'", name) } else { format!("'{}', {}", name, also) });
        }
        self.i += 1;
        Ok(self.texts[self.i - 1].clone())
    }

    fn separator(&mut self) -> R<()> {
        if !SEPARATOR.contains(&self.ty()) {
            return self.fail(SEPARATORS);
        }
        self.i += 1;
        Ok(())
    }

    fn run(&mut self, set: &[u8], what: &str) -> R<String> {
        if !set.contains(&self.ty()) {
            return self.fail(what);
        }
        let mut s = self.texts[self.i].clone();
        self.i += 1;
        while set.contains(&self.ty()) {
            s.push_str(&self.texts[self.i]);
            self.i += 1;
        }
        Ok(s)
    }

    fn id_string(&mut self) -> R<String> {
        self.run(ID, "'NODE_STRING'")
    }

    fn alpha_num(&mut self) -> R<String> {
        self.run(ALPHANUM, "'NODE_STRING'")
    }

    fn text(&mut self) -> R<FlowText> {
        let ty = self.ty();
        let type_ = if ty == t::STR {
            LabelType::String
        } else if ty == t::MD_STR {
            LabelType::Markdown
        } else if ty == t::TEXT || ty == t::TAGSTART || ty == t::TAGEND || ty == t::UNICODE_TEXT {
            LabelType::Text
        } else {
            return self.fail(&format!("{}, 'TAGEND', 'STR', 'MD_STR'", TEXT_TOKENS));
        };
        let mut s = self.texts[self.i].clone();
        self.i += 1;
        loop {
            let n = self.ty();
            if n != t::TEXT && n != t::TAGSTART && n != t::TAGEND && n != t::UNICODE_TEXT {
                break;
            }
            s.push_str(&self.texts[self.i]);
            self.i += 1;
        }
        Ok(FlowText { text: s, type_ })
    }

    fn closed(&mut self, ty: u8) -> R<()> {
        self.expect(ty, TEXT_TOKENS).map(|_| ())
    }

    fn edge_text(&mut self) -> R<FlowText> {
        let ty = self.ty();
        let type_ = if ty == t::STR {
            LabelType::String
        } else if ty == t::MD_STR {
            LabelType::Markdown
        } else if ty == t::EDGE_TEXT || ty == t::UNICODE_TEXT {
            LabelType::Text
        } else {
            return self.fail("'EDGE_TEXT', 'STR', 'MD_STR'");
        };
        let mut s = self.texts[self.i].clone();
        self.i += 1;
        while self.ty() == t::EDGE_TEXT || self.ty() == t::UNICODE_TEXT {
            s.push_str(&self.texts[self.i]);
            self.i += 1;
        }
        Ok(FlowText { text: s, type_ })
    }

    fn text_no_tags(&mut self) -> R<FlowText> {
        let ty = self.ty();
        let type_ = if ty == t::STR {
            LabelType::Text
        } else if ty == t::MD_STR {
            LabelType::Markdown
        } else if NO_TAGS.contains(&ty) {
            LabelType::Text
        } else {
            return self.fail("'NODE_STRING', 'STR', 'MD_STR'");
        };
        let mut s = self.texts[self.i].clone();
        self.i += 1;
        while NO_TAGS.contains(&self.ty()) {
            s.push_str(&self.texts[self.i]);
            self.i += 1;
        }
        Ok(FlowText { text: s, type_ })
    }

    fn shape_data(&mut self) -> String {
        self.data_from = self.i;
        let mut s = self.texts[self.i].clone();
        self.i += 1;
        while self.ty() == t::SHAPE_DATA {
            s.push_str(&self.texts[self.i]);
            self.i += 1;
        }
        s
    }

    fn attach(&self, db: &mut AgentflowDb, id: &str, data: &str, from: usize) -> R<()> {
        db.add_vertex(id, None, None, None, Some((data, self.loc(from))))
    }

    fn attach_data(&self, db: &mut AgentflowDb, id: &str, frame: &Frame) -> R<()> {
        match &frame.data {
            Some(d) => self.attach(db, id, d, frame.data_from),
            None => Ok(()),
        }
    }

    fn vertex(&mut self, db: &mut AgentflowDb) -> R<String> {
        let id = self.id_string()?;
        let shaped = |p: &mut Self, close: u8, shape: &'static str| -> R<(Option<FlowText>, Option<&'static str>)> {
            p.i += 1;
            let label = p.text()?;
            p.closed(close)?;
            Ok((Some(label), Some(shape)))
        };
        let (label, type_) = match self.ty() {
            t::SQS => shaped(self, t::SQE, "square")?,
            t::DOUBLECIRCLESTART => shaped(self, t::DOUBLECIRCLEEND, "doublecircle")?,
            t::PS => {
                self.i += 1;
                if self.ty() == t::PS {
                    self.i += 1;
                    let label = self.text()?;
                    self.closed(t::PE)?;
                    self.expect(t::PE, "")?;
                    (Some(label), Some("circle"))
                } else {
                    let label = self.text()?;
                    self.closed(t::PE)?;
                    (Some(label), Some("round"))
                }
            }
            t::ELLIPSE_START => shaped(self, t::ELLIPSE_END, "ellipse")?,
            t::STADIUMSTART => shaped(self, t::STADIUMEND, "stadium")?,
            t::SUBROUTINESTART => shaped(self, t::SUBROUTINEEND, "subroutine")?,
            t::VERTEX_WITH_PROPS_START => {
                self.i += 1;
                self.expect(t::NODE_STRING, "")?;
                self.expect(t::COLON, "")?;
                self.expect(t::NODE_STRING, "")?;
                self.expect(t::PIPE, "")?;
                let label = self.text()?;
                self.closed(t::SQE)?;
                (Some(label), Some("rect"))
            }
            t::CYLINDERSTART => shaped(self, t::CYLINDEREND, "cylinder")?,
            t::DIAMOND_START => {
                self.i += 1;
                if self.ty() == t::DIAMOND_START {
                    self.i += 1;
                    let label = self.text()?;
                    self.closed(t::DIAMOND_STOP)?;
                    self.expect(t::DIAMOND_STOP, "")?;
                    (Some(label), Some("hexagon"))
                } else {
                    let label = self.text()?;
                    self.closed(t::DIAMOND_STOP)?;
                    (Some(label), Some("diamond"))
                }
            }
            t::TAGEND => shaped(self, t::SQE, "odd")?,
            t::TRAPSTART => {
                self.i += 1;
                let label = self.text()?;
                if self.ty() == t::INVTRAPEND {
                    self.i += 1;
                    (Some(label), Some("lean_right"))
                } else {
                    self.closed(t::TRAPEND)?;
                    (Some(label), Some("trapezoid"))
                }
            }
            t::INVTRAPSTART => {
                self.i += 1;
                let label = self.text()?;
                if self.ty() == t::TRAPEND {
                    self.i += 1;
                    (Some(label), Some("lean_left"))
                } else {
                    self.closed(t::INVTRAPEND)?;
                    (Some(label), Some("inv_trapezoid"))
                }
            }
            _ => (None, None),
        };
        db.add_vertex(&id, label.as_ref(), type_, None, None)?;
        Ok(id)
    }

    fn styled_vertex(&mut self, db: &mut AgentflowDb) -> R<String> {
        let id = self.vertex(db)?;
        if self.ty() == t::STYLE_SEPARATOR {
            self.i += 1;
            let class = self.id_string()?;
            db.set_class(&id, &class);
        }
        Ok(id)
    }

    fn node(&mut self, db: &mut AgentflowDb) -> R<Vec<String>> {
        let mut ids = vec![self.styled_vertex(db)?];
        self.pending_data = None;
        loop {
            let data = if self.ty() == t::SHAPE_DATA { Some(self.shape_data()) } else { None };
            let from = self.data_from;
            if self.ty() != t::SPACE {
                self.pending_data = data;
                return Ok(ids);
            }
            while self.ty() == t::SPACE {
                self.i += 1;
            }
            if self.ty() != t::AMP {
                if data.is_some() {
                    return self.fail("'AMP'");
                }
                return Ok(ids);
            }
            self.i += 1;
            if self.ty() != t::SPACE {
                return self.fail("'SPACE'");
            }
            while self.ty() == t::SPACE {
                self.i += 1;
            }
            let next = self.styled_vertex(db)?;
            if let Some(d) = &data {
                self.attach(db, ids.last().unwrap(), d, from)?;
            }
            ids.push(next);
        }
    }

    fn link(&mut self, db: &mut AgentflowDb) -> R<LinkInfo> {
        let mut id: Option<String> = None;
        if self.ty() == t::LINK_ID {
            id = Some(self.texts[self.i].clone());
            self.i += 1;
        }
        if self.ty() == t::LINK {
            let mut info = db.destruct_link(&self.texts[self.i], None);
            self.i += 1;
            if id.is_some() {
                info.id = id;
            }
            if self.ty() == t::PIPE {
                self.i += 1;
                info.text = Some(self.text()?);
                self.closed(t::PIPE)?;
                if self.ty() == t::SPACE {
                    self.i += 1;
                }
            }
            return Ok(info);
        }
        if self.ty() == t::START_LINK {
            let start = self.texts[self.i].clone();
            self.i += 1;
            let label = self.edge_text()?;
            let end = self.expect(t::LINK, "'EDGE_TEXT'")?;
            let mut info = db.destruct_link(&end, Some(&start));
            info.text = Some(label);
            if id.is_some() {
                info.id = id;
            }
            return Ok(info);
        }
        self.fail("'LINK', 'START_LINK'")
    }

    fn vertex_statement(&mut self, db: &mut AgentflowDb) -> R<Vec<String>> {
        let mut stmt = self.node(db)?;
        if let Some(d) = self.pending_data.clone() {
            self.attach(db, stmt.last().unwrap(), &d, self.data_from)?;
        }
        let mut nodes = stmt.clone();
        loop {
            let ty = self.ty();
            if ty != t::LINK && ty != t::START_LINK && ty != t::LINK_ID {
                return Ok(nodes);
            }
            let info = self.link(db)?;
            let next = self.node(db)?;
            if let Some(d) = self.pending_data.clone() {
                self.attach(db, next.last().unwrap(), &d, self.data_from)?;
            }
            db.add_link(&stmt, &next, &info)?;
            let mut joined = next.clone();
            joined.extend(nodes);
            nodes = joined;
            stmt = next;
        }
    }

    fn style(&mut self) -> R<String> {
        self.run(STYLE_PART, "'NODE_STRING', 'NUM', 'COLON', 'BRKT'")
    }

    fn styles_opt(&mut self) -> R<Vec<String>> {
        let mut out = vec![self.style()?];
        while self.ty() == t::COMMA {
            self.i += 1;
            out.push(self.style()?);
        }
        Ok(out)
    }

    fn link_style_statement(&mut self, db: &mut AgentflowDb) -> R<()> {
        self.i += 1;
        self.expect(t::SPACE, "")?;
        let mut positions: Vec<String>;
        if self.ty() == t::DEFAULT {
            positions = vec![self.texts[self.i].clone()];
            self.i += 1;
        } else {
            positions = vec![self.expect(t::NUM, "'DEFAULT'")?];
            while self.ty() == t::COMMA {
                self.i += 1;
                positions.push(self.expect(t::NUM, "")?);
            }
        }
        self.expect(t::SPACE, "")?;
        if self.ty() != t::INTERPOLATE {
            let styles = self.styles_opt()?;
            return db.update_link(&positions, styles);
        }
        self.i += 1;
        self.expect(t::SPACE, "")?;
        let interpolate = self.alpha_num()?;
        if self.ty() == t::SPACE {
            self.i += 1;
            let styles = self.styles_opt()?;
            db.update_link_interpolate(&positions, &interpolate)?;
            db.update_link(&positions, styles)
        } else {
            db.update_link_interpolate(&positions, &interpolate)
        }
    }

    fn link_tail(&mut self, db: &mut AgentflowDb, id: &str, url: &str) -> R<()> {
        if self.ty() != t::SPACE {
            db.set_link(id, url, None);
            return Ok(());
        }
        self.i += 1;
        if self.ty() == t::LINK_TARGET {
            let target = self.texts[self.i].clone();
            self.i += 1;
            db.set_link(id, url, Some(&target));
            return Ok(());
        }
        let tooltip = self.expect(t::STR, "'LINK_TARGET'")?;
        if self.ty() == t::SPACE {
            self.i += 1;
            let target = self.expect(t::LINK_TARGET, "")?;
            db.set_link(id, url, Some(&target));
        } else {
            db.set_link(id, url, None);
        }
        db.set_tooltip(id, &tooltip);
        Ok(())
    }

    fn click_statement(&mut self, db: &mut AgentflowDb) -> R<()> {
        let id = self.texts[self.i].clone();
        self.i += 1;
        match self.ty() {
            t::CALLBACKNAME => {
                self.i += 1;
                if self.ty() == t::CALLBACKARGS {
                    self.i += 1;
                }
                let mut tooltip = None;
                if self.ty() == t::SPACE {
                    self.i += 1;
                    tooltip = Some(self.expect(t::STR, "")?);
                }
                db.set_click_event(&id);
                if let Some(tip) = tooltip {
                    db.set_tooltip(&id, &tip);
                }
                Ok(())
            }
            t::HREF => {
                self.i += 1;
                let url = self.expect(t::STR, "")?;
                self.link_tail(db, &id, &url)
            }
            t::STR => {
                let url = self.texts[self.i].clone();
                self.i += 1;
                self.link_tail(db, &id, &url)
            }
            _ => {
                self.alpha_num()?;
                if self.ty() == t::SPACE {
                    self.i += 1;
                    let tooltip = self.expect(t::STR, "")?;
                    db.set_click_event(&id);
                    db.set_tooltip(&id, &tooltip);
                } else {
                    db.set_click_event(&id);
                }
                Ok(())
            }
        }
    }

    // Reads `flow` or `connector` up to the separator: an id, an optional bracketed title, optional metadata.
    fn header(&mut self) -> R<Frame> {
        self.expect(t::SPACE, SEPARATORS)?;
        let id = self.text_no_tags()?;
        let mut title = FlowText { text: String::new(), type_: LabelType::Text };
        if self.ty() == t::SQS {
            self.i += 1;
            title = self.text()?;
            self.closed(t::SQE)?;
        }
        let data = if self.ty() == t::SHAPE_DATA { Some(self.shape_data()) } else { None };
        if !SEPARATOR.contains(&self.ty()) {
            return self.fail(&format!("{}, 'SHAPE_DATA'", SEPARATORS));
        }
        Ok(Frame { global: false, id: Some(id), title: Some(title), data, data_from: self.data_from, list: Vec::new(), collect: false })
    }

    fn statement(&mut self, db: &mut AgentflowDb) -> R<DocItem> {
        let ty = self.ty();
        match ty {
            t::STYLE => {
                self.i += 1;
                self.expect(t::SPACE, "")?;
                let id = self.id_string()?;
                self.expect(t::SPACE, "")?;
                let styles = self.styles_opt()?;
                db.add_vertex(&id, None, None, Some(styles), None)?;
                self.separator()?;
                Ok(DocItem::List(Vec::new()))
            }
            t::LINKSTYLE => {
                self.link_style_statement(db)?;
                self.separator()?;
                Ok(DocItem::List(Vec::new()))
            }
            t::CLASSDEF => {
                self.i += 1;
                self.expect(t::SPACE, "")?;
                let id = self.id_string()?;
                self.expect(t::SPACE, "")?;
                let styles = self.styles_opt()?;
                db.add_class(&id, &styles);
                self.separator()?;
                Ok(DocItem::List(Vec::new()))
            }
            t::CLASS => {
                self.i += 1;
                self.expect(t::SPACE, "")?;
                let ids = self.id_string()?;
                self.expect(t::SPACE, "")?;
                let class = self.id_string()?;
                db.set_class(&ids, &class);
                self.separator()?;
                Ok(DocItem::List(Vec::new()))
            }
            t::CLICK => {
                self.click_statement(db)?;
                self.separator()?;
                Ok(DocItem::List(Vec::new()))
            }
            t::connector => {
                self.i += 1;
                let frame = self.header()?;
                self.i += 1;
                let id = db.add_connector(frame.id.as_ref().unwrap(), frame.title.as_ref().unwrap());
                self.attach_data(db, &id, &frame)?;
                Ok(DocItem::Str(id))
            }
            t::direction_tb | t::direction_bt | t::direction_rl | t::direction_lr | t::direction_td => {
                self.i += 1;
                Ok(DocItem::Dir(DIRECTIONS[(ty - t::direction_tb) as usize].into()))
            }
            t::acc_title => {
                self.i += 1;
                let value = js::trim(&self.expect(t::acc_title_value, "")?).to_string();
                db.set_acc_title(&value);
                Ok(DocItem::Str(value))
            }
            t::acc_descr => {
                self.i += 1;
                let value = js::trim(&self.expect(t::acc_descr_value, "")?).to_string();
                db.set_acc_description(&value);
                Ok(DocItem::Str(value))
            }
            t::acc_descr_multiline_value => {
                let value = js::trim(&self.texts[self.i]).to_string();
                self.i += 1;
                db.set_acc_description(&value);
                Ok(DocItem::Str(value))
            }
            _ => {
                if !ID.contains(&ty) {
                    return self.fail(STATEMENT_START);
                }
                let nodes = self.vertex_statement(db)?;
                self.separator()?;
                Ok(DocItem::List(nodes))
            }
        }
    }

    // Blocks nest without recursion: each open one keeps the list and collect flag of its parent.
    fn document(&mut self, db: &mut AgentflowDb) -> R<()> {
        let mut open: Vec<Frame> = Vec::new();
        let mut list: Vec<DocItem> = Vec::new();
        let mut collect = false;
        loop {
            let ty = self.ty();
            if ty == t::END {
                break;
            }
            if ty == t::end {
                let Some(frame) = open.pop() else { break };
                self.i += 1;
                if frame.global {
                    db.add_global(&list);
                    list = frame.list;
                    collect = frame.collect;
                    continue;
                }
                let id = db.add_sub_graph(frame.id.as_ref(), &list, frame.title.as_ref());
                self.attach_data(db, &id, &frame)?;
                list = frame.list;
                collect = frame.collect;
                if collect {
                    list.push(DocItem::Str(id));
                }
            } else if ty == t::SEMI || ty == t::NEWLINE || ty == t::SPACE || ty == t::EOF {
                if collect {
                    list.push(DocItem::Str(self.texts[self.i].clone()));
                }
                self.i += 1;
            } else if ty == t::COMMENT {
                self.i += 1;
            } else if ty == t::flow || ty == t::global {
                self.i += 1;
                let mut frame;
                if ty == t::global || SEPARATOR.contains(&self.ty()) {
                    frame = Frame { global: ty == t::global, id: None, title: None, data: None, data_from: 0, list: Vec::new(), collect: false };
                    self.separator()?;
                } else {
                    frame = self.header()?;
                    self.i += 1;
                }
                frame.list = std::mem::take(&mut list);
                frame.collect = collect;
                open.push(frame);
                collect = true;
            } else {
                let item = self.statement(db)?;
                if collect && !matches!(&item, DocItem::List(l) if l.is_empty()) {
                    list.push(item);
                }
            }
        }
        if !open.is_empty() {
            return self.fail("'end'");
        }
        Ok(())
    }
}

pub fn parse_agentflow(source: &[u16], db: &mut AgentflowDb) -> R<()> {
    let (src, edits) = if source.contains(&(b'}' as u16)) { brace_trail(source) } else { (source.to_vec(), Vec::new()) };
    let tk = tokenize(&src);
    let mut p = P { source, src, edits, types: tk.types, texts: tk.texts.iter().map(|t| js::from16(t)).collect(), starts: tk.starts, locs: tk.locs, i: 0, pending_data: None, data_from: 0 };

    while p.ty() == t::SPACE || p.ty() == t::NEWLINE || p.ty() == t::COMMENT {
        p.i += 1;
    }
    p.expect(t::GRAPH, "'SPACE', 'NEWLINE', 'COMMENT'")?;
    if p.ty() == t::NODIR {
        p.i += 1;
        db.set_direction("TB");
    } else {
        let dir = p.expect(t::DIR, "'NODIR'")?;
        let mut ty = p.ty();
        if ty == t::SPACE {
            while p.ty() == t::SPACE {
                p.i += 1;
            }
            ty = p.ty();
            if ty != t::NEWLINE {
                return p.fail("'NEWLINE', 'SPACE'");
            }
        } else if ty != t::SEMI && ty != t::NEWLINE && ty != t::COMMENT {
            return p.fail("'COMMENT', 'SEMI', 'NEWLINE', 'SPACE'");
        }
        p.i += 1;
        db.set_direction(&dir);
    }
    p.document(db)?;
    if p.ty() != t::END {
        return p.fail(STATEMENT_START);
    }
    Ok(())
}
