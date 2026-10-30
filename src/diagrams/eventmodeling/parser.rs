// Port of src/diagrams/eventmodeling/parser.ts.

use crate::diagrams::common::acc_descr::acc_descr;
use crate::diagrams::common::tokens::{acc_title, directive, keyword, line_or_space, newline, single_line_comment, title_before, tokenize, yaml, Reader, TokenType};
use crate::errors::PeleError;
use crate::js;
use crate::util::chars::is_word;

fn at(src: &[u16], i: usize) -> u32 {
    src.get(i).map_or(u32::MAX, |&c| c as u32)
}

fn is_lt(c: u32) -> bool {
    c == 10 || c == 13 || c == 0x2028 || c == 0x2029
}

const WORDS: [&str; 23] = [
    "resetframe",
    "readmodel",
    "processor",
    "timeframe",
    "command",
    "entity",
    "event",
    "jsobj",
    "figma",
    "given",
    "json",
    "salt",
    "html",
    "text",
    "data",
    "note",
    "when",
    "then",
    "rmo",
    "cmd",
    "evt",
    "pcr",
    "uri",
];

// /\{(.*)\}|"(.*)"|'(.*)'/y: `.` stops at a line terminator, and the greedy run backs off to
// the last closer on the line.
fn inline(src: &[u16], p: usize) -> Option<usize> {
    let open = at(src, p);
    let close = match open {
        123 => 125,
        34 => 34,
        39 => 39,
        _ => return None,
    };
    let mut end = p + 1;
    while end < src.len() && !is_lt(src[end] as u32) {
        end += 1;
    }
    (p + 1..end).rev().find(|&k| src[k] as u32 == close).map(|k| k + 1)
}

// /\{[\t ]*\r?\n(?:[\S\s]*?\r?\n)?\}(?:\r?\n|(?!\S))/y
fn block(src: &[u16], p: usize) -> Option<usize> {
    if at(src, p) != 123 {
        return None;
    }
    let mut q = p + 1;
    while matches!(at(src, q), 9 | 32) {
        q += 1;
    }
    let q = newline(src, q)?;
    let close = |k: usize| if at(src, k) == 125 { line_or_space(src, k + 1) } else { None };
    for k in q..src.len() {
        if let Some(after) = newline(src, k) {
            if let Some(end) = close(after) {
                return Some(end);
            }
        }
    }
    close(q)
}

pub fn tokens() -> Vec<TokenType> {
    let mut types: Vec<TokenType> = Vec::new();
    types.push(keyword("eventmodeling"));
    let mut longer = vec![0];
    for w in WORDS {
        longer.push(types.len());
        types.push(TokenType::lit(w, w));
    }
    types.push(TokenType::lit("->>", "->>"));
    for (w, is_word) in [("gwt", true), ("ui", true), ("md", true), ("tf", true), ("[[", false), ("]]", false), ("rf", true), ("`", false), (".", false)] {
        if is_word {
            longer.push(types.len());
        }
        types.push(TokenType::lit(w, w));
    }
    let id = types.len();
    types.push(TokenType::re("EM_ID", None, |src, p| {
        let c = at(src, p);
        if !(c == 95 || (65..=90).contains(&c) || (97..=122).contains(&c)) {
            return None;
        }
        let mut e = p + 1;
        while is_word(at(src, e)) {
            e += 1;
        }
        Some(e)
    }));
    types.push(TokenType::re("EM_FID", None, |src, p| {
        let mut e = p;
        while e < p + 3 && (48..=57).contains(&at(src, e)) {
            e += 1;
        }
        (e > p).then_some(e)
    }));
    types.push(TokenType::re("EM_DATA_INLINE", None, inline));
    types.push(TokenType::re("EM_DATA_BLOCK", None, block));
    types.push(acc_descr("EM_ACC_DESCR"));
    types.push(acc_title().named("EM_ACC_TITLE"));
    types.push(title_before(|src, q| is_word(at(src, q))).named("EM_TITLE"));
    types.push(
        TokenType::re("EM_WS", None, |src, p| {
            let mut e = p;
            while e < src.len() && js::is_space_u(src[e] as u32) {
                e += 1;
            }
            (e > p).then_some(e)
        })
        .hidden(),
    );
    types.push(yaml().named("EM_YAML"));
    types.push(directive().named("EM_DIRECTIVE"));
    types.push(single_line_comment().named("EM_SINGLE_LINE_COMMENT"));
    types.push(
        TokenType::re("EM_ML_COMMENT", None, |src, p| {
            if at(src, p) != 47 || at(src, p + 1) != 42 {
                return None;
            }
            (p + 2..src.len()).find(|&k| at(src, k) == 42 && at(src, k + 1) == 47).map(|k| k + 2)
        })
        .hidden(),
    );
    types.push(
        TokenType::re("EM_SL_COMMENT", None, |src, p| {
            if at(src, p) != 47 || at(src, p + 1) != 47 {
                return None;
            }
            let mut e = p + 2;
            while e < src.len() && src[e] != 10 && src[e] != 13 {
                e += 1;
            }
            Some(e)
        })
        .hidden(),
    );
    // Langium gives every keyword that is also an identifier the identifier as a longer alternative.
    for k in longer {
        types[k].longer = vec![id];
    }
    types
}

