// Port of src/diagrams/agentflow/db.ts. Left out, because nothing drawn reads them: the source
// mappings, the diagnostics, the semantic model, node props, and metadata other than the keys
// below (`shape`, `label`, `labelType`, an edge's `curve`, a flow's `view`).

use super::shapes::{is_valid_shape, resolve_shape_alias};
use super::types::*;
use crate::errors::{locate, PeleError, PeleErrorCode};
use crate::js;
use crate::util::url::safe_url;
use crate::util::yaml::{parse_yaml, Map, Value};
use std::collections::{HashMap, HashSet};

type R<T> = Result<T, PeleError>;

const TOOL_SHAPES: &[&str] = &["subroutine", "subprocess", "subproc", "framed-rectangle", "tool"];

fn label_type(type_: Option<&Value>) -> LabelType {
    match type_ {
        Some(Value::Str(s)) if s == "text" => LabelType::Text,
        Some(Value::Str(s)) if s == "string" => LabelType::String,
        _ => LabelType::Markdown,
    }
}

fn unquote(text: &str) -> String {
    if text.len() >= 2 && text.starts_with('"') && text.ends_with('"') {
        text[1..text.len() - 1].to_string()
    } else {
        text.to_string()
    }
}

/// `JSON.stringify` of a YAML value.
fn json(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Num(n) if n.is_finite() => js::num_str(*n),
        Value::Num(_) => "null".into(),
        Value::Str(s) => {
            let mut out = String::from("\"");
            for c in s.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    '\u{8}' => out.push_str("\\b"),
                    '\u{c}' => out.push_str("\\f"),
                    c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                    c => out.push(c),
                }
            }
            out.push('"');
            out
        }
        Value::Seq(items) => format!("[{}]", items.iter().map(json).collect::<Vec<_>>().join(",")),
        Value::Map(m) => format!("{{{}}}", m.0.iter().map(|(k, v)| format!("{}:{}", json(&Value::Str(k.clone())), json(v))).collect::<Vec<_>>().join(",")),
    }
}

/// /:\s*[>|][\d+-]*\s*$/
fn block_scalar(code: &[char]) -> bool {
    (0..code.len()).any(|i| {
        if code[i] != ':' {
            return false;
        }
        let mut k = i + 1;
        while k < code.len() && js::is_space(code[k]) {
            k += 1;
        }
        if k >= code.len() || (code[k] != '>' && code[k] != '|') {
            return false;
        }
        k += 1;
        while k < code.len() && (code[k].is_ascii_digit() || code[k] == '+' || code[k] == '-') {
            k += 1;
        }
        code[k..].iter().all(|&c| js::is_space(c))
    })
}

// Removes the comma that ends a line of a multi-line metadata body, which block YAML does not allow.
// Commas inside quoted scalars, flow collections and block scalars are content and stay.
fn strip_line_trailing_commas(body: &str) -> String {
    let mut in_single = false;
    let mut in_double = false;
    let mut flow_depth: i64 = 0;
    let mut block_scalar_indent: Option<usize> = None;
    let mut out: Vec<String> = Vec::new();
    let indent_of = |line: &str| js::len16(line) - js::len16(js::trim_start(line));
    for line in body.split('\n') {
        if let Some(indent) = block_scalar_indent {
            if js::trim(line).is_empty() || indent_of(line) > indent {
                out.push(line.to_string());
                continue;
            }
            block_scalar_indent = None;
        }
        let chars: Vec<char> = line.chars().collect();
        let mut comment_start: Option<usize> = None;
        let mut i = 0;
        while i < chars.len() {
            let ch = chars[i];
            if in_double {
                if ch == '\\' {
                    i += 1;
                } else if ch == '"' {
                    in_double = false;
                }
            } else if in_single {
                if ch == '\'' {
                    in_single = false;
                }
            } else if ch == '"' {
                in_double = true;
            } else if ch == '\'' {
                in_single = true;
            } else if ch == '#' && (i == 0 || chars[i - 1] == ' ' || chars[i - 1] == '\t') {
                comment_start = Some(i);
                break;
            } else if ch == '[' || ch == '{' {
                flow_depth += 1;
            } else if ch == ']' || ch == '}' {
                flow_depth = (flow_depth - 1).max(0);
            }
            i += 1;
        }
        if in_single || in_double || flow_depth > 0 {
            out.push(line.to_string());
            continue;
        }
        let split = comment_start.unwrap_or(chars.len());
        let code = &chars[..split];
        let comment: String = chars[split..].iter().collect();
        if block_scalar(code) {
            block_scalar_indent = Some(indent_of(line));
            out.push(line.to_string());
            continue;
        }
        // code.replace(/,([\t ]*)$/, '$1')
        let mut e = code.len();
        while e > 0 && (code[e - 1] == ' ' || code[e - 1] == '\t') {
            e -= 1;
        }
        let code: String = if e > 0 && code[e - 1] == ',' { code[..e - 1].iter().chain(code[e..].iter()).collect() } else { code.iter().collect() };
        out.push(code + &comment);
    }
    out.join("\n")
}

