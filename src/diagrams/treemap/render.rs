// Port of src/diagrams/treemap/render.ts.

use super::format::formatter;
use super::layout::{layout_treemap, Box, Insets};
use super::{TreemapModel, TreemapNode};
use crate::diagrams::common::fit_text::fit_text;
use crate::diagrams::common::fit_width::{cap_width, title_room};
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num};
use crate::svg::ink::ink_fill;
use crate::svg::root::{svg_document, Accessible};
use crate::svg::theme::{class_names, resolve_style, rounded, series_color};
use crate::text::entities::decode_entities;
use crate::text::label::layout_label;
use crate::text::measurer::{style as st, width as measure};
use crate::types::{RenderOptions, Rendered};
use crate::util::yaml::Value;

const HEADER: f64 = 28.0;
const SHORT_HEADER: f64 = 18.0;
const SIDE: f64 = 6.0;
const LINE: f64 = 1.25;

fn title_part(raw: &str) -> String {
    js::trim(&esc(js::trim(&decode_entities(raw)))).to_string()
}

fn leaf_count(roots: &[TreemapNode]) -> usize {
    let mut count = 0;
    let mut stack: Vec<&TreemapNode> = roots.iter().collect();
    while let Some(node) = stack.pop() {
        match &node.children {
            Some(children) => stack.extend(children.iter()),
            None => {
                if node.value.is_some_and(|v| v.is_finite() && v > 0.0) {
                    count += 1;
                }
            }
        }
    }
    count
}

// Caps corners at a quarter of a small box's shorter side; 4px is the default radius.
fn corners(attrs: &str, b: &Box) -> String {
    let cap = b.w.min(b.h) / 4.0;
    if cap < 4.0 {
        rounded(attrs).replacen("rx:var(--_r)", &format!("rx:min(var(--_r),{}px)", num(cap)), 1)
    } else {
        rounded(attrs)
    }
}

fn option(config: &Config, key: &str, fallback: f64, min: f64, max: f64) -> f64 {
    match config.get("treemap").and_then(|v| v.as_map()).and_then(|m| m.get(key)) {
        Some(Value::Num(v)) if *v >= min => v.min(max),
        _ => fallback,
    }
}

