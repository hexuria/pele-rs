// Port of src/diagrams/wardley (index.ts, model.ts, builder.ts).

pub mod parser;
pub mod render;

use crate::diagrams::cynefin::collapse_indents;
use crate::diagrams::Diagram;
use crate::errors::{PeleError, PeleErrorCode};
use crate::js;
use parser::{parse_wardley, LabelAst, WardleyAst};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Forward,
    Backward,
    Bidirectional,
}

// Coordinates are percentages: x is evolution, y is visibility.
#[derive(Clone, Default)]
pub struct WardleyNode {
    pub id: String,
    pub label: String,
    pub x: f64,
    pub y: f64,
    // 'anchor', 'component', or 'pipeline-component'
    pub class_name: Option<&'static str>,
    pub label_offset_x: Option<f64>,
    pub label_offset_y: Option<f64>,
    pub in_pipeline: bool,
    pub is_pipeline_parent: bool,
    pub inertia: Option<bool>,
    pub source_strategy: Option<String>,
}

pub struct WardleyLink {
    pub source: String,
    pub target: String,
    pub dashed: bool,
    pub label: Option<String>,
    pub flow: Option<Flow>,
}

pub struct WardleyTrend {
    pub node_id: String,
    pub target_x: f64,
    pub target_y: f64,
}

pub struct WardleyPipeline {
    pub node_id: String,
    pub component_ids: Vec<String>,
}

pub struct WardleyAnnotation {
    pub number: f64,
    pub coordinates: Vec<(f64, f64)>,
    pub text: String,
}

pub struct WardleyNote {
    pub text: String,
    pub x: f64,
    pub y: f64,
}

pub struct WardleyForce {
    pub name: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Default)]
pub struct WardleyModel {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub nodes: Vec<WardleyNode>,
    pub links: Vec<WardleyLink>,
    pub trends: Vec<WardleyTrend>,
    pub pipelines: Vec<WardleyPipeline>,
    pub annotations: Vec<WardleyAnnotation>,
    pub notes: Vec<WardleyNote>,
    pub accelerators: Vec<WardleyForce>,
    pub deaccelerators: Vec<WardleyForce>,
    pub annotations_box: Option<(f64, f64)>,
    pub stages: Vec<String>,
    // Right edge of each stage, from 0 to 1, when the source gives an evolution axis.
    pub stage_boundaries: Option<Vec<f64>>,
    pub size: Option<(f64, f64)>,
}

fn semantic(message: String) -> PeleError {
    PeleError::typed(message, PeleErrorCode::Semantic, "wardley")
}

// Values up to 1 are fractions; larger ones are already percentages.
fn to_percent(value: f64, context: &str) -> Result<f64, PeleError> {
    let normalized = if value <= 1.0 { value * 100.0 } else { value };
    if normalized < 0.0 || normalized > 100.0 {
        return Err(semantic(format!("{} must be between 0-1 (decimal) or 0-100 (percentage). Received: {}", context, js::num_str(value))));
    }
    Ok(normalized)
}

fn to_coordinates(visibility: f64, evolution: f64, context: &str) -> Result<(f64, f64), PeleError> {
    let x = to_percent(evolution, &format!("{} evolution", context))?;
    let y = to_percent(visibility, &format!("{} visibility", context))?;
    Ok((x, y))
}

fn flow_from_port(port: Option<&str>) -> Option<Flow> {
    match port {
        Some("+<>") => Some(Flow::Bidirectional),
        Some("+<") => Some(Flow::Backward),
        Some("+>") => Some(Flow::Forward),
        _ => None,
    }
}

fn flow_from_arrow(arrow: Option<&str>) -> (Option<Flow>, Option<String>) {
    let Some(arrow) = arrow.filter(|a| a.starts_with('+')) else { return (None, None) };
    let end = arrow.get(2..).and_then(|rest| rest.find('\'')).map(|k| k + 2);
    let label = match end {
        Some(end) if arrow.as_bytes().get(1) == Some(&b'\'') => Some(arrow[2..end].to_string()),
        _ => None,
    };
    if arrow.contains("<>") {
        (Some(Flow::Bidirectional), label)
    } else if arrow.contains('<') {
        (Some(Flow::Backward), label)
    } else if arrow.contains('>') {
        (Some(Flow::Forward), label)
    } else {
        (None, label)
    }
}

