// Port of src/diagrams/class/markers.ts.

use crate::svg::builder::num;
use crate::svg::edges::{marker, marker_trim};

const TRIANGLE: f64 = 12.0;
const DIAMOND: f64 = 16.0;
const HALF: f64 = 6.0;
const RING: f64 = 5.0;

/// How far the line stops short of the node so that it meets the back of the marker.
pub fn class_marker_trim(type_: &str) -> f64 {
    match type_ {
        "extension" => TRIANGLE,
        "composition" | "aggregation" => DIAMOND,
        "lollipop" => 2.0 * RING,
        "dependency" => marker_trim("arrow_point"),
        _ => 0.0,
    }
}

/// Draws an end marker whose tip is at (x, y), pointing along (dx, dy).
pub fn class_marker(type_: &str, x: f64, y: f64, dx: f64, dy: f64, color: &str) -> String {
    let (nx, ny) = (-dy, dx);
    match type_ {
        "extension" => {
            let (bx, by) = (x - dx * TRIANGLE, y - dy * TRIANGLE);
            format!(
                "<path class=\"pele-marker\" d=\"M{},{}L{},{}L{},{}Z\" stroke-linejoin=\"round\"/>",
                num(x),
                num(y),
                num(bx + nx * HALF),
                num(by + ny * HALF),
                num(bx - nx * HALF),
                num(by - ny * HALF)
            )
        }
        "composition" | "aggregation" => {
            let (mx, my) = (x - (dx * DIAMOND) / 2.0, y - (dy * DIAMOND) / 2.0);
            let half = HALF - 1.0;
            format!(
                "<path class=\"pele-marker\" d=\"M{},{}L{},{}L{},{}L{},{}Z\" stroke-linejoin=\"round\"{}/>",
                num(x),
                num(y),
                num(mx + nx * half),
                num(my + ny * half),
                num(x - dx * DIAMOND),
                num(y - dy * DIAMOND),
                num(mx - nx * half),
                num(my - ny * half),
                if type_ == "composition" { format!(" fill=\"{}\"", color) } else { String::new() }
            )
        }
        "lollipop" => format!("<circle class=\"pele-marker\" cx=\"{}\" cy=\"{}\" r=\"{}\"/>", num(x - dx * RING), num(y - dy * RING), RING),
        "dependency" => marker("arrow_point", x, y, dx, dy, color, "none"),
        _ => String::new(),
    }
}
