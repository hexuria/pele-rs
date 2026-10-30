// Port of src/diagrams/sankey/render.ts. With no `idPrefix`, links are never painted with
// gradients (an empty prefix fails `/^[\w-]+$/`), so that branch is left out.

use super::layout::{layout_sankey, LayoutOptions};
use super::SankeyDb;
use crate::diagrams::common::fit_width::{fit_width, title_room};
use crate::errors::PeleError;
use crate::js::{self, num_str};
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num};
use crate::svg::root::{svg_document, Accessible};
use crate::svg::theme::{resolve_style, series_color};
use crate::text::entities::decode_entities;
use crate::text::label::layout_label;
use crate::text::measurer::{style, width as measure};
use crate::types::{RenderOptions, Rendered};
use crate::util::yaml::{Map, Value};
use std::collections::HashMap;

const LABEL_GAP: f64 = 6.0;
const VALUE_GAP: f64 = 15.0;
const LABEL_ROOM: f64 = 1.8;
const CAREFUL: usize = 300;

fn number_option(conf: &Map, key: &str, fallback: f64, min: f64, max: f64) -> f64 {
    match conf.get(key) {
        Some(Value::Num(v)) if *v >= min => v.min(max),
        _ => fallback,
    }
}

fn text_option(conf: &Map, key: &str) -> String {
    match conf.get(key) {
        Some(Value::Str(s)) => s.clone(),
        Some(Value::Num(v)) if v.is_finite() => num_str(*v),
        _ => String::new(),
    }
}

// Two decimals, as Mermaid shows values; numbers too large for that are left as they are.
fn rounded(value: f64) -> f64 {
    if value.abs() < 1e15 {
        js::round(value * 100.0) / 100.0
    } else {
        value
    }
}

// A colour from the configuration, as an inline style that overrides the series colour.
fn fill_style(color: &Value) -> String {
    match color {
        Value::Str(c) => resolve_style(&[format!("fill:{}", c)]).shape,
        _ => String::new(),
    }
}

pub fn render_sankey(model: &SankeyDb, config: &Config, options: &RenderOptions) -> Result<Rendered, PeleError> {
    let empty = Map::default();
    let conf = match config.get("sankey") {
        Some(Value::Map(m)) => m,
        _ => &empty,
    };
    let natural = number_option(conf, "width", 600.0, 1.0, 10000.0);
    let mut result = None;
    let rendered = fit_width(options, natural, 160.0, |chart_width, _| match draw(model, conf, options, chart_width, natural) {
        Ok(r) => r,
        Err(e) => {
            result = Some(e);
            Rendered { svg: String::new(), width: 0.0, height: 0.0, links: Vec::new() }
        }
    });
    match result {
        Some(e) => Err(e),
        None => Ok(rendered),
    }
}

struct LabelPlace {
    value: String,
    width: f64,
    after: bool,
    left: f64,
    y: f64,
}

