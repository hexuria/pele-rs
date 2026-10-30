// Port of src/diagrams/common/fit.ts.

use crate::js;
use crate::text::label::{layout_label, Label};
use crate::text::measurer::width as measure;

// `<br\s*\/?>` at `i` (case-insensitive): its length in chars.
fn br_at(c: &[char], i: usize) -> Option<usize> {
    if c.get(i) != Some(&'<') || !c.get(i + 1).is_some_and(|x| x.eq_ignore_ascii_case(&'b')) || !c.get(i + 2).is_some_and(|x| x.eq_ignore_ascii_case(&'r')) {
        return None;
    }
    let mut p = i + 3;
    while p < c.len() && js::is_space(c[p]) {
        p += 1;
    }
    if c.get(p) == Some(&'/') {
        p += 1;
    }
    (c.get(p) == Some(&'>')).then_some(p + 1 - i)
}

fn is_w(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Lays out text the way Mermaid's journey and timeline draw it: as plain text in which only
/// <br> is markup, so other tags and icon names show as written instead of being read as formatting.
pub fn plain_label(raw: Option<&str>, size: f64, max: f64, base: u32) -> Label {
    let mut text = raw.map(str::to_string);
    if let Some(t) = &mut text {
        if t.contains('<') {
            // RE_TAG_START /<(?!br\s*\/?>)/gi
            let c: Vec<char> = t.chars().collect();
            let mut out = String::new();
            for i in 0..c.len() {
                if c[i] == '<' && br_at(&c, i).is_none() {
                    out.push_str("&lt;");
                } else {
                    out.push(c[i]);
                }
            }
            *t = out;
        }
        if t.contains(":fa-") {
            *t = t.replace(":fa-", "&colon;fa-");
        }
    }
    layout_label(text.as_deref(), false, size, max, base)
}

// RE_UNIT: /<br\s*\/?>|ﬂ°°?\w+¶ß|&#?\w+;|[\ud800-\udbff][\udc00-\udfff]|[\s\S]/gi, as chars.
fn units(raw: &str) -> Vec<String> {
    let c: Vec<char> = raw.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let mut len = br_at(&c, i);
        if len.is_none() && c[i] == '\u{fb02}' && c.get(i + 1) == Some(&'°') {
            let mut p = i + 2;
            if c.get(p) == Some(&'°') {
                p += 1;
            }
            let s = p;
            while p < c.len() && is_w(c[p]) {
                p += 1;
            }
            if p > s && c.get(p) == Some(&'¶') && c.get(p + 1) == Some(&'ß') {
                len = Some(p + 2 - i);
            }
        }
        if len.is_none() && c[i] == '&' {
            let mut p = i + 1;
            if c.get(p) == Some(&'#') {
                p += 1;
            }
            let s = p;
            while p < c.len() && is_w(c[p]) {
                p += 1;
            }
            if p > s && c.get(p) == Some(&';') {
                len = Some(p + 1 - i);
            }
        }
        let len = len.unwrap_or(1);
        out.push(c[i..i + len].iter().collect());
        i += len;
    }
    out
}

/// layoutLabel wraps at spaces only. For a box of fixed width, this also cuts any word too long to fit.
pub fn fit_label(raw: Option<&str>, size: f64, max: f64, base: u32) -> Label {
    let label = plain_label(raw, size, max, base);
    let Some(raw) = raw else { return label };
    if label.width <= max {
        return label;
    }
    let mut out = String::new();
    let mut width = 0.0;
    for unit in units(raw) {
        let len = js::len16(&unit);
        if js::trim(&unit).is_empty() || (len > 2 && unit.starts_with('<')) {
            width = 0.0;
        } else {
            let w = measure(if len > 2 { "x" } else { &unit }, size, base);
            if width > 0.0 && width + w > max {
                out.push('\n');
                width = 0.0;
            }
            width += w;
        }
        out.push_str(&unit);
    }
    plain_label(Some(&out), size, max, base)
}
