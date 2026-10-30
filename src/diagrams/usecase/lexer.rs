// Port of src/diagrams/usecase/tokens.ts and lexer.ts.

use crate::errors::{written, PeleError, PeleErrorCode};
use crate::js;

// Mermaid's token types, in an order that makes its token categories ranges:
// WORD is USECASE to IDENTIFIER, and LABEL_TEXT is USECASE to LABEL_SYMBOL.
pub mod t {
    pub const EOF: u8 = 0;
    pub const USECASE: u8 = 1;
    pub const ACTOR: u8 = 2;
    pub const SYSTEM_BOUNDARY: u8 = 3;
    pub const END: u8 = 4;
    pub const DIRECTION: u8 = 5;
    pub const TD: u8 = 6;
    pub const TB: u8 = 7;
    pub const BT: u8 = 8;
    pub const LR: u8 = 9;
    pub const RL: u8 = 10;
    pub const NOTE: u8 = 11;
    pub const FOR: u8 = 12;
    pub const JSON: u8 = 13;
    pub const CLASS_DEF: u8 = 14;
    pub const CLASS: u8 = 15;
    pub const STYLE: u8 = 16;
    pub const INCLUDE: u8 = 17;
    pub const EXTEND: u8 = 18;
    pub const TRUE: u8 = 19;
    pub const FALSE: u8 = 20;
    pub const IDENTIFIER: u8 = 21;
    pub const NUMBER: u8 = 22;
    pub const AT: u8 = 23;
    pub const COMMA: u8 = 24;
    pub const COLON: u8 = 25;
    pub const HASH_COLOR: u8 = 26;
    pub const CSS_IDENTIFIER: u8 = 27;
    pub const CSS_ESCAPED_COMMA: u8 = 28;
    pub const DASH: u8 = 29;
    pub const DOT: u8 = 30;
    pub const PERCENT: u8 = 31;
    pub const CSS_PUNCTUATION: u8 = 32;
    pub const LABEL_PUNCTUATION: u8 = 33;
    pub const LABEL_SYMBOL: u8 = 34;
    pub const PLAIN_STRING: u8 = 35;
    pub const MARKDOWN_STRING: u8 = 36;
    pub const UNCLOSED_MARKDOWN_STRING: u8 = 37;
    pub const COMMENT: u8 = 38;
    pub const NEWLINE: u8 = 39;
    pub const ACC_DESCR_BLOCK: u8 = 40;
    pub const ACC_TITLE_LINE: u8 = 41;
    pub const ACC_DESCR_LINE: u8 = 42;
    pub const JSON_DECLARATION_START: u8 = 43;
    pub const JSON_OBJECT_LITERAL: u8 = 44;
    pub const UNCLOSED_JSON_OBJECT_LITERAL: u8 = 45;
    pub const GENERALIZATION: u8 = 46;
    pub const DEPENDENCY_ARROW: u8 = 47;
    pub const STEREOTYPE_START: u8 = 48;
    pub const STEREOTYPE_END: u8 = 49;
    pub const STEREOTYPE_TEXT: u8 = 50;
    pub const UNCLOSED_STEREOTYPE_TEXT: u8 = 51;
    pub const CLASS_SEPARATOR: u8 = 52;
    pub const FORWARD_SOLID: u8 = 53;
    pub const BACKWARD_SOLID: u8 = 54;
    pub const FORWARD_CIRCLE: u8 = 55;
    pub const BACKWARD_CIRCLE: u8 = 56;
    pub const FORWARD_CROSS: u8 = 57;
    pub const BACKWARD_CROSS: u8 = 58;
    pub const MARKERLESS_SOLID: u8 = 59;
    pub const METADATA_START: u8 = 60;
    pub const LBRACE: u8 = 61;
    pub const RBRACE: u8 = 62;
    pub const LBRACKET: u8 = 63;
    pub const RBRACKET: u8 = 64;
    pub const LPAREN: u8 = 65;
    pub const RPAREN: u8 = 66;
}

