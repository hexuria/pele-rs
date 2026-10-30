// Port of src/diagrams/info/render.ts.

use super::InfoModel;
use crate::svg::builder::label_svg;
use crate::svg::root::{svg_document, Accessible};
use crate::text::label::layout_label;
use crate::text::measurer::style;
use crate::types::{RenderOptions, Rendered};

// Upstream's package.json version at the pinned commit.
const VERSION: &str = "1.12.0";

pub fn render_info(model: &InfoModel, options: &RenderOptions) -> Rendered {
    let size = 16.0;
    let pad = 8.0;
    let title = layout_label(model.title.as_deref(), false, size, 4000.0, style::BOLD);
    let title_height = if title.height > 0.0 { title.height + 8.0 } else { 0.0 };
    let text = layout_label(Some(&format!("Pele v{}", VERSION)), false, size, 4000.0, 0);
    let width = (title.width.max(text.width) + 2.0 * pad).ceil();
    let height = (title_height + text.height + 2.0 * pad).ceil();
    let content = label_svg(&title, width / 2.0, pad + title.height / 2.0, " class=\"pele-title\" font-weight=\"var(--_tw)\"")
        + &label_svg(&text, width / 2.0, pad + title_height + text.height / 2.0, " class=\"pele-version\" fill=\"var(--_m)\"");
    let acc = Accessible { acc_title: model.acc_title.as_deref(), acc_descr: model.acc_descr.as_deref() };
    Rendered { svg: svg_document("info", width, height, size, options.responsive, &acc, &content), width, height, links: Vec::new() }
}
