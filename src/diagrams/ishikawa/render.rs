// Port of src/diagrams/ishikawa/render.ts. With no `maxWidth` the diagram always runs across,
// so `verticalCauses` is not ported (see common/fit_width.rs).

use super::{IshikawaDb, IshikawaNode};
use crate::diagrams::common::fit_width::turn_to_fit;
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc_text, label_svg, num};
use crate::svg::edges::{marker, ARROW};
use crate::svg::root::{svg_document, Accessible};
use crate::svg::theme::ROUNDED;
use crate::text::label::{layout_label, Label};
use crate::text::measurer::style;
use crate::types::{RenderOptions, Rendered};

// How far a bone moves toward the tail for each unit it rises from the spine.
const SLANT: f64 = 0.5;
const FIRST_ROW: f64 = 14.0;
const ROW_GAP: f64 = 6.0;
const BONE_END: f64 = 8.0;
const TWIG_STEP: f64 = 22.0;
const STUB: f64 = 18.0;
const TEXT_GAP: f64 = 5.0;
const PAIR_GAP: f64 = 26.0;
const HEAD_GAP: f64 = 34.0;
const TAIL: f64 = 28.0;
const BOX_PAD_X: f64 = 10.0;
const BOX_PAD_Y: f64 = 5.0;

fn length() -> f64 {
    SLANT.hypot(1.0)
}

// A cause below a category. Bones at even depths run level, from the slanted bone above them
// toward the tail; bones at odd depths slant away from the spine, from the level bone above them.
struct Bone {
    label: Label,
    depth: usize,
    parent: Option<usize>,
    // Its place among its siblings, counted from the spine outward.
    slot: usize,
    kids: usize,
    // How far its row is from the spine.
    rise: f64,
    // Where it meets its parent, measured level from the category's bone: 0 on it, negative toward the tail.
    at: f64,
    // Where its text ends.
    text_end: f64,
}

struct Category<'a> {
    node: &'a IshikawaNode,
    label: Label,
    box_w: f64,
    box_h: f64,
    bones: Vec<Bone>,
    // The length of its bone, as distance from the spine.
    rise: f64,
    // How far its causes reach toward the tail, measured level from its bone.
    reach: f64,
}

struct Frame<'a> {
    node: &'a IshikawaNode,
    depth: usize,
    parent: Option<usize>,
    slot: usize,
    // Set when the bone was made and only its row is still to come.
    bone: Option<usize>,
}

// Gives every cause of a category a row of its own, so that no two texts can meet, and measures
// how far the whole of it reaches. A level bone takes the row nearest the spine and its twigs the
// rows beyond; a slanted twig takes the row beyond everything that hangs off it.
fn arrange(node: &IshikawaNode, up: bool, size: f64, small: f64) -> Category<'_> {
    let label = layout_label(Some(&node.text), false, size, 180.0, 0);
    let line_height = js::round(small * 1.5);
    let mut bones: Vec<Bone> = Vec::new();
    let mut rows: Vec<usize> = Vec::new();
    let mut stack: Vec<Frame> = Vec::new();
    // Above the spine the first cause goes farthest out, so that the causes read from the top down.
    fn push<'a>(stack: &mut Vec<Frame<'a>>, children: &'a [IshikawaNode], up: bool, depth: usize, parent: Option<usize>) {
        let n = children.len();
        for slot in (0..n).rev() {
            stack.push(Frame { node: &children[if up { n - 1 - slot } else { slot }], depth, parent, slot, bone: None });
        }
    }
    push(&mut stack, &node.children, up, 2, None);
    while let Some(mut frame) = stack.pop() {
        if let Some(b) = frame.bone {
            rows.push(b);
            continue;
        }
        let index = bones.len();
        bones.push(Bone {
            label: layout_label(Some(&frame.node.text), false, small, 200.0, 0),
            depth: frame.depth,
            parent: frame.parent,
            slot: frame.slot,
            kids: frame.node.children.len(),
            rise: 0.0,
            at: 0.0,
            text_end: 0.0,
        });
        let node = frame.node;
        let depth = frame.depth;
        if frame.depth % 2 == 0 {
            rows.push(index);
        } else {
            frame.bone = Some(index);
            stack.push(frame);
        }
        push(&mut stack, &node.children, up, depth + 1, Some(index));
    }

    let mut rise = FIRST_ROW;
    for &index in &rows {
        let height = bones[index].label.height.max(line_height) + ROW_GAP;
        bones[index].rise = rise + height / 2.0;
        rise += height;
    }
    let box_w = label.width + 2.0 * BOX_PAD_X;
    let mut reach = box_w / 2.0;
    for i in 0..bones.len() {
        let parent = bones[i].parent.map(|p| (bones[p].at, bones[p].kids));
        let bone = &mut bones[i];
        if bone.depth % 2 == 0 {
            bone.at = parent.map_or(0.0, |p| p.0);
            bone.text_end = bone.at - (if bone.kids > 0 { (bone.kids + 1) as f64 * TWIG_STEP } else { STUB }) - TEXT_GAP;
        } else {
            // Twigs stand along their level bone, the one nearest the spine nearest the tail.
            let (pat, pkids) = parent.unwrap();
            bone.at = pat - (pkids - bone.slot) as f64 * TWIG_STEP;
            bone.text_end = bone.at - TEXT_GAP;
        }
        reach = reach.max(bone.label.width - bone.text_end);
    }
    Category { node, box_h: label.height + 2.0 * BOX_PAD_Y, label, box_w, bones, rise: rise + BONE_END, reach }
}

