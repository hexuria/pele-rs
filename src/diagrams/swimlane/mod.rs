// Port of src/diagrams/swimlane/index.ts. A swimlane diagram is a flowchart whose top-level
// subgraphs are lanes: same syntax, same model.

use crate::diagrams::flowchart::db::FlowDb;
use crate::diagrams::flowchart::graph::{FlowGraph, GraphNode};
use crate::diagrams::flowchart::render::{render_flowchart, FlowVariant};
use crate::diagrams::flowchart::types::LabelType;
use crate::diagrams::Diagram;
use crate::layout::compound::{CEdge, CNode, CompoundResult, Dir};
use crate::layout::lanes::{lane_layout, LANE_PAD};
use crate::layout::layered::LayeredOptions;
use crate::svg::builder::{label_svg, num};
use crate::svg::ink::ink_fill;
use crate::svg::theme::ResolvedStyle;
use crate::text::label::Label;

const DEFAULT_LANE: &str = "__swimlane_default__";

struct Swimlane;

impl FlowVariant for Swimlane {
    fn type_(&self) -> &'static str {
        "swimlane"
    }

    // Nodes outside every lane share one unnamed lane, as in Mermaid. Lanes all run the same way,
    // so a `direction` inside a subgraph has no effect.
    fn prepare(&self, graph: &mut FlowGraph) {
        let mut loose = false;
        for node in graph.nodes.iter_mut() {
            if node.is_group {
                node.dir = None;
            } else if node.parent_id.is_none() {
                loose = true;
            }
        }
        if !loose {
            return;
        }
        for node in graph.nodes.iter_mut() {
            if !node.is_group && node.parent_id.is_none() {
                node.parent_id = Some(DEFAULT_LANE.to_string());
            }
        }
        graph.nodes.push(GraphNode {
            id: DEFAULT_LANE.to_string(),
            label: None,
            label_type: LabelType::Text,
            shape: "rect".to_string(),
            is_group: true,
            parent_id: None,
            css_styles: Vec::new(),
            css_compiled_styles: Vec::new(),
            css_classes: String::new(),
            dir: None,
            link: None,
            link_target: None,
            tooltip: None,
            icon: None,
            pos: None,
            img: None,
            asset_width: None,
            asset_height: None,
            constraint: None,
            color_index: None,
            metadata: None,
        });
    }

    fn layout(&self, nodes: &mut [CNode], edges: &mut [CEdge], dir: Dir, opt: &LayeredOptions, tight: bool) -> CompoundResult {
        lane_layout(nodes, edges, dir, opt, if tight { 12.0 } else { LANE_PAD })
    }

    // A lane: an outline the length of the diagram, with its title in a band at the start.
    fn draw_group(&self, c: &CNode, label: &Label, style: &ResolvedStyle, classes: &str, id: &str, dir: Dir) -> Option<String> {
        let left = c.x - c.w / 2.0;
        let top = c.y - c.h / 2.0;
        let sideways = matches!(dir, Dir::LR | Dir::RL);
        let cx = if sideways { left + c.pad_top / 2.0 } else { c.x };
        let cy = if sideways { c.y } else { top + c.pad_top / 2.0 };
        let text = label_svg(label, cx, cy, &format!(" class=\"pele-cluster-label\"{}{}", ink_fill(&style.shape, &style.text, Some("var(--_m)")), style.text));
        Some(format!(
            "<g class=\"pele-cluster pele-lane{}\" data-id=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"var(--_b)\"/><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_a)\" stroke=\"var(--_b)\"{}/>{}</g>",
            classes,
            id,
            num(left),
            num(top),
            num(c.w),
            num(c.h),
            num(left),
            num(top),
            num(if sideways { c.pad_top } else { c.w }),
            num(if sideways { c.h } else { c.pad_top }),
            style.shape,
            if sideways && !text.is_empty() { format!("<g transform=\"rotate(-90 {} {})\">{}</g>", num(cx), num(cy), text) } else { text }
        ))
    }

    fn filled_box(&self, c: &CNode, dir: Dir) -> Option<[f64; 4]> {
        let left = c.x - c.w / 2.0;
        let top = c.y - c.h / 2.0;
        Some(if matches!(dir, Dir::LR | Dir::RL) { [left, top, left + c.pad_top, top + c.h] } else { [left, top, left + c.w, top + c.pad_top] })
    }
}

pub static DIAGRAM: Diagram = Diagram {
    type_: "swimlane",
    section: None,
    keep_comments: false,
    parse: |source, config, title, _line_offset| Ok(Box::new(crate::diagrams::flowchart::parse(source, config, title)?)),
    render: |model, config, options| Ok(render_flowchart(model.downcast_ref::<FlowDb>().unwrap(), config, options, Some(&Swimlane))),
};