/// A value that upstream keeps as whatever YAML gave it, here kept as a string when truthy.
fn truthy_string(v: &Value) -> Option<String> {
    match v {
        Value::Str(s) if !s.is_empty() => Some(s.clone()),
        v if v.truthy() => Some(format!("\u{0}{}", v.js_string())),
        _ => None,
    }
}

fn destruct_end_link(raw: &str) -> (String, String, f64) {
    let s = js::trim(raw);
    let mut line = s.char_indices().last().map_or("", |(k, _)| &s[..k]);
    let mut type_ = "arrow_open";
    match s.chars().last() {
        Some('x') => type_ = "arrow_cross",
        Some('>') => type_ = "arrow_point",
        Some('-') | Some('.') => line = s,
        _ => {}
    }
    let mut stroke = "normal";
    let mut length = js::len16(line) as f64 - 1.0;
    let dots = line.chars().filter(|&c| c == '.').count();
    if dots > 0 {
        stroke = "dotted";
        length = dots as f64;
    }
    (type_.into(), stroke.into(), length)
}

pub struct AgentflowDbOptions {
    pub max_edges: Option<usize>,
    pub inherit_dir: bool,
    pub direction: Option<String>,
    // The text the parser sees, where errors are located.
    pub source: Vec<u16>,
}

pub struct AgentflowDb {
    pub direction: Option<String>,
    pub nodes: Vec<AgentNode>,
    pub node_index: HashMap<String, usize>,
    pub edges: Vec<AgentEdge>,
    pub subgraphs: Vec<AgentSubgraph>,
    pub connectors: HashSet<String>,
    pub classes: HashMap<String, AgentClassDef>,
    pub tooltips: HashMap<String, String>,
    pub default_edge_style: Option<Vec<String>>,
    pub default_interpolate: Option<String>,
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,

    subgraph_lookup: HashMap<String, usize>,
    global_nodes: HashSet<String>,
    members: HashSet<String>,
    edge_by_id: HashMap<String, usize>,
    pair_count: HashMap<(String, String), usize>,
    sub_count: usize,
    options: AgentflowDbOptions,
}

impl AgentflowDb {
    pub fn new(options: AgentflowDbOptions) -> Self {
        AgentflowDb {
            direction: None,
            nodes: Vec::new(),
            node_index: HashMap::new(),
            edges: Vec::new(),
            subgraphs: Vec::new(),
            connectors: HashSet::new(),
            classes: HashMap::new(),
            tooltips: HashMap::new(),
            default_edge_style: None,
            default_interpolate: None,
            title: None,
            acc_title: None,
            acc_descr: None,
            subgraph_lookup: HashMap::new(),
            global_nodes: HashSet::new(),
            members: HashSet::new(),
            edge_by_id: HashMap::new(),
            pair_count: HashMap::new(),
            sub_count: 0,
            options,
        }
    }

    pub fn node(&self, id: &str) -> Option<&AgentNode> {
        self.node_index.get(id).map(|&i| &self.nodes[i])
    }

    fn node_mut(&mut self, id: &str) -> Option<&mut AgentNode> {
        match self.node_index.get(id) {
            Some(&i) => Some(&mut self.nodes[i]),
            None => None,
        }
    }