fn index(types: &[TokenType], name: &str) -> i32 {
    types.iter().position(|t| t.name == name).unwrap() as i32
}

pub struct EmFrame {
    pub reset: bool,
    pub name: String,
    pub model_entity_type: String,
    pub entity_identifier: String,
    pub source_frames: Vec<String>,
    pub data_reference: Option<String>,
    pub data_type: Option<String>,
    pub data_inline_value: Option<String>,
    // Offset of the frame in the source, for error messages.
    pub offset: usize,
}

pub struct EmDataEntity {
    pub name: String,
    pub data_block_value: String,
}

pub struct EmNoteEntity {
    pub source_frame: String,
    pub data_type: Option<String>,
    pub data_block_value: String,
}

pub struct EmGwt {
    pub source_frame: String,
    pub given: Vec<String>,
    pub when: Vec<String>,
    pub then: Vec<String>,
}

#[derive(Default)]
pub struct EventModelAst {
    // The whole token, keyword included: the grammar's own terminals get no value conversion.
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub model_entities: Vec<String>,
    pub frames: Vec<EmFrame>,
    pub data_entities: Vec<EmDataEntity>,
    pub note_entities: Vec<EmNoteEntity>,
    pub gwt_entities: Vec<EmGwt>,
}

pub fn parse_event_model(src: &[u16]) -> Result<EventModelAst, PeleError> {
    let types = tokens();
    let tokens = tokenize(src, &types, "eventmodeling")?;
    let mut r = Reader::new(&tokens, &types, "eventmodeling");
    let mut ast = EventModelAst::default();
    let ix = |n: &str| index(&types, n);
    let (arrow, open, close, tick, dot, id, frame_id, inline_, block_) = (ix("->>"), ix("[["), ix("]]"), ix("`"), ix("."), ix("EM_ID"), ix("EM_FID"), ix("EM_DATA_INLINE"), ix("EM_DATA_BLOCK"));
    let (acc_descr_, acc_title_, title_) = (ix("EM_ACC_DESCR"), ix("EM_ACC_TITLE"), ix("EM_TITLE"));
    let (entity, data, note, gwt, given, when, then) = (ix("entity"), ix("data"), ix("note"), ix("gwt"), ix("given"), ix("when"), ix("then"));
    let kinds = |names: &[&str]| -> Vec<i32> { names.iter().map(|n| ix(n)).collect() };
    let time_frame = kinds(&["tf", "timeframe"]);
    let reset_frame = kinds(&["rf", "resetframe"]);
    let entity_type = kinds(&["rmo", "readmodel", "ui", "cmd", "command", "evt", "event", "pcr", "processor"]);
    let data_types = kinds(&["json", "jsobj", "figma", "salt", "uri", "md", "html", "text"]);

    let one = |r: &mut Reader, set: &[i32], expected: &str| -> Result<String, PeleError> {
        if set.contains(&r.kind()) {
            Ok(r.take())
        } else {
            Err(r.fail(expected))
        }
    };
    let qualified_name = |r: &mut Reader| -> Result<String, PeleError> {
        let mut name = r.expect(id)?;
        while r.accept(dot) {
            name.push('.');
            name.push_str(&r.expect(id)?);
        }
        Ok(name)
    };
    let data_type = |r: &mut Reader| -> Result<Option<String>, PeleError> {
        if !r.accept(tick) {
            return Ok(None);
        }
        let t = one(r, &data_types, "a data type")?;
        r.expect(tick)?;
        Ok(Some(t))
    };
    let statements = |r: &mut Reader| -> Result<Vec<String>, PeleError> {
        let mut out = Vec::new();
        loop {
            one(r, &entity_type, "an entity type")?;
            out.push(r.expect(id)?);
            if !entity_type.contains(&r.kind()) {
                break;
            }
        }
        Ok(out)
    };

    r.expect(0)?;
    while r.kind() != -1 {
        let kind = r.kind();
        if kind == acc_descr_ {
            ast.acc_descr = Some(r.take());
        } else if kind == acc_title_ {
            ast.acc_title = Some(r.take());
        } else if kind == title_ {
            ast.title = Some(r.take());
        } else if kind == entity {
            r.i += 1;
            let name = qualified_name(&mut r)?;
            ast.model_entities.push(name);
        } else if time_frame.contains(&kind) || reset_frame.contains(&kind) {
            let offset = tokens.starts[r.i];
            r.i += 1;
            let name = r.expect(frame_id)?;
            let model_entity_type = one(&mut r, &entity_type, "an entity type")?;
            let entity_identifier = qualified_name(&mut r)?;
            let mut frame = EmFrame {
                reset: !time_frame.contains(&kind),
                name,
                model_entity_type,
                entity_identifier,
                source_frames: Vec::new(),
                data_reference: None,
                data_type: None,
                data_inline_value: None,
                offset,
            };
            while r.accept(arrow) {
                frame.source_frames.push(r.expect(frame_id)?);
            }
            if r.accept(open) {
                frame.data_reference = Some(r.expect(id)?);
                r.expect(close)?;
            }
            if r.kind() == tick || r.kind() == inline_ {
                frame.data_type = data_type(&mut r)?;
                frame.data_inline_value = Some(r.expect(inline_)?);
            }
            ast.frames.push(frame);
        } else if kind == data {
            r.i += 1;
            let name = r.expect(id)?;
            data_type(&mut r)?;
            let data_block_value = r.expect(block_)?;
            ast.data_entities.push(EmDataEntity { name, data_block_value });
        } else if kind == note {
            r.i += 1;
            let source_frame = r.expect(frame_id)?;
            let data_type = data_type(&mut r)?;
            let data_block_value = r.expect(block_)?;
            ast.note_entities.push(EmNoteEntity { source_frame, data_type, data_block_value });
        } else if kind == gwt {
            r.i += 1;
            let source_frame = r.expect(frame_id)?;
            r.expect(given)?;
            let given = statements(&mut r)?;
            let when = if r.accept(when) { statements(&mut r)? } else { Vec::new() };
            r.expect(then)?;
            let then = statements(&mut r)?;
            ast.gwt_entities.push(EmGwt { source_frame, given, when, then });
        } else {
            return Err(r.fail("a frame, an entity, a data block, a note, or a specification"));
        }
    }
    Ok(ast)
}
