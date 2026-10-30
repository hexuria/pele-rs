// Port of src/diagrams/sequence/db.ts. Maps keep insertion order, as upstream's do.

use super::colors::is_css_color;
use super::types::{linetype, MessageText, SeqActor, SeqBox, SeqBoxData, SeqItem, SeqMessage, SeqNumbering, SeqStatement, SeqText};
use crate::errors::{PeleError, PeleErrorCode};
use crate::js;
use crate::util::json::parse_json;
use crate::util::yaml::{parse_yaml, Value};
use std::collections::HashMap;

#[derive(Default)]
pub struct SeqDbOptions {
    /// The top-level `wrap` setting, which the `%%{wrap}%%` directive also sets.
    pub wrap: Option<bool>,
    /// `sequence.wrap`.
    pub sequence_wrap: bool,
}

fn fail(message: String) -> PeleError {
    PeleError::typed(message, PeleErrorCode::Semantic, "sequence")
}

fn is_line_terminator(c: char) -> bool {
    matches!(c, '\r' | '\u{2028}' | '\u{2029}')
}

fn extract_wrap(raw: &str) -> SeqText {
    let text = js::trim(raw);
    let body = text.strip_prefix(':').unwrap_or(text);
    let wrap = if body.starts_with("wrap:") {
        Some(true)
    } else if body.starts_with("nowrap:") {
        Some(false)
    } else {
        None
    };
    let out = match wrap {
        None => text.to_string(),
        Some(true) => body["wrap:".len()..].to_string(),
        Some(false) => body["nowrap:".len()..].to_string(),
    };
    SeqText { text: js::trim(&out).to_string(), wrap }
}

// Splits a box line into a leading color and the rest, as Mermaid's
// /^((?:rgba?|hsla?)\s*\(.*\)|\w*)(.*)$/ does. `.` stops at a line terminator, so a line that
// holds one outside the gap before the parenthesis does not match at all.
fn split_box(s: &str) -> Option<(String, String)> {
    // RE_BOX_FUNCTION /^(?:rgba?|hsla?)\s*\(/
    let fn_len = ["rgba", "rgb", "hsla", "hsl"].iter().find_map(|name| {
        let rest = s.strip_prefix(name)?;
        let trimmed = rest.trim_start_matches(|c: char| js::is_space(c));
        trimmed.strip_prefix('(').map(|_| s.len() - trimmed.len() + 1)
    });
    if let Some(len) = fn_len {
        if let Some(close) = s.rfind(')') {
            if close >= len {
                if s[len..].chars().any(is_line_terminator) {
                    return None;
                }
                return Some((s[..close + 1].to_string(), s[close + 1..].to_string()));
            }
        }
    }
    if s.chars().any(is_line_terminator) {
        return None;
    }
    let word_len = s.bytes().take_while(|b| b.is_ascii_alphanumeric() || *b == b'_').count();
    Some((s[..word_len].to_string(), s[word_len..].to_string()))
}

pub struct SeqDb {
    /// Map<string, SeqActor>, in insertion order.
    pub actors: Vec<(String, SeqActor)>,
    actor_index: HashMap<String, usize>,
    pub created_actors: HashMap<String, usize>,
    pub destroyed_actors: HashMap<String, usize>,
    pub boxes: Vec<SeqBox>,
    pub messages: Vec<SeqMessage>,
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    prev_actor: Option<String>,
    current_box: Option<usize>,
    last_created: Option<String>,
    last_destroyed: Option<String>,
    wrap_enabled: Option<bool>,
    sequence_wrap: bool,
    active: HashMap<String, i64>,
}

impl SeqDb {
    pub fn new(options: SeqDbOptions) -> Self {
        SeqDb {
            actors: Vec::new(),
            actor_index: HashMap::new(),
            created_actors: HashMap::new(),
            destroyed_actors: HashMap::new(),
            boxes: Vec::new(),
            messages: Vec::new(),
            title: None,
            acc_title: None,
            acc_descr: None,
            prev_actor: None,
            current_box: None,
            last_created: None,
            last_destroyed: None,
            wrap_enabled: options.wrap,
            sequence_wrap: options.sequence_wrap,
            active: HashMap::new(),
        }
    }