pub const TOKEN_NAMES: &str = "EOF USECASE ACTOR SYSTEM_BOUNDARY END DIRECTION TD TB BT LR RL NOTE FOR JSON CLASS_DEF CLASS STYLE INCLUDE EXTEND TRUE FALSE IDENTIFIER NUMBER AT COMMA COLON HASH_COLOR CSS_IDENTIFIER CSS_ESCAPED_COMMA DASH DOT PERCENT CSS_PUNCTUATION LABEL_PUNCTUATION LABEL_SYMBOL PLAIN_STRING MARKDOWN_STRING UNCLOSED_MARKDOWN_STRING COMMENT NEWLINE ACC_DESCR_BLOCK ACC_TITLE_LINE ACC_DESCR_LINE JSON_DECLARATION_START JSON_OBJECT_LITERAL UNCLOSED_JSON_OBJECT_LITERAL GENERALIZATION DEPENDENCY_ARROW STEREOTYPE_START STEREOTYPE_END STEREOTYPE_TEXT UNCLOSED_STEREOTYPE_TEXT CLASS_SEPARATOR FORWARD_SOLID BACKWARD_SOLID FORWARD_CIRCLE BACKWARD_CIRCLE FORWARD_CROSS BACKWARD_CROSS MARKERLESS_SOLID METADATA_START LBRACE RBRACE LBRACKET RBRACKET LPAREN RPAREN";

pub fn token_name(k: u8) -> &'static str {
    TOKEN_NAMES.split(' ').nth(k as usize).unwrap()
}

pub fn token_index(name: &str) -> u8 {
    TOKEN_NAMES.split(' ').position(|n| n == name).unwrap() as u8
}

pub struct Tokens {
    pub types: Vec<u8>,
    pub starts: Vec<usize>,
    pub ends: Vec<usize>,
}

const KEYWORD_LENGTH: [usize; 21] = [0, 12, 5, 14, 3, 9, 2, 2, 2, 2, 2, 4, 3, 4, 8, 5, 5, 7, 6, 4, 5];

fn c_at(src: &[u16], i: usize) -> u32 {
    src.get(i).map_or(u32::MAX, |&c| c as u32)
}

fn is_word(c: u32) -> bool {
    crate::util::chars::is_word(c)
}

fn is_punct(c: u32) -> bool {
    c < 128 && "!#$&*+/=?^|~".contains(char::from(c as u8))
}

fn is_blank(c: u32) -> bool {
    c == 32 || c == 9
}

fn is_digit(c: u32) -> bool {
    (48..=57).contains(&c)
}

fn is_letter(c: u32) -> bool {
    (65..=90).contains(&c) || (97..=122).contains(&c)
}

fn is_hex(c: u32) -> bool {
    is_digit(c) || (65..=70).contains(&c) || (97..=102).contains(&c)
}

pub fn starts_with(src: &[u16], at: usize, word: &str) -> bool {
    word.bytes().enumerate().all(|(k, b)| c_at(src, at + k) == b as u32)
}

// Mermaid's INCLUDE and EXTEND are the only keywords matched without regard to case.
fn starts_with_no_case(src: &[u16], at: usize, word: &str) -> bool {
    word.bytes().enumerate().all(|(k, b)| c_at(src, at + k) | 32 == b as u32)
}

// The keyword that is a prefix of the text at `at`, tried in the order of Mermaid's token list.
fn keyword_at(src: &[u16], at: usize, c: u32) -> u8 {
    let s = |w: &str| starts_with(src, at, w);
    match c {
        97 => {
            if s("actor") {
                t::ACTOR
            } else {
                0
            }
        }
        115 => {
            if s("systemBoundary") {
                t::SYSTEM_BOUNDARY
            } else if s("style") {
                t::STYLE
            } else {
                0
            }
        }
        101 => {
            if s("end") {
                t::END
            } else if starts_with_no_case(src, at, "extend") {
                t::EXTEND
            } else {
                0
            }
        }
        69 => {
            if starts_with_no_case(src, at, "extend") {
                t::EXTEND
            } else {
                0
            }
        }
        100 => {
            if s("direction") {
                t::DIRECTION
            } else {
                0
            }
        }
        84 => {
            if s("TD") {
                t::TD
            } else if s("TB") {
                t::TB
            } else {
                0
            }
        }
        66 => {
            if s("BT") {
                t::BT
            } else {
                0
            }
        }
        76 => {
            if s("LR") {
                t::LR
            } else {
                0
            }
        }
        82 => {
            if s("RL") {
                t::RL
            } else {
                0
            }
        }
        110 => {
            if s("note") {
                t::NOTE
            } else {
                0
            }
        }
        102 => {
            if s("for") {
                t::FOR
            } else if s("false") {
                t::FALSE
            } else {
                0
            }
        }
        106 => {
            if s("json") {
                t::JSON
            } else {
                0
            }
        }
        99 => {
            if s("classDef") {
                t::CLASS_DEF
            } else if s("class") {
                t::CLASS
            } else {
                0
            }
        }
        105 | 73 => {
            if starts_with_no_case(src, at, "include") {
                t::INCLUDE
            } else {
                0
            }
        }
        116 => {
            if s("true") {
                t::TRUE
            } else {
                0
            }
        }
        117 => {
            if s("usecase-beta") {
                t::USECASE
            } else {
                0
            }
        }
        _ => 0,
    }
}

