// Compares the port with upstream Pele, byte for byte, on every golden of a ported type (and
// every input no detector matches). The goldens come from tools/golden.mjs: one JSONL per type,
// tests/golden/<type>.jsonl.
//
// PELE_GOLDEN=<name> limits the run to inputs whose name contains <name>.

use pele::util::json::parse_json;
use pele::util::yaml::Value;
use pele::{render, Scene, PORTED};
use std::fs;
use std::path::Path;

fn first_diff(a: &str, b: &str) -> String {
    let at = a.bytes().zip(b.bytes()).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
    let from = at.saturating_sub(80);
    let cut = |s: &str| {
        let mut f = from;
        while !s.is_char_boundary(f) {
            f -= 1;
        }
        let mut t = (at + 120).min(s.len());
        while !s.is_char_boundary(t) {
            t += 1;
        }
        s[f..t].to_string()
    };
    format!("at byte {}\n  want: {}\n  got:  {}", at, cut(a), cut(b))
}

fn check(file: &Path, failures: &mut Vec<String>) -> usize {
    let filter = std::env::var("PELE_GOLDEN").ok();
    let text = fs::read_to_string(file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
    let mut count = 0;
    for line in text.lines().filter(|l| !l.is_empty()) {
        let row = parse_json(line).unwrap();
        let want = row.as_map().unwrap();
        let name = want.get("name").unwrap().js_string();
        if filter.as_ref().is_some_and(|f| !name.contains(f.as_str())) {
            continue;
        }
        count += 1;
        let code = want.get("mmd").unwrap().js_string();
        let got = std::panic::catch_unwind(|| render(&code));
        let got = match got {
            Ok(g) => g,
            Err(_) => {
                failures.push(format!("{}: panicked", name));
                continue;
            }
        };
        match (want.get("error"), got) {
            (None, Ok(r)) => {
                let svg = want.get("svg").unwrap().js_string();
                if r.svg != svg {
                    failures.push(format!("{}: svg differs {}", name, first_diff(&svg, &r.svg)));
                    continue;
                }
                let w = want.get("width").unwrap().js_number();
                let h = want.get("height").unwrap().js_number();
                if r.width != w || r.height != h {
                    failures.push(format!("{}: size {}x{} != {}x{}", name, r.width, r.height, w, h));
                }
                if Some(r.mermaid_type) != want.get("mermaidType").and_then(|v| v.as_str()) {
                    failures.push(format!("{}: mermaidType {}", name, r.mermaid_type));
                }
                let links: Vec<(String, String, bool)> = match want.get("links") {
                    Some(Value::Seq(items)) => items
                        .iter()
                        .map(|l| {
                            let m = l.as_map().unwrap();
                            (m.get("id").unwrap().js_string(), m.get("href").unwrap().js_string(), m.get("internal") == Some(&Value::Bool(true)))
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                let got_links: Vec<(String, String, bool)> = r.links.iter().map(|l| (l.id.clone(), l.href.clone(), l.internal)).collect();
                if links != got_links {
                    failures.push(format!("{}: links {:?} != {:?}", name, got_links, links));
                }
                // The scene reads the SVG back and writes the same bytes.
                match Scene::parse(&r.svg) {
                    Some(scene) if pele::svg::write::write(&scene) == r.svg => {}
                    _ => failures.push(format!("{}: scene round trip", name)),
                }
            }
            (Some(e), Err(err)) => {
                let e = e.as_map().unwrap();
                let s = |k: &str| e.get(k).map(|v| if matches!(v, Value::Null) { String::new() } else { v.js_string() }).unwrap_or_default();
                let got = [
                    err.code.as_str().to_string(),
                    err.type_.clone().unwrap_or_default(),
                    err.mermaid_type.clone().unwrap_or_default(),
                    err.line.to_string(),
                    err.column.to_string(),
                    err.message.clone(),
                    err.snippet.clone(),
                ];
                let want = [s("code"), s("type"), s("mermaidType"), s("line"), s("column"), s("message"), s("snippet")];
                if got != want {
                    failures.push(format!("{}: error {:?}\n  want {:?}", name, got, want));
                }
            }
            (None, Err(err)) => failures.push(format!("{}: unexpected error {:?}: {}", name, err.code, err.message)),
            (Some(e), Ok(_)) => failures.push(format!("{}: expected error {:?}", name, e)),
        }
    }
    count
}

#[test]
fn goldens_match_upstream_byte_for_byte() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let mut failures = Vec::new();
    let mut total = 0;
    for ty in PORTED.iter().copied().chain(["none"]) {
        total += check(&root.join(format!("{ty}.jsonl")), &mut failures);
    }
    for f in failures.iter().take(40) {
        eprintln!("{}\n", f);
    }
    eprintln!("golden: {}/{} identical", total - failures.len(), total);
    assert!(failures.is_empty(), "{} of {} goldens differ", failures.len(), total);
}
