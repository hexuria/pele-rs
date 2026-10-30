// Port of src/diagrams/packet/render.ts. With no `maxWidth`, `turnToFit` never asks for the
// packet drawn down, so only the across branch is ported.

use super::{packet_option, PacketModel};
use crate::diagrams::common::fit_text::fit_text;
use crate::diagrams::common::fit_width::turn_to_fit;
use crate::js::{self, num_str};
use crate::preprocess::Config;
use crate::svg::builder::{esc, label_svg, num};
use crate::svg::root::{svg_document, Accessible};
use crate::svg::theme::ROUNDED;
use crate::text::entities::decode_entities;
use crate::text::label::layout_label;
use crate::text::measurer::style;
use crate::types::{RenderOptions, Rendered};
use crate::util::yaml::Value;

pub fn render_packet(model: &PacketModel, config: &Config, options: &RenderOptions) -> Rendered {
    turn_to_fit(options, |_down| draw(model, config, options))
}

fn draw(model: &PacketModel, config: &Config, options: &RenderOptions) -> Rendered {
    let size = 16.0;
    let label_size = js::round(size * 0.875);
    let bit_size = js::round(size * 0.625);
    let pad = 8.0;

    let packet = config.get("packet").and_then(|v| v.as_map());
    let row_height = packet_option(config, "rowHeight", 32.0, 1.0, 400.0);
    let bit_width = packet_option(config, "bitWidth", 32.0, 1.0, 400.0);
    let padding_x = packet_option(config, "paddingX", 5.0, 0.0, 400.0);
    let padding_y = packet_option(config, "paddingY", 5.0, 0.0, 400.0);
    let show_bits = !matches!(packet.and_then(|p| p.get("showBits")), Some(Value::Bool(false)));
    let descending = matches!(packet.and_then(|p| p.get("bitOrder")), Some(Value::Str(s)) if s == "descending");
    let bits_per_row = model.bits_per_row;
    let rows = &model.rows;

    // Bit numbers sit above each row and need room of their own.
    let numbers = if show_bits { bit_size + 4.0 } else { 0.0 };
    let mut tops: Vec<f64> = Vec::new();
    let mut rows_height = 0.0;
    for _ in rows {
        tops.push(rows_height);
        rows_height += numbers + row_height + padding_y;
    }
    if !rows.is_empty() {
        rows_height -= padding_y;
    }

    let gap = padding_x.min(bit_width - 1.0);
    let width = if !rows.is_empty() { bits_per_row * bit_width - gap } else { 0.0 };
    let title = layout_label(model.title.as_deref(), false, size, 4000.0, style::BOLD);
    let title_height = if title.height > 0.0 { title.height + if rows_height > 0.0 { 12.0 } else { 0.0 } } else { 0.0 };

    let left = pad + 0f64.max((title.width - width) / 2.0);
    let mut fields = String::new();
    let mut bits = String::new();
    for (index, row) in rows.iter().enumerate() {
        let y = pad + tops[index] + numbers;
        for block in row {
            let count = block.end - block.start + 1.0;
            let first = block.start % bits_per_row;
            let x = left + if descending { bits_per_row - first - count } else { first } * bit_width;
            let w = count * bit_width - gap;
            let text = fit_text(js::trim(&decode_entities(&block.label)), w - 6.0, label_size, js::round(size * 0.5), 0);
            fields.push_str(&format!(
                "<g class=\"pele-node\" data-id=\"{}-{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_s)\" stroke=\"var(--_b)\"{}/>",
                num_str(block.start),
                num_str(block.end),
                num(x),
                num(y),
                num(w),
                num(row_height),
                ROUNDED
            ));
            if let Some(text) = text.filter(|t| t.size <= row_height) {
                fields.push_str(&format!(
                    "<text x=\"{}\" y=\"{}\"{}>{}</text>",
                    num(x + w / 2.0),
                    num(y + row_height / 2.0 + text.size * 0.35),
                    if text.size == label_size { String::new() } else { format!(" font-size=\"{}\"", num(text.size)) },
                    esc(&text.text)
                ));
            }
            fields.push_str("</g>");
            if !show_bits {
                continue;
            }
            let leading = if descending { block.end } else { block.start };
            let trailing = if descending { block.start } else { block.end };
            if count == 1.0 {
                bits.push_str(&format!("<text x=\"{}\" y=\"{}\" text-anchor=\"middle\">{}</text>", num(x + w / 2.0), num(y - 4.0), num_str(leading)));
            } else {
                bits.push_str(&format!(
                    "<text x=\"{}\" y=\"{}\">{}</text><text x=\"{}\" y=\"{}\" text-anchor=\"end\">{}</text>",
                    num(x),
                    num(y - 4.0),
                    num_str(leading),
                    num(x + w),
                    num(y - 4.0),
                    num_str(trailing)
                ));
            }
        }
    }

    let total_width = (width.max(title.width) + 2.0 * pad).ceil();
    let total_height = (rows_height + title_height + 2.0 * pad).ceil();
    let mut content = String::new();
    if !fields.is_empty() {
        content.push_str(&format!("<g class=\"pele-packet-fields\" font-size=\"{}\" text-anchor=\"middle\">{}</g>", label_size, fields));
    }
    if !bits.is_empty() {
        content.push_str(&format!("<g class=\"pele-packet-bits\" font-size=\"{}\" fill=\"var(--_m)\">{}</g>", bit_size, bits));
    }
    // Mermaid puts a packet's title under it.
    content.push_str(&label_svg(&title, total_width / 2.0, total_height - pad - title.height / 2.0, " class=\"pele-title\" font-weight=\"var(--_tw)\""));
    let acc = Accessible { acc_title: model.acc_title.as_deref(), acc_descr: model.acc_descr.as_deref() };
    Rendered { svg: svg_document("packet", total_width, total_height, size, options.responsive, &acc, &content), width: total_width, height: total_height, links: Vec::new() }
}