fn offsets(label: &Option<LabelAst>) -> (Option<f64>, Option<f64>) {
    match label {
        Some(l) => (Some(if l.neg_x { -1.0 } else { 1.0 } * l.offset_x), Some(if l.neg_y { -1.0 } else { 1.0 } * l.offset_y)),
        None => (None, None),
    }
}

#[derive(Default)]
struct Builder {
    nodes: Vec<WardleyNode>,
    trends: Vec<WardleyTrend>,
    pipelines: Vec<WardleyPipeline>,
}

impl Builder {
    fn node(&mut self, id: &str) -> Option<&mut WardleyNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    // A node added again keeps its class and label offsets unless the new one sets them. `full`
    // is a node that sets inertia and strategy, even to nothing (a component, not an anchor).
    fn add_node(&mut self, node: WardleyNode, full: bool) {
        match self.nodes.iter_mut().find(|n| n.id == node.id) {
            Some(existing) => {
                let merged = WardleyNode {
                    class_name: node.class_name.or(existing.class_name),
                    label_offset_x: node.label_offset_x.or(existing.label_offset_x),
                    label_offset_y: node.label_offset_y.or(existing.label_offset_y),
                    in_pipeline: existing.in_pipeline,
                    is_pipeline_parent: existing.is_pipeline_parent,
                    inertia: if full { node.inertia } else { existing.inertia },
                    source_strategy: if full { node.source_strategy.clone() } else { existing.source_strategy.clone() },
                    ..node
                };
                *existing = merged;
            }
            None => self.nodes.push(node),
        }
    }

    // A name is a node id, or else the label of the first node that has it. Pipeline components
    // have ids like "Parent_Child" and are linked by label.
    fn resolve_node_id(&self, name: &str) -> String {
        if self.nodes.iter().any(|n| n.id == name) {
            return name.to_string();
        }
        self.nodes.iter().find(|n| n.label == name).map_or(name.to_string(), |n| n.id.clone())
    }
}

