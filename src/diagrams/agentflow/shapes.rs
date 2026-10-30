// Port of src/diagrams/agentflow/shapes.ts. The diagnostics it reports are not kept: nothing
// drawn depends on them.

use crate::diagrams::flowchart::shapes::is_valid_shape as is_flowchart_shape;

// Author-facing names for the shapes that carry meaning in an agentflow.
const SHAPE_ALIASES: &[(&str, &str)] =
    &[("task", "roundedRect"), ("tool", "subroutine"), ("input", "lean-right"), ("decision", "diamond"), ("refdoc", "lin-doc"), ("action", "hexagon"), ("round", "rect")];

const REMOVED_SHAPES: &[&str] = &[
    "doc",
    "stadium",
    "terminal",
    "circle",
    "trapezoid",
    "inv_trapezoid",
    "inv-trapezoid",
    "doublecircle",
    "double-circle",
    "typeDeclaration",
    "procs",
    "lean_left",
    "lean-left",
    "in-out",
    "cylinder",
    "ellipse",
    "odd",
    "tag-rect",
    "tagged-rectangle",
    "delay",
    "half-rounded-rectangle",
    "lin-rect",
    "lined-rectangle",
    "win-pane",
    "window-pane",
    "curv-trap",
    "curved-trapezoid",
];

const ALLOWED_SHAPES: &[&str] =
    &["roundedRect", "subroutine", "subprocess", "subproc", "framed-rectangle", "lean-right", "diamond", "lin-doc", "lined-document", "hexagon", "hex", "connector", "collapsedGroup"];

pub const DEFAULT_SHAPE: &str = "roundedRect";

pub fn resolve_shape_alias(shape: &str) -> &str {
    SHAPE_ALIASES.iter().find(|(k, _)| *k == shape).map_or(shape, |(_, v)| v)
}

// Mermaid looks a shape name up with `in` on a plain object, so `constructor` passes there too.
pub fn is_valid_shape(name: &str) -> bool {
    is_flowchart_shape(name) || name == "constructor"
}

/// Reduces a node's shape to one agentflow draws.
pub fn normalise_shape(shape: &str) -> &str {
    let shape = if shape.is_empty() || shape == "squareRect" || shape == "rect" { DEFAULT_SHAPE } else { shape };
    if REMOVED_SHAPES.contains(&shape) || !ALLOWED_SHAPES.contains(&shape) {
        DEFAULT_SHAPE
    } else {
        shape
    }
}
