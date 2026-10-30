// Port of src/diagrams/treeview/render.ts. With no `maxWidth`, `tighten` draws once with room to
// spare, so the tight branch (wrapped labels) is left out; with no icon resolver every icon is
// drawn from the built-in outlines.

use super::icons::{get_node_icon, glyph, icon_config, BUILTIN_PACK};
use super::{TreeNode, TreeViewModel};
use crate::diagrams::common::fit_width::tighten;
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num};
use crate::svg::root::{svg_document, Accessible};
use crate::svg::theme::{class_names, ROUNDED};
use crate::text::entities::decode_entities;
use crate::text::label::layout_label;
use crate::text::measurer::{style, width as measure};
use crate::types::{RenderOptions, Rendered};
use crate::util::yaml::Value;
use std::collections::HashMap;

const ICON_GAP: f64 = 6.0;
const DESC_GAP: f64 = 16.0;
const GUIDE_INSET: f64 = 7.0;
const ELBOW: f64 = 4.0;
const HIGHLIGHT_OVERHANG: f64 = 4.0;

fn setting(config: &Config, key: &str, fallback: f64, min: f64, max: f64) -> f64 {
    match config.get("treeView").and_then(|v| v.as_map()).and_then(|m| m.get(key)) {
        Some(Value::Num(v)) if *v >= min && *v <= max => *v,
        _ => fallback,
    }
}

struct Row<'a> {
    node: &'a TreeNode,
    x: f64,
    cy: f64,
    icon: Option<String>,
    text: String,
    label_x: f64,
    highlighted: bool,
    top: f64,
    height: f64,
}

pub fn render_tree_view(model: &TreeViewModel, config: &Config, options: &RenderOptions) -> Rendered {
    tighten(options, |_tight| draw(model, config, options))
}

