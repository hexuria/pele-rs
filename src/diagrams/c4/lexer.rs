// Port of src/diagrams/c4/lexer.ts and tokens.ts. Follows the lexer Jison compiles from
// c4Diagram.jison: its rule order, its start conditions, and what it returns once the input runs
// out in each of them. Token text is src[starts[i]..ends[i]].

use crate::util::chars::{is_space, is_word};

pub const END: u32 = 0;
pub const ERROR: u32 = 1;
pub const DIRECTION_TB: u32 = 2;
pub const TITLE: u32 = 6;
pub const ACC_DESCRIPTION: u32 = 7;
pub const ACC_TITLE: u32 = 8;
pub const ACC_TITLE_VALUE: u32 = 9;
pub const ACC_DESCR: u32 = 10;
pub const ACC_DESCR_VALUE: u32 = 11;
pub const ACC_DESCR_MULTILINE_VALUE: u32 = 12;
pub const NEWLINE: u32 = 13;
pub const C4_CONTEXT: u32 = 14;
pub const C4_DEPLOYMENT: u32 = 18;
pub const PERSON: u32 = 19;
pub const SYSTEM_EXT_QUEUE: u32 = 26;
pub const CONTAINER_EXT_QUEUE: u32 = 32;
pub const COMPONENT_EXT_QUEUE: u32 = 38;
pub const ENTERPRISE_BOUNDARY: u32 = 39;
pub const SYSTEM_BOUNDARY: u32 = 40;
pub const CONTAINER_BOUNDARY: u32 = 42;
pub const NODE: u32 = 43;
pub const NODE_R: u32 = 45;
pub const REL: u32 = 46;
pub const REL_INDEX: u32 = 53;
pub const UPDATE_EL_STYLE: u32 = 54;
pub const UPDATE_REL_STYLE: u32 = 55;
pub const UPDATE_LAYOUT_CONFIG: u32 = 56;
pub const EOF_IN_STRUCT: u32 = 57;
pub const ATTRIBUTE_EMPTY: u32 = 58;
pub const STR: u32 = 59;
pub const STR_KEY: u32 = 60;
pub const STR_VALUE: u32 = 61;
pub const LBRACE: u32 = 62;
pub const RBRACE: u32 = 63;
pub const EOF: u32 = 64;

pub const TOKEN_NAMES: [&str; 65] = [
    "$end",
    "INVALID",
    "direction_tb",
    "direction_bt",
    "direction_rl",
    "direction_lr",
    "title",
    "accDescription",
    "acc_title",
    "acc_title_value",
    "acc_descr",
    "acc_descr_value",
    "acc_descr_multiline_value",
    "NEWLINE",
    "C4_CONTEXT",
    "C4_CONTAINER",
    "C4_COMPONENT",
    "C4_DYNAMIC",
    "C4_DEPLOYMENT",
    "PERSON",
    "PERSON_EXT",
    "SYSTEM",
    "SYSTEM_DB",
    "SYSTEM_QUEUE",
    "SYSTEM_EXT",
    "SYSTEM_EXT_DB",
    "SYSTEM_EXT_QUEUE",
    "CONTAINER",
    "CONTAINER_DB",
    "CONTAINER_QUEUE",
    "CONTAINER_EXT",
    "CONTAINER_EXT_DB",
    "CONTAINER_EXT_QUEUE",
    "COMPONENT",
    "COMPONENT_DB",
    "COMPONENT_QUEUE",
    "COMPONENT_EXT",
    "COMPONENT_EXT_DB",
    "COMPONENT_EXT_QUEUE",
    "ENTERPRISE_BOUNDARY",
    "SYSTEM_BOUNDARY",
    "BOUNDARY",
    "CONTAINER_BOUNDARY",
    "NODE",
    "NODE_L",
    "NODE_R",
    "REL",
    "BIREL",
    "REL_U",
    "REL_D",
    "REL_L",
    "REL_R",
    "REL_B",
    "REL_INDEX",
    "UPDATE_EL_STYLE",
    "UPDATE_REL_STYLE",
    "UPDATE_LAYOUT_CONFIG",
    "EOF_IN_STRUCT",
    "ATTRIBUTE_EMPTY",
    "STR",
    "STR_KEY",
    "STR_VALUE",
    "LBRACE",
    "RBRACE",
    "EOF",
];