pub fn line_column(src: &[u16], offset: usize) -> (usize, usize) {
    let (src, offset) = written(src, offset);
    let mut line = 1;
    let mut line_start = 0;
    let mut i = 0;
    while i < offset {
        let c = c_at(&src, i);
        if c != 10 && c != 13 {
            i += 1;
            continue;
        }
        if c == 13 && i + 1 < offset && c_at(&src, i + 1) == 10 {
            i += 1;
        }
        line += 1;
        line_start = i + 1;
        i += 1;
    }
    (line, offset - line_start + 1)
}

pub fn location_text(src: &[u16], start: usize, end: usize) -> String {
    let (line, column) = line_column(src, start);
    format!("line {}, column {} [{},{})", line, column, written(src, start).1, written(src, end).1)
}

// The end of a quoted string that opens at `at`, or None when the line ends first.
fn string_end(src: &[u16], at: usize, quote: u32) -> Option<usize> {
    for k in at + 1..src.len() {
        let c = src[k] as u32;
        if c == quote {
            return Some(k + 1);
        }
        if c == 10 || c == 13 {
            return None;
        }
    }
    None
}

fn starts_token(src: &[u16], at: usize) -> bool {
    let c = c_at(src, at);
    if c != 34 && c != 39 {
        return true;
    }
    (c == 34 && c_at(src, at + 1) == 96) || string_end(src, at, c).is_some()
}

// Mermaid reports the first character no token starts with, and how many characters its lexer
// skipped before one did.
fn lex_error(src: &[u16], offset: usize, length: usize) -> PeleError {
    let (line, column) = line_column(src, offset);
    let (from, to) = (written(src, offset).1, written(src, offset + length).1);
    let ch = src.get(offset).map_or(String::new(), |&c| js::from16(&[c]));
    let mut e = PeleError::typed(
        format!("Error lexing usecase diagram: unexpected character: ->{}<- at offset: {}, skipped {} characters. at line {}, column {} [{},{})", ch, from, to - from, line, column, from, to),
        PeleErrorCode::Syntax,
        "usecase",
    );
    e.line = line;
    e.column = column;
    e
}

fn find16(src: &[u16], from: usize, pat: &[u16]) -> Option<usize> {
    if from > src.len() {
        return None;
    }
    src[from..].windows(pat.len()).position(|w| w == pat).map(|k| from + k)
}

