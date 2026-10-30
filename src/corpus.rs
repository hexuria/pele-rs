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

#[cfg(test)]
mod tests {
    #[test]
    fn corpus_holds_every_input() {
        assert_eq!(super::samples().len(), 63);
        assert!(super::flowchart().len() > 300);
        assert!(super::flowchart().iter().all(|(_, mmd)| !mmd.is_empty()));
        assert!(super::er().len() > 100);
    }
}
