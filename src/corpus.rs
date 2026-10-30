//! The inputs the port is tested on, for downstream crates that test their own drawing of
//! Pele's output (feature `corpus`). Not an upstream file. Each item is `(name, mmd)`, in name
//! order, read from the same JSONL the golden test reads.

use crate::util::json::parse_json;

fn pairs(jsonl: &str) -> Vec<(String, String)> {
    jsonl
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            let row = parse_json(l).expect("corpus row");
            let m = row.as_map().expect("corpus row");
            (m.get("name").unwrap().js_string(), m.get("mmd").unwrap().js_string())
        })
        .collect()
}

/// The hand-written samples: every diagram type, and a few errors.
pub fn samples() -> Vec<(String, String)> {
    pairs(include_str!("../tests/samples.jsonl"))
}

/// Every input upstream detects as a flowchart (the ones that render and the ones that fail).
pub fn flowchart() -> Vec<(String, String)> {
    pairs(include_str!("../tests/golden/flowchart.jsonl"))
}

/// Every input upstream detects as an entity relationship diagram.
pub fn er() -> Vec<(String, String)> {
    pairs(include_str!("../tests/golden/er.jsonl"))
}

/// Every input upstream detects as a class diagram.
pub fn class_diagram() -> Vec<(String, String)> {
    pairs(include_str!("../tests/golden/class.jsonl"))
}

/// Every input upstream detects as a state diagram.
pub fn state() -> Vec<(String, String)> {
    pairs(include_str!("../tests/golden/state.jsonl"))
}

/// Every input upstream detects as a sequence diagram.
pub fn sequence() -> Vec<(String, String)> {
    pairs(include_str!("../tests/golden/sequence.jsonl"))
}

/// Every ported type's inputs, as `(type, inputs)` in `PORTED` order.
pub fn all() -> Vec<(&'static str, Vec<(String, String)>)> {
    [
        ("flowchart", include_str!("../tests/golden/flowchart.jsonl")),
        ("sequence", include_str!("../tests/golden/sequence.jsonl")),
        ("state", include_str!("../tests/golden/state.jsonl")),
        ("class", include_str!("../tests/golden/class.jsonl")),
        ("er", include_str!("../tests/golden/er.jsonl")),
        ("ishikawa", include_str!("../tests/golden/ishikawa.jsonl")),
        ("cynefin", include_str!("../tests/golden/cynefin.jsonl")),
        ("venn", include_str!("../tests/golden/venn.jsonl")),
        ("swimlane", include_str!("../tests/golden/swimlane.jsonl")),
        ("eventmodeling", include_str!("../tests/golden/eventmodeling.jsonl")),
        ("wardley", include_str!("../tests/golden/wardley.jsonl")),
        ("info", include_str!("../tests/golden/info.jsonl")),
        ("pie", include_str!("../tests/golden/pie.jsonl")),
        ("radar", include_str!("../tests/golden/radar.jsonl")),
        ("packet", include_str!("../tests/golden/packet.jsonl")),
        ("mindmap", include_str!("../tests/golden/mindmap.jsonl")),
        ("kanban", include_str!("../tests/golden/kanban.jsonl")),
        ("journey", include_str!("../tests/golden/journey.jsonl")),
        ("timeline", include_str!("../tests/golden/timeline.jsonl")),
        ("quadrantChart", include_str!("../tests/golden/quadrantChart.jsonl")),
        ("xychart", include_str!("../tests/golden/xychart.jsonl")),
        ("sankey", include_str!("../tests/golden/sankey.jsonl")),
        ("treemap", include_str!("../tests/golden/treemap.jsonl")),
        ("requirement", include_str!("../tests/golden/requirement.jsonl")),
        ("railroad", include_str!("../tests/golden/railroad.jsonl")),
        ("usecase", include_str!("../tests/golden/usecase.jsonl")),
        ("treeView", include_str!("../tests/golden/treeView.jsonl")),
        ("gitGraph", include_str!("../tests/golden/gitGraph.jsonl")),
        ("gantt", include_str!("../tests/golden/gantt.jsonl")),
        ("block", include_str!("../tests/golden/block.jsonl")),
        ("c4", include_str!("../tests/golden/c4.jsonl")),
        ("agentflow", include_str!("../tests/golden/agentflow.jsonl")),
    ]
    .into_iter()
    .map(|(ty, jsonl)| (ty, pairs(jsonl)))
    .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn corpus_holds_every_input() {
        assert_eq!(super::samples().len(), 63);
        assert!(super::flowchart().len() > 300);
        assert!(super::flowchart().iter().all(|(_, mmd)| !mmd.is_empty()));
        assert!(super::er().len() > 100);
        let types: Vec<&str> = super::all().iter().map(|(ty, _)| *ty).collect();
        assert_eq!(types, crate::PORTED);
        assert!(super::all().iter().all(|(_, inputs)| !inputs.is_empty()));
    }
}
