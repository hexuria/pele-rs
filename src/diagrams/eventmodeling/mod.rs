// Port of src/diagrams/eventmodeling (index.ts, model.ts).

pub mod parser;
pub mod render;

use crate::diagrams::common::tokens::{acc_descr_value, acc_title_value, title_value};
use crate::diagrams::Diagram;
use crate::errors::{locate, PeleError, PeleErrorCode};
use crate::js;
use parser::{parse_event_model, EventModelAst};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EntityKind {
    Ui,
    Processor,
    Command,
    ReadModel,
    Event,
}

impl EntityKind {
    pub fn name(self) -> &'static str {
        match self {
            EntityKind::Ui => "ui",
            EntityKind::Processor => "processor",
            EntityKind::Command => "command",
            EntityKind::ReadModel => "readmodel",
            EntityKind::Event => "event",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LaneGroup {
    Trigger,
    State,
    Stream,
}

pub struct EmLane {
    // Mermaid's ordering key: 0 for UI/Automation, 100 for Command/Read Model, 200 for Events,
    // and the next free number after those for each namespace.
    pub index: f64,
    pub label: String,
}

pub struct EmBox {
    // The frame number as written, such as "01".
    pub id: String,
    pub kind: EntityKind,
    pub name: String,
    pub reset: bool,
    // Index into the model's lanes.
    pub lane: usize,
    // Example data from the frame or from the data block it refers to.
    pub data: Option<String>,
}

// Indexes into the model's boxes.
pub struct EmRelation {
    pub source: usize,
    pub target: usize,
}

pub struct EmNote {
    pub frame: String,
    pub text: String,
}

pub struct EmSpecification {
    pub frame: String,
    pub given: Vec<String>,
    pub when: Vec<String>,
    pub then: Vec<String>,
}

pub struct EventModelingModel {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub boxes: Vec<EmBox>,
    // Sorted from top to bottom.
    pub lanes: Vec<EmLane>,
    pub relations: Vec<EmRelation>,
    pub notes: Vec<EmNote>,
    pub specifications: Vec<EmSpecification>,
}

fn kind_of(name: &str) -> EntityKind {
    match name {
        "ui" => EntityKind::Ui,
        "pcr" | "processor" => EntityKind::Processor,
        "rmo" | "readmodel" => EntityKind::ReadModel,
        "cmd" | "command" => EntityKind::Command,
        _ => EntityKind::Event,
    }
}

// [first index, label, label prefix for a namespace]
fn group_info(group: LaneGroup) -> (f64, &'static str, &'static str) {
    match group {
        LaneGroup::Trigger => (0.0, "UI/Automation", "UI/A: "),
        LaneGroup::State => (100.0, "Command/Read Model", "C/RM: "),
        LaneGroup::Stream => (200.0, "Events", "Stream: "),
    }
}

fn group_of(kind: EntityKind) -> LaneGroup {
    match kind {
        EntityKind::Ui | EntityKind::Processor => LaneGroup::Trigger,
        EntityKind::Event => LaneGroup::Stream,
        _ => LaneGroup::State,
    }
}

// The text between the outer braces, without the indentation its lines share.
fn block_text(raw: &str) -> String {
    let open = raw.find('{');
    let close = raw.rfind('}');
    let inner: String = match (open, close) {
        (Some(o), Some(c)) if c > o => raw[o + 1..c].to_string(),
        (None, _) if js::len16(raw) >= 2 => {
            let mut chars = raw.chars();
            chars.next();
            chars.next_back();
            chars.as_str().to_string()
        }
        _ => String::new(),
    };
    let inner = inner.replace('\r', "");
    let mut lines: Vec<&str> = inner.split('\n').collect();
    while !lines.is_empty() && js::trim(lines[0]).is_empty() {
        lines.remove(0);
    }
    while !lines.is_empty() && js::trim(lines[lines.len() - 1]).is_empty() {
        lines.pop();
    }
    let mut indent = usize::MAX;
    for line in &lines {
        if js::trim(line).is_empty() {
            continue;
        }
        indent = indent.min(line.chars().count() - js::trim_start(line).chars().count());
    }
    lines
        .iter()
        .map(|line| {
            let n = line.chars().count();
            let rest: String = line.chars().skip(indent.min(n)).collect();
            js::trim_end(&rest).to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// `.replace(/\n\s+/g, '\n')`
pub fn collapse_indents(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '\n' {
            while chars.peek().is_some_and(|&n| js::is_space(n)) {
                chars.next();
            }
        }
    }
    out
}

fn build_event_model(src: &[u16], ast: EventModelAst, title: Option<&str>) -> Result<EventModelingModel, PeleError> {
    let mut ids: Vec<&str> = Vec::new();
    for frame in &ast.frames {
        if ids.contains(&frame.name.as_str()) {
            let line = locate(src, frame.offset).line;
            let mut e = PeleError::typed(format!("Duplicate event modeling frame ID \"{}\" on line {}", frame.name, line), PeleErrorCode::Semantic, "eventmodeling");
            e.line = line;
            return Err(e);
        }
        ids.push(&frame.name);
    }

    let mut data: Vec<(&str, &str)> = Vec::new();
    for entity in &ast.data_entities {
        if !data.iter().any(|(k, _)| *k == entity.name) {
            data.push((&entity.name, &entity.data_block_value));
        }
    }

    // One lane per group, and one more per namespace within a group.
    let mut lanes: Vec<EmLane> = Vec::new();
    let mut lane_keys: Vec<String> = Vec::new();
    let mut next: Vec<(LaneGroup, f64)> = Vec::new();
    let mut lane = |kind: EntityKind, namespace: Option<&str>| -> usize {
        let group = group_of(kind);
        let gname = match group {
            LaneGroup::Trigger => "trigger",
            LaneGroup::State => "state",
            LaneGroup::Stream => "stream",
        };
        let key = match namespace {
            None => gname.to_string(),
            Some(ns) => format!("{} {}", gname, ns),
        };
        if let Some(k) = lane_keys.iter().position(|k| *k == key) {
            return k;
        }
        let (first, label, prefix) = group_info(group);
        let mut index = first;
        if namespace.is_some() {
            index = next.iter().find(|(g, _)| *g == group).map_or(first + 1.0, |(_, v)| *v);
            match next.iter_mut().find(|(g, _)| *g == group) {
                Some(slot) => slot.1 = index + 1.0,
                None => next.push((group, index + 1.0)),
            }
        }
        lanes.push(EmLane { index, label: namespace.map_or(label.to_string(), |ns| format!("{}{}", prefix, ns)) });
        lane_keys.push(key);
        lanes.len() - 1
    };

    let mut boxes: Vec<EmBox> = Vec::new();
    let mut relations: Vec<EmRelation> = Vec::new();
    for (index, frame) in ast.frames.iter().enumerate() {
        // Only a two-part name has a namespace.
        let parts: Vec<&str> = frame.entity_identifier.split('.').collect();
        let namespace = if parts.len() == 2 { Some(parts[0]) } else { None };
        let kind = kind_of(&frame.model_entity_type);
        let mut text = frame.data_inline_value.as_deref().map(block_text);
        if let Some(reference) = &frame.data_reference {
            if let Some((_, block)) = data.iter().find(|(k, _)| k == reference) {
                text = Some(block_text(block));
            }
        }
        let b = EmBox {
            id: frame.name.clone(),
            kind,
            name: if parts.len() == 2 { parts[1].to_string() } else { frame.entity_identifier.clone() },
            reset: frame.reset,
            lane: lane(kind, namespace),
            data: text,
        };
        let me = boxes.len();
        let (blane, breset) = (b.lane, b.reset);
        boxes.push(b);

        if !frame.source_frames.is_empty() {
            // Sources connect in frame order, and only frames that already have a box.
            for (s, source) in boxes.iter().enumerate() {
                if frame.source_frames.contains(&source.id) {
                    relations.push(EmRelation { source: s, target: me });
                }
            }
        } else if index > 0 && !breset {
            // Otherwise the nearest earlier frame in another lane feeds this one.
            for i in (0..index).rev() {
                if boxes[i].lane != blane {
                    relations.push(EmRelation { source: i, target: me });
                    break;
                }
            }
        }
    }

    // Lanes sorted from top to bottom; boxes keep pointing at their lanes.
    let mut order: Vec<usize> = (0..lanes.len()).collect();
    order.sort_by(|&a, &b| js::ord(lanes[a].index - lanes[b].index));
    let mut new_index = vec![0; lanes.len()];
    for (k, &old) in order.iter().enumerate() {
        new_index[old] = k;
    }
    for b in boxes.iter_mut() {
        b.lane = new_index[b.lane];
    }
    let mut slots: Vec<Option<EmLane>> = lanes.into_iter().map(Some).collect();
    let lanes: Vec<EmLane> = order.iter().map(|&k| slots[k].take().unwrap()).collect();

    Ok(EventModelingModel {
        title: ast.title.as_deref().map(title_value).filter(|t| !t.is_empty()).or(title.map(str::to_string)),
        acc_title: ast.acc_title.as_deref().map(|t| js::trim_start(&acc_title_value(t)).to_string()),
        acc_descr: ast.acc_descr.as_deref().map(|t| collapse_indents(&acc_descr_value(t))),
        boxes,
        lanes,
        relations,
        notes: ast.note_entities.iter().map(|note| EmNote { frame: note.source_frame.clone(), text: block_text(&note.data_block_value) }).collect(),
        specifications: ast.gwt_entities.iter().map(|g| EmSpecification { frame: g.source_frame.clone(), given: g.given.clone(), when: g.when.clone(), then: g.then.clone() }).collect(),
    })
}

pub static DIAGRAM: Diagram = Diagram {
    type_: "eventmodeling",
    section: None,
    keep_comments: true,
    parse: |source, _config, title, _line_offset| Ok(Box::new(build_event_model(source, parse_event_model(source)?, title)?)),
    render: |model, config, options| Ok(render::render_event_model(model.downcast_ref::<EventModelingModel>().unwrap(), config, options)),
};