    pub fn actor(&self, id: &str) -> Option<&SeqActor> {
        self.actor_index.get(id).map(|&i| &self.actors[i].1)
    }

    fn auto_wrap(&self) -> bool {
        self.wrap_enabled.unwrap_or(self.sequence_wrap)
    }

    pub fn set_acc_title(&mut self, text: &str) {
        self.acc_title = Some(js::trim_start(text).to_string());
    }

    pub fn set_acc_description(&mut self, text: &str) {
        // text.replace(/\n\s+/g, '\n')
        let c: Vec<char> = text.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < c.len() {
            if c[i] == '\n' && i + 1 < c.len() && js::is_space(c[i + 1]) {
                out.push('\n');
                i += 1;
                while i < c.len() && js::is_space(c[i]) {
                    i += 1;
                }
                continue;
            }
            out.push(c[i]);
            i += 1;
        }
        self.acc_descr = Some(out);
    }

    pub fn set_diagram_title(&mut self, text: &str) {
        self.title = Some(text.to_string());
    }

    pub fn parse_message(&self, s: &str) -> SeqText {
        extract_wrap(s)
    }

    pub fn parse_box_data(&self, s: &str) -> SeqBoxData {
        let m = split_box(s);
        let mut color = match &m {
            Some((c, _)) if !c.is_empty() => js::trim(c).to_string(),
            _ => "transparent".to_string(),
        };
        let mut title = match &m {
            Some((_, t)) if !t.is_empty() => Some(js::trim(t).to_string()),
            _ => None,
        };
        if !is_css_color(&color) {
            color = "transparent".into();
            title = Some(js::trim(s).to_string());
        }
        let Some(title) = title else { return SeqBoxData { text: None, color, wrap: None } };
        let SeqText { text, wrap } = extract_wrap(&title);
        SeqBoxData { text: if text.is_empty() { None } else { Some(text) }, color, wrap }
    }

    fn add_box(&mut self, data: &SeqBoxData) {
        self.boxes.push(SeqBox { name: data.text.clone(), wrap: data.wrap.unwrap_or(self.auto_wrap()), fill: data.color.clone(), actor_keys: Vec::new() });
        self.current_box = Some(self.boxes.len() - 1);
    }

    fn add_actor(&mut self, id: &str, name: &str, description: Option<&SeqText>, type_: Option<&str>, metadata: Option<&str>) -> Result<(), PeleError> {
        let mut assigned_box = self.current_box;
        let mut type_: Option<String> = type_.map(str::to_string);
        let mut description: Option<SeqText> = description.cloned();
        let mut doc: Option<crate::util::yaml::Map> = None;
        if let Some(metadata) = metadata {
            let src = if metadata.contains('\n') { format!("{}\n", metadata) } else { format!("{{\n{}\n}}", metadata) };
            match parse_yaml(&src) {
                Ok(Value::Map(m)) => doc = Some(m),
                Ok(_) => {}
                Err(message) => return Err(PeleError::typed(format!("Config of participant \"{}\" is not valid YAML. {}", id, message), PeleErrorCode::Syntax, "sequence")),
            }
        }
        let doc_type = doc.as_ref().and_then(|d| d.get("type")).cloned();
        let doc_alias = doc.as_ref().and_then(|d| d.get("alias")).cloned();
        if let Some(t) = doc_type.filter(|t| !matches!(t, Value::Null)) {
            type_ = Some(t.js_string());
        }
        if let Some(alias) = doc_alias.filter(|a| a.truthy()) {
            if description.as_ref().is_none_or(|d| d.text == name) {
                description = Some(SeqText { text: alias.js_string(), wrap: description.as_ref().and_then(|d| d.wrap) });
            }
        }

        if let Some(&k) = self.actor_index.get(id) {
            let old_box = self.actors[k].1.box_;
            if let (Some(cur), Some(ob)) = (self.current_box, old_box) {
                if cur != ob {
                    let old = &self.actors[k].1;
                    return Err(fail(format!(
                        "A same participant should only be defined in one Box: {} can't be in '{}' and in '{}' at the same time.",
                        old.name,
                        self.boxes[ob].name.clone().unwrap_or_else(|| "undefined".into()),
                        self.boxes[cur].name.clone().unwrap_or_else(|| "undefined".into())
                    )));
                }
            }
            assigned_box = if old_box.is_some() { old_box } else { self.current_box };
            self.actors[k].1.box_ = assigned_box;
            if name == self.actors[k].1.name && description.is_none() {
                return Ok(());
            }
        }
        // A participant that is only mentioned in a message has no kind and takes its name as its label.
        if description.is_none() || type_.is_none() {
            description = Some(SeqText { text: name.to_string(), wrap: None });
        }
        let description = description.unwrap();
        let actor = SeqActor {
            box_: assigned_box,
            name: name.to_string(),
            description: description.text,
            wrap: description.wrap.unwrap_or(self.auto_wrap()),
            prev_actor: self.prev_actor.clone(),
            next_actor: None,
            class: None,
            details: None,
            type_: type_.unwrap_or_else(|| "participant".into()),
        };
        match self.actor_index.get(id) {
            Some(&k) => self.actors[k].1 = actor,
            None => {
                self.actors.push((id.to_string(), actor));
                self.actor_index.insert(id.to_string(), self.actors.len() - 1);
            }
        }
        if let Some(prev) = self.prev_actor.clone() {
            if let Some(&k) = self.actor_index.get(&prev) {
                self.actors[k].1.next_actor = Some(id.to_string());
            }
        }
        if let Some(b) = self.current_box {
            self.boxes[b].actor_keys.push(id.to_string());
        }
        self.prev_actor = Some(id.to_string());
        Ok(())
    }