fn draw(model: &SankeyDb, conf: &Map, options: &RenderOptions, chart_width: f64, natural: f64) -> Result<Rendered, PeleError> {
    let size = 16.0;
    let least = js::round(size * 0.625);
    let mut small = least.max(js::round(size * 0.875 * 1f64.min(chart_width / natural)));
    let pad = 8.0;

    let chart_height = number_option(conf, "height", 400.0, 1.0, 10000.0);
    let node_width = number_option(conf, "nodeWidth", 10.0, 1.0, 200.0).min(chart_width);
    let show_values = !matches!(conf.get("showValues"), Some(Value::Bool(false)));
    let prefix = decode_entities(&text_option(conf, "prefix"));
    let suffix = decode_entities(&text_option(conf, "suffix"));
    let outlined = matches!(conf.get("labelStyle"), Some(Value::Str(s)) if s == "outlined");
    let alignment: &'static str = match conf.get("nodeAlignment") {
        Some(Value::Str(s)) if s == "left" => "left",
        Some(Value::Str(s)) if s == "right" => "right",
        Some(Value::Str(s)) if s == "center" => "center",
        _ => "justify",
    };
    let link_color = match conf.get("linkColor") {
        Some(Value::Str(s)) => s.clone(),
        _ => "gradient".to_string(),
    };

    let lay = |label_size: f64| {
        layout_sankey(
            model,
            &LayoutOptions {
                width: chart_width,
                height: chart_height,
                node_width,
                node_padding: number_option(conf, "nodePadding", 12.0, 0.0, 200.0) + if show_values { VALUE_GAP } else { 0.0 },
                label_height: label_size * 1.15,
                alignment,
            },
        )
    };
    let mut layout = lay(small)?;
    // Labels also shrink with the most crowded column, so each keeps about LABEL_ROOM lines.
    let mut rows: HashMap<usize, usize> = HashMap::new();
    let mut fullest = 1;
    for node in &layout.nodes {
        let count = rows.get(&node.layer).copied().unwrap_or(0) + 1;
        rows.insert(node.layer, count);
        if count > fullest {
            fullest = count;
        }
    }
    let crowded = least.max(js::round(chart_height / fullest as f64 / LABEL_ROOM));
    if crowded < small {
        small = crowded;
        layout = lay(small)?;
    }
    let nodes = &layout.nodes;
    let links = &layout.links;
    let line_height = small * 1.5;

    let mut custom: HashMap<String, String> = HashMap::new();
    if let Some(Value::Map(colors)) = conf.get("nodeColors") {
        for (id, color) in &colors.0 {
            let style = fill_style(color);
            if !style.is_empty() {
                custom.insert(id.clone(), style);
            }
        }
    }
    let node_style = |i: usize| custom.get(&model.nodes[i].id).cloned().unwrap_or_default();

    // Labels sit beside the bars, on the side that faces the middle of the chart unless that
    // side is crowded.
    let mut central = 0;
    let mut most = 0.0;
    for node in nodes {
        if node.value > most {
            most = node.value;
            central = node.layer;
        }
    }
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (0f64, chart_width, 0f64, chart_height);
    let mut labels: Vec<LabelPlace> = nodes
        .iter()
        .enumerate()
        .map(|(i, node)| {
            let name = decode_entities(&model.nodes[i].id);
            let value = if show_values { format!("{}{}{}", prefix, num_str(rounded(node.value)), suffix) } else { String::new() };
            let width = measure(&if !value.is_empty() { format!("{} {}", name, value) } else { name }, small, 0);
            let after = if outlined { node.layer >= central } else { node.x < chart_width / 2.0 };
            let x = if after { node.x + node_width + LABEL_GAP } else { node.x - LABEL_GAP };
            let y = node.y + node.height / 2.0;
            min_x = min_x.min(if after { x } else { x - width });
            max_x = max_x.max(if after { x + width } else { x });
            min_y = min_y.min(y - small * 0.75);
            max_y = max_y.max(y + small * 0.75);
            LabelPlace { value, width, after, left: if after { x } else { x - width }, y }
        })
        .collect();
    // A label moves at most one line; retain two bands on each side. The bands hold indexes
    // into `labels`, which upstream shares by reference, so later moves are seen.
    let mut bands: HashMap<i64, Vec<usize>> = HashMap::new();
    if nodes.len() <= CAREFUL {
        for (k, label) in labels.iter().enumerate() {
            let band = (label.y / line_height).floor() as i64;
            for key in [band - 2, band - 1, band, band + 1, band + 2] {
                bands.entry(key).or_default().push(k);
            }
        }
    }
    let mut labels_out = String::new();
    let mut nodes_out = String::new();
    for (i, node) in nodes.iter().enumerate() {
        let id = &model.nodes[i].id;
        let name = decode_entities(id);
        let height = node.height.max(1.0);
        nodes_out.push_str(&format!(
            "<rect class=\"pele-sankey-node\" data-id=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\" fill=\"{}\"{}/>",
            esc_text(id),
            num(node.x),
            num(node.y),
            num(node_width),
            num(height),
            num(2f64.min(height / 2.0)),
            series_color(i as i64),
            node_style(i)
        ));

        let overlaps_label = |labels: &[LabelPlace], left: f64, y: f64| -> bool {
            let me = &labels[i];
            bands.get(&((y / line_height).floor() as i64)).is_some_and(|list| {
                list.iter().any(|&o| {
                    let other = &labels[o];
                    o != i && (other.y - y).abs() < line_height && left < other.left + other.width + LABEL_GAP && left + me.width > other.left - LABEL_GAP
                })
            })
        };
        // Try the other side, then an adjacent line when both sides are crowded.
        if nodes.len() <= CAREFUL && overlaps_label(&labels, labels[i].left, labels[i].y) {
            'place: for dy in [0.0, -line_height, line_height] {
                for after in [labels[i].after, !labels[i].after] {
                    let width = labels[i].width;
                    let left = if after { node.x + node_width + LABEL_GAP } else { node.x - LABEL_GAP - width };
                    let y = labels[i].y + dy;
                    if overlaps_label(&labels, left, y) || left < min_x || left + width > max_x || y - small * 0.75 < min_y || y + small * 0.75 > max_y {
                        continue;
                    }
                    if nodes
                        .iter()
                        .enumerate()
                        .any(|(j, bar)| j != i && left < bar.x + node_width + LABEL_GAP && left + width > bar.x - LABEL_GAP && y - small * 0.75 < bar.y + bar.height && y + small * 0.75 > bar.y)
                    {
                        continue;
                    }
                    // A label stays nearer its own bar than any other in the column.
                    let own = 0f64.max(node.y - y).max(y - node.y - node.height);
                    if nodes.iter().enumerate().any(|(j, bar)| j != i && bar.x == node.x && 0f64.max(bar.y - y).max(y - bar.y - bar.height) <= own) {
                        continue;
                    }
                    labels[i].left = left;
                    labels[i].y = y;
                    labels[i].after = after;
                    break 'place;
                }
            }
        }
        let label = &labels[i];
        let (value, after, y) = (&label.value, label.after, label.y);
        let x = if after { node.x + node_width + LABEL_GAP } else { node.x - LABEL_GAP };
        let position = format!(" x=\"{}\" y=\"{}\" text-anchor=\"{}\"", num(x), num(y + small * 0.35), if after { "start" } else { "end" });
        let text = esc(&name) + &if !value.is_empty() { format!(" <tspan class=\"pele-sankey-value\" fill=\"var(--_m)\">{}</tspan>", esc(value)) } else { String::new() };
        labels_out.push_str(&format!("<g class=\"pele-label\" data-id=\"{}\">", esc_text(id)));
        if outlined {
            labels_out.push_str(&format!(
                "<text{} fill=\"none\" stroke=\"var(--_bg)\" stroke-width=\"3\" stroke-linejoin=\"round\">{}{}</text>",
                position,
                esc(&name),
                if !value.is_empty() { format!(" {}", esc(value)) } else { String::new() }
            ));
        }
        labels_out.push_str(&format!("<text{}>{}</text></g>", position, text));
    }

    let fixed = if link_color == "gradient" || link_color == "source" || link_color == "target" { String::new() } else { fill_style(&Value::Str(link_color.clone())) };
    let mut links_out = String::new();
    for link in links {
        let end = if link_color == "target" { link.target } else { link.source };
        let half = link.thickness.max(1.0) / 2.0;
        let x0 = nodes[link.source].x + node_width;
        let x1 = nodes[link.target].x;
        let paint = format!("fill=\"{}\"{}", series_color(end as i64), if !fixed.is_empty() { fixed.clone() } else { node_style(end) });
        let mid = num((x0 + x1) / 2.0);
        let from = esc_text(&model.nodes[link.source].id);
        let to = esc_text(&model.nodes[link.target].id);
        links_out.push_str(&format!(
            "<path class=\"pele-sankey-link\" data-id=\"{from}-&gt;{to}\" d=\"M{},{}C{mid},{} {mid},{} {},{}L{},{}C{mid},{} {mid},{} {},{}Z\" {paint}><title>{from} → {to}: {}{}{}</title></path>",
            num(x0),
            num(link.source_y - half),
            num(link.source_y - half),
            num(link.target_y - half),
            num(x1),
            num(link.target_y - half),
            num(x1),
            num(link.target_y + half),
            num(link.target_y + half),
            num(link.source_y + half),
            num(x0),
            num(link.source_y + half),
            esc(&prefix),
            num_str(rounded(link.value)),
            esc(&suffix),
        ));
    }

    let title = layout_label(model.title.as_deref(), false, size, title_room(options), style::BOLD);
    let title_height = if title.height > 0.0 { title.height + 12.0 } else { 0.0 };
    let inner = (max_x - min_x).max(title.width);
    let ox = pad - min_x + (inner - (max_x - min_x)) / 2.0;
    let oy = pad + title_height - min_y;
    let width = (inner + 2.0 * pad).ceil();
    let height = (max_y - min_y + title_height + 2.0 * pad).ceil();
    let content = label_svg(&title, width / 2.0, pad + title.height / 2.0, " class=\"pele-title\" font-weight=\"var(--_tw)\"")
        + &format!(
            "<g transform=\"translate({},{})\"><g class=\"pele-sankey-links\" fill-opacity=\"0.4\">{}</g><g class=\"pele-sankey-nodes\">{}</g><g class=\"pele-labels\" font-size=\"{}\">{}</g></g>",
            num(ox),
            num(oy),
            links_out,
            nodes_out,
            small,
            labels_out
        );
    Ok(Rendered { svg: svg_document("sankey", width, height, size, options.responsive, &Accessible::default(), &content), width, height, links: Vec::new() })
}
