// Port of src/diagrams/block/layout.ts. Cells are in breadth-first order from the root at 0,
// and a cell's children are indexes into the same list.

use crate::errors::{PeleError, PeleErrorCode};

#[derive(Clone, Default)]
pub struct Cell {
    pub children: Vec<usize>,
    // Grid width of this container: a positive count, or -1 to put every child on one row.
    pub columns: f64,
    pub span: f64,
    pub space: bool,
    // Height reserved above a container's grid for its title.
    pub head: f64,
    // Size the content needs. Set by the caller for leaves, computed here for containers.
    pub nw: f64,
    pub nh: f64,
    pub col: f64,
    pub row: f64,
    pub cols: f64,
    pub heights: Vec<f64>,
    // Extra space after each row or column; None until a gap is widened (upstream's shared
    // empty list).
    pub row_gaps: Option<Vec<f64>>,
    pub col_gaps: Option<Vec<f64>>,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

const MAX_SPAN: f64 = 1000.0;

pub fn calculate_block_position(columns: f64, position: f64) -> Result<(f64, f64), PeleError> {
    if columns == 0.0 || columns.fract() != 0.0 || !columns.is_finite() {
        return Err(PeleError::typed("Columns must be an integer !== 0.", PeleErrorCode::Semantic, "block"));
    }
    if columns < 0.0 {
        return Ok((position, 0.0));
    }
    Ok((position % columns, (position / columns).floor()))
}

fn total(gaps: &Option<Vec<f64>>) -> f64 {
    gaps.as_ref().map_or(0.0, |g| g.iter().sum())
}

fn get(gaps: &Option<Vec<f64>>, k: f64) -> f64 {
    gaps.as_ref().and_then(|g| g.get(k as usize)).copied().unwrap_or(0.0)
}

// Puts each child in its grid cell and sizes the container.
fn measure(cells: &mut [Cell], b: usize, gap: f64, inset: f64) -> Result<(), PeleError> {
    let columns = cells[b].columns;
    let mut heights: Vec<f64> = Vec::new();
    let mut position = 0.0;
    let mut cols: f64 = 0.0;
    let mut cw: f64 = 0.0;
    let mut tallest: f64 = 0.0;
    for k in 0..cells[b].children.len() {
        let c = cells[b].children[k];
        let (px, py) = calculate_block_position(columns, position)?;
        // Mermaid lets a block wider than the rest of its row stick out of the grid. Here it stops at the last column.
        let child = &mut cells[c];
        let mut span = if child.span >= 1.0 { child.span.min(MAX_SPAN) } else { 1.0 };
        if columns > 0.0 && span > columns - px {
            span = columns - px;
        }
        child.span = span;
        child.col = px;
        child.row = py;
        position += span;
        if px + span > cols {
            cols = px + span;
        }
        if py as usize == heights.len() {
            heights.push(0.0);
        }
        let row = py as usize;
        if child.children.is_empty() {
            heights[row] = -1.0;
        } else if heights[row] >= 0.0 && child.nh > heights[row] {
            heights[row] = child.nh;
        }
        if child.space {
            continue;
        }
        let width = (child.nw - gap * (span - 1.0)) / span;
        if width > cw {
            cw = width;
        }
        if child.nh > tallest {
            tallest = child.nh;
        }
    }
    let bx = &mut cells[b];
    let mut height = (heights.len() as f64 - 1.0) * gap + total(&bx.row_gaps);
    for h in heights.iter_mut() {
        if *h < 0.0 {
            *h = tallest;
        }
        height += *h;
    }
    bx.cols = cols;
    bx.heights = heights;
    bx.nw = bx.nw.max(cols * cw + (cols - 1.0) * gap + total(&bx.col_gaps) + 2.0 * inset);
    bx.nh = height + 2.0 * inset + bx.head;
    Ok(())
}

// Gives each child its cell, sharing out any room the container has beyond what it needs.
fn arrange(cells: &mut [Cell], b: usize, gap: f64, inset: f64) {
    let bx = cells[b].clone();
    let cols = bx.cols;
    let rows = bx.heights.len() as f64;
    let cw = (bx.w - 2.0 * inset - (cols - 1.0) * gap - total(&bx.col_gaps)) / cols;
    let room = bx.h - 2.0 * inset - bx.head - (rows - 1.0) * gap - total(&bx.row_gaps);
    let mut offsets: Vec<f64> = Vec::new();
    if bx.col_gaps.as_ref().is_some_and(|g| !g.is_empty()) {
        let mut sum = 0.0;
        let mut col = 0.0;
        while col < cols {
            offsets.push(sum);
            sum += get(&bx.col_gaps, col);
            col += 1.0;
        }
    }
    let mut needed = 0.0;
    for h in &bx.heights {
        needed += h;
    }
    let scale = if needed > 0.0 { room / needed } else { 0.0 };
    let share = if needed > 0.0 { 0.0 } else { room / rows };
    let left = bx.x - bx.w / 2.0 + inset;
    let mut top = bx.y - bx.h / 2.0 + inset + bx.head;
    let mut row = 0usize;
    let offset = |k: f64| offsets.get(k as usize).copied().unwrap_or(0.0);
    for &c in &bx.children {
        let child = &mut cells[c];
        while (row as f64) < child.row {
            top += bx.heights[row] * scale + share + gap + get(&bx.row_gaps, row as f64);
            row += 1;
        }
        let before = offset(child.col);
        child.w = cw * child.span + gap * (child.span - 1.0) + offset(child.col + child.span - 1.0) - before;
        child.h = bx.heights[row] * scale + share;
        child.x = left + child.col * (cw + gap) + before + child.w / 2.0;
        child.y = top + child.h / 2.0;
    }
}

/// Lays out a tree of cells in place. The root has no border, so its grid starts at the origin.
pub fn layout(cells: &mut [Cell], gap: f64) -> Result<(f64, f64), PeleError> {
    let mut order = vec![0usize];
    let mut i = 0;
    while i < order.len() {
        let kids = cells[order[i]].children.clone();
        order.extend(kids);
        i += 1;
    }
    for i in (0..order.len()).rev() {
        let b = order[i];
        if !cells[b].children.is_empty() {
            measure(cells, b, gap, if i == 0 { 0.0 } else { gap })?;
        }
    }
    let root = &mut cells[0];
    root.w = root.nw;
    root.h = root.nh;
    root.x = root.w / 2.0;
    root.y = root.h / 2.0;
    for (i, &b) in order.iter().enumerate() {
        if !cells[b].children.is_empty() {
            arrange(cells, b, gap, if i == 0 { 0.0 } else { gap });
        }
    }
    Ok((cells[0].w, cells[0].h))
}