pub fn render_treemap(model: &TreemapModel, config: &Config, options: &RenderOptions) -> Rendered {
    let size = 16.0;
    let small = js::round(size * 0.875);
    let tiny = js::round(size * 0.75);
    let least = js::round(size * 0.5625);
    let treemap = config.get("treemap").and_then(|v| v.as_map());
    let pad = option(config, "diagramPadding", 8.0, 0.0, 2000.0);

    // Mermaid sizes the diagram as ten times `nodeWidth` by ten times `nodeHeight`.
    let width = js::round(cap_width(options, option(config, "nodeWidth", 64.0, 1.0, 400.0) * 10.0, 200.0, 2.0 * pad));
    let height = js::round(option(config, "nodeHeight", 40.0, 1.0, 400.0) * 10.0);
    let show_values = !matches!(treemap.and_then(|t| t.get("showValues")), Some(Value::Bool(false)));
    let format = formatter(match treemap.and_then(|t| t.get("valueFormat")) {
        Some(Value::Str(s)) => s,
        _ => ",",
    });
    // Reduce gaps and insets as average leaf area shrinks.
    let crowd = 1f64.min(0f64.max(((width * height) / (leaf_count(&model.roots).max(1) as f64) / 800.0).log10()));
    let (boxes, placed) = layout_treemap(
        &model.roots,
        width,
        height,
        &Insets { gap: option(config, "padding", js::round(2.0 + 2.0 * crowd), 0.0, 200.0), header: HEADER, short: SHORT_HEADER, side: js::round(3.0 + 3.0 * crowd) },
    );

    let title = layout_label(model.title.as_deref(), false, size, title_room(options), st::BOLD);
    let title_height = if title.height > 0.0 { title.height + 8.0 } else { 0.0 };
    let left = pad + 0f64.max((title.width - width) / 2.0);
    let top = pad + title_height;

    let mut sections = String::new();
    let mut leaves = String::new();
    for &k in &placed {
        let b = &boxes[k];
        let node = b.node;
        let style = resolve_style(node.css_compiled_styles.as_deref().unwrap_or(&[]));
        let color = series_color(b.series as i64);
        let x = left + b.x;
        let y = top + b.y;
        let name = js::trim(&decode_entities(&node.name)).to_string();
        let value = if show_values { format.format(b.value) } else { String::new() };
        let group = format!("data-id=\"{}\">", esc_text(&node.name));
        // Include headerless ancestors in tooltips because their children cover them.
        let own = title_part(&node.name);
        let mut path: Vec<String> = if own.is_empty() { Vec::new() } else { vec![own.clone()] };
        let mut p = b.parent;
        while let Some(pi) = p {
            if boxes[pi].headed {
                break;
            }
            let section = title_part(&boxes[pi].node.name);
            if !section.is_empty() {
                path.insert(0, section);
            }
            p = boxes[pi].parent;
        }
        let full = path.join(" › ");
        let tooltip = |drawn: Option<&str>| if full.is_empty() || (drawn == Some(name.as_str()) && full == own) { String::new() } else { format!("<title>{}</title>", full) };
        let rect = format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"", num(x), num(y), num(b.w), num(b.h), color);
        // Inline font sizes override SVG attributes, so truncate instead of shrinking.
        let fixed = style.font_size;
        let text_style = (if style.bold { st::BOLD } else { 0 }) | (if style.italic { st::ITALIC } else { 0 });
        let ink = ink_fill(&style.shape, &style.text, None);

        if node.children.is_some() {
            let mut text = String::new();
            let mut drawn: Option<String> = None;
            if b.headed {
                let short = b.header < HEADER;
                let label_size = fixed.unwrap_or(if short { tiny } else { small });
                let value_size = fixed.unwrap_or(tiny);
                let room = b.w - 2.0 * SIDE - 4.0;
                let value_width = if !value.is_empty() { measure(&value, value_size, text_style) } else { 0.0 };
                let with_value = !short && value_width > 0.0 && room - value_width - 12.0 >= 3.0 * label_size;
                let label = if label_size * LINE < b.header { fit_text(&name, if with_value { room - value_width - 12.0 } else { room }, label_size, label_size, text_style | st::BOLD) } else { None };
                let baseline = y + b.header / 2.0 + 1.0;
                if let Some(label) = &label {
                    drawn = Some(label.text.clone());
                    text.push_str(&format!(
                        "<text x=\"{}\" y=\"{}\" font-weight=\"var(--_hw)\"{}{}{}>{}</text>",
                        num(x + SIDE + 2.0),
                        num(baseline + label_size * 0.35),
                        if short { format!(" font-size=\"{}\"", js::num_str(label_size)) } else { String::new() },
                        ink,
                        style.text,
                        esc(&label.text)
                    ));
                }
                if with_value && value_size * LINE <= HEADER {
                    text.push_str(&format!(
                        "<text class=\"pele-value\" x=\"{}\" y=\"{}\" font-size=\"{}\" text-anchor=\"end\"{}{}>{}</text>",
                        num(x + b.w - SIDE - 2.0),
                        num(baseline + value_size * 0.35),
                        tiny,
                        if ink.is_empty() { " fill=\"var(--_m)\"" } else { &ink },
                        style.text,
                        esc(&value)
                    ));
                }
            }
            // A section is a tint of its color, unless a class gives it a fill of its own.
            let tint = if style.shape.contains("\"fill:") || style.shape.contains(";fill:") { "" } else { " fill-opacity=\"0.12\"" };
            let shape = if b.headed {
                format!("{}{}/>", rect, corners(&format!("{} stroke=\"{}\"{}", tint, color, style.shape), b))
            } else {
                format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\"/>", num(x), num(y), num(b.w), num(b.h))
            };
            sections.push_str(&format!("<g class=\"pele-node pele-section{}\" {}{}{}{}</g>", class_names(node.class_selector.as_deref().unwrap_or("")), group, tooltip(drawn.as_deref()), shape, text));
            continue;
        }

        let mut text = String::new();
        let room = b.w - 8.0;
        let smallest = fixed.unwrap_or(least);
        let tallest = match fixed {
            None => size.min(((b.h - 4.0) / LINE).floor()),
            Some(f) => {
                if f * LINE <= b.h - 4.0 {
                    f
                } else {
                    0.0
                }
            }
        };
        let label = if tallest >= smallest { fit_text(&name, room, tallest, smallest, text_style) } else { None };
        if let Some(label) = &label {
            let cx = num(x + b.w / 2.0);
            let cy = y + b.h / 2.0;
            let value_size = fixed.unwrap_or_else(|| least.max(js::round(label.size * 0.8)));
            let lines = (label.size + value_size) * LINE;
            if !value.is_empty() && lines <= b.h - 4.0 && measure(&value, value_size, text_style) <= room {
                let first = cy - lines / 2.0 + (label.size * LINE) / 2.0;
                let second = cy + lines / 2.0 - (value_size * LINE) / 2.0;
                text = format!(
                    "<text x=\"{cx}\" y=\"{}\" font-size=\"{}\"{ink}{}>{}</text><text class=\"pele-value\" x=\"{cx}\" y=\"{}\" font-size=\"{}\"{ink}{}>{}</text>",
                    num(first + label.size * 0.35),
                    num(label.size),
                    style.text,
                    esc(&label.text),
                    num(second + value_size * 0.35),
                    num(value_size),
                    style.text,
                    esc(&value)
                );
            } else {
                text = format!("<text x=\"{cx}\" y=\"{}\" font-size=\"{}\"{ink}{}>{}</text>", num(cy + label.size * 0.35), num(label.size), style.text, esc(&label.text));
            }
        }
        leaves.push_str(&format!(
            "<g class=\"pele-node pele-leaf{}\" {}{}{}{}/>{}</g>",
            class_names(node.class_selector.as_deref().unwrap_or("")),
            group,
            tooltip(label.as_ref().map(|l| l.text.as_str())),
            rect,
            corners(&style.shape, b),
            text
        ));
    }

    let total_width = (width.max(title.width) + 2.0 * pad).ceil();
    let total_height = (height + title_height + 2.0 * pad).ceil();
    let mut content = label_svg(&title, total_width / 2.0, pad + title.height / 2.0, " class=\"pele-title\" font-weight=\"var(--_tw)\"");
    if !sections.is_empty() {
        content.push_str(&format!("<g class=\"pele-sections\" font-size=\"{}\">{}</g>", small, sections));
    }
    if !leaves.is_empty() {
        content.push_str(&format!("<g class=\"pele-leaves\" text-anchor=\"middle\" fill=\"var(--_bg)\">{}</g>", leaves));
    }
    let acc = Accessible { acc_title: model.acc_title.as_deref(), acc_descr: model.acc_descr.as_deref() };
    Rendered { svg: svg_document("treemap", total_width, total_height, size, options.responsive, &acc, &content), width: total_width, height: total_height, links: Vec::new() }
}
