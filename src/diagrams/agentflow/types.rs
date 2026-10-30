// Port of src/diagrams/agentflow/types.ts: the parts the drawing reads. Labels, document items
// and texts are the flowchart's, which upstream's match field for field.

pub use crate::diagrams::flowchart::types::{DocItem, FlowText, LabelType};
use crate::util::yaml::Map;

// A source span as Jison reports it: lines start at 1, columns at 0.
#[derive(Clone, Copy, Debug)]
pub struct Loc {
    pub first_line: usize,
    pub first_column: usize,
}

#[derive(Clone, Debug, Default)]
pub struct AgentNode {
    pub id: String,
    pub text: Option<String>,
    pub label_type: LabelType,
    pub type_: Option<String>,
    pub styles: Vec<String>,
    pub classes: Vec<String>,
    pub link: Option<String>,
    pub link_target: Option<String>,
    pub is_connector: bool,
}

#[derive(Clone, Debug)]
pub struct AgentEdge {
    pub start: String,
    pub end: String,
    pub type_: Option<String>,
    pub text: String,
    pub label_type: LabelType,
    pub stroke: Option<String>,
    pub length: Option<f64>,
    pub style: Option<Vec<String>>,
    pub classes: Vec<String>,
    pub id: String,
    pub is_user_defined_id: bool,
    // A non-string curve from metadata is kept as "\0" and its text, so it names no curve.
    pub interpolate: Option<String>,
}

#[derive(Clone, Debug)]
pub struct AgentClassDef {
    pub styles: Vec<String>,
    pub text_styles: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct AgentSubgraph {
    pub id: String,
    pub nodes: Vec<String>,
    pub title: String,
    pub classes: Vec<String>,
    pub dir: Option<String>,
    pub label_type: LabelType,
    pub metadata: Option<Map>,
}

#[derive(Clone, Debug)]
pub struct LinkInfo {
    pub type_: String,
    pub stroke: String,
    pub length: Option<f64>,
    pub text: Option<FlowText>,
    pub id: Option<String>,
}
