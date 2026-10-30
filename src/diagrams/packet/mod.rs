// Port of src/diagrams/packet/index.ts and model.ts.

pub mod parser;
pub mod render;

use crate::diagrams::info::newline_dedent;
use crate::diagrams::Diagram;
use crate::errors::{PeleError, PeleErrorCode};
use crate::js::{self, num_str};
use crate::preprocess::Config;
use crate::util::yaml::Value;
use parser::PacketAst;

#[derive(Clone)]
pub struct PacketBlock {
    pub start: f64,
    pub end: f64,
    // As Mermaid counts it: one less than the width for the pieces of a field split across rows.
    pub bits: f64,
    pub label: String,
}

pub struct PacketModel {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub bits_per_row: f64,
    pub rows: Vec<Vec<PacketBlock>>,
}

const MAX_ROWS: usize = 10_000;

pub fn packet_option(config: &Config, key: &str, fallback: f64, min: f64, max: f64) -> f64 {
    match config.get("packet").and_then(|v| v.as_map()).and_then(|m| m.get(key)) {
        Some(Value::Num(v)) if *v >= min => v.min(max),
        _ => fallback,
    }
}

fn invalid(message: String) -> PeleError {
    PeleError::typed(message, PeleErrorCode::Semantic, "packet")
}

pub fn build_packet(ast: PacketAst, config: &Config, title: Option<&str>) -> Result<PacketModel, PeleError> {
    let bits_per_row = packet_option(config, "bitsPerRow", 32.0, 1.0, 4096.0).floor();
    let mut rows: Vec<Vec<PacketBlock>> = Vec::new();
    let mut last_bit = -1.0;
    let mut word: Vec<PacketBlock> = Vec::new();
    let mut row = 1.0;

    for block in ast.blocks {
        let (start, end, bits) = (block.start, block.end, block.bits);
        if let (Some(s), Some(e)) = (start, end) {
            if e < s {
                return Err(invalid(format!("Packet block {} - {} is invalid. End must be greater than start.", num_str(s), num_str(e))));
            }
        }
        let mut start = start.unwrap_or(last_bit + 1.0);
        if start != last_bit + 1.0 {
            return Err(invalid(format!("Packet block {} - {} is not contiguous. It should start from {}.", num_str(start), num_str(end.unwrap_or(start)), num_str(last_bit + 1.0))));
        }
        if bits == Some(0.0) {
            return Err(invalid(format!("Packet block {} is invalid. Cannot have a zero bit field.", num_str(start))));
        }
        let end = end.unwrap_or(start + bits.unwrap_or(1.0) - 1.0);
        let mut bits = bits.unwrap_or(end - start + 1.0);
        last_bit = end;

        while word.len() as f64 <= bits_per_row + 1.0 && rows.len() < MAX_ROWS {
            let row_end = row * bits_per_row - 1.0;
            if end <= row_end {
                word.push(PacketBlock { start, end, bits, label: block.label.clone() });
                if end == row_end {
                    rows.push(std::mem::take(&mut word));
                    row += 1.0;
                }
                break;
            }
            word.push(PacketBlock { start, end: row_end, bits: row_end - start, label: block.label.clone() });
            rows.push(std::mem::take(&mut word));
            row += 1.0;
            start = row_end + 1.0;
            bits = end - start;
        }
    }
    if !word.is_empty() {
        rows.push(word);
    }
    Ok(PacketModel {
        title: ast.title.filter(|t| !t.is_empty()).or(title.map(str::to_string)),
        acc_title: ast.acc_title.filter(|t| !t.is_empty()).map(|t| js::trim_start(&t).to_string()),
        acc_descr: ast.acc_descr.filter(|t| !t.is_empty()).map(|d| newline_dedent(&d)),
        bits_per_row,
        rows,
    })
}

pub static DIAGRAM: Diagram = Diagram {
    type_: "packet",
    section: None,
    keep_comments: false,
    parse: |source, config, title, _line_offset| Ok(Box::new(build_packet(parser::parse_packet(source)?, config, title)?)),
    render: |model, config, options| Ok(render::render_packet(model.downcast_ref::<PacketModel>().unwrap(), config, options)),
};
