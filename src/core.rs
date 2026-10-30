// Port of src/core.ts: render() with the default options the host uses.

use crate::detect::detect_diagram;
use crate::diagrams;
use crate::errors::{input_offset, locating_in, Edit, Origin, PeleError, PeleErrorCode};
use crate::js;
use crate::preprocess::{cleanup_comments, encode_entities, normalize_line_breaks, preprocess};
use crate::text::entities::{decode_entities, written_codes};
use crate::types::{RenderOptions, RenderResult};
use crate::util::yaml::Value;

// Mermaid's default maxTextSize.
pub const DEFAULT_LIMIT: usize = 50000;
// A source within the limit can still describe a drawing of many megabytes.
pub const DEFAULT_OUTPUT_LIMIT: usize = 4_000_000;
// `A & B --> C & D` multiplies: without a limit, a few kilobytes of source ask for millions of edges.
pub const DEFAULT_MAX_EDGES: usize = 5000;

/// The diagram types this port renders (the types in `diagrams::ALL`). Any other type that
/// upstream renders is a `NotYetPorted` error, for the host to draw some other way.
pub const PORTED: &[&str] = &[
    "flowchart",
    "sequence",
    "state",
    "class",
    "er",
    "ishikawa",
    "cynefin",
    "venn",
    "swimlane",
    "eventmodeling",
    "wardley",
    "info",
    "pie",
    "radar",
    "packet",
    "mindmap",
    "kanban",
    "journey",
    "timeline",
    "quadrantChart",
    "xychart",
    "sankey",
    "treemap",
    "requirement",
    "railroad",
    "usecase",
    "treeView",
    "gitGraph",
    "gantt",
];

fn tag(mut error: PeleError, type_: &str, mermaid_type: &str) -> PeleError {
    if error.type_.is_none() {
        error.type_ = Some(type_.to_string());
    }
    if error.mermaid_type.is_none() {
        error.mermaid_type = Some(mermaid_type.to_string());
    }
    error.message = written_codes(&error.message);
    error.snippet = written_codes(&error.snippet);
    error
}

// Record preprocessing edits only when a source position is needed.
fn in_source(text: Vec<u16>, keep_comments: bool) -> impl Fn(usize) -> (Vec<u16>, usize) {
    move |at| {
        let mut steps: Vec<Vec<Edit>> = Vec::new();
        if let Ok(pre) = preprocess(&text, Some(&mut steps)) {
            if keep_comments {
                encode_entities(&pre.with_comments, Some(&mut steps));
            } else {
                let cleaned = cleanup_comments(&pre.with_comments, Some(&mut steps));
                encode_entities(&cleaned, Some(&mut steps));
            }
        }
        // Match the line break appended for parsing.
        let mut source = normalize_line_breaks(&text);
        source.push(10);
        (source, input_offset(&steps, at))
    }
}

/// The type a text declares, or None.
pub fn detect_type(text: &str) -> Option<&'static str> {
    let pre = preprocess(&js::u16s(text), None).ok()?;
    detect_diagram(&js::from16(&pre.text)).map(|d| d.type_)
}

pub fn render(text: &str) -> Result<RenderResult, PeleError> {
    let source = js::u16s(text);
    if source.len() > DEFAULT_LIMIT {
        return Err(PeleError::new(format!("Diagram source is longer than the limit of {} characters.", DEFAULT_LIMIT), PeleErrorCode::Limit));
    }
    let pre = preprocess(&source, None)?;
    let Some(found) = detect_diagram(&js::from16(&pre.text)) else {
        return Err(PeleError::new("No diagram type detected for the given text.", PeleErrorCode::UnsupportedDiagram));
    };
    let (type_, mermaid_type) = (found.type_, found.mermaid_type);
    let Some(diagram) = diagrams::diagram(type_) else {
        let mut e = PeleError::new(format!("Diagram type \"{}\" is not yet ported.", type_), PeleErrorCode::NotYetPorted);
        e.type_ = Some(type_.into());
        e.mermaid_type = Some(mermaid_type.into());
        return Err(e);
    };
    let config = pre.config;
    let keep = diagram.keep_comments;
    let mut parsed = encode_entities(if keep { &pre.with_comments } else { &pre.text }, None);
    parsed.push(10);
    let origin = Origin { parsed: parsed.clone(), find: Box::new(in_source(source.clone(), keep)) };
    let model = locating_in(origin, || (diagram.parse)(&parsed, &config, pre.title.as_deref(), pre.line_offset)).map_err(|e| tag(e, type_, mermaid_type))?;
    let fixed = matches!(config.get(diagram.section.unwrap_or(type_)), Some(Value::Map(m)) if matches!(m.get("useMaxWidth"), Some(Value::Bool(false))));
    let options = RenderOptions { responsive: !fixed };
    let mut rendered = (diagram.render)(model.as_ref(), &config, &options).map_err(|e| tag(e, type_, mermaid_type))?;
    if rendered.svg.len() > DEFAULT_OUTPUT_LIMIT && js::len16(&rendered.svg) > DEFAULT_OUTPUT_LIMIT {
        let mut e = PeleError::new(format!("Diagram output is longer than the limit of {} characters.", DEFAULT_OUTPUT_LIMIT), PeleErrorCode::Limit);
        e.type_ = Some(type_.into());
        e.mermaid_type = Some(mermaid_type.into());
        return Err(e);
    }
    for link in rendered.links.iter_mut() {
        link.id = decode_entities(&link.id);
    }
    Ok(RenderResult { type_, mermaid_type, svg: rendered.svg, width: rendered.width, height: rendered.height, links: rendered.links })
}

#[cfg(test)]
mod tests {
    #[test]
    fn ported_lists_the_registered_types() {
        let all: Vec<&str> = super::diagrams::ALL.iter().map(|d| d.type_).collect();
        assert_eq!(all, super::PORTED);
    }
}
