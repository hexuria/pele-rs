// Port of src/diagrams/treemap/format.ts: the parts of d3-format's specifier that treemap
// values use: an optional `$`, `,` for thousands separators, a precision, and a type of `f`,
// `%`, `d`, `e` or none. Fill, alignment and width are read and ignored. Anything else falls
// back to the default, `,`.

use crate::js;

struct Spec {
    sign: Option<char>,
    dollar: bool,
    comma: bool,
    precision: Option<usize>,
    trim: bool,
    ty: Option<char>,
}

// /^(?:.?[<>=^])?([-+( ])?(\$)?0?\d*(,)?(?:\.(\d+))?(~)?([a-zA-Z%])?$/
fn rest(c: &[char]) -> Option<Spec> {
    let mut i = 0;
    let mut spec = Spec { sign: None, dollar: false, comma: false, precision: None, trim: false, ty: None };
    if i < c.len() && matches!(c[i], '-' | '+' | '(' | ' ') {
        spec.sign = Some(c[i]);
        i += 1;
    }
    if i < c.len() && c[i] == '$' {
        spec.dollar = true;
        i += 1;
    }
    while i < c.len() && c[i].is_ascii_digit() {
        i += 1;
    }
    if i < c.len() && c[i] == ',' {
        spec.comma = true;
        i += 1;
    }
    if i + 1 < c.len() && c[i] == '.' && c[i + 1].is_ascii_digit() {
        let from = i + 1;
        i += 1;
        while i < c.len() && c[i].is_ascii_digit() {
            i += 1;
        }
        let digits: String = c[from..i].iter().collect();
        spec.precision = Some(js::to_number(&digits).min(20.0) as usize);
    }
    if i < c.len() && c[i] == '~' {
        spec.trim = true;
        i += 1;
    }
    if i < c.len() && (c[i].is_ascii_alphabetic() || c[i] == '%') {
        spec.ty = Some(c[i]);
        i += 1;
    }
    (i == c.len()).then_some(spec)
}

fn parse(spec: &str) -> Option<Spec> {
    let c: Vec<char> = spec.chars().collect();
    let align = |ch: char| matches!(ch, '<' | '>' | '=' | '^');
    if c.len() >= 2 && !matches!(c[0], '\n' | '\r' | '\u{2028}' | '\u{2029}') && align(c[1]) {
        if let Some(s) = rest(&c[2..]) {
            return Some(s);
        }
    }
    if !c.is_empty() && align(c[0]) {
        if let Some(s) = rest(&c[1..]) {
            return Some(s);
        }
    }
    rest(&c)
}

fn group(digits: &str) -> String {
    let d: Vec<char> = digits.chars().collect();
    let mut out = String::new();
    let mut i = d.len() as i64;
    while i > 0 {
        let from = (i - 3).max(0) as usize;
        let piece: String = d[from..i as usize].iter().collect();
        out = if out.is_empty() { piece } else { format!("{},{}", piece, out) };
        i -= 3;
    }
    out
}

pub struct Formatter(Spec);

pub fn formatter(spec: &str) -> Formatter {
    // Mermaid documents `$0,0` for a dollar amount with thousands separators.
    Formatter(parse(if spec == "$0,0" { "$," } else { spec }).unwrap_or_else(|| parse(",").unwrap()))
}

impl Formatter {
    pub fn format(&self, value: f64) -> String {
        let m = &self.0;
        let abs = value.abs();
        let mut suffix = "";
        let mut body = match m.ty {
            Some('f') => js::to_fixed(abs, m.precision.unwrap_or(6)),
            Some('%') => {
                suffix = "%";
                js::to_fixed(abs * 100.0, m.precision.unwrap_or(6))
            }
            Some('d') => js::to_fixed(js::round(abs), 0),
            Some('e') => js::to_exponential(abs, m.precision.unwrap_or(6)),
            _ => js::num_str(js::to_number(&js::to_precision(abs, m.precision.unwrap_or(12).max(1)))),
        };
        if m.trim && body.contains('.') && !body.contains('e') {
            // .replace(/\.?0+$/, '')
            let trimmed = body.trim_end_matches('0');
            if trimmed.len() < body.len() {
                body = trimmed.strip_suffix('.').unwrap_or(trimmed).to_string();
            }
        }
        if m.comma && !body.contains('e') {
            body = match body.find('.') {
                None => group(&body),
                Some(dot) => group(&body[..dot]) + &body[dot..],
            };
        }
        let sign = if value < 0.0 {
            "-"
        } else if m.sign == Some('+') {
            "+"
        } else {
            ""
        };
        format!("{}{}{}{}", sign, if m.dollar { "$" } else { "" }, body, suffix)
    }
}
