// Port of src/diagrams/state/types.ts.

#[derive(Clone, Debug)]
pub struct StateNote {
    pub position: Option<String>,
    pub text: String,
}

/// `string | string[]`.
#[derive(Clone, Debug, PartialEq)]
pub enum Descr {
    One(String),
    Many(Vec<String>),
}

#[derive(Clone, Debug, Default)]
pub struct StateStmt {
    pub id: String,
    pub type_: Option<String>,
    pub description: Option<Descr>,
    pub doc: Option<Vec<Stmt>>,
    pub note: Option<StateNote>,
    pub start: Option<bool>,
    pub classes: Option<Vec<String>>,
}

// Mermaid's grammar also leaves the text of statements that have no effect in a document
// (`hide empty description`, `scale`, a bare `state name`, a floating note); those are `Text`.
#[derive(Clone, Debug)]
pub enum Stmt {
    Text(String),
    State(StateStmt),
    Relation { state1: StateStmt, state2: StateStmt, description: Option<String> },
    ClassDef { id: String, classes: String },
    Style { id: String, style_class: String },
    ApplyClass { id: String, style_class: String },
    Dir(String),
    Click { id: StateStmt, url: String, tooltip: String },
}

#[derive(Clone, Debug)]
pub struct StateRelation {
    pub id1: String,
    pub id2: String,
    pub relation_title: String,
}

#[derive(Clone, Debug)]
pub struct StateClassDef {
    pub id: String,
    pub styles: Vec<String>,
    pub text_styles: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct StateLink {
    pub url: String,
    pub tooltip: String,
}