fn keyword(word: &str) -> Option<u32> {
    Some(match word {
        "C4Context" => 14,
        "C4Container" => 15,
        "C4Component" => 16,
        "C4Dynamic" => 17,
        "C4Deployment" => 18,
        "Person" => 19,
        "Person_Ext" => 20,
        "System" => 21,
        "SystemDb" => 22,
        "SystemQueue" => 23,
        "System_Ext" => 24,
        "SystemDb_Ext" => 25,
        "SystemQueue_Ext" => 26,
        "Container" => 27,
        "ContainerDb" => 28,
        "ContainerQueue" => 29,
        "Container_Ext" => 30,
        "ContainerDb_Ext" => 31,
        "ContainerQueue_Ext" => 32,
        "Component" => 33,
        "ComponentDb" => 34,
        "ComponentQueue" => 35,
        "Component_Ext" => 36,
        "ComponentDb_Ext" => 37,
        "ComponentQueue_Ext" => 38,
        "Enterprise_Boundary" => 39,
        "System_Boundary" => 40,
        "Boundary" => 41,
        "Container_Boundary" => 42,
        "Deployment_Node" | "Node" => 43,
        "Node_L" => 44,
        "Node_R" => 45,
        "Rel" => 46,
        "BiRel" => 47,
        "Rel_Up" | "Rel_U" => 48,
        "Rel_Down" | "Rel_D" => 49,
        "Rel_Left" | "Rel_L" => 50,
        "Rel_Right" | "Rel_R" => 51,
        "Rel_Back" => 52,
        "RelIndex" => 53,
        "UpdateElementStyle" => 54,
        "UpdateRelStyle" => 55,
        "UpdateLayoutConfig" => 56,
        _ => return None,
    })
}

pub struct Tokens {
    pub types: Vec<u32>,
    pub starts: Vec<usize>,
    pub ends: Vec<usize>,
}

const S_INITIAL: u8 = 0;
const S_STRUCT: u8 = 1;
const S_ATTRIBUTE: u8 = 2;
const S_STRING: u8 = 3;
const S_STRING_KV: u8 = 4;
const S_STRING_KV_KEY: u8 = 5;
const S_STRING_KV_VALUE: u8 = 6;
const S_ACC: u8 = 7;

fn is_line_end(c: u16) -> bool {
    matches!(c, 10 | 13 | 0x2028 | 0x2029)
}

fn starts_with(src: &[u16], p: usize, w: &str) -> bool {
    w.encode_utf16().enumerate().all(|(k, u)| src.get(p + k) == Some(&u))
}

// /.*direction\s+XX[^\n]*/y at p: `.` stops at a line terminator and backtracks to the last
// `direction` before it that matches.
fn direction(src: &[u16], p: usize, line_end: usize, xx: &str) -> Option<usize> {
    let mut k = line_end.saturating_sub(9);
    loop {
        if k < p {
            return None;
        }
        if k + 9 <= line_end && starts_with(src, k, "direction") {
            let mut q = k + 9;
            let s = q;
            while q < src.len() && is_space(src[q] as u32) {
                q += 1;
            }
            if q > s && starts_with(src, q, xx) {
                let mut e = q + 2;
                while e < src.len() && src[e] != 10 {
                    e += 1;
                }
                return Some(e);
            }
        }
        if k == 0 {
            return None;
        }
        k -= 1;
    }
}