    fn add_signal(&mut self, from: Option<&str>, to: Option<&str>, message: Option<&SeqText>, type_: Option<u32>, activate: bool, central_connection: Option<u32>) -> Result<(), PeleError> {
        // Mermaid recounts every earlier message here. A running count per participant says the same.
        if type_ == Some(linetype::ACTIVE_END) {
            let count = from.map_or(0, |f| self.active.get(f).copied().unwrap_or(0));
            if count < 1 {
                return Err(fail(format!("Trying to inactivate an inactive participant ({})", from.unwrap_or("undefined"))));
            }
            self.active.insert(from.unwrap().to_string(), count - 1);
        } else if type_ == Some(linetype::ACTIVE_START) {
            if let Some(f) = from.filter(|f| !f.is_empty()) {
                *self.active.entry(f.to_string()).or_insert(0) += 1;
            }
        }
        self.messages.push(SeqMessage {
            id: self.messages.len().to_string(),
            from: from.map(str::to_string),
            to: to.map(str::to_string),
            message: MessageText::Text(message.map(|m| m.text.clone()).unwrap_or_default()),
            wrap: message.and_then(|m| m.wrap).unwrap_or(self.auto_wrap()),
            type_,
            activate,
            placement: None,
            central_connection: central_connection.unwrap_or(0),
        });
        Ok(())
    }

    fn add_note(&mut self, actor: (String, String), placement: u32, message: &SeqText) {
        let wrap = message.wrap.unwrap_or(self.auto_wrap());
        self.messages.push(SeqMessage {
            id: self.messages.len().to_string(),
            from: Some(actor.0),
            to: Some(actor.1),
            message: MessageText::Text(message.text.clone()),
            wrap,
            type_: Some(linetype::NOTE),
            activate: false,
            placement: Some(placement),
            central_connection: 0,
        });
    }

    // Mermaid logs and ignores menu text that is not valid JSON. Only the `class` property is drawn.
    fn add_properties(&mut self, actor_id: &str, text: &SeqText) {
        let Some(&k) = self.actor_index.get(actor_id) else { return };
        let Ok(parsed) = parse_json(&text.text) else { return };
        // entries(): a string's characters, an object's (or array's) own entries.
        if let Value::Map(m) = &parsed {
            if let Some(v) = m.get("class") {
                self.actors[k].1.class = Some(v.clone());
            }
        }
    }

