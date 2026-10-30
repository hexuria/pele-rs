// Port of src/diagrams/treeview/boxDrawing.ts: Mermaid's box-drawing preprocessor, which turns
// `├──`, `└──`, `│` (and the heavy variants) into the indentation the grammar reads. Lines are
// read as UTF-16 units, as upstream indexes them.

use crate::errors::{locate, PeleError, PeleErrorCode};
use crate::js;
use std::collections::HashMap;

const INDENT_UNIT: &str = "    ";

fn is_box(c: u16) -> bool {
    matches!(c, 0x2500 | 0x2501 | 0x2502 | 0x2503 | 0x2514 | 0x2517 | 0x251c | 0x2523)
}
fn is_branch(c: u16) -> bool {
    matches!(c, 0x2514 | 0x2517 | 0x251c | 0x2523)
}
fn is_dash(c: u16) -> bool {
    matches!(c, 0x2500 | 0x2501)
}
fn is_space(c: u16) -> bool {
    js::is_space_u(c as u32)
}
fn is_vertical(c: u16) -> bool {
    matches!(c, 0x2502 | 0x2503)
}

pub type Columns = HashMap<usize, Box<dyn Fn(usize) -> usize>>;

pub struct PreprocessResult {
    pub text: Vec<u16>,
    // Output line number to original line number, both from 1. Empty when the text is unchanged.
    pub line_map: HashMap<usize, usize>,
    // Per output line, maps rewritten indices to original indices.
    pub columns: Columns,
}

fn has_box(line: &[u16]) -> bool {
    line.iter().any(|&c| is_box(c))
}

// /^[\s│┃]+$/
fn decoration_only(line: &[u16]) -> bool {
    !line.is_empty() && line.iter().all(|&c| is_space(c) || is_vertical(c))
}

// /^[\s─━│┃└┗├┣]+$/
fn box_only(line: &[u16]) -> bool {
    !line.is_empty() && line.iter().all(|&c| is_space(c) || is_box(c))
}

fn starts(line: &[u16], p: usize, word: &str) -> bool {
    word.encode_utf16().enumerate().all(|(i, u)| line.get(p + i) == Some(&u))
}

fn lead(line: &[u16]) -> usize {
    line.iter().take_while(|&&c| is_space(c)).count()
}

// /^\s*(title[\t ]|accTitle[\t ]*:|accDescr[\t ]*[:{])/
fn metadata_line(line: &[u16]) -> bool {
    let p = lead(line);
    let blanks = |mut q: usize| {
        while matches!(line.get(q), Some(9 | 32)) {
            q += 1;
        }
        q
    };
    if starts(line, p, "title") && matches!(line.get(p + 5), Some(9 | 32)) {
        return true;
    }
    if starts(line, p, "accTitle") && line.get(blanks(p + 8)) == Some(&58) {
        return true;
    }
    starts(line, p, "accDescr") && matches!(line.get(blanks(p + 8)), Some(58 | 123))
}

// /^\s*%%/
fn comment_line(line: &[u16]) -> bool {
    starts(line, lead(line), "%%")
}

fn trim(line: &[u16]) -> String {
    js::trim(&js::from16(line)).to_string()
}

fn expand_tabs(line: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(line.len());
    for &c in line {
        if c == 9 {
            out.extend_from_slice(&[32, 32, 32, 32]);
        } else {
            out.push(c);
        }
    }
    out
}

fn infer_segment_width(content_lines: &[Vec<u16>]) -> usize {
    for line in content_lines {
        if let Some(index) = line.iter().position(|&c| is_branch(c)) {
            if index > 0 {
                return index;
            }
        }
    }
    4
}

