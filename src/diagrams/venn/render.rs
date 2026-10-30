// Port of src/diagrams/venn/render.ts.

use super::layout::{find_spot, json_list, layout_circles, region_path, Circle, Spot};
use super::{normalize_text, VennDb, VennSubset};
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc_text, label_svg, num};
use crate::svg::ink::ink;
use crate::svg::root::{svg_document, Accessible};
use crate::svg::theme::{resolve_style, series_color, ResolvedStyle};
use crate::text::label::{layout_label, Label};
use crate::text::measurer::style as text_style;
use crate::types::{RenderOptions, Rendered};

const RADIUS: f64 = 100.0;
const MAX_GROWTH: f64 = 1.6;

struct Line {
    id: Option<String>,
    raw: String,
    size: f64,
    base: u32,
    label: Label,
}

struct Region {
    sets: Vec<String>,
    inside: Vec<usize>,
    spot: Spot,
    // What is written in the region: its label first, then its texts. The id is that of a text.
    lines: Vec<Line>,
    style: ResolvedStyle,
    // Whether its text is written across other circles.
    across: bool,
}

// One key for a list of set ids. Ids may hold any character, so they are not simply joined.
fn key_of(ids: &[String]) -> String {
    if ids.len() == 1 {
        ids[0].clone()
    } else {
        json_list(ids)
    }
}

// `/[";]fill-opacity:/`
fn sets_opacity(shape: &str) -> bool {
    shape.contains("\"fill-opacity:") || shape.contains(";fill-opacity:")
}