    fn metadata(&self, id: &str, source: &str, loc: Loc) -> R<Value> {
        let multiline = source.contains('\n');
        let parsed = match parse_yaml(&if multiline { format!("{}\n", source) } else { format!("{{\n{}\n}}", source) }) {
            Ok(v) => v,
            Err(message) => {
                // Block YAML rejects the comma-separated style of the single-line form, so try once more without the commas.
                let stripped = if multiline { strip_line_trailing_commas(source) } else { source.to_string() };
                let retried = if stripped != source { parse_yaml(&format!("{}\n", stripped)).ok() } else { None };
                match retried {
                    Some(v) => v,
                    None => {
                        let (line, column) = self.error_at(loc);
                        let mut e = PeleError::typed(format!("Metadata of \"{}\" is not valid YAML. {} ({}:{})", id, message, line, column), PeleErrorCode::Syntax, "agentflow");
                        e.line = line;
                        e.column = column;
                        return Err(e);
                    }
                }
            }
        };
        Ok(if parsed == Value::Null { Value::Map(Map::default()) } else { parsed })
    }

    fn error_at(&self, loc: Loc) -> (usize, usize) {
        let source = &self.options.source;
        let mut start = 0;
        for _ in 1..loc.first_line {
            start = source.iter().skip(start).position(|&c| c == 10).map_or(0, |k| start + k + 1);
        }
        let at = locate(source, start + loc.first_column);
        (at.line, at.column)
    }

    fn shape_error(&self, message: String, loc: Loc) -> PeleError {
        let (line, column) = self.error_at(loc);
        let mut e = PeleError::typed(message, PeleErrorCode::Semantic, "agentflow");
        e.line = line;
        e.column = column;
        e
    }

    pub fn add_vertex(&mut self, id: &str, text_obj: Option<&FlowText>, type_: Option<&str>, style: Option<Vec<String>>, metadata: Option<(&str, Loc)>) -> R<()> {
        if id.is_empty() || js::trim(id).is_empty() {
            return Ok(());
        }

        let mut doc: Option<Value> = None;
        let mut authored_shape: Option<String> = None;
        let mut loc = Loc { first_line: 0, first_column: 0 };
        if let Some((source, at)) = metadata {
            loc = at;
            let mut d = self.metadata(id, source, at)?;
            if let Value::Map(m) = &mut d {
                if let Some(Value::Str(s)) = m.get("shape") {
                    let s = s.clone();
                    m.set("shape", Value::Str(resolve_shape_alias(&s).to_string()));
                    authored_shape = Some(s);
                }
            }
            doc = Some(d);
        }
        let truthy = doc.as_ref().is_some_and(|d| d.truthy());

        if let (Some(&si), true) = (self.subgraph_lookup.get(id), truthy) {
            let sg = &mut self.subgraphs[si];
            let mut merged = sg.metadata.take().unwrap_or_default();
            if let Some(Value::Map(m)) = &doc {
                for (k, v) in &m.0 {
                    merged.set(k, v.clone());
                }
            }
            sg.metadata = Some(merged);
            return Ok(());
        }

        if truthy && self.connectors.contains(id) {
            return Ok(());
        }

        // Mermaid reserves this id and creates no node for it.
        if id == "connectors" {
            return Ok(());
        }

        if let Some(&ei) = self.edge_by_id.get(id) {
            if let Some(Value::Map(m)) = &doc {
                if let Some(v) = m.get("curve") {
                    self.edges[ei].interpolate = truthy_string(v);
                }
            }
            return Ok(());
        }

        if !self.node_index.contains_key(id) {
            self.node_index.insert(id.to_string(), self.nodes.len());
            self.nodes.push(AgentNode { id: id.to_string(), ..Default::default() });
        }
        let vertex = self.node_mut(id).unwrap();

        if let Some(t) = text_obj {
            vertex.label_type = t.type_;
            vertex.text = Some(unquote(js::trim(&t.text)));
        } else if vertex.text.is_none() {
            vertex.text = Some(id.to_string());
        }
        if let Some(t) = type_ {
            vertex.type_ = Some(t.to_string());
        }
        if let Some(style) = style {
            vertex.styles.extend(style);
        }

        if !truthy {
            return Ok(());
        }
        let Some(Value::Map(fields)) = doc else { return Ok(()) };

        if let Some(shape) = fields.get("shape").filter(|v| v.truthy()) {
            let authored = match &authored_shape {
                Some(s) => s.clone(),
                None => return Err(self.shape_error(format!("No such shape: {}.", json(shape)), loc)),
            };
            if authored != authored.to_lowercase() || authored.contains('_') {
                return Err(self.shape_error(format!("No such shape: {}. Shape names should be lowercase.", authored), loc));
            }
            let shape = shape.js_string();
            if !is_valid_shape(&shape) {
                return Err(self.shape_error(format!("No such shape: {}.", shape), loc));
            }
            self.node_mut(id).unwrap().type_ = Some(shape);
        }
        if let Some(label) = fields.get("label").filter(|v| v.truthy()) {
            let vertex = self.node_mut(id).unwrap();
            vertex.text = Some(label.js_string());
            vertex.label_type = label_type(fields.get("labelType"));
        }
        Ok(())
    }