/// `message.replace(/\bline\s+(\d+)\b/gi, …)` with the lines in `line_map`.
pub fn remap_error_lines(message: &str, line_map: &HashMap<usize, usize>) -> String {
    let c: Vec<char> = message.chars().collect();
    let word = |i: usize| i < c.len() && js::is_word(c[i]);
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        let at_word_start = !(i > 0 && word(i - 1));
        if at_word_start && i + 4 <= c.len() && c[i..i + 4].iter().collect::<String>().eq_ignore_ascii_case("line") {
            let mut j = i + 4;
            let s = j;
            while j < c.len() && js::is_space(c[j]) {
                j += 1;
            }
            let d = j;
            while j < c.len() && c[j].is_ascii_digit() {
                j += 1;
            }
            // \d+ backtracks to a word boundary; digits followed by a word char cannot end one.
            if j > s && d > s && j > d && !word(j) {
                let n: usize = c[d..j].iter().collect::<String>().parse().unwrap_or(0);
                match line_map.get(&n).filter(|&&o| o != 0) {
                    Some(o) => out.push_str(&format!("line {}", o)),
                    None => out.extend(&c[i..j]),
                }
                i = j;
                continue;
            }
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

pub fn line_start(text: &[u16], line: usize) -> usize {
    let mut at = 0;
    for _ in 1..line {
        at = text[at.min(text.len())..].iter().position(|&c| c == 10).map_or(0, |k| at + k + 1);
    }
    at
}

fn fail(input: &[u16], index: usize, message: &str) -> PeleError {
    let line = locate(input, line_start(input, index + 1)).line;
    let mut e = PeleError::typed(format!("Line {}: {}", line, message), PeleErrorCode::Syntax, "treeView");
    e.line = line;
    e
}

// Undo four-space tab expansion.
fn before_tabs(line: &[u16], at: usize) -> usize {
    let mut i = 0;
    let mut n = 0;
    while i < line.len() && n < at {
        n += if line[i] == 9 { 4 } else { 1 };
        i += 1;
    }
    i
}

pub fn preprocess_box_drawing(input: &[u16]) -> Result<PreprocessResult, PeleError> {
    let mut line_map: HashMap<usize, usize> = HashMap::new();
    let mut columns: Columns = HashMap::new();
    let unchanged = |line_map, columns| Ok(PreprocessResult { text: input.to_vec(), line_map, columns });
    if !has_box(input) {
        return unchanged(line_map, columns);
    }
    let lines: Vec<&[u16]> = input.split(|&c| c == 10).collect();
    let Some(keyword_idx) = lines.iter().position(|l| trim(l) == "treeView-beta") else { return unchanged(line_map, columns) };
    let passes = |line: &[u16]| trim(line).is_empty() || comment_line(line) || metadata_line(line);

    let mut content_lines: Vec<Vec<u16>> = Vec::new();
    for line in &lines[keyword_idx + 1..] {
        if passes(line) || decoration_only(line) {
            continue;
        }
        content_lines.push(expand_tabs(line));
    }
    if !content_lines.iter().any(|l| has_box(l)) {
        return unchanged(line_map, columns);
    }

    let segment_width = infer_segment_width(&content_lines);
    let mut out: Vec<Vec<u16>> = Vec::new();
    for (i, line) in lines.iter().enumerate().take(keyword_idx + 1) {
        out.push(line.to_vec());
        line_map.insert(out.len(), i + 1);
    }
    for (i, &line) in lines.iter().enumerate().skip(keyword_idx + 1) {
        if passes(line) {
            out.push(line.to_vec());
            line_map.insert(out.len(), i + 1);
            continue;
        }
        if decoration_only(line) {
            continue;
        }
        let normalized = expand_tabs(line);
        if let Some(index) = normalized.iter().position(|&c| is_branch(c)) {
            let depth = js::round(index as f64 / segment_width as f64) as usize + 1;
            let mut pos = index + 1;
            while pos < normalized.len() && is_dash(normalized[pos]) {
                pos += 1;
            }
            while pos < normalized.len() && normalized[pos] == 32 {
                pos += 1;
            }
            let mut end = normalized.len();
            while end > pos && is_space(normalized[end - 1]) {
                end -= 1;
            }
            let content = &normalized[pos..end];
            if content.is_empty() {
                return Err(fail(input, i, "Empty node — expected a filename or directory name after the box-drawing prefix"));
            }
            let indent = INDENT_UNIT.repeat(depth);
            let indent_len = indent.len();
            let mut text = js::u16s(&indent);
            text.extend_from_slice(content);
            out.push(text);
            line_map.insert(out.len(), i + 1);
            let original = line.to_vec();
            columns.insert(out.len(), Box::new(move |at| before_tabs(&original, pos + at.saturating_sub(indent_len))));
        } else if box_only(&normalized) {
            continue;
        } else if has_box(&normalized) {
            // A name that happens to contain a box character, such as "Section ─ A.txt", is a root item.
            out.push(line.to_vec());
            line_map.insert(out.len(), i + 1);
        } else if normalized.first().is_some_and(|&c| is_space(c)) {
            return Err(fail(input, i, "Unexpected indentation without box-drawing characters. In box-drawing format, use ├── or └── prefixes for indented nodes."));
        } else {
            out.push(line.to_vec());
            line_map.insert(out.len(), i + 1);
        }
    }
    Ok(PreprocessResult { text: out.join(&10u16), line_map, columns })
}