// Produces the tokens of Mermaid's Chevrotain lexer for use case diagrams: the first pattern in
// its list that matches wins, a keyword gives way to a longer identifier, and `json id@` and `<<`
// switch to modes that read a JSON object and a stereotype.
pub fn tokenize(src: &[u16]) -> Result<Tokens, PeleError> {
    let mut types: Vec<u8> = Vec::new();
    let mut starts: Vec<usize> = Vec::new();
    let mut ends: Vec<usize> = Vec::new();
    let n = src.len();
    let mut i = 0;
    // Whether only blanks came before on this line, where comments and accessibility lines may start.
    let mut line_start = true;
    let mut no_brace = false;

    'outer: while i < n {
        let c = src[i] as u32;
        if c == 32 || c == 9 {
            i += 1;
            continue;
        }
        let mut type_: u8;
        let mut end = i + 1;
        let mut fresh = false;

        if is_word(c) {
            type_ = 0;
            if c == 97 && line_start && starts_with(src, i, "acc") {
                let descr = starts_with(src, i + 3, "Descr");
                if descr || starts_with(src, i + 3, "Title") {
                    let mut j = i + 8;
                    while is_blank(c_at(src, j)) {
                        j += 1;
                    }
                    let next = c_at(src, j);
                    if next == 58 {
                        type_ = if descr { t::ACC_DESCR_LINE } else { t::ACC_TITLE_LINE };
                        end = j + 1;
                        while end < n && src[end] != 10 && src[end] != 13 {
                            end += 1;
                        }
                    } else if next == 123 && descr && !no_brace {
                        match src[(j + 1).min(n)..].iter().position(|&c| c == 125) {
                            None => no_brace = true,
                            Some(k) => {
                                type_ = t::ACC_DESCR_BLOCK;
                                end = j + 1 + k + 1;
                            }
                        }
                    }
                }
            } else if c == 106 && starts_with(src, i, "json") && is_blank(c_at(src, i + 4)) {
                let mut j = i + 5;
                while is_blank(c_at(src, j)) {
                    j += 1;
                }
                let id = j;
                while is_word(c_at(src, j)) {
                    j += 1;
                }
                if j > id {
                    while is_blank(c_at(src, j)) {
                        j += 1;
                    }
                    if c_at(src, j) == 64 {
                        j += 1;
                        while is_blank(c_at(src, j)) {
                            j += 1;
                        }
                        if c_at(src, j) == 123 {
                            types.push(t::JSON_DECLARATION_START);
                            starts.push(i);
                            ends.push(j);
                            let mut depth = 0i64;
                            let mut quoted = false;
                            let mut escaped = false;
                            let mut k = j;
                            while k < n {
                                let ch = src[k];
                                if quoted {
                                    if escaped {
                                        escaped = false;
                                    } else if ch == 92 {
                                        escaped = true;
                                    } else if ch == 34 {
                                        quoted = false;
                                    }
                                } else if ch == 34 {
                                    quoted = true;
                                } else if ch == 123 {
                                    depth += 1;
                                } else if ch == 125 {
                                    depth -= 1;
                                    if depth == 0 {
                                        break;
                                    }
                                }
                                k += 1;
                            }
                            types.push(if k < n { t::JSON_OBJECT_LITERAL } else { t::UNCLOSED_JSON_OBJECT_LITERAL });
                            starts.push(j);
                            i = if k < n { k + 1 } else { n };
                            ends.push(i);
                            line_start = false;
                            continue 'outer;
                        }
                    }
                }
            }
            if type_ == 0 {
                while end < n && is_word(src[end] as u32) {
                    end += 1;
                }
                let keyword = keyword_at(src, i, c);
                if keyword != 0 {
                    // A keyword that is only the start of a longer word is an identifier.
                    if keyword == t::USECASE || end - i == KEYWORD_LENGTH[keyword as usize] {
                        type_ = keyword;
                        end = i + KEYWORD_LENGTH[keyword as usize];
                    } else {
                        type_ = t::IDENTIFIER;
                    }
                } else if (c == 111 || c == 120) && c_at(src, i + 1) == 45 && c_at(src, i + 2) == 45 {
                    type_ = if c == 111 { t::BACKWARD_CIRCLE } else { t::BACKWARD_CROSS };
                    end = i + 3;
                } else if is_digit(c) {
                    // `1mg` is an identifier, and `1.5px` is a number because the identifier stops at the dot.
                    let mut digits = i + 1;
                    while is_digit(c_at(src, digits)) {
                        digits += 1;
                    }
                    if c_at(src, digits) == 46 && is_digit(c_at(src, digits + 1)) {
                        type_ = t::NUMBER;
                        end = digits + 2;
                        while is_digit(c_at(src, end)) {
                            end += 1;
                        }
                        while is_letter(c_at(src, end)) {
                            end += 1;
                        }
                    } else {
                        type_ = t::IDENTIFIER;
                    }
                } else {
                    type_ = t::IDENTIFIER;
                    while c_at(src, end) == 45 && is_word(c_at(src, end + 1)) {
                        type_ = t::CSS_IDENTIFIER;
                        end += 2;
                        while is_word(c_at(src, end)) {
                            end += 1;
                        }
                    }
                }
            }
        } else if c == 10 {
            type_ = t::NEWLINE;
            fresh = true;
        } else if c == 13 {
            type_ = t::NEWLINE;
            fresh = true;
            if c_at(src, end) == 10 {
                end += 1;
            }
        } else if c == 34 || c == 39 {
            if c == 34 && c_at(src, i + 1) == 96 {
                let close = find16(src, i + 2, &[96, 34]);
                type_ = if close.is_none() { t::UNCLOSED_MARKDOWN_STRING } else { t::MARKDOWN_STRING };
                end = close.map_or(n, |k| k + 2);
            } else {
                type_ = t::PLAIN_STRING;
                match string_end(src, i, c) {
                    Some(e) => end = e,
                    None => {
                        let mut resume = i + 1;
                        while resume < n && !starts_token(src, resume) {
                            resume += 1;
                        }
                        return Err(lex_error(src, i, resume - i));
                    }
                }
            }
        } else if c == 37 {
            if line_start && c_at(src, end) == 37 {
                type_ = t::COMMENT;
                while end < n && src[end] != 10 && src[end] != 13 {
                    end += 1;
                }
            } else {
                type_ = t::PERCENT;
            }
        } else if c == 45 {
            if c_at(src, end) != 45 {
                type_ = t::DASH;
            } else {
                while c_at(src, end) == 45 {
                    end += 1;
                }
                let next = c_at(src, end);
                let two = end - i == 2;
                if two && next == 124 && c_at(src, end + 1) == 62 {
                    type_ = t::GENERALIZATION;
                    end += 2;
                } else if next == 62 {
                    type_ = t::FORWARD_SOLID;
                    end += 1;
                } else if two && (next == 111 || next == 120) {
                    type_ = if next == 111 { t::FORWARD_CIRCLE } else { t::FORWARD_CROSS };
                    end += 1;
                } else {
                    type_ = t::MARKERLESS_SOLID;
                }
            }
        } else if c == 46 {
            let next = c_at(src, end);
            if next == 46 && c_at(src, end + 1) == 62 {
                type_ = t::DEPENDENCY_ARROW;
                end += 2;
            } else if is_digit(next) {
                type_ = t::NUMBER;
                while is_digit(c_at(src, end)) {
                    end += 1;
                }
                while is_letter(c_at(src, end)) {
                    end += 1;
                }
            } else {
                type_ = t::DOT;
            }
        } else if c == 60 {
            if c_at(src, end) == 60 {
                types.push(t::STEREOTYPE_START);
                starts.push(i);
                ends.push(i + 2);
                i += 2;
                if i >= n {
                    break;
                }
                if c_at(src, i) == 62 && c_at(src, i + 1) == 62 {
                    type_ = t::STEREOTYPE_END;
                    end = i + 2;
                } else {
                    let mut close = i;
                    let mut found = false;
                    while close < n {
                        let ch = src[close];
                        if ch == 10 || ch == 13 {
                            break;
                        }
                        if ch == 62 && c_at(src, close + 1) == 62 {
                            found = true;
                            break;
                        }
                        close += 1;
                    }
                    if found && !js::trim(&js::from16(&src[i..close])).is_empty() {
                        types.push(t::STEREOTYPE_TEXT);
                        starts.push(i);
                        ends.push(close);
                        type_ = t::STEREOTYPE_END;
                        i = close;
                        end = close + 2;
                    } else {
                        while close < n && src[close] != 10 && src[close] != 13 {
                            close += 1;
                        }
                        if close == i {
                            while close < n && (src[close] == 10 || src[close] == 13) {
                                close += 1;
                            }
                            return Err(lex_error(src, i, close - i));
                        }
                        type_ = t::UNCLOSED_STEREOTYPE_TEXT;
                        end = close;
                    }
                }
            } else if c_at(src, end) == 45 && c_at(src, end + 1) == 45 {
                type_ = t::BACKWARD_SOLID;
                end += 2;
                while c_at(src, end) == 45 {
                    end += 1;
                }
            } else {
                type_ = t::LABEL_PUNCTUATION;
            }
        } else if c == 58 {
            if c_at(src, end) == 58 && c_at(src, end + 1) == 58 {
                type_ = t::CLASS_SEPARATOR;
                end += 2;
            } else {
                type_ = t::COLON;
            }
        } else if c == 64 {
            if c_at(src, end) == 123 {
                type_ = t::METADATA_START;
                end += 1;
            } else {
                type_ = t::AT;
            }
        } else if c == 123 {
            type_ = t::LBRACE;
        } else if c == 125 {
            type_ = t::RBRACE;
        } else if c == 91 {
            type_ = t::LBRACKET;
        } else if c == 93 {
            type_ = t::RBRACKET;
        } else if c == 40 {
            type_ = t::LPAREN;
        } else if c == 41 {
            type_ = t::RPAREN;
        } else if c == 44 {
            type_ = t::COMMA;
        } else if c == 35 {
            while is_hex(c_at(src, end)) {
                end += 1;
            }
            type_ = if end > i + 1 { t::HASH_COLOR } else { t::CSS_PUNCTUATION };
        } else if c == 92 {
            if c_at(src, end) == 44 {
                type_ = t::CSS_ESCAPED_COMMA;
                end += 1;
            } else {
                type_ = t::LABEL_PUNCTUATION;
            }
        } else if c < 127 && c > 32 {
            type_ = if is_punct(c) { t::CSS_PUNCTUATION } else { t::LABEL_PUNCTUATION };
        } else {
            type_ = t::LABEL_SYMBOL;
            while end < n {
                let ch = src[end] as u32;
                if (32..127).contains(&ch) || ch == 9 || ch == 10 || ch == 13 {
                    break;
                }
                end += 1;
            }
        }

        types.push(type_);
        starts.push(i);
        ends.push(end);
        i = end;
        line_start = fresh;
    }

    for _ in 0..3 {
        types.push(t::EOF);
        starts.push(n);
        ends.push(n);
    }
    Ok(Tokens { types, starts, ends })
}
