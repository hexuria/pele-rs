// Port of src/svg/title.ts.

use crate::svg::builder::{label_svg, num};
use crate::text::label::{layout_label, Label};
use crate::text::measurer::style;

// Mermaid draws diagram titles as text, whatever they hold.
pub fn title_label(title: Option<&str>, size: f64, max_width: f64) -> Label {
    layout_label(title, false, size, max_width, style::BOLD)
}

pub struct Titled {
    pub width: f64,
    pub height: f64,
    pub content: String,
}

/// Puts a diagram title above content that was laid out without one.
pub fn with_title(title: Option<&str>, size: f64, pad: f64, width: f64, height: f64, content: String) -> Titled {
    let label = title_label(title, size, 4000.0);
    if label.height == 0.0 {
        return Titled { width, height, content };
    }
    let shift = label.height + 12.0;
    let full = width.max((label.width + 2.0 * pad).ceil());
    Titled {
        width: full,
        height: (height + shift).ceil(),
        content: format!(
            "{}<g transform=\"translate({},{})\">{}</g>",
            label_svg(&label, full / 2.0, pad + label.height / 2.0, " class=\"pele-title\" font-weight=\"var(--_tw)\""),
            num((full - width) / 2.0),
            num(shift),
            content
        ),
    }
}
