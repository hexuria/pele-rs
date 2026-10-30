// Port of src/diagrams/common/fit-width.ts. The host gives no `maxWidth`, so every fitting step
// draws once at the chart's own width: these keep upstream's call shape for the renderers.

use crate::types::{RenderOptions, Rendered};

/// Draws a chart whose plot can be any width. With no width limit, the plot keeps its own.
pub fn fit_width(_options: &RenderOptions, natural: f64, _least: f64, mut draw: impl FnMut(f64, bool) -> Rendered) -> Rendered {
    draw(natural, false)
}

/// The width a title may take before it wraps.
pub fn title_room(_options: &RenderOptions) -> f64 {
    4000.0
}

/// For a chart that is drawn to a set width: its own, with no width limit.
pub fn cap_width(_options: &RenderOptions, natural: f64, _least: f64, _around: f64) -> f64 {
    natural
}

/// For a diagram that can run across or down: across, with no width limit.
pub fn turn_to_fit(_options: &RenderOptions, mut draw: impl FnMut(bool) -> Rendered) -> Rendered {
    draw(false)
}

pub fn runs_across(direction: Option<&str>) -> bool {
    matches!(direction, Some("LR") | Some("RL"))
}

/// Draws a diagram with room to spare; with no width limit it always fits.
pub fn tighten(_options: &RenderOptions, mut draw: impl FnMut(bool) -> Rendered) -> Rendered {
    draw(false)
}
