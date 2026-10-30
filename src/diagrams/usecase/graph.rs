// Port of src/diagrams/usecase/graph.ts: every actor, use case, note, JSON table and boundary as a
// node with its class styles, and every relationship and note connector as an edge with its
// markers. Ports getData from Mermaid's use case DB.

use super::json::{flatten_json_rows, JsonRow};
use super::{arrow, Relationship, UsecaseModel};
use crate::js;

pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub markdown: bool,
    // Mermaid's shape names.
    pub shape: &'static str,
    pub is_group: bool,
    pub padding: f64,
    pub css_classes: String,
    pub css_styles: Vec<String>,
    pub css_compiled_styles: Vec<String>,
    pub actor_type: Option<String>,
    pub business: bool,
    pub icon: Option<String>,
    pub stereotype: Option<String>,
    pub parent_id: Option<String>,
    pub json_rows: Option<Vec<JsonRow>>,
    pub boundary_type: Option<&'static str>,
}

pub struct GraphEdge {
    pub id: String,
    pub start: String,
    pub end: String,
    pub relationship_type: &'static str,
    pub dotted: bool,
    pub arrow_type_start: &'static str,
    pub arrow_type_end: &'static str,
    pub label: Option<String>,
    pub markdown: bool,
    pub classes: String,
    pub style: Vec<String>,
    pub css_compiled_styles: Vec<String>,
    pub animate: bool,
    pub minlen: f64,
    // The dotted line from a note to what it is about.
    pub internal: bool,
}

// Start and end markers for each arrow type, in the order of the Arrow enum.
const MARKERS: [(&str, &str); 7] =
    [("none", "arrow_point"), ("arrow_point", "none"), ("none", "none"), ("none", "arrow_circle"), ("none", "arrow_cross"), ("arrow_circle", "none"), ("arrow_cross", "none")];

fn class_list(names: &[&str]) -> String {
    names.iter().filter(|n| !n.is_empty()).copied().collect::<Vec<_>>().join(" ")
}

fn markers(r: &Relationship) -> (&'static str, &'static str) {
    if r.type_ == "generalization" {
        return ("none", "extension");
    }
    if r.type_ != "association" {
        return MARKERS[arrow::SOLID_ARROW as usize];
    }
    *MARKERS.get(r.arrow_type as usize).unwrap_or(&MARKERS[arrow::LINE_SOLID as usize])
}

