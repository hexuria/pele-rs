// Port of src/diagrams/c4/layout.ts. With no `maxWidth` there is no room to fit (`room` is
// undefined upstream), so that branch is left out.

use super::db::{C4Db, ElRef, Element};
use super::route::{BoxR, HEAD, LABEL_PAD, LANE, NARROW};
use crate::svg::edges::ARROW;
use crate::text::label::{layout_label, Label};
use crate::text::measurer::style;
use std::collections::HashMap;

const SHAPE_W: f64 = 216.0;
const SHAPE_MIN_H: f64 = 64.0;
const PAD_X: f64 = 14.0;
const PAD_Y: f64 = 12.0;
// Room between two columns for the label of a line across them, clear of an arrowhead on either side.
const GAP_X: f64 = NARROW + LABEL_PAD + 3.0 * ARROW;
const GAP_Y: f64 = 88.0;
// Room for a relation to run along the inside of a boundary.
pub const GROUP_PAD: f64 = LANE + 8.0;
const GROUP_GAP: f64 = 48.0;
// Where a boundary's heading starts, from its top-left corner.
pub const HEAD_LEFT: f64 = 16.0;
pub const HEAD_TOP: f64 = 10.0;

// Shape keywords Mermaid accepts in $shape, $sprite or $tags, as names of the shared shapes.
fn form_name(word: &str) -> Option<&'static str> {
    match word {
        "person" => Some("person"),
        "box" | "rounded" | "component" => Some("rect"),
        "cylinder" | "database" | "db" => Some("cyl"),
        "queue" | "pipe" => Some("h-cyl"),
        _ => None,
    }
}

pub struct Node {
    pub shape: usize,
    // Index into the boxes.
    pub b: usize,
    // Type, name, technology, description.
    pub labels: Vec<Label>,
    pub text_h: f64,
}

pub struct Group {
    pub boundary: usize,
    // Index into the boxes.
    pub b: usize,
    pub kids: Vec<usize>,
    pub nodes: Vec<Node>,
    // Name, type, description.
    pub labels: Vec<Label>,
    pub head: f64,
    pub head_w: f64,
    pub head_x: f64,
}

pub struct Layout {
    // The boundaries that are drawn, parents before children, as indexes into `groups`. The
    // first is the unnamed top level.
    pub order: Vec<usize>,
    pub groups: Vec<Group>,
    pub boxes: Vec<BoxR>,
    // The box of each drawn element.
    pub of: HashMap<ElRef, usize>,
}

fn form_of(shape: &Element) -> &'static str {
    let explicit = form_name(&shape.shape.clone().unwrap_or_default().to_lowercase()).or_else(|| form_name(&shape.sprite.clone().unwrap_or_default().to_lowercase()));
    if let Some(f) = explicit {
        return f;
    }
    if let Some(tags) = shape.tags.as_deref().filter(|t| !t.is_empty()) {
        for tag in tags.split(',') {
            if let Some(f) = form_name(&crate::js::trim(tag).to_lowercase()) {
                return f;
            }
        }
    }
    let kind = &shape.kind;
    if kind.ends_with("person") {
        "person"
    } else if kind.ends_with("_db") {
        "cyl"
    } else if kind.ends_with("_queue") {
        "h-cyl"
    } else {
        "rect"
    }
}

fn measure(shape: &Element, index: usize, size: f64, small: f64, boxes: &mut Vec<BoxR>) -> Node {
    let width = SHAPE_W;
    let form = form_of(shape);
    let cap_w = if form == "h-cyl" { 36.0 } else { 0.0 };
    let cap_h = if form == "person" {
        HEAD
    } else if form == "cyl" {
        24.0
    } else {
        0.0
    };
    let inner = width - 2.0 * PAD_X - cap_w;
    let labels = vec![
        layout_label(Some(&format!("«{}»", shape.kind.replace('_', " "))), false, small - 1.0, inner, 0),
        layout_label(Some(&shape.label), false, size, inner, style::BOLD),
        layout_label(shape.techn.as_deref().filter(|t| !t.is_empty()).map(|t| format!("[{}]", t)).as_deref(), false, small, inner, 0),
        layout_label(shape.descr.as_deref(), false, small, inner, 0),
    ];
    let mut tw: f64 = 0.0;
    let mut text_h = if labels[3].height > 0.0 { 4.0 } else { 0.0 };
    for label in &labels {
        tw = tw.max(label.width);
        text_h += label.height;
    }
    let b = boxes.len();
    boxes.push(BoxR { x: 0.0, y: 0.0, w: width.max(tw + 2.0 * PAD_X + cap_w), h: SHAPE_MIN_H.max(text_h + 2.0 * PAD_Y + cap_h), form });
    Node { shape: index, b, labels, text_h }
}

