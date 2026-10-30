// Port of src/diagrams/agentflow/index.ts.

pub mod db;
pub mod graph;
pub mod lexer;
pub mod parser;
pub mod render;
pub mod shapes;
pub mod tokens;
pub mod types;

use crate::diagrams::Diagram;
use crate::errors::PeleError;
use crate::preprocess::Config;
use crate::util::yaml::Value;
use db::{AgentflowDb, AgentflowDbOptions};
use graph::{build_agent_graph, AgentGraph};

pub struct Agentflow {
    pub db: AgentflowDb,
    // Built once, as upstream's agentGraph caches it per model.
    pub graph: AgentGraph,
}

pub fn parse(source: &[u16], config: &Config, title: Option<&str>) -> Result<Agentflow, PeleError> {
    // Mermaid reads this setting from the flowchart section for agentflow too.
    let flow = config.get("flowchart").and_then(|f| f.as_map());
    let inherit = matches!(flow.and_then(|f| f.get("inheritDir")), Some(Value::Bool(true)));
    let mut db = AgentflowDb::new(AgentflowDbOptions {
        inherit_dir: inherit,
        direction: config.get("direction").and_then(|d| d.as_str()).map(str::to_string),
        source: source.to_vec(),
        max_edges: Some(crate::core::DEFAULT_MAX_EDGES),
    });
    if let Some(t) = title.filter(|t| !t.is_empty()) {
        db.title = Some(t.to_string());
    }
    parser::parse_agentflow(source, &mut db)?;
    let curve = flow.and_then(|f| f.get("curve")).and_then(|c| c.as_str());
    let graph = build_agent_graph(&db, render::PALETTE, curve);
    Ok(Agentflow { db, graph })
}

pub static DIAGRAM: Diagram = Diagram {
    type_: "agentflow",
    section: None,
    keep_comments: true,
    parse: |source, config, title, _line_offset| Ok(Box::new(parse(source, config, title)?)),
    render: |model, config, options| {
        let m = model.downcast_ref::<Agentflow>().unwrap();
        Ok(render::render_agentflow(&m.db, &m.graph, config, options))
    },
};