pub fn render_venn(model: &VennDb, _config: &Config, options: &RenderOptions) -> Rendered {
    let size = 16.0;
    let small = js::round(size * 0.875);
    let tiny = js::round(size * 0.8125);
    let pad = 8.0;
    let no_style = resolve_style(&[]);

    let circles: Vec<Circle> = layout_circles(&model.subsets);
    let index = |id: &str| circles.iter().rposition(|c| c.id == id);

    // Styles for the same targets add up, later declarations winning.
    let mut styles: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for declared in &model.styles {
        let key = key_of(&declared.targets);
        let at = match styles.iter().position(|(k, _)| *k == key) {
            Some(at) => at,
            None => {
                styles.push((key, Vec::new()));
                styles.len() - 1
            }
        };
        let merged = &mut styles[at].1;
        for (property, value) in &declared.styles {
            match merged.iter_mut().find(|(k, _)| k == property) {
                Some(slot) => slot.1 = value.clone(),
                None => merged.push((property.clone(), value.clone())),
            }
        }
    }
    let style_of = |key: &str| -> ResolvedStyle {
        let Some((_, declared)) = styles.iter().find(|(k, _)| k == key) else { return no_style.clone() };
        let list: Vec<String> = declared.iter().map(|(p, v)| format!("{}:{}", p, v)).collect();
        resolve_style(&list)
    };

    // Circles that touch each circle; only those can cut into a region of it.
    let mut neighbors: Vec<Vec<usize>> = circles.iter().map(|_| Vec::new()).collect();
    for a in 0..circles.len() {
        for b in a + 1..circles.len() {
            if js::hypot(circles[a].x - circles[b].x, circles[a].y - circles[b].y) < circles[a].r + circles[b].r {
                neighbors[a].push(b);
                neighbors[b].push(a);
            }
        }
    }

    // One region for each set and each stated overlap; a later statement about the same sets replaces an earlier one.
    let mut stated: Vec<(String, &VennSubset)> = Vec::new();
    for subset in &model.subsets {
        let key = key_of(&subset.sets);
        match stated.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = subset,
            None => stated.push((key, subset)),
        }
    }
    let mut regions: Vec<(String, Region)> = Vec::new();
    for (key, subset) in &stated {
        let mut sets: Vec<String> = Vec::new();
        for id in &subset.sets {
            if !sets.contains(id) {
                sets.push(id.clone());
            }
        }
        let inside: Vec<usize> = sets.iter().map(|id| index(id).unwrap()).collect();
        let mut smallest = inside[0];
        for &i in &inside {
            if circles[i].r < circles[smallest].r {
                smallest = i;
            }
        }
        let style = style_of(key);
        let text = subset.label.clone().or_else(|| if subset.sets.len() == 1 { Some(subset.sets[0].clone()) } else { None });
        let base = (if style.bold { text_style::BOLD } else { 0 }) | (if style.italic { text_style::ITALIC } else { 0 });
        let others: Vec<usize> = neighbors[smallest].iter().copied().filter(|i| !inside.contains(i)).collect();
        let mut spot = find_spot(&circles, &inside, &others);
        // A region that other circles cover, or all but cover, is written across them instead.
        let across = !others.is_empty() && spot.margin < circles[smallest].r * 0.2;
        if across {
            spot = find_spot(&circles, &inside, &[]);
        }
        let mut region = Region { sets, inside, spot, lines: Vec::new(), style, across };
        if let Some(text) = text.filter(|t| !t.is_empty()) {
            let font_size = if region.sets.len() == 1 { size } else { small };
            let label = layout_label(Some(&text), false, font_size, 4000.0, base);
            region.lines.push(Line { id: None, raw: text, size: font_size, base, label });
        }
        regions.push((key.clone(), region));
    }
    for node in &model.text_nodes {
        let raw = node.label.clone().unwrap_or_else(|| node.id.clone());
        let key = key_of(&node.sets);
        if let Some((_, region)) = regions.iter_mut().find(|(k, _)| *k == key) {
            let label = layout_label(Some(&raw), false, tiny, 4000.0, 0);
            region.lines.push(Line { id: Some(node.id.clone()), raw, size: tiny, base: 0, label });
        }
    }

    // Lengths so far are in units of the layout. The scale gives the largest circle a set radius,
    // and grows, within limits, until every region holds what is written in it.
    let mut largest = 0.0;
    for c in &circles {
        if c.r > largest {
            largest = c.r;
        }
    }
    let mut scale = if largest > 0.0 { RADIUS / largest } else { 1.0 };
    let mut growth: f64 = 1.0;
    for (_, region) in &regions {
        if region.spot.margin <= 0.0 {
            continue;
        }
        let mut width: f64 = 0.0;
        let mut height = 0.0;
        for line in &region.lines {
            width = width.max(line.label.width);
            height += line.label.height;
        }
        let need = js::hypot(width, height) / 2.0 + 4.0;
        growth = growth.max((need / (region.spot.margin * scale)).min(MAX_GROWTH));
    }
    scale *= growth;

    // What is still too wide for its region is wrapped to it.
    for (_, region) in regions.iter_mut() {
        let room = (region.spot.margin * scale * 1.8).max(72.0);
        for line in region.lines.iter_mut() {
            if line.label.width > room {
                line.label = layout_label(Some(&line.raw), false, line.size, room, line.base);
            }
        }
    }

    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for c in &circles {
        min_x = min_x.min((c.x - c.r) * scale);
        min_y = min_y.min((c.y - c.r) * scale);
        max_x = max_x.max((c.x + c.r) * scale);
        max_y = max_y.max((c.y + c.r) * scale);
    }
    for (_, region) in &regions {
        let mut wide: f64 = 0.0;
        let mut tall = 0.0;
        for line in &region.lines {
            wide = wide.max(line.label.width);
            tall += line.label.height;
        }
        min_x = min_x.min(region.spot.x * scale - wide / 2.0);
        max_x = max_x.max(region.spot.x * scale + wide / 2.0);
        min_y = min_y.min(region.spot.y * scale - tall / 2.0);
        max_y = max_y.max(region.spot.y * scale + tall / 2.0);
    }
    if circles.is_empty() {
        min_x = 0.0;
        min_y = 0.0;
        max_x = 0.0;
        max_y = 0.0;
    }

    let title_text = model.title.as_ref().map(|t| if t.is_empty() { t.clone() } else { normalize_text(t) });
    let title = layout_label(title_text.as_deref(), false, size, 4000.0, text_style::BOLD);
    let title_height = if title.height > 0.0 { title.height + 12.0 } else { 0.0 };
    let inner = (max_x - min_x).max(title.width);
    let ox = pad + (inner - (max_x - min_x)) / 2.0 - min_x;
    let oy = pad + title_height - min_y;
    let width = (inner + 2.0 * pad).ceil();
    let height = (max_y - min_y + title_height + 2.0 * pad).ceil();
    let placed: Vec<Circle> = circles.iter().map(|c| Circle { id: c.id.clone(), x: c.x * scale + ox, y: c.y * scale + oy, r: c.r * scale }).collect();

    let mut sets = String::new();
    for (i, c) in placed.iter().enumerate() {
        let color = series_color(i as i64);
        let shape = regions.iter().find(|(k, _)| *k == c.id).map_or("", |(_, r)| r.style.shape.as_str());
        sets.push_str(&format!(
            "<circle class=\"pele-venn-set\" data-id=\"{}\" cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{}\" fill-opacity=\"0.2\" stroke=\"{}\"{}/>",
            esc_text(&c.id),
            num(c.x),
            num(c.y),
            num(c.r),
            color,
            color,
            shape
        ));
    }

    let painted: Vec<&Region> = regions.iter().map(|(_, r)| r).filter(|r| r.sets.len() > 1 && !r.style.shape.is_empty()).collect();
    // The fill under a region's text is the last overlap drawn over all of it, or its own circle when the style sets an opacity.
    let under = |region: &Region| -> String {
        if region.across {
            return String::new();
        }
        let mut fill = if region.sets.len() == 1 && sets_opacity(&region.style.shape) { region.style.shape.clone() } else { String::new() };
        for other in &painted {
            if other.sets.iter().all(|id| region.sets.contains(id)) {
                fill = other.style.shape.clone();
            }
        }
        fill
    };

    let mut overlaps = String::new();
    let mut labels = String::new();
    for (_, region) in &regions {
        let id = esc_text(&region.sets.join("|"));
        // An overlap is only drawn when a style gives it a look of its own; otherwise the circles show it.
        if region.sets.len() > 1 && !region.style.shape.is_empty() {
            let shared: Vec<Circle> = region.inside.iter().map(|&i| placed[i].clone()).collect();
            let d = region_path(&shared, num);
            if !d.is_empty() {
                overlaps.push_str(&format!("<path class=\"pele-venn-overlap\" data-id=\"{}\" d=\"{}\" fill=\"none\"{}/>", id, d, region.style.shape));
            }
        }
        let mut total = 0.0;
        for line in &region.lines {
            total += line.label.height;
        }
        if total == 0.0 {
            continue;
        }
        let x = region.spot.x * scale + ox;
        let mut y = region.spot.y * scale + oy - total / 2.0;
        let fill = under(region);
        let mut content = String::new();
        for line in &region.lines {
            let own = match &line.id {
                None => region.style.text.clone(),
                Some(id) => style_of(id).text,
            };
            let color = ink(&fill, &own);
            let attrs = match &line.id {
                None => format!(
                    " class=\"pele-label\"{}{}{}",
                    if line.size == size { String::new() } else { format!(" font-size=\"{}\"", js::num_str(line.size)) },
                    color.map_or(String::new(), |c| format!(" fill=\"{}\"", c)),
                    own
                ),
                Some(id) => format!(" class=\"pele-venn-text\" data-id=\"{}\" font-size=\"{}\" fill=\"{}\"{}", esc_text(id), js::num_str(tiny), color.unwrap_or("var(--_m)"), own),
            };
            content.push_str(&label_svg(&line.label, x, y + line.label.height / 2.0, &attrs));
            y += line.label.height;
        }
        labels.push_str(&format!("<g class=\"pele-venn-region\" data-id=\"{}\">{}</g>", id, content));
    }

    let mut body = label_svg(&title, width / 2.0, pad + title.height / 2.0, " class=\"pele-title\" font-weight=\"var(--_tw)\"");
    body.push_str(&format!("<g class=\"pele-venn-sets\">{}</g>", sets));
    if !overlaps.is_empty() {
        body.push_str(&format!("<g class=\"pele-venn-overlaps\">{}</g>", overlaps));
    }
    if !labels.is_empty() {
        body.push_str(&format!("<g class=\"pele-venn-labels\">{}</g>", labels));
    }
    let svg = svg_document("venn", width, height, size, options.responsive, &Accessible::default(), &body);
    Rendered { svg, width, height, links: Vec::new() }
}