    fn add_details(&mut self, actor_id: &str, text: &SeqText) {
        if let Some(&k) = self.actor_index.get(actor_id) {
            self.actors[k].1.details = Some(text.text.clone());
        }
    }

    /// Mermaid walks the nested statement lists by recursion; this does too, depth first.
    pub fn apply(&mut self, item: &SeqItem) -> Result<(), PeleError> {
        match item {
            SeqItem::List(list) => {
                for i in list {
                    self.apply(i)?;
                }
                Ok(())
            }
            SeqItem::Text(_) => Ok(()),
            SeqItem::Stmt(s) => self.apply_one(s),
        }
    }

    fn apply_one(&mut self, param: &SeqStatement) -> Result<(), PeleError> {
        let actor = param.actor.clone().unwrap_or_default();
        match param.type_.as_str() {
            "sequenceIndex" => {
                self.messages.push(SeqMessage {
                    id: self.messages.len().to_string(),
                    from: None,
                    to: None,
                    message: MessageText::Numbering(SeqNumbering { start: param.sequence_index, step: param.sequence_index_step, visible: param.sequence_visible }),
                    wrap: false,
                    type_: param.signal_type,
                    activate: false,
                    placement: None,
                    central_connection: 0,
                });
            }
            "addParticipant" => self.add_actor(&actor, &actor, param.description.as_ref(), param.draw.as_deref(), param.config.as_deref())?,
            "createParticipant" => {
                if self.actor_index.contains_key(&actor) {
                    return Err(fail("It is not possible to have actors with the same id, even if one is destroyed before the next is created. Use 'AS' aliases to simulate the behavior".into()));
                }
                self.last_created = Some(actor.clone());
                self.add_actor(&actor, &actor, param.description.as_ref(), param.draw.as_deref(), param.config.as_deref())?;
                self.created_actors.insert(actor, self.messages.len());
            }
            "destroyParticipant" => {
                self.last_destroyed = Some(actor.clone());
                self.destroyed_actors.insert(actor, self.messages.len());
            }
            "activeStart" | "activeEnd" | "centralConnection" | "centralConnectionReverse" => self.add_signal(Some(&actor), None, None, param.signal_type, false, None)?,
            "addNote" => {
                let pair = match &param.actors {
                    Some([a, b]) => (a.clone(), b.clone()),
                    None => (actor.clone(), actor.clone()),
                };
                self.add_note(pair, param.placement.unwrap_or(0), param.text.as_ref().unwrap());
            }
            "addLinks" | "addALink" => {}
            "addProperties" => self.add_properties(&actor, param.text.as_ref().unwrap()),
            "addDetails" => self.add_details(&actor, param.text.as_ref().unwrap()),
            "addMessage" => {
                if let Some(created) = self.last_created.clone() {
                    if param.to.as_deref() != Some(created.as_str()) {
                        return Err(fail(format!("The created participant {} does not have an associated creating message after its declaration. Please check the sequence diagram.", created)));
                    }
                    self.last_created = None;
                } else if let Some(destroyed) = self.last_destroyed.clone() {
                    if param.to.as_deref() != Some(destroyed.as_str()) && param.from.as_deref() != Some(destroyed.as_str()) {
                        return Err(fail(format!("The destroyed participant {} does not have an associated destroying message after its declaration. Please check the sequence diagram.", destroyed)));
                    }
                    self.last_destroyed = None;
                }
                self.add_signal(param.from.as_deref(), param.to.as_deref(), param.msg.as_ref(), param.signal_type, param.activate, param.central_connection)?;
            }
            "boxStart" => self.add_box(param.box_data.as_ref().unwrap()),
            "boxEnd" => self.current_box = None,
            "loopStart" | "rectStart" | "optStart" | "altStart" | "else" | "parStart" | "and" | "criticalStart" | "option" | "breakStart" => {
                self.add_signal(None, None, param.text.as_ref(), param.signal_type, false, None)?
            }
            "loopEnd" | "rectEnd" | "optEnd" | "altEnd" | "parEnd" | "criticalEnd" | "breakEnd" => self.add_signal(None, None, None, param.signal_type, false, None)?,
            _ => {}
        }
        Ok(())
    }
}