    pub fn add_single_link(&mut self, start: &str, end: &str, link: &LinkInfo, id: Option<&str>) -> R<()> {
        let mut edge = AgentEdge {
            start: start.to_string(),
            end: end.to_string(),
            type_: Some(link.type_.clone()),
            text: String::new(),
            label_type: LabelType::Text,
            stroke: Some(link.stroke.clone()),
            length: link.length.map(|l| if l > 10.0 { 10.0 } else { l }),
            style: None,
            classes: Vec::new(),
            id: String::new(),
            is_user_defined_id: false,
            interpolate: self.default_interpolate.clone(),
        };
        if let Some(t) = &link.text {
            edge.text = unquote(js::trim(&t.text));
            edge.label_type = t.type_;
        }
        let pair = (start.to_string(), end.to_string());
        let existing = *self.pair_count.get(&pair).unwrap_or(&0);
        match id.filter(|i| !i.is_empty() && !self.edge_by_id.contains_key(*i)) {
            Some(i) => {
                edge.id = i.to_string();
                edge.is_user_defined_id = true;
            }
            None => edge.id = format!("L_{}_{}_{}", start, end, if existing == 0 { 0 } else { existing + 1 }),
        }

        if let Some(max) = self.options.max_edges {
            if self.edges.len() >= max {
                return Err(PeleError::typed(format!("Edge limit exceeded. The limit is {}.", max), PeleErrorCode::Limit, "agentflow"));
            }
        }
        let eid = edge.id.clone();
        self.edges.push(edge);
        self.pair_count.insert(pair, existing + 1);
        self.edge_by_id.entry(eid).or_insert(self.edges.len() - 1);
        Ok(())
    }

    pub fn add_link(&mut self, starts: &[String], ends: &[String], link: &LinkInfo) -> R<()> {
        let id = link.id.as_ref().map(|i| i.replacen('@', "", 1));
        let last_start = starts.last().unwrap();
        let first_end = &ends[0];
        for start in starts {
            for end in ends {
                let this_id = if start == last_start && end == first_end { id.as_deref() } else { None };
                self.add_single_link(start, end, link, this_id)?;
            }
        }
        Ok(())
    }

    // Mermaid checks the bounds only for a numeric index and crashes on the strings its own parser passes.
    fn edge_at(&self, pos: &str) -> R<usize> {
        let k = js::to_number(pos);
        if k >= 0.0 && k.fract() == 0.0 && (k as usize) < self.edges.len() {
            return Ok(k as usize);
        }
        Err(PeleError::typed(
            format!(
                "The index {} for linkStyle is out of bounds. Valid indices for linkStyle are between 0 and {}. (Help: Ensure that the index is within the range of existing edges.)",
                pos,
                self.edges.len() as i64 - 1
            ),
            PeleErrorCode::Semantic,
            "agentflow",
        ))
    }

    pub fn update_link_interpolate(&mut self, positions: &[String], interpolate: &str) -> R<()> {
        for pos in positions {
            if pos == "default" {
                self.default_interpolate = Some(interpolate.to_string());
            } else {
                let k = self.edge_at(pos)?;
                self.edges[k].interpolate = Some(interpolate.to_string());
            }
        }
        Ok(())
    }

    // Upstream hands one array to every position and appends `fill:none` to it once.
    pub fn update_link(&mut self, positions: &[String], mut style: Vec<String>) -> R<()> {
        let mut at: Vec<usize> = Vec::new();
        for pos in positions {
            if pos == "default" {
                self.default_edge_style = Some(style.clone());
                continue;
            }
            at.push(self.edge_at(pos)?);
            if !style.is_empty() && !style.iter().any(|s| s.starts_with("fill")) {
                style.push("fill:none".into());
            }
        }
        // Every position shares the one array, as extended.
        for k in at {
            self.edges[k].style = Some(style.clone());
        }
        Ok(())
    }

