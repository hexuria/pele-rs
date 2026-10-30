// Port of src/diagrams/sankey/parser.ts.

use super::lexer::*;
use super::SankeyDb;
use crate::errors::{input_offset, syntax_error, Edit, PeleError};
use crate::js;
use crate::text::entities::decode_entities;

fn is_blank(c: u32) -> bool {
    if c == 10 || c == 13 {
        return false;
    }
    if c < 128 {
        return c == 32 || c == 9 || c == 11 || c == 12;
    }
    c == 0xa0 || c == 0x1680 || (0x2000..=0x200a).contains(&c) || c == 0x2028 || c == 0x2029 || c == 0x202f || c == 0x205f || c == 0x3000 || c == 0xfeff
}

fn at(src: &[u16], i: usize) -> u32 {
    src.get(i).map_or(u32::MAX, |&c| c as u32)
}

fn trim_rows(text: &[u16], edits: &mut Vec<Edit>) -> Vec<u16> {
    let mut end = text.len();
    while end > 0 && is_blank(text[end - 1] as u32) {
        end -= 1;
    }
    let mut out: Vec<u16> = Vec::new();
    let mut kept = 0;
    let mut quoted = false;
    let mut i = 0;
    while i < end {
        let c = text[i] as u32;
        if quoted {
            if c == 34 {
                if at(text, i + 1) == 34 {
                    i += 1;
                } else {
                    quoted = false;
                }
            }
            i += 1;
        } else if c == 34 {
            quoted = true;
            i += 1;
        } else if is_blank(c) {
            let mut j = i + 1;
            while j < text.len() && is_blank(text[j] as u32) {
                j += 1;
            }
            let p = if i == 0 { u32::MAX } else { text[i - 1] as u32 };
            let q = at(text, j);
            // Preserve blank first fields, which Mermaid accepts as empty names.
            if i == 0 || q == 10 || q == 13 || ((p == 10 || p == 13) && q != 44) {
                out.extend_from_slice(&text[kept..i]);
                edits.push((i, j, 0));
                kept = j;
            }
            i = j;
        } else {
            i += 1;
        }
    }
    if end < text.len() {
        edits.push((end, text.len(), 0));
    }
    if kept < end {
        out.extend_from_slice(&text[kept..end]);
    }
    out
}

/// Unlike Mermaid's sankeyUtils, also trims row edges outside quoted fields.
pub fn prepare_text_for_parsing(text: &[u16], steps: Option<&mut Vec<Vec<Edit>>>) -> Vec<u16> {
    let mut edits: Vec<Edit> = Vec::new();
    let rows = trim_rows(text, &mut edits);
    // .replace(/[\n\r]+/g, '\n')
    let mut collapsed: Vec<u16> = Vec::new();
    let mut breaks: Vec<Edit> = Vec::new();
    let mut i = 0;
    while i < rows.len() {
        if rows[i] == 10 || rows[i] == 13 {
            let mut j = i + 1;
            while j < rows.len() && (rows[j] == 10 || rows[j] == 13) {
                j += 1;
            }
            breaks.push((i, j, 1));
            collapsed.push(10);
            i = j;
        } else {
            collapsed.push(rows[i]);
            i += 1;
        }
    }
    let mut s = 0;
    while s < collapsed.len() && js::is_space_u(collapsed[s] as u32) {
        s += 1;
    }
    let mut e = collapsed.len();
    while e > s && js::is_space_u(collapsed[e - 1] as u32) {
        e -= 1;
    }
    if let Some(steps) = steps {
        let lead = s;
        steps.push(edits);
        steps.push(breaks);
        steps.push(vec![(0, lead, 0), (lead + (e - s), collapsed.len(), 0)]);
    }
    collapsed[s..e].to_vec()
}

fn is_invalid_xml(c: char) -> bool {
    matches!(c, '\u{0}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' | '\u{fffe}' | '\u{ffff}')
}

/// Reports errors in `source`, the text before prepare_text_for_parsing.
pub fn parse_sankey(src: &[u16], db: &mut SankeyDb, source: &[u16]) -> Result<(), PeleError> {
    let t = tokenize(src);
    let mut i = 0usize;

    let error = |at: usize, detail: &str| -> PeleError {
        let mut steps: Vec<Vec<Edit>> = Vec::new();
        if source != src {
            prepare_text_for_parsing(source, Some(&mut steps));
        }
        // Map EOF before the trimmed trailing blanks.
        let offset = if at > 0 && at >= src.len() { input_offset(&steps, src.len() - 1) + 1 } else { input_offset(&steps, at) };
        syntax_error("sankey", source, offset, detail, false)
    };
    let fail = |i: usize, expected: &str| -> PeleError {
        let ty = t.types[i];
        let got = if ty == STUCK {
            if i == 0 {
                "undefined"
            } else {
                TOKEN_NAMES[t.types[i - 1] as usize]
            }
        } else {
            TOKEN_NAMES[ty as usize]
        };
        error(t.starts[i], &format!("Expecting {}, got '{}'", expected, got))
    };
    let expect = |i: &mut usize, ty: u32, also: &str| -> Result<(), PeleError> {
        if t.types[*i] != ty {
            return Err(fail(*i, &format!("'{}'{}", TOKEN_NAMES[ty as usize], also)));
        }
        *i += 1;
        Ok(())
    };
    let field = |i: &mut usize| -> Result<String, PeleError> {
        if t.types[*i] == NON_ESCAPED_TEXT {
            *i += 1;
            return Ok(t.texts[*i - 1].clone());
        }
        if t.types[*i] != DQUOTE {
            return Err(fail(*i, "'DQUOTE', 'NON_ESCAPED_TEXT'"));
        }
        *i += 1;
        if t.types[*i] != ESCAPED_TEXT {
            return Err(fail(*i, "'ESCAPED_TEXT'"));
        }
        let text = t.texts[*i].clone();
        *i += 1;
        expect(i, DQUOTE, "")?;
        Ok(text)
    };
    let name = |i: &mut usize| -> Result<String, PeleError> {
        let start = t.starts[*i];
        let text = js::trim(&field(i)?).replace("\"\"", "\"");
        // Entity codes can encode characters excluded by the lexer.
        if decode_entities(&text).chars().any(is_invalid_xml) {
            return Err(error(start, "Invalid XML character in node name"));
        }
        Ok(text)
    };

    expect(&mut i, SANKEY, "")?;
    expect(&mut i, NEWLINE, "")?;
    loop {
        let from = name(&mut i)?;
        expect(&mut i, COMMA, ", 'NEWLINE', 'EOF'")?;
        let to = name(&mut i)?;
        expect(&mut i, COMMA, ", 'NEWLINE', 'EOF'")?;
        let start = t.starts[i];
        // Keep Mermaid's numeric prefixes, but reject non-finite values.
        let value = js::parse_float(&field(&mut i)?);
        if !value.is_finite() {
            return Err(error(start, "Expecting a number"));
        }
        let source = db.find_or_create_node(&from);
        let target = db.find_or_create_node(&to);
        db.add_link(source, target, value);
        if t.types[i] != NEWLINE {
            break;
        }
        i += 1;
    }
    if t.types[i] == EOF {
        i += 1;
    }
    if t.types[i] != END {
        return Err(fail(i, "'NEWLINE', 'EOF'"));
    }
    Ok(())
}