fn draw(model: &TreeViewModel, config: &Config, options: &RenderOptions) -> Rendered {
    let size = 16.0;
    let pad = 8.0;
    let icons = icon_config(config);

    let padding_x = setting(config, "paddingX", 5.0, 0.0, 100.0);
    let padding_y = setting(config, "paddingY", 2.0, 0.0, 100.0);
    let row_indent = setting(config, "rowIndent", 15.0, 0.0, 400.0);
    let line_thickness = setting(config, "lineThickness", 1.0, 0.25, 12.0);
    let step = row_indent + padding_x;
    let row_height = js::round(size * 1.5) + 2.0 * padding_y;
    let icon_size = size;

    let title = layout_label(model.title.as_deref(), false, size, 4000.0, style::BOLD);
    let title_height = if title.height > 0.0 { title.height + 12.0 } else { 0.0 };

    // Siblings line up: when one entry of a folder has an icon, the others leave room for one.
    let has_icon = |list: &[TreeNode]| list.iter().any(|c| get_node_icon(c.icon.as_deref(), &c.name, c.directory, &icons).is_some());
    let mut rows: Vec<Row> = Vec::new();
    let mut row_of: HashMap<*const TreeNode, usize> = HashMap::new();
    let mut pending: Vec<(&TreeNode, usize, bool)> = Vec::new();
    fn push<'a>(pending: &mut Vec<(&'a TreeNode, usize, bool)>, list: &'a [TreeNode], depth: usize, slot: bool) {
        for node in list.iter().rev() {
            pending.push((node, depth, slot));
        }
    }
    push(&mut pending, &model.root.children, 0, has_icon(&model.root.children));
    let mut label_right: f64 = 0.0;
    let mut any_highlight = false;
    let mut rows_height = 0.0;
    while let Some((node, depth, slot)) = pending.pop() {
        let x = depth as f64 * step;
        let text = decode_entities(&node.name);
        let label_x = x + if slot { icon_size + ICON_GAP } else { 0.0 };
        let st = if node.directory { style::BOLD } else { 0 };
        let width = measure(&text, size, st);
        if label_x + width > label_right {
            label_right = label_x + width;
        }
        let highlighted = node.css_class.as_deref().is_some_and(|c| c.split(|ch: char| js::is_space(ch)).any(|p| p == "highlight"));
        if highlighted {
            any_highlight = true;
        }
        let height = row_height;
        let cy = rows_height + height / 2.0;
        let icon = get_node_icon(node.icon.as_deref(), &node.name, node.directory, &icons);
        row_of.insert(node as *const TreeNode, rows.len());
        rows.push(Row { node, x, cy, icon, text, label_x, highlighted, top: rows_height, height });
        rows_height += height;
        push(&mut pending, &node.children, depth + 1, has_icon(&node.children));
    }

    let desc_x = label_right + DESC_GAP;
    let mut width = label_right;
    for row in &rows {
        let Some(description) = row.node.description.as_deref() else { continue };
        let w = measure(&decode_entities(description), size, style::ITALIC);
        width = width.max(desc_x + w);
    }
    width = width.max(title.width);
    let inset = pad + if any_highlight { HIGHLIGHT_OVERHANG } else { 0.0 };
    let top = pad + title_height;
    let total_width = (width + 2.0 * inset).ceil();
    let total_height = (top + rows_height + pad).ceil();

    let mut guides = String::new();
    for row in &rows {
        let children = &row.node.children;
        if children.is_empty() {
            continue;
        }
        let gx = inset + row.x + GUIDE_INSET;
        let tick = gx.max(inset + row.x + step - padding_x);
        let cy_of = |n: &TreeNode| rows[row_of[&(n as *const TreeNode)]].cy;
        let last = top + cy_of(&children[children.len() - 1]);
        let r = ELBOW.min(tick - gx);
        let mut d = format!("M{},{}V{}", num(gx), num(top + row.top + row.height - 2.0), num(last - r));
        if r > 0.0 {
            d.push_str(&format!("Q{},{} {},{}H{}", num(gx), num(last), num(gx + r), num(last), num(tick)));
        }
        let mut i = 0;
        while i + 1 < children.len() && tick > gx {
            d.push_str(&format!("M{},{}H{}", num(gx), num(top + cy_of(&children[i])), num(tick)));
            i += 1;
        }
        guides.push_str(&format!("<path d=\"{}\"/>", d));
    }

    let baseline = size * 0.35;
    let mut body = String::new();
    for row in &rows {
        let node = row.node;
        let cy = top + row.cy;
        let directory = node.directory;
        let mut inner = String::new();
        if row.highlighted {
            let x = inset + row.x - HIGHLIGHT_OVERHANG;
            inner.push_str(&format!(
                "<rect class=\"pele-tree-highlight\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_c)\" fill-opacity=\"0.14\"{}/>",
                num(x),
                num(top + row.top + 1.0),
                num(total_width - inset + HIGHLIGHT_OVERHANG - x),
                num(row.height - 2.0),
                ROUNDED
            ));
        }
        if let Some(icon) = &row.icon {
            let name = icon.strip_prefix(&format!("{}:", BUILTIN_PACK)).unwrap_or("");
            let g = glyph(name).or_else(|| glyph(if directory { "folder" } else { "file" })).unwrap();
            inner.push_str(&format!(
                "<svg class=\"pele-icon\" data-icon=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><path d=\"{}\" stroke=\"var(--_m)\" stroke-width=\"1.5\"/></svg>",
                esc_text(icon),
                num(inset + row.x),
                num(cy - icon_size / 2.0),
                num(icon_size),
                num(icon_size),
                g
            ));
        }
        if !row.text.is_empty() {
            inner.push_str(&format!(
                "<text class=\"pele-label\" x=\"{}\" y=\"{}\"{} xml:space=\"preserve\">{}</text>",
                num(inset + row.label_x),
                num(cy + baseline),
                if directory { " font-weight=\"var(--_hw)\"" } else { "" },
                esc(&row.text)
            ));
        }
        if let Some(description) = node.description.as_deref() {
            inner.push_str(&format!(
                "<text class=\"pele-tree-description\" x=\"{}\" y=\"{}\" font-style=\"italic\" fill=\"var(--_m)\" xml:space=\"preserve\">{}</text>",
                num(inset + desc_x),
                num(cy + baseline),
                esc(&decode_entities(description))
            ));
        }
        body.push_str(&format!(
            "<g class=\"pele-node pele-tree-{}{}\" data-id=\"{}\">{}</g>",
            if directory { "dir" } else { "file" },
            class_names(node.css_class.as_deref().unwrap_or("")),
            esc_text(&node.name),
            inner
        ));
    }

    let mut content = label_svg(&title, total_width / 2.0, pad + title.height / 2.0, " class=\"pele-title\" font-weight=\"var(--_tw)\"");
    if !guides.is_empty() {
        content.push_str(&format!("<g class=\"pele-tree-guides\" fill=\"none\" stroke=\"var(--_b)\" stroke-width=\"{}\">{}</g>", num(line_thickness), guides));
    }
    content.push_str(&body);
    let acc = Accessible { acc_title: model.acc_title.as_deref(), acc_descr: model.acc_descr.as_deref() };
    Rendered { svg: svg_document("treeView", total_width, total_height, size, options.responsive, &acc, &content), width: total_width, height: total_height, links: Vec::new() }
}