    pub fn add_class(&mut self, ids: &str, styles: &[String]) {
        let joined = styles.join(",").replace("\\,", "§§§").replace(',', ";").replace("§§§", ",");
        let style: Vec<&str> = joined.split(';').collect();
        for id in ids.split(',') {
            let class = self.classes.entry(id.to_string()).or_insert_with(|| AgentClassDef { styles: Vec::new(), text_styles: Vec::new() });
            for s in &style {
                if s.contains("color") {
                    class.text_styles.push(s.replacen("fill", "bgFill", 1));
                }
                class.styles.push(s.to_string());
            }
        }
    }

    pub fn set_direction(&mut self, dir: &str) {
        let mut d = js::trim(dir).to_string();
        if d.contains('<') {
            d = "RL".into();
        }
        if d.contains('^') {
            d = "BT".into();
        }
        if d.contains('>') {
            d = "LR".into();
        }
        if d.contains('v') {
            d = "TB".into();
        }
        if d == "TD" {
            d = "TB".into();
        }
        self.direction = Some(d);
    }

    pub fn set_class(&mut self, ids: &str, class_name: &str) {
        for id in ids.split(',') {
            if let Some(v) = self.node_mut(id) {
                v.classes.push(class_name.to_string());
            }
            if let Some(&ei) = self.edge_by_id.get(id) {
                self.edges[ei].classes.push(class_name.to_string());
            }
            if let Some(&si) = self.subgraph_lookup.get(id) {
                self.subgraphs[si].classes.push(class_name.to_string());
            }
        }
    }

    pub fn set_tooltip(&mut self, ids: &str, tooltip: &str) {
        for id in ids.split(',') {
            self.tooltips.insert(id.to_string(), tooltip.to_string());
        }
    }

    pub fn set_link(&mut self, ids: &str, link: &str, target: Option<&str>) {
        let url = js::trim(link);
        for id in ids.split(',') {
            if let Some(v) = self.node_mut(id) {
                v.link = if url.is_empty() { None } else { Some(safe_url(url)) };
                v.link_target = target.map(str::to_string);
            }
        }
        self.set_class(ids, "clickable");
    }

    // Callbacks are never executed; the node is only marked clickable, as in Mermaid's strict mode.
    pub fn set_click_event(&mut self, ids: &str) {
        self.set_class(ids, "clickable");
    }

    pub fn set_acc_title(&mut self, text: &str) {
        self.acc_title = Some(js::trim_start(text).to_string());
    }

