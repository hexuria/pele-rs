// Port of src/diagrams/quadrant/db.ts.

use crate::diagrams::info::newline_dedent;
use crate::errors::{PeleError, PeleErrorCode};
use crate::js;

#[derive(Clone, Default)]
pub struct QuadrantText {
    pub text: String,
    pub markdown: bool,
}

#[derive(Clone, Default)]
pub struct PointStyle {
    pub radius: Option<f64>,
    pub color: Option<String>,
    pub stroke_color: Option<String>,
    pub stroke_width: Option<String>,
}

pub struct QuadrantPoint {
    pub x: f64,
    pub y: f64,
    pub text: QuadrantText,
    pub class_name: String,
    pub style: PointStyle,
}

fn invalid(style: &str, value: &str, kind: &str) -> PeleError {
    PeleError::typed(format!("value for {} {} is invalid, please use a valid {}", style, value, kind), PeleErrorCode::Semantic, "quadrantChart")
}

// The first two parts of Mermaid's `style.trim().split(/\s*:\s*/)`, found in one pass.
fn key_value(style: &str) -> (String, String) {
    let s = js::trim(style);
    let Some(colon) = s.find(':') else { return (s.to_string(), "undefined".into()) };
    let rest = js::trim_start(&s[colon + 1..]);
    let value = match rest.find(':') {
        None => rest,
        Some(next) => js::trim_end(&rest[..next]),
    };
    (js::trim_end(&s[..colon]).to_string(), value.to_string())
}

fn is_number(v: &str) -> bool {
    !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit())
}

// /^#?([\dA-Fa-f]{6}|[\dA-Fa-f]{3})$/
fn is_hex(v: &str) -> bool {
    let h = v.strip_prefix('#').unwrap_or(v);
    (h.len() == 6 || h.len() == 3) && h.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_pixels(v: &str) -> bool {
    v.strip_suffix("px").is_some_and(is_number)
}

pub fn parse_styles(styles: &[String]) -> Result<PointStyle, PeleError> {
    let mut out = PointStyle::default();
    for style in styles {
        let (key, value) = key_value(style);
        match key.as_str() {
            "radius" => {
                if !is_number(&value) {
                    return Err(invalid(&key, &value, "number"));
                }
                out.radius = Some(js::parse_int(&value, 10));
            }
            "color" => {
                if !is_hex(&value) {
                    return Err(invalid(&key, &value, "hex code"));
                }
                out.color = Some(value);
            }
            "stroke-color" => {
                if !is_hex(&value) {
                    return Err(invalid(&key, &value, "hex code"));
                }
                out.stroke_color = Some(value);
            }
            "stroke-width" => {
                if !is_pixels(&value) {
                    return Err(invalid(&key, &value, "number of pixels (eg. 10px)"));
                }
                out.stroke_width = Some(value);
            }
            _ => return Err(PeleError::typed(format!("style named {} is not supported.", key), PeleErrorCode::Semantic, "quadrantChart")),
        }
    }
    Ok(out)
}

fn trimmed(label: QuadrantText) -> QuadrantText {
    QuadrantText { text: js::trim(&label.text).to_string(), markdown: label.markdown }
}

#[derive(Default)]
pub struct QuadrantDb {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    // Quadrants 1 to 4: top right, top left, bottom left, bottom right.
    pub quadrants: [Option<QuadrantText>; 4],
    pub x_axis_left: Option<QuadrantText>,
    pub x_axis_right: Option<QuadrantText>,
    pub y_axis_bottom: Option<QuadrantText>,
    pub y_axis_top: Option<QuadrantText>,
    pub points: Vec<QuadrantPoint>,
    // A Map, in insertion order.
    pub classes: Vec<(String, PointStyle)>,
}

impl QuadrantDb {
    pub fn set_quadrant_text(&mut self, k: usize, label: QuadrantText) {
        self.quadrants[k] = Some(trimmed(label));
    }
    pub fn set_x_axis_left_text(&mut self, label: QuadrantText) {
        self.x_axis_left = Some(trimmed(label));
    }
    pub fn set_x_axis_right_text(&mut self, label: QuadrantText) {
        self.x_axis_right = Some(trimmed(label));
    }
    pub fn set_y_axis_bottom_text(&mut self, label: QuadrantText) {
        self.y_axis_bottom = Some(trimmed(label));
    }
    pub fn set_y_axis_top_text(&mut self, label: QuadrantText) {
        self.y_axis_top = Some(trimmed(label));
    }
    pub fn add_point(&mut self, label: QuadrantText, class_name: String, x: &str, y: &str, styles: &[String]) -> Result<(), PeleError> {
        let style = parse_styles(styles)?;
        self.points.push(QuadrantPoint { x: js::to_number(x), y: js::to_number(y), text: trimmed(label), class_name, style });
        Ok(())
    }
    pub fn add_class(&mut self, class_name: String, styles: &[String]) -> Result<(), PeleError> {
        let style = parse_styles(styles)?;
        match self.classes.iter_mut().find(|(k, _)| *k == class_name) {
            Some(slot) => slot.1 = style,
            None => self.classes.push((class_name, style)),
        }
        Ok(())
    }
    pub fn class(&self, name: &str) -> Option<&PointStyle> {
        self.classes.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }
    pub fn set_diagram_title(&mut self, text: &str) {
        self.title = Some(text.to_string());
    }
    pub fn set_acc_title(&mut self, text: &str) {
        self.acc_title = Some(js::trim_start(text).to_string());
    }
    pub fn set_acc_description(&mut self, text: &str) {
        self.acc_descr = Some(newline_dedent(text));
    }
}
