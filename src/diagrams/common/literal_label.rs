// Port of src/diagrams/common/literal-label.ts.

use crate::js;
use crate::svg::builder::{esc, num, shared_style, span_style, text_style};
use crate::text::label::{Label, Span};
use crate::text::measurer::width as measure;

/// Literal labels, including filenames and protocol fields, must retain every character but the
/// spaces a line breaks at. Long words can break between code points; measurement stays bounded
/// by one line of text.
pub fn literal_label(text: &str, size: f64, room: f64, style: u32) -> Label {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let chars: Vec<char> = raw.chars().collect();
        if chars.is_empty() {
            lines.push(String::new());
        }
        let mut from = 0;
        while from < chars.len() {
            let fits = |length: usize| measure(&chars[from..(from + length).min(chars.len())].iter().collect::<String>(), size, style) <= room;
            let mut hi = 1;
            while hi < chars.len() - from && fits(hi) {
                hi = (chars.len() - from).min(hi * 2);
            }
            let mut lo = 1;
            while lo < hi {
                let mid = (lo + hi).div_ceil(2);
                if fits(mid) {
                    lo = mid;
                } else {
                    hi = mid - 1;
                }
            }
            let mut take = lo;
            let wraps = from + take < chars.len();
            if wraps && !js::is_space(chars[from + take]) {
                for i in (1..take).rev() {
                    if js::is_space(chars[from + i]) {
                        take = i + 1;
                        break;
                    }
                }
            }
            let line: String = chars[from..from + take].iter().collect();
            let trimmed = js::trim_end(&line);
            lines.push(if wraps && !trimmed.is_empty() { trimmed.to_string() } else { line });
            from += take;
            while wraps && from < chars.len() && js::is_space(chars[from]) {
                from += 1;
            }
        }
    }
    if text.is_empty() {
        lines.clear();
    }
    let widths: Vec<f64> = lines.iter().map(|l| measure(l, size, style)).collect();
    let width = widths.iter().fold(0f64, |a, &w| a.max(w));
    let line_height = js::round(size * 1.5);
    Label { height: lines.len() as f64 * line_height, lines: lines.into_iter().zip(&widths).map(|(text, &width)| vec![Span { text, style, width }]).collect(), widths, width, size, line_height }
}

pub fn break_label(label: &Label, room: f64) -> Label {
    if label.width <= room {
        return label.clone();
    }
    let mut lines: Vec<Vec<Span>> = Vec::new();
    let mut widths: Vec<f64> = Vec::new();
    for (i, src) in label.lines.iter().enumerate() {
        if label.widths[i] <= room {
            lines.push(src.clone());
            widths.push(label.widths[i]);
            continue;
        }
        let mut current: Vec<Span> = Vec::new();
        let mut width = 0.0;
        for span in src {
            for ch in span.text.chars() {
                let joined = current.last().is_some_and(|l| l.style == span.style);
                let next_width = if joined { measure(&format!("{}{}", current.last().unwrap().text, ch), label.size, span.style) } else { measure(&ch.to_string(), label.size, span.style) };
                let mut added = next_width - if joined { current.last().unwrap().width } else { 0.0 };
                let mut has_last = !current.is_empty();
                if !current.is_empty() && width + added > room {
                    lines.push(std::mem::take(&mut current));
                    widths.push(width);
                    width = 0.0;
                    has_last = false;
                    added = measure(&ch.to_string(), label.size, span.style);
                }
                match current.last_mut() {
                    Some(last) if has_last && last.style == span.style => {
                        last.text.push(ch);
                        last.width += added;
                    }
                    _ => current.push(Span { text: ch.to_string(), style: span.style, width: added }),
                }
                width += added;
            }
        }
        if !current.is_empty() || src.is_empty() {
            lines.push(current);
            widths.push(width);
        }
    }
    let width = widths.iter().fold(0f64, |a, &w| a.max(w));
    Label { height: lines.len() as f64 * label.line_height, lines, widths, width, size: label.size, line_height: label.line_height }
}

/// Writes a label with every line starting at x. A line is anchored at its start and keeps its
/// spaces, so that where it begins does not depend on how its text was measured.
pub fn left_label_svg(label: &Label, x: f64, top: f64, attrs: &str) -> String {
    let shared = shared_style(label);
    let open = format!("<text{}", text_style(attrs, shared));
    let mut body = String::new();
    for (i, spans) in label.lines.iter().enumerate() {
        let cy = top + (i as f64 + 0.5) * label.line_height;
        let mut text = String::new();
        for span in spans {
            let own = span_style(span.style & !shared);
            if own.is_empty() {
                text.push_str(&esc(&span.text));
            } else {
                text.push_str(&format!("<tspan{}>{}</tspan>", own, esc(&span.text)));
            }
        }
        if !text.is_empty() {
            body.push_str(&format!("{} x=\"{}\" y=\"{}\" text-anchor=\"start\" xml:space=\"preserve\">{}</text>", open, num(x), num(cy + label.size * 0.35), text));
        }
    }
    body
}
