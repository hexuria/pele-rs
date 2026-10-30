// The diagram types, as upstream's src/diagrams, and the part of src/types.ts's `Diagram` and
// src/diagrams/registry.ts the port uses. A type that is not in ALL is not ported yet.

pub mod class;
pub mod common;
pub mod cynefin;
pub mod er;
pub mod eventmodeling;
pub mod flowchart;
pub mod git;
pub mod info;
pub mod ishikawa;
pub mod journey;
pub mod kanban;
pub mod mindmap;
pub mod packet;
pub mod pie;
pub mod quadrant;
pub mod radar;
pub mod railroad;
pub mod requirement;
pub mod sankey;
pub mod sequence;
pub mod state;
pub mod swimlane;
pub mod timeline;
pub mod treemap;
pub mod treeview;
pub mod usecase;
pub mod venn;
pub mod wardley;
pub mod xychart;

use crate::errors::PeleError;
use crate::preprocess::Config;
use crate::types::{RenderOptions, Rendered};
use std::any::Any;

/// What a diagram type provides. `source` has been through preprocessing and entity encoding
/// and ends with a newline, as Mermaid's parsers expect.
pub struct Diagram {
    pub type_: &'static str,
    // The name of this type's section in Mermaid's config, when it is not the type's own name.
    pub section: Option<&'static str>,
    // A diagram that reports source positions is given the text with its `%%` comment lines kept.
    // `line_offset` is the number of front matter lines that were removed before that text.
    pub keep_comments: bool,
    pub parse: fn(source: &[u16], config: &Config, title: Option<&str>, line_offset: usize) -> Result<Box<dyn Any>, PeleError>,
    pub render: fn(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered, PeleError>,
}

/// Every ported diagram type.
pub static ALL: &[&Diagram] = &[
    &flowchart::DIAGRAM,
    &sequence::DIAGRAM,
    &state::DIAGRAM,
    &class::DIAGRAM,
    &er::DIAGRAM,
    &ishikawa::DIAGRAM,
    &cynefin::DIAGRAM,
    &venn::DIAGRAM,
    &swimlane::DIAGRAM,
    &eventmodeling::DIAGRAM,
    &wardley::DIAGRAM,
    &info::DIAGRAM,
    &pie::DIAGRAM,
    &radar::DIAGRAM,
    &packet::DIAGRAM,
    &mindmap::DIAGRAM,
    &kanban::DIAGRAM,
    &journey::DIAGRAM,
    &timeline::DIAGRAM,
    &quadrant::DIAGRAM,
    &xychart::DIAGRAM,
    &sankey::DIAGRAM,
    &treemap::DIAGRAM,
    &requirement::DIAGRAM,
    &railroad::DIAGRAM,
    &usecase::DIAGRAM,
    &treeview::DIAGRAM,
    &git::DIAGRAM,
];

pub fn diagram(type_: &str) -> Option<&'static Diagram> {
    ALL.iter().copied().find(|d| d.type_ == type_)
}
