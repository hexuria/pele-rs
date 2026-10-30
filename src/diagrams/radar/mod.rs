// Port of src/diagrams/radar/index.ts and model.ts.

pub mod parser;
pub mod render;

use crate::diagrams::info::newline_dedent;
use crate::diagrams::Diagram;
use crate::errors::{PeleError, PeleErrorCode};
use crate::js;
use parser::{OptionValue, RadarAst};

pub struct RadarAxis {
    pub name: String,
    pub label: String,
}

pub struct RadarCurve {
    pub name: String,
    pub label: String,
    pub entries: Vec<f64>,
}

pub struct RadarOptions {
    pub show_legend: bool,
    pub ticks: f64,
    pub max: Option<f64>,
    pub min: f64,
    pub graticule: String,
}

pub struct RadarModel {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub axes: Vec<RadarAxis>,
    pub curves: Vec<RadarCurve>,
    pub options: RadarOptions,
}

const MAX_TICKS: f64 = 32.0;

fn invalid(message: String) -> PeleError {
    PeleError::typed(message, PeleErrorCode::Semantic, "radar")
}

pub fn build_radar(ast: RadarAst, title: Option<&str>) -> Result<RadarModel, PeleError> {
    let axes: Vec<RadarAxis> = ast.axes.into_iter().map(|a| RadarAxis { label: a.label.unwrap_or_else(|| a.name.clone()), name: a.name }).collect();
    let mut curves = Vec::new();
    for curve in ast.curves {
        let entries: Vec<f64> = if curve.entries[0].0.is_none() {
            curve.entries.iter().map(|e| e.1).collect()
        } else {
            if axes.is_empty() {
                return Err(invalid("Axes must be populated before curves for reference entries".into()));
            }
            let mut by_axis: Vec<(&str, f64)> = Vec::new();
            for (axis, value) in &curve.entries {
                if let Some(a) = axis {
                    if !by_axis.iter().any(|(k, _)| k == a) {
                        by_axis.push((a, *value));
                    }
                }
            }
            let mut out = Vec::new();
            for axis in &axes {
                match by_axis.iter().find(|(k, _)| *k == axis.name) {
                    Some((_, v)) => out.push(*v),
                    None => return Err(invalid(format!("Missing entry for axis {}", axis.label))),
                }
            }
            out
        };
        curves.push(RadarCurve { label: curve.label.unwrap_or_else(|| curve.name.clone()), name: curve.name, entries });
    }
    let mut options = RadarOptions { show_legend: true, ticks: 5.0, max: None, min: 0.0, graticule: "circle".into() };
    for (name, value) in ast.options {
        match (name.as_str(), value) {
            ("showLegend", OptionValue::Bool(b)) => options.show_legend = b,
            ("graticule", OptionValue::Str(s)) => options.graticule = s,
            ("ticks", OptionValue::Num(n)) => options.ticks = n,
            ("max", OptionValue::Num(n)) => options.max = Some(n),
            ("min", OptionValue::Num(n)) => options.min = n,
            _ => {}
        }
    }
    if options.ticks > MAX_TICKS {
        options.ticks = MAX_TICKS;
    }
    Ok(RadarModel {
        title: ast.title.filter(|t| !t.is_empty()).or(title.map(str::to_string)),
        acc_title: ast.acc_title.filter(|t| !t.is_empty()).map(|t| js::trim_start(&t).to_string()),
        acc_descr: ast.acc_descr.filter(|t| !t.is_empty()).map(|d| newline_dedent(&d)),
        axes,
        curves,
        options,
    })
}

pub static DIAGRAM: Diagram = Diagram {
    type_: "radar",
    section: None,
    keep_comments: false,
    parse: |source, _config, title, _line_offset| Ok(Box::new(build_radar(parser::parse_radar(source)?, title)?)),
    render: |model, config, options| Ok(render::render_radar(model.downcast_ref::<RadarModel>().unwrap(), config, options)),
};