// Places boxes left to right in rows of `per_row`, from (0, 0), and returns the width and height
// the rows take. Shapes are stretched to the height of their row; boundaries keep their own.
fn rows(boxes: &mut [BoxR], list: &[usize], per_row: usize, gap_x: f64, gap_y: f64, stretch: bool) -> (f64, f64) {
    let mut width: f64 = 0.0;
    let mut y = 0.0;
    let mut from = 0;
    while from < list.len() {
        let to = list.len().min(from + per_row);
        let mut x = 0.0;
        let mut row_h: f64 = 0.0;
        for &k in &list[from..to] {
            row_h = row_h.max(boxes[k].h);
        }
        for &k in &list[from..to] {
            let b = &mut boxes[k];
            b.x = x;
            b.y = y;
            if stretch {
                b.h = row_h;
            }
            x += b.w + gap_x;
        }
        width = width.max(x - gap_x);
        y += row_h + gap_y;
        from += per_row;
    }
    (width, if !list.is_empty() { y - gap_y } else { 0.0 })
}

/// Mermaid's grid: in each boundary, its shapes in rows of c4ShapeInRow, then the boundaries
/// inside it in rows of c4BoundaryInRow, each sized around its own content.
pub fn layout_c4(db: &C4Db, size: f64, small: f64) -> Layout {
    let group_pad = GROUP_PAD;
    let mut boxes: Vec<BoxR> = Vec::new();
    let mut groups: Vec<Group> = Vec::new();
    for (k, boundary) in db.boundaries.iter().enumerate() {
        let labels = if boundary.parent.is_some() {
            vec![
                layout_label(Some(&boundary.label), false, size, 4000.0, style::BOLD),
                layout_label(boundary.type_.as_deref().filter(|t| !t.is_empty()).map(|t| format!("[{}]", t)).as_deref(), false, small, 4000.0, 0),
                layout_label(boundary.descr.as_deref(), false, small, 4000.0, 0),
            ]
        } else {
            Vec::new()
        };
        let mut head = 0.0;
        let mut head_w: f64 = 0.0;
        for label in &labels {
            head += label.height;
            head_w = head_w.max(label.width);
        }
        let b = boxes.len();
        boxes.push(BoxR { form: "rect", ..Default::default() });
        groups.push(Group { boundary: k, b, kids: Vec::new(), nodes: Vec::new(), labels, head: if head > 0.0 { head + HEAD_TOP + 6.0 + LANE } else { group_pad }, head_w, head_x: HEAD_LEFT });
    }
    for k in 0..groups.len() {
        if let Some(p) = db.boundaries[k].parent {
            groups[p].kids.push(k);
        }
    }
    // A boundary that ends up inside itself is not reachable from the top, as in Mermaid.
    let mut order: Vec<usize> = vec![0];
    let mut i = 0;
    while i < order.len() {
        let kids = groups[order[i]].kids.clone();
        order.extend(kids);
        i += 1;
    }
    for (k, shape) in db.shapes.iter().enumerate() {
        if let Some(p) = shape.parent {
            let node = measure(shape, k, size, small, &mut boxes);
            groups[p].nodes.push(node);
        }
    }

    let per_row = (db.shape_in_row.max(1.0)) as usize;
    let groups_per_row = (db.boundary_in_row.max(1.0)) as usize;
    let mut kids_top = vec![0.0; groups.len()];
    for &g in order.iter().rev() {
        let node_boxes: Vec<usize> = groups[g].nodes.iter().map(|n| n.b).collect();
        let kid_boxes: Vec<usize> = groups[g].kids.iter().map(|&k| groups[k].b).collect();
        let (grid_w, grid_h) = rows(&mut boxes, &node_boxes, per_row, GAP_X, GAP_Y, true);
        let (kids_w, kids_h) = rows(&mut boxes, &kid_boxes, groups_per_row, GROUP_GAP, GROUP_GAP, false);
        let between = if grid_h > 0.0 && kids_h > 0.0 { GROUP_GAP } else { 0.0 };
        let frame = if g == 0 { 0.0 } else { group_pad };
        kids_top[g] = grid_h + between;
        let group = &groups[g];
        let w = grid_w.max(kids_w).max(group.head_w + 2.0 * HEAD_LEFT - 2.0 * frame) + 2.0 * frame;
        let h = (if frame != 0.0 { group.head } else { 0.0 }) + grid_h + between + kids_h + frame;
        let b = &mut boxes[group.b];
        b.w = w;
        b.h = h;
    }

    let mut of: HashMap<ElRef, usize> = HashMap::new();
    for &g in &order {
        let group = &groups[g];
        let gb = boxes[group.b].clone();
        let left = gb.x + if g == 0 { 0.0 } else { group_pad };
        let top = gb.y + if g == 0 { 0.0 } else { group.head };
        for node in &group.nodes {
            boxes[node.b].x += left;
            boxes[node.b].y += top;
            of.insert(ElRef::Shape(node.shape), node.b);
        }
        for &kid in &group.kids {
            let kb = groups[kid].b;
            boxes[kb].x += left;
            boxes[kb].y += top + kids_top[g];
        }
        if g != 0 {
            of.insert(ElRef::Boundary(group.boundary), group.b);
        }
    }
    Layout { order, groups, boxes, of }
}