    pub fn set_acc_description(&mut self, text: &str) {
        // text.replace(/\n\s+/g, '\n')
        let chars: Vec<char> = text.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '\n' && i + 1 < chars.len() && js::is_space(chars[i + 1]) {
                out.push('\n');
                i += 1;
                while i < chars.len() && js::is_space(chars[i]) {
                    i += 1;
                }
                continue;
            }
            out.push(chars[i]);
            i += 1;
        }
        self.acc_descr = Some(out);
    }

    // Upstream compares the id and title objects by identity; agentflow's parser always gives
    // two objects, so a flow keeps its id.
    pub fn add_sub_graph(&mut self, id_text: Option<&FlowText>, list: &[DocItem], title_text: Option<&FlowText>) -> String {
        let id: Option<String> = id_text.map(|t| js::trim(&t.text).to_string());
        let title = title_text.map_or("", |t| t.text.as_str()).to_string();

        let mut seen: HashSet<String> = HashSet::new();
        let mut node_list: Vec<String> = Vec::new();
        let mut dir: Option<String> = None;
        for entry in list {
            match entry {
                DocItem::Str(s) => {
                    if !js::trim(s).is_empty() && seen.insert(s.clone()) {
                        node_list.push(s.clone());
                    }
                }
                DocItem::List(items) => {
                    for item in items {
                        if !js::trim(item).is_empty() && seen.insert(item.clone()) {
                            node_list.push(item.clone());
                        }
                    }
                }
                DocItem::Dir(d) => dir = Some(d.clone()),
            }
        }
        if dir.is_none() && self.options.inherit_dir {
            dir = self.direction.clone().or_else(|| self.options.direction.clone());
        }

        let id = id.unwrap_or_else(|| format!("subGraph{}", self.sub_count));
        self.sub_count += 1;

        // A flow never contains itself, a globally scoped node, or a node another flow already holds.
        let nodes: Vec<String> = node_list.into_iter().filter(|n| *n != id && !self.global_nodes.contains(n) && !self.members.contains(n)).collect();
        for n in &nodes {
            self.members.insert(n.clone());
        }

        if let Some(&si) = self.subgraph_lookup.get(&id) {
            // A repeated id adds its members to the first flow; its title and direction win.
            let existing = &mut self.subgraphs[si];
            existing.nodes.extend(nodes);
            if !js::trim(&title).is_empty() {
                existing.title = js::trim(&title).to_string();
            }
            if let Some(d) = dir.filter(|d| !d.is_empty()) {
                existing.dir = Some(d);
            }
            return id;
        }
        self.subgraph_lookup.insert(id.clone(), self.subgraphs.len());
        self.subgraphs.push(AgentSubgraph {
            id: id.clone(),
            nodes,
            title: js::trim(&title).to_string(),
            classes: Vec::new(),
            dir,
            label_type: title_text.map_or(LabelType::Markdown, |t| t.type_),
            metadata: None,
        });
        id
    }

    // Every node named inside a `global` block stays out of the flows that mention it.
    pub fn add_global(&mut self, list: &[DocItem]) {
        let mut release = false;
        let mut add = |item: &str, db: &mut Self| {
            let id = js::trim(item);
            if id.is_empty() {
                return;
            }
            db.global_nodes.insert(id.to_string());
            if db.members.remove(id) {
                release = true;
            }
        };
        for entry in list {
            match entry {
                DocItem::Str(s) => add(s, self),
                DocItem::List(items) => {
                    for item in items {
                        add(item, self);
                    }
                }
                DocItem::Dir(_) => {}
            }
        }
        if !release {
            return;
        }
        for sg in self.subgraphs.iter_mut() {
            sg.nodes.retain(|n| !self.global_nodes.contains(n));
        }
    }

    pub fn add_connector(&mut self, text_obj: &FlowText, title_obj: &FlowText) -> String {
        let id = js::trim(&text_obj.text).to_string();
        if id.is_empty() {
            return String::new();
        }
        let title = js::trim(&title_obj.text).to_string();
        if !self.connectors.contains(&id) {
            let created = AgentNode { id: id.clone(), is_connector: true, text: Some(if title.is_empty() { id.clone() } else { title }), ..Default::default() };
            self.connectors.insert(id.clone());
            // Map.set keeps the place of a node already there.
            match self.node_index.get(&id) {
                Some(&i) => self.nodes[i] = created,
                None => {
                    self.node_index.insert(id.clone(), self.nodes.len());
                    self.nodes.push(created);
                }
            }
        } else if !title.is_empty() {
            self.node_mut(&id).unwrap().text = Some(title);
        }
        id
    }

    pub fn destruct_link(&self, end: &str, start: Option<&str>) -> LinkInfo {
        let (type_, stroke, length) = destruct_end_link(end);
        if let Some(start) = start.filter(|s| !s.is_empty()) {
            // The first half of a split arrow only says whether the line is dotted.
            let s = if start.contains('.') { "dotted" } else { "normal" };
            if s != stroke {
                return LinkInfo { type_: "INVALID".into(), stroke: "INVALID".into(), length: None, text: None, id: None };
            }
        }
        LinkInfo { type_, stroke, length: Some(length), text: None, id: None }
    }

    pub fn subgraph(&self, id: &str) -> Option<&AgentSubgraph> {
        self.subgraph_lookup.get(id).map(|&i| &self.subgraphs[i])
    }

    pub fn vertex_kind(&self, vertex: &AgentNode) -> &'static str {
        let type_ = vertex.type_.as_deref();
        if type_.is_some_and(|t| TOOL_SHAPES.contains(&t)) {
            return "tool";
        }
        match type_.filter(|t| !t.is_empty()).map(resolve_shape_alias) {
            Some("hexagon" | "hex") => "action",
            Some("lean-right" | "lean_right") => "input",
            Some("lin-doc" | "lined-document") => "refdoc",
            Some("diamond") => "decision",
            _ => "task",
        }
    }
}