// Mermaid's populateDb: fills the builder from the syntax tree, in Mermaid's order.
fn build_wardley(ast: WardleyAst, title: Option<&str>) -> Result<WardleyModel, PeleError> {
    let mut db = Builder::default();
    let mut model = WardleyModel { size: ast.size, ..Default::default() };

    if let Some(stages) = &ast.evolution {
        model.stages = stages
            .iter()
            .map(|s| match &s.second_name {
                Some(second) if !second.is_empty() => format!("{} / {}", js::trim(&s.name), js::trim(second)),
                _ => js::trim(&s.name).to_string(),
            })
            .collect();
        model.stage_boundaries = Some(stages.iter().filter_map(|s| s.boundary).collect());
    }

    for (name, visibility, evolution) in &ast.anchors {
        let (x, y) = to_coordinates(*visibility, *evolution, &format!("Anchor \"{}\"", name))?;
        db.add_node(WardleyNode { id: name.clone(), label: name.clone(), x, y, class_name: Some("anchor"), ..Default::default() }, false);
    }

    for component in &ast.components {
        let (x, y) = to_coordinates(component.visibility, component.evolution, &format!("Component \"{}\"", component.name))?;
        let (label_offset_x, label_offset_y) = offsets(&component.label);
        db.add_node(
            WardleyNode {
                id: component.name.clone(),
                label: component.name.clone(),
                x,
                y,
                class_name: Some("component"),
                label_offset_x,
                label_offset_y,
                inertia: Some(component.inertia),
                source_strategy: component.strategy.clone(),
                ..Default::default()
            },
            true,
        );
    }

    for (text, visibility, evolution) in &ast.notes {
        let (x, y) = to_coordinates(*visibility, *evolution, &format!("Note \"{}\"", text))?;
        model.notes.push(WardleyNote { text: text.clone(), x, y });
    }

    for (parent, components) in &ast.pipelines {
        let Some(y) = db.node(parent).map(|n| n.y) else {
            return Err(semantic(format!("Pipeline \"{}\" must reference an existing component with coordinates.", parent)));
        };
        match db.pipelines.iter_mut().find(|p| p.node_id == *parent) {
            Some(p) => p.component_ids.clear(),
            None => db.pipelines.push(WardleyPipeline { node_id: parent.clone(), component_ids: Vec::new() }),
        }
        if let Some(node) = db.node(parent) {
            node.is_pipeline_parent = true;
        }
        for component in components {
            let id = format!("{}_{}", parent, component.name);
            let (label_offset_x, label_offset_y) = offsets(&component.label);
            let x = to_percent(component.evolution, &format!("Pipeline component \"{}\" evolution", component.name))?;
            db.add_node(WardleyNode { id: id.clone(), label: component.name.clone(), x, y, class_name: Some("pipeline-component"), label_offset_x, label_offset_y, ..Default::default() }, true);
            if let Some(p) = db.pipelines.iter_mut().find(|p| p.node_id == *parent) {
                p.component_ids.push(id.clone());
            }
            if let Some(node) = db.node(&id) {
                node.in_pipeline = true;
            }
        }
    }

    for link in &ast.links {
        let dashed = link.arrow.as_deref().is_some_and(|a| !a.is_empty() && (a.contains("-.->") || a.contains(".-.")));
        let (arrow_flow, arrow_label) = flow_from_arrow(link.arrow.as_deref());
        let flow = flow_from_port(link.from_port.as_deref()).or(flow_from_port(link.to_port.as_deref())).or(arrow_flow);
        model.links.push(WardleyLink { source: db.resolve_node_id(&link.from), target: db.resolve_node_id(&link.to), dashed, label: arrow_label.or(link.link_label.clone()), flow });
    }

    for (component, target) in &ast.evolves {
        if let Some(y) = db.node(component).map(|n| n.y) {
            let trend = WardleyTrend { node_id: component.clone(), target_x: to_percent(*target, &format!("Evolve target for \"{}\"", component))?, target_y: y };
            match db.trends.iter_mut().find(|t| t.node_id == *component) {
                Some(slot) => *slot = trend,
                None => db.trends.push(trend),
            }
        }
    }

    if let Some(&(x, y)) = ast.annotations.first() {
        model.annotations_box = Some(to_coordinates(x, y, "Annotations box")?);
    }

    for (number, x, y, text) in &ast.annotation {
        let at = to_coordinates(*x, *y, &format!("Annotation {}", js::num_str(*number)))?;
        model.annotations.push(WardleyAnnotation { number: *number, coordinates: vec![at], text: text.clone() });
    }

    for a in &ast.accelerators {
        let (x, y) = to_coordinates(a.x, a.y, &format!("Accelerator \"{}\"", a.name))?;
        model.accelerators.push(WardleyForce { name: a.name.clone(), x, y });
    }

    for d in &ast.deaccelerators {
        let (x, y) = to_coordinates(d.x, d.y, &format!("Deaccelerator \"{}\"", d.name))?;
        model.deaccelerators.push(WardleyForce { name: d.name.clone(), x, y });
    }

    model.nodes = db.nodes;
    model.trends = db.trends;
    model.pipelines = db.pipelines;
    model.title = ast.title.filter(|t| !t.is_empty()).or(title.map(str::to_string));
    model.acc_title = ast.acc_title.filter(|t| !t.is_empty()).map(|t| js::trim_start(&t).to_string());
    model.acc_descr = ast.acc_descr.filter(|t| !t.is_empty()).map(|t| collapse_indents(&t));
    Ok(model)
}

pub static DIAGRAM: Diagram = Diagram {
    type_: "wardley",
    section: Some("wardley-beta"),
    keep_comments: false,
    parse: |source, _config, title, _line_offset| Ok(Box::new(build_wardley(parse_wardley(source)?, title)?)),
    render: |model, config, options| Ok(render::render_wardley(model.downcast_ref::<WardleyModel>().unwrap(), config, options)),
};