#[allow(unused_assignments)]
pub fn tokenize(src: &[u16]) -> Tokens {
    let n = src.len();
    let mut out = Tokens { types: Vec::new(), starts: Vec::new(), ends: Vec::new() };
    let mut state = S_INITIAL;
    let mut acc_value = ACC_TITLE_VALUE;
    let mut p = 0usize;
    let mut done = false;
    let mut dir_failed = 0usize;
    macro_rules! emit {
        ($ty:expr, $s:expr, $e:expr) => {{
            let (s, e) = ($s, $e);
            out.types.push($ty);
            out.starts.push(s);
            out.ends.push(e);
            p = e;
        }};
    }
    let at = |i: usize| src.get(i).map_or(u32::MAX, |&c| c as u32);
    let skip_ws = |mut q: usize| {
        while q < n && is_space(src[q] as u32) {
            q += 1;
        }
        q
    };
    let up_to = |ch: u16, from: usize| src[from.min(n)..].iter().position(|&c| c == ch).map_or(n, |k| from + k);
    // `word\s[^#\n;]+`
    let statement = |p: usize, word: &str| -> Option<usize> {
        let q = p + word.len();
        if !starts_with(src, p, word) || !is_space(at(q)) {
            return None;
        }
        let from = q + 1;
        let mut q = from;
        while q < n && !matches!(src[q], 35 | 10 | 59) {
            q += 1;
        }
        (q > from).then_some(q)
    };

    loop {
        // Jison notes the end of input one call before it reports it.
        if done {
            emit!(END, n, n);
            break;
        }
        if p >= n {
            done = true;
        }
        let c = at(p);

        if state == S_INITIAL {
            if p >= n {
                emit!(EOF, n, n);
                continue;
            }
            if p >= dir_failed {
                let line_end = src[p..].iter().position(|&c| is_line_end(c)).map_or(n, |k| p + k);
                let has = src[p..line_end].windows(9).any(|w| w == crate::js::u16s("direction").as_slice());
                if has {
                    let mut matched = false;
                    for (i, xx) in ["TB", "BT", "RL", "LR"].iter().enumerate() {
                        if let Some(e) = direction(src, p, line_end, xx) {
                            emit!(DIRECTION_TB + i as u32, p, e);
                            matched = true;
                            break;
                        }
                    }
                    if matched {
                        continue;
                    }
                }
                // No direction statement starts anywhere in the rest of this line either.
                dir_failed = line_end;
            }
            if c == 116 {
                // /title[^\S\r\n]*(?=[\r\n]|$)\s*/y
                if starts_with(src, p, "title") {
                    let mut q = p + 5;
                    while q < n && is_space(src[q] as u32) && src[q] != 10 && src[q] != 13 {
                        q += 1;
                    }
                    if q >= n || src[q] == 10 || src[q] == 13 {
                        p = skip_ws(q);
                        continue;
                    }
                }
                if let Some(e) = statement(p, "title") {
                    emit!(TITLE, p, e);
                    continue;
                }
            }
            if c == 97 {
                if let Some(e) = statement(p, "accDescription") {
                    emit!(ACC_DESCRIPTION, p, e);
                    continue;
                }
                let title = starts_with(src, p, "accTitle");
                if title || starts_with(src, p, "accDescr") {
                    let q = skip_ws(p + 8);
                    let d = at(q);
                    if d == 58 {
                        emit!(if title { ACC_TITLE } else { ACC_DESCR }, p, skip_ws(q + 1));
                        acc_value = if title { ACC_TITLE_VALUE } else { ACC_DESCR_VALUE };
                        state = S_ACC;
                        continue;
                    }
                    if d == 123 && !title {
                        p = skip_ws(q + 1);
                        state = 8;
                        continue;
                    }
                }
            }
            if c == 37 && at(p + 1) == 37 {
                // A comment takes the line breaks after it, so no NEWLINE follows.
                let mut q = up_to(10, p + 2);
                while q < n && (src[q] == 10 || src[q] == 13) {
                    q += 1;
                }
                p = q;
                continue;
            }
            if is_space(c) {
                let mut q = p;
                let mut last = None;
                while q < n {
                    let d = src[q] as u32;
                    if !is_space(d) {
                        break;
                    }
                    if d == 10 {
                        last = Some(q);
                    }
                    q += 1;
                }
                match last {
                    Some(l) => emit!(NEWLINE, p, l + 1),
                    None => p = q,
                }
                continue;
            }
            if c == 123 {
                emit!(LBRACE, p, p + 1);
                continue;
            }
            if c == 125 {
                emit!(RBRACE, p, p + 1);
                continue;
            }
            if is_word(c) {
                let mut q = p + 1;
                while q < n && is_word(src[q] as u32) {
                    q += 1;
                }
                let ty = if q - p <= 19 { keyword(&String::from_utf16_lossy(&src[p..q])) } else { None };
                if let Some(ty) = ty {
                    if ty > C4_DEPLOYMENT {
                        state = S_STRUCT;
                    }
                    emit!(ty, p, q);
                    continue;
                }
            }
            emit!(ERROR, p, p);
            break;
        }

        if state == S_STRUCT {
            if p >= n {
                emit!(EOF_IN_STRUCT, n, n);
            } else if c == 40 {
                let mut q = p + 1;
                while at(q) == 32 {
                    q += 1;
                }
                if at(q) == 44 {
                    emit!(ATTRIBUTE_EMPTY, p, q + 1);
                } else {
                    p += 1;
                }
                state = S_ATTRIBUTE;
            } else if c == 41 {
                p += 1;
                state = S_INITIAL;
            } else {
                emit!(ERROR, p, p);
                break;
            }
            continue;
        }

        if state == S_ATTRIBUTE {
            if p >= n {
                continue;
            }
            if c == 41 {
                p += 1;
                state = S_INITIAL;
            } else if c == 44 {
                if at(p + 1) == 44 {
                    emit!(ATTRIBUTE_EMPTY, p, p + 2);
                } else {
                    p += 1;
                }
            } else {
                let mut q = p;
                while at(q) == 32 {
                    q += 1;
                }
                let d = at(q);
                if d == 34 {
                    if at(q + 1) == 34 {
                        emit!(ATTRIBUTE_EMPTY, p, q + 2);
                    } else {
                        p = q + 1;
                        state = S_STRING;
                    }
                } else if d == 36 {
                    p = q + 1;
                    state = S_STRING_KV;
                } else {
                    // An unquoted value runs to the next comma, through closing parentheses and line breaks.
                    emit!(STR, p, up_to(44, p));
                }
            }
            continue;
        }

        if state == S_STRING {
            if c == 34 {
                p += 1;
                state = S_ATTRIBUTE;
            } else {
                emit!(STR, p.min(n), up_to(34, p));
            }
            continue;
        }

        if state == S_STRING_KV {
            emit!(STR_KEY, p.min(n), up_to(61, p));
            state = S_STRING_KV_KEY;
            continue;
        }

        if state == S_STRING_KV_KEY {
            if p >= n {
                continue;
            }
            let mut q = p + 1;
            if c == 61 {
                while at(q) == 32 {
                    q += 1;
                }
            }
            if c != 61 || at(q) != 34 {
                emit!(ERROR, p, p);
                break;
            }
            p = q + 1;
            state = S_STRING_KV_VALUE;
            continue;
        }

        if state == S_STRING_KV_VALUE {
            if p >= n {
                continue;
            }
            if c == 34 {
                p += 1;
                state = S_ATTRIBUTE;
            } else {
                emit!(STR_VALUE, p, up_to(34, p));
            }
            continue;
        }

        if state == S_ACC {
            emit!(acc_value, p.min(n), up_to(10, p));
            state = S_INITIAL;
            continue;
        }

        if c == 125 {
            p += 1;
            state = S_INITIAL;
        } else {
            emit!(ACC_DESCR_MULTILINE_VALUE, p.min(n), up_to(125, p));
        }
    }
    out
}