pub fn build_usecase_graph(model: &UsecaseModel) -> (Vec<GraphNode>, Vec<GraphEdge>) {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    // A later class wins a property but keeps the place the property first took.
    let compiled = |class_names: &[String]| -> Vec<String> {
        let mut out: Vec<(String, String)> = Vec::new();
        for name in std::iter::once("default").chain(class_names.iter().map(|s| s.as_str())) {
            let Some(styles) = model.class_defs.get(name) else { continue };
            for raw in styles {
                let style = js::trim(raw);
                let property = js::trim(match style.find(':') {
                    None => style,
                    Some(c) => &style[..c],
                });
                if !property.is_empty() {
                    match out.iter_mut().find(|(k, _)| k == property) {
                        Some(slot) => slot.1 = style.to_string(),
                        None => out.push((property.to_string(), style.to_string())),
                    }
                }
            }
        }
        out.into_iter().map(|(_, v)| v).collect()
    };
    let classes_of = |fixed: &[&str], extra: &[String]| -> String {
        let mut all: Vec<&str> = fixed.to_vec();
        all.extend(extra.iter().map(|s| s.as_str()));
        class_list(&all)
    };

    for actor in model.actors.values() {
        let shape = match actor.type_.as_str() {
            "hollow" => "usecaseActorHollow",
            "awesome" => "usecaseActorAwesome",
            "icon" => "usecaseActorIcon",
            _ => "usecaseActor",
        };
        let type_class = format!("usecase-actor-{}", actor.type_);
        nodes.push(GraphNode {
            id: actor.id.clone(),
            label: actor.label.clone(),
            markdown: actor.markdown,
            shape,
            is_group: false,
            padding: 10.0,
            css_classes: classes_of(&["default", "usecase-actor", &type_class, if actor.business { "usecase-business" } else { "" }], &actor.classes),
            css_styles: actor.styles.clone(),
            css_compiled_styles: compiled(&actor.classes),
            actor_type: Some(actor.type_.clone()),
            business: actor.business,
            icon: actor.icon.clone().filter(|s| !s.is_empty()),
            stereotype: actor.stereotype.clone().filter(|s| !s.is_empty()),
            parent_id: actor.parent_id.clone().filter(|s| !s.is_empty()),
            json_rows: None,
            boundary_type: None,
        });
    }
    for use_case in model.use_cases.values() {
        let ellipse = use_case.shape == "ellipse";
        let shape_class = format!("usecase-{}", use_case.shape);
        nodes.push(GraphNode {
            id: use_case.id.clone(),
            label: use_case.label.clone(),
            markdown: use_case.markdown,
            shape: if !ellipse {
                "rect"
            } else if use_case.business {
                "usecaseBusiness"
            } else {
                "usecaseEllipse"
            },
            is_group: false,
            padding: if ellipse { 20.0 } else { 10.0 },
            css_classes: classes_of(&["default", "usecase-element", &shape_class, if use_case.business { "usecase-business" } else { "" }], &use_case.classes),
            css_styles: use_case.styles.clone(),
            css_compiled_styles: compiled(&use_case.classes),
            actor_type: None,
            business: use_case.business,
            icon: None,
            stereotype: use_case.stereotype.clone().filter(|s| !s.is_empty()),
            parent_id: use_case.parent_id.clone().filter(|s| !s.is_empty()),
            json_rows: None,
            boundary_type: None,
        });
    }
    for note in &model.notes {
        nodes.push(GraphNode {
            id: note.id.clone(),
            label: note.label.clone(),
            markdown: note.markdown,
            shape: "note",
            is_group: false,
            padding: 10.0,
            css_classes: "default usecase-note".into(),
            css_styles: Vec::new(),
            css_compiled_styles: compiled(&[]),
            actor_type: None,
            business: false,
            icon: None,
            stereotype: None,
            parent_id: None,
            json_rows: None,
            boundary_type: None,
        });
    }
    for json in model.json_nodes.values() {
        nodes.push(GraphNode {
            id: json.id.clone(),
            label: json.id.clone(),
            markdown: false,
            shape: "usecaseJsonTable",
            is_group: false,
            padding: 10.0,
            css_classes: classes_of(&["default", "usecase-json-table"], &json.classes),
            css_styles: json.styles.clone(),
            css_compiled_styles: compiled(&json.classes),
            actor_type: None,
            business: false,
            icon: None,
            stereotype: None,
            parent_id: None,
            json_rows: Some(flatten_json_rows(&json.value)),
            boundary_type: None,
        });
    }
    for boundary in model.boundaries.values() {
        let type_class = format!("system-boundary-{}", boundary.type_);
        nodes.push(GraphNode {
            id: boundary.id.clone(),
            label: boundary.label.clone(),
            markdown: boundary.markdown,
            shape: "usecaseSystemBoundary",
            is_group: true,
            padding: 20.0,
            css_classes: classes_of(&["default", "system-boundary", &type_class], &boundary.classes),
            css_styles: boundary.styles.clone(),
            css_compiled_styles: compiled(&boundary.classes),
            actor_type: None,
            business: false,
            icon: None,
            stereotype: None,
            parent_id: None,
            json_rows: None,
            boundary_type: Some(boundary.type_),
        });
    }

    for r in &model.relationships {
        let semantic = r.type_ == "include" || r.type_ == "extend";
        let (start, end) = markers(r);
        let animated = r.animate || r.animation.is_some();
        let type_class = format!("relationship-{}", r.type_);
        let animation_class = format!("edge-animation-{}", r.animation.as_deref().unwrap_or("fast"));
        let mut all: Vec<&str> = vec!["default", "relationship", &type_class];
        all.extend(r.classes.iter().map(|s| s.as_str()));
        all.push(if animated { &animation_class } else { "" });
        let mut edge = GraphEdge {
            id: r.id.clone(),
            start: r.source.clone(),
            end: r.target.clone(),
            relationship_type: r.type_,
            dotted: semantic,
            arrow_type_start: start,
            arrow_type_end: end,
            label: None,
            markdown: false,
            classes: class_list(&all),
            style: r.styles.clone(),
            css_compiled_styles: compiled(&r.classes),
            animate: r.animate,
            minlen: r.minlen,
            internal: false,
        };
        // Include and extend are labelled with their kind, whatever label the relationship carries.
        if semantic {
            edge.label = Some(r.type_.to_string());
        } else if r.type_ == "association" {
            edge.label = r.label.clone().filter(|l| !l.is_empty());
            edge.markdown = r.markdown == Some(true);
        }
        edges.push(edge);
    }
    for note in &model.notes {
        edges.push(GraphEdge {
            id: format!("{}-edge", note.id),
            start: note.id.clone(),
            end: note.target.clone(),
            relationship_type: "note",
            dotted: true,
            arrow_type_start: "none",
            arrow_type_end: "none",
            label: None,
            markdown: false,
            classes: "default relationship relationship-note".into(),
            style: Vec::new(),
            css_compiled_styles: compiled(&[]),
            animate: false,
            minlen: 1.0,
            internal: true,
        });
    }
    (nodes, edges)
}
