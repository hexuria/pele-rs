// Port of src/diagrams/treeview/index.ts and model.ts.

pub mod box_drawing;
pub mod icons;
pub mod parser;
pub mod render;

use crate::diagrams::info::newline_dedent;
use crate::diagrams::Diagram;
use crate::errors::{locate, PeleError};
use crate::js;
use box_drawing::{line_start, preprocess_box_drawing, remap_error_lines};
use parser::{parse_tree_view, TreeViewAst};

pub struct TreeNode {
    pub id: usize,
    pub level: i64,
    pub name: String,
    pub directory: bool,
    // An icon name as written (`pack:name`, `file`, `folder`), or `none` to hide the icon.
    pub icon: Option<String>,
    pub css_class: Option<String>,
    pub description: Option<String>,
    pub children: Vec<TreeNode>,
}

pub struct TreeViewModel {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    // A directory named `/` at level -1 that holds the top-level entries.
    pub root: TreeNode,
}

// Mermaid's treeView database: nodes arrive in source order with their indentation. A node is
// added under the last open node indented less.
fn populate(ast: &TreeViewAst) -> TreeNode {
    let mut root = TreeNode { id: 0, level: -1, name: "/".into(), directory: true, icon: None, css_class: None, description: None, children: Vec::new() };
    // The path of child indexes from the root to each open node.
    let mut stack: Vec<(usize, i64)> = Vec::new();
    let mut count = 1;
    for node in &ast.nodes {
        let level = node.indent.unwrap_or(0) as i64;
        let mut name = node.name.clone();
        let directory = name.ends_with('/');
        if directory {
            name.pop();
        }
        while stack.last().is_some_and(|&(_, l)| level <= l) {
            stack.pop();
        }
        let mut parent = &mut root;
        for &(k, _) in &stack {
            parent = &mut parent.children[k];
        }
        parent.children.push(TreeNode {
            id: count,
            level,
            name,
            directory,
            icon: node.icon_annotation.as_ref().map(|i| if i.is_empty() { "none".to_string() } else { i.clone() }),
            css_class: node.class_annotation.clone().filter(|c| !c.is_empty()),
            description: node.desc_annotation.clone().filter(|d| !d.is_empty()),
            children: Vec::new(),
        });
        count += 1;
        stack.push((parent.children.len() - 1, level));
    }
    root
}

/// Reads the source to a syntax tree, accepting box-drawing input. Errors name lines of the
/// original text.
pub fn read_tree_view(source: &[u16]) -> Result<TreeViewAst, PeleError> {
    let pre = preprocess_box_drawing(source)?;
    let error = match parse_tree_view(&pre.text) {
        Ok(ast) => return Ok(ast),
        Err(e) => e,
    };
    // The parser already located errors in unchanged text.
    if pre.text == source {
        return Err(error);
    }
    let Some(&line) = pre.line_map.get(&error.line) else { return Err(error) };
    let at = if error.column > 0 { pre.columns.get(&error.line).map_or(error.column - 1, |f| f(error.column - 1)) } else { 0 };
    let written = locate(source, line_start(source, line) + at);
    let head = error.message.find('\n').unwrap_or(error.message.len());
    let rest = &error.message[head..];
    let mut map = std::collections::HashMap::new();
    map.insert(error.line, written.line);
    let message = remap_error_lines(&error.message[..head], &map) + &if !error.snippet.is_empty() { rest.replacen(&error.snippet, &written.snippet, 1) } else { rest.to_string() };
    let mut e = PeleError::new(message, error.code);
    e.type_ = error.type_;
    e.line = written.line;
    e.column = if error.column > 0 { written.column } else { 0 };
    e.snippet = if !error.snippet.is_empty() { written.snippet } else { String::new() };
    Err(e)
}

pub fn build_tree_view(ast: TreeViewAst, title: Option<&str>) -> TreeViewModel {
    let root = populate(&ast);
    TreeViewModel {
        title: ast.title.filter(|t| !t.is_empty()).or(title.map(str::to_string)),
        acc_title: ast.acc_title.filter(|t| !t.is_empty()).map(|t| js::trim_start(&t).to_string()),
        acc_descr: ast.acc_descr.filter(|t| !t.is_empty()).map(|d| newline_dedent(&d)),
        root,
    }
}

pub static DIAGRAM: Diagram = Diagram {
    type_: "treeView",
    section: None,
    keep_comments: false,
    parse: |source, _config, title, _line_offset| Ok(Box::new(build_tree_view(read_tree_view(source)?, title))),
    render: |model, config, options| Ok(render::render_tree_view(model.downcast_ref::<TreeViewModel>().unwrap(), config, options)),
};
