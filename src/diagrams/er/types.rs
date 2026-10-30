// Port of src/diagrams/er/types.ts.

#[derive(Clone, Debug)]
pub struct ErAttribute {
    pub type_: String,
    pub name: String,
    pub keys: Vec<String>,
    pub comment: String,
}

/// What the parser hands over for one attribute; keys and comment are present only when written.
#[derive(Clone, Debug)]
pub struct ParsedAttribute {
    pub type_: String,
    pub name: String,
    pub keys: Option<Vec<String>>,
    pub comment: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ErEntity {
    pub id: String,
    pub label: String,
    pub alias: String,
    pub attributes: Vec<ErAttribute>,
    pub css_classes: String,
    pub css_styles: Vec<String>,
}

/// cardB sits at entityA's end of the line and cardA at entityB's, as in Mermaid.
#[derive(Clone, Copy, Debug)]
pub struct RelSpec {
    pub card_a: &'static str,
    pub identifying: bool,
    pub card_b: &'static str,
}

/// entityA and entityB hold an entity's id, or a subgraph's id when the name was a subgraph at the time.
#[derive(Clone, Debug)]
pub struct ErRelationship {
    pub entity_a: String,
    pub role_a: String,
    pub entity_b: String,
    pub rel_spec: RelSpec,
}

#[derive(Clone, Debug)]
pub struct ErClassDef {
    pub id: String,
    pub styles: Vec<String>,
    pub text_styles: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ErSubgraph {
    pub id: String,
    pub nodes: Vec<String>,
    pub title: String,
    pub classes: Vec<String>,
    pub css_styles: Vec<String>,
    pub dir: Option<String>,
    pub label_type: &'static str,
}

#[derive(Clone, Debug)]
pub enum DocItem {
    Name(String),
    Dir(&'static str),
}