pub fn render_ishikawa(model: &IshikawaDb, config: &Config, options: &RenderOptions) -> Rendered {
    turn_to_fit(options, |_down| draw(model, config, options))
}

fn draw(model: &IshikawaDb, _config: &Config, options: &RenderOptions) -> Rendered {
    let size = 16.0;
    let small = js::round(size * 0.875);
    let pad = 8.0;
    let title = layout_label(model.title.as_deref(), false, size, 4000.0, style::BOLD);
    let title_height = if title.height > 0.0 { title.height + 12.0 } else { 0.0 };
    let len = length();

    let mut body = String::new();
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (0f64, 0f64, 0f64, 0f64);
    if let Some(root) = &model.root {
        let head = layout_label(Some(&root.text), false, size, 200.0, style::BOLD);
        let head_h = (head.height + 24.0).max(48.0);
        let head_w = head.width + 24.0 + head_h / 2.0;
        let mut categories: Vec<Category> = root.children.iter().enumerate().map(|(i, node)| arrange(node, i % 2 == 0, size, small)).collect();
        // Even a category without causes has a bone long enough to carry its name clear of the head.
        for category in categories.iter_mut() {
            category.rise = category.rise.max(head_h / 2.0 + 14.0);
        }

        // Categories meet the spine in pairs, one above and one below, each pair as far toward the
        // tail from the one before as the wider of the two needs.
        let mut xs: Vec<f64> = Vec::new();
        let mut x = -HEAD_GAP;
        let mut i = 0;
        while i < categories.len() {
            if i > 0 {
                let mut room = 0f64;
                for side in 0..2 {
                    let before = categories.get(i - 2 + side);
                    let next = categories.get(i + side);
                    if let Some(before) = before {
                        room = room.max(before.reach + next.map_or(0.0, |n| n.box_w / 2.0));
                    }
                }
                x -= room + PAIR_GAP;
            }
            xs.push(x);
            xs.push(x);
            i += 2;
        }
        let tail = if !categories.is_empty() { x - TAIL } else { -HEAD_GAP };

        max_x = head_w;
        min_x = tail;
        min_y = -head_h / 2.0;
        max_y = head_h / 2.0;
        let mut groups = String::new();
        for (i, category) in categories.iter().enumerate() {
            let sign = if i % 2 == 0 { -1.0 } else { 1.0 };
            let ax = xs[i];
            let px = |at: f64, rise: f64| ax + at - rise * SLANT;
            let end_x = px(0.0, category.rise);
            let end_y = sign * category.rise;
            // The bone stops short of the spine to leave its arrowhead a clean point.
            let trim = ARROW - 1.0;
            let mut d = format!("M{},{}L{},{}", num(ax - (SLANT / len) * trim), num((sign * trim) / len), num(end_x), num(end_y));
            let mut texts = String::new();
            for bone in &category.bones {
                let y = sign * bone.rise;
                if bone.depth % 2 == 0 {
                    d.push_str(&format!("M{},{}H{}", num(px(bone.at, bone.rise)), num(y), num(px(bone.text_end + TEXT_GAP, bone.rise))));
                } else {
                    let from = category.bones[bone.parent.unwrap()].rise;
                    d.push_str(&format!("M{},{}L{},{}", num(px(bone.at, from)), num(sign * from), num(px(bone.at, bone.rise)), num(y)));
                }
                let right = px(bone.text_end, bone.rise);
                texts.push_str(&label_svg(&bone.label, right - bone.label.width / 2.0, y, if bone.depth > 2 { " fill=\"var(--_m)\"" } else { "" }));
                min_x = min_x.min(right - bone.label.width);
            }
            let box_y = end_y + sign * (category.box_h / 2.0);
            min_x = min_x.min(end_x - category.box_w / 2.0);
            max_x = max_x.max(end_x + category.box_w / 2.0);
            min_y = min_y.min(box_y - category.box_h / 2.0);
            max_y = max_y.max(box_y + category.box_h / 2.0);
            groups.push_str(&format!("<g class=\"pele-ishikawa-category\" data-id=\"{}\">", esc_text(&category.node.text)));
            groups.push_str(&format!("<path class=\"pele-ishikawa-bones\" d=\"{}\" fill=\"none\" stroke=\"var(--_l)\"/>", d));
            groups.push_str(&marker("arrow_point", ax, 0.0, SLANT / len, -sign / len, "var(--_l)", "none"));
            groups.push_str(&format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_s)\" stroke=\"var(--_b)\"{}/>",
                num(end_x - category.box_w / 2.0),
                num(box_y - category.box_h / 2.0),
                num(category.box_w),
                num(category.box_h),
                ROUNDED
            ));
            groups.push_str(&label_svg(&category.label, end_x, box_y, " class=\"pele-label\""));
            if !texts.is_empty() {
                groups.push_str(&format!("<g class=\"pele-ishikawa-causes\" font-size=\"{}\">{}</g>", js::num_str(small), texts));
            }
            groups.push_str("</g>");
        }

        let r = head_h / 2.0;
        body = format!(
            "<path class=\"pele-ishikawa-spine\" d=\"M{},0H{}\" fill=\"none\" stroke=\"var(--_l)\"/>{}{}<g class=\"pele-ishikawa-head\" data-id=\"{}\"><path d=\"M0,{}H{}A{},{} 0 0 1 {},{}H0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\" stroke-linejoin=\"round\"/>{}</g>",
            num(tail),
            num(-(ARROW - 1.0)),
            marker("arrow_point", 0.0, 0.0, 1.0, 0.0, "var(--_l)", "none"),
            groups,
            esc_text(&root.text),
            num(-r),
            num(head_w - r),
            num(r),
            num(r),
            num(head_w - r),
            num(r),
            label_svg(&head, 12.0 + head.width / 2.0, 0.0, " class=\"pele-label\" font-weight=\"var(--_hw)\"")
        );
    }

    let inner = (max_x - min_x).max(title.width);
    let width = (inner + 2.0 * pad).ceil();
    let height = (max_y - min_y + title_height + 2.0 * pad).ceil();
    let ox = pad + (inner - (max_x - min_x)) / 2.0 - min_x;
    let oy = pad + title_height - min_y;
    let mut content = label_svg(&title, width / 2.0, pad + title.height / 2.0, " class=\"pele-title\" font-weight=\"var(--_tw)\"");
    if !body.is_empty() {
        content.push_str(&format!("<g transform=\"translate({},{})\">{}</g>", num(ox), num(oy), body));
    }
    let svg = svg_document("ishikawa", width, height, size, options.responsive, &Accessible::default(), &content);
    Rendered { svg, width, height, links: Vec::new() }
}
