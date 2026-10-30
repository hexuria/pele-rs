// Port of src/diagrams/treemap/index.ts and model.ts.

pub mod format;
pub mod layout;
pub mod parser;
pub mod render;

use crate::diagrams::info::newline_dedent;
use crate::diagrams::Diagram;
use crate::js;
use parser::{Row, TreemapAst};

pub struct TreemapNode {
    pub name: String,
    // Sections have a list, which may be empty. Leaves have none.
    pub children: Option<Vec<TreemapNode>>,
    pub value: Option<f64>,
    pub class_selector: Option<String>,
    pub css_compiled_styles: Option<Vec<String>>,
}

pub struct TreemapModel {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub roots: Vec<TreemapNode>,
}

struct Item {
    level: usize,
    name: String,
    leaf: bool,
    value: Option<f64>,
    class_selector: Option<String>,
    css_compiled_styles: Option<Vec<String>>,
}

// A row belongs to the nearest section above it that is indented less.
fn build_hierarchy(items: Vec<Item>) -> Vec<TreemapNode> {
    // Nodes are built in an arena, then nested, as each keeps its place in its parent's list.
    let mut arena: Vec<(TreemapNode, Option<usize>)> = Vec::new();
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for item in items {
        let node = TreemapNode {
            name: item.name,
            children: if item.leaf { None } else { Some(Vec::new()) },
            value: if item.leaf { item.value } else { None },
            class_selector: item.class_selector,
            css_compiled_styles: item.css_compiled_styles,
        };
        while stack.last().is_some_and(|&(_, level)| level >= item.level) {
            stack.pop();
        }
        let parent = stack.last().map(|&(i, _)| i);
        let index = arena.len();
        arena.push((node, parent));
        if !item.leaf {
            stack.push((index, item.level));
        }
    }
    let mut kids: Vec<Vec<usize>> = vec![Vec::new(); arena.len()];
    let mut roots: Vec<usize> = Vec::new();
    for (i, (_, parent)) in arena.iter().enumerate() {
        match parent {
            Some(p) => kids[*p].push(i),
            None => roots.push(i),
        }
    }
    let mut slots: Vec<Option<TreemapNode>> = arena.into_iter().map(|(n, _)| Some(n)).collect();
    fn take(i: usize, slots: &mut Vec<Option<TreemapNode>>, kids: &[Vec<usize>]) -> TreemapNode {
        let mut node = slots[i].take().unwrap();
        if let Some(children) = node.children.as_mut() {
            for &k in &kids[i] {
                children.push(take(k, slots, kids));
            }
        }
        node
    }
    roots.into_iter().map(|r| take(r, &mut slots, &kids)).collect()
}

pub fn build_treemap(ast: TreemapAst, title: Option<&str>) -> TreemapModel {
    let mut classes: Vec<(String, Vec<String>)> = Vec::new();
    for row in &ast.rows {
        let Row::ClassDef { class_name, style_text, .. } = row else { continue };
        let k = match classes.iter().position(|(c, _)| c == class_name) {
            Some(k) => k,
            None => {
                classes.push((class_name.clone(), Vec::new()));
                classes.len() - 1
            }
        };
        // An escaped comma stays in the value; the others separate declarations.
        let text = style_text.clone().unwrap_or_default().replace("\\,", "§§§").replace(',', ";").replace("§§§", ",");
        for style in text.split(';') {
            classes[k].1.push(style.to_string());
        }
    }
    let mut items = Vec::new();
    for row in ast.rows {
        let Row::Node { indent, name, leaf, value, class_selector } = row else { continue };
        let styles = class_selector.as_deref().filter(|c| !c.is_empty()).and_then(|c| classes.iter().find(|(k, _)| k == c)).map(|(_, s)| s.clone());
        items.push(Item { level: indent.unwrap_or(0), name, leaf, value: if leaf { Some(value) } else { None }, class_selector, css_compiled_styles: styles.filter(|s| !s.is_empty()) });
    }
    TreemapModel {
        title: ast.title.filter(|t| !t.is_empty()).or(title.map(str::to_string)),
        acc_title: ast.acc_title.filter(|t| !t.is_empty()).map(|t| js::trim_start(&t).to_string()),
        acc_descr: ast.acc_descr.filter(|t| !t.is_empty()).map(|d| newline_dedent(&d)),
        roots: build_hierarchy(items),
    }
}

pub static DIAGRAM: Diagram = Diagram {
    type_: "treemap",
    section: None,
    keep_comments: false,
    parse: |source, _config, title, _line_offset| Ok(Box::new(build_treemap(parser::parse_treemap(source)?, title))),
    render: |model, config, options| Ok(render::render_treemap(model.downcast_ref::<TreemapModel>().unwrap(), config, options)),
};
