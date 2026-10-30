// Port of src/diagrams/git/db.ts.

use super::parser::{CommitTypeName, GitGraphAst, StatementAst};
use crate::diagrams::info::newline_dedent;
use crate::errors::{PeleError, PeleErrorCode};
use crate::js;
use crate::preprocess::Config;
use crate::util::yaml::Value;

pub const NORMAL: u32 = 0;
pub const REVERSE: u32 = 1;
pub const HIGHLIGHT: u32 = 2;
pub const MERGE: u32 = 3;
pub const CHERRY_PICK: u32 = 4;

#[derive(Clone)]
pub struct GitCommit {
    pub id: String,
    pub message: String,
    pub seq: usize,
    pub type_: u32,
    pub tags: Vec<String>,
    pub parents: Vec<String>,
    pub branch: String,
    pub custom_type: Option<u32>,
    pub custom_id: bool,
}

pub struct GitModel {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub direction: &'static str,
    // Keyed by commit id, in the order the ids first appeared.
    pub commits: Vec<GitCommit>,
    // Branch names in drawing order: by `order`, then by creation.
    pub lanes: Vec<String>,
}

fn fail(message: String) -> PeleError {
    PeleError::typed(message, PeleErrorCode::Semantic, "gitGraph")
}

fn type_code(name: CommitTypeName) -> u32 {
    match name {
        CommitTypeName::Reverse => REVERSE,
        CommitTypeName::Highlight => HIGHLIGHT,
        CommitTypeName::Normal => NORMAL,
    }
}

// An insertion-ordered map, as JS's Map: setting a key that exists keeps its place.
struct Ordered<V>(Vec<(String, V)>);

impl<V> Ordered<V> {
    fn get(&self, key: &str) -> Option<&V> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }
    fn set(&mut self, key: &str, value: V) {
        match self.0.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value,
            None => self.0.push((key.to_string(), value)),
        }
    }
}

struct State {
    commits: Ordered<GitCommit>,
    branches: Ordered<Option<String>>,
    head: Option<String>,
    current: String,
    seq: usize,
}

impl State {
    // Mermaid appends seven random hex digits to the sequence number. These digits are derived
    // from the sequence number instead, so the same text always gives the same ids.
    fn next_id(&self) -> String {
        let mut salt = self.seq as f64 + 1.0;
        loop {
            let s = (salt.rem_euclid(4294967296.0)) as u32;
            let h = s.wrapping_mul(0x9e3779b1);
            let id = format!("{}-{}", self.seq, &format!("{:08x}", h)[..7]);
            if !self.commits.has(&id) {
                return id;
            }
            salt += 0x9e3779b1u32 as f64;
        }
    }
    fn head_commit(&self) -> Option<&GitCommit> {
        self.head.as_ref().and_then(|id| self.commits.get(id))
    }
    fn add(&mut self, commit: GitCommit) {
        self.head = Some(commit.id.clone());
        let id = commit.id.clone();
        self.commits.set(&id, commit);
        let current = self.current.clone();
        self.branches.set(&current, Some(id));
    }
    fn checkout(&mut self, branch: &str) -> Result<(), PeleError> {
        if !self.branches.has(branch) {
            return Err(fail(format!("Trying to checkout branch which is not yet created. (Help try using \"branch {}\")", branch)));
        }
        self.current = branch.to_string();
        let id = self.branches.get(branch).cloned().flatten();
        self.head = id.filter(|i| !i.is_empty() && self.commits.has(i));
        Ok(())
    }
}

pub fn build_git(ast: GitGraphAst, config: &Config, title: Option<&str>) -> Result<GitModel, PeleError> {
    let git = config.get("gitGraph").and_then(|v| v.as_map());
    let main_name = match git.and_then(|g| g.get("mainBranchName")) {
        Some(Value::Str(s)) => s.clone(),
        _ => "main".to_string(),
    };
    let main_order = match git.and_then(|g| g.get("mainBranchOrder")) {
        Some(Value::Num(n)) => *n,
        _ => 0.0,
    };
    let mut st = State { commits: Ordered(Vec::new()), branches: Ordered(vec![(main_name.clone(), None)]), head: None, current: main_name.clone(), seq: 0 };
    let mut orders: Ordered<f64> = Ordered(vec![(main_name, main_order)]);
    let mut direction = "LR";
    if let Some(d) = ast.dir {
        direction = d;
    }

    for statement in ast.statements {
        match statement {
            StatementAst::Commit { id, message, tags, type_ } => {
                let parents = st.head_commit().map(|h| vec![h.id.clone()]).unwrap_or_default();
                let commit = GitCommit {
                    id: id.filter(|i| !i.is_empty()).unwrap_or_else(|| st.next_id()),
                    message: message.unwrap_or_default(),
                    seq: st.seq,
                    type_: type_.map_or(NORMAL, type_code),
                    tags,
                    parents,
                    branch: st.current.clone(),
                    custom_type: None,
                    custom_id: false,
                };
                st.seq += 1;
                st.add(commit);
            }
            StatementAst::Branch { name, order } => {
                if st.branches.has(&name) {
                    return Err(fail(format!("Trying to create an existing branch. (Help: Either use a new name if you want create a new branch or try using \"checkout {}\")", name)));
                }
                let head = st.head_commit().map(|h| h.id.clone());
                st.branches.set(&name, head);
                orders.set(&name, order.unwrap_or(0.0));
                st.checkout(&name)?;
            }
            StatementAst::Merge { branch: other, id, tags, type_ } => {
                let custom_id = id.unwrap_or_default();
                let current = st.current.clone();
                let current_head = st.branches.get(&current).cloned().flatten().filter(|s| !s.is_empty());
                let other_head = st.branches.get(&other).cloned().flatten().filter(|s| !s.is_empty());
                let current_commit = current_head.as_ref().and_then(|h| st.commits.get(h)).cloned();
                let other_commit = other_head.as_ref().and_then(|h| st.commits.get(h)).cloned();
                if let (Some(c), Some(_)) = (&current_commit, &other_commit) {
                    if c.branch == other {
                        return Err(fail(format!("Cannot merge branch '{}' into itself.", other)));
                    }
                }
                if current == other {
                    return Err(fail("Incorrect usage of \"merge\". Cannot merge a branch to itself".into()));
                }
                let Some(current_commit) = current_commit else {
                    return Err(fail(format!("Incorrect usage of \"merge\". Current branch ({})has no commits", current)));
                };
                if !st.branches.has(&other) {
                    return Err(fail(format!("Incorrect usage of \"merge\". Branch to be merged ({}) does not exist", other)));
                }
                let Some(other_commit) = other_commit else {
                    return Err(fail(format!("Incorrect usage of \"merge\". Branch to be merged ({}) has no commits", other)));
                };
                if current_head == other_head && current_commit.id == other_commit.id {
                    return Err(fail("Incorrect usage of \"merge\". Both branches have same head".into()));
                }
                if !custom_id.is_empty() && st.commits.has(&custom_id) {
                    return Err(fail(format!("Incorrect usage of \"merge\". Commit with id:{} already exists, use different custom id", custom_id)));
                }
                let parents = match st.head_commit() {
                    None => Vec::new(),
                    Some(h) => vec![h.id.clone(), other_head.clone().unwrap_or_default()],
                };
                let commit = GitCommit {
                    id: if custom_id.is_empty() { st.next_id() } else { custom_id.clone() },
                    message: format!("merged branch {} into {}", other, current),
                    seq: st.seq,
                    parents,
                    branch: current,
                    type_: MERGE,
                    custom_type: type_.map(type_code),
                    custom_id: !custom_id.is_empty(),
                    tags,
                };
                st.seq += 1;
                st.add(commit);
            }
            StatementAst::Checkout { branch } => st.checkout(&branch)?,
            StatementAst::CherryPicking { id, tags, parent } => {
                let tags = if tags.is_empty() { None } else { Some(tags) };
                let source = id.as_deref().filter(|i| !i.is_empty()).and_then(|i| st.commits.get(i)).cloned();
                let Some(source) = source else {
                    return Err(fail("Incorrect usage of \"cherryPick\". Source commit id should exist and provided".into()));
                };
                let parent = parent.filter(|p| !p.is_empty());
                if let Some(p) = &parent {
                    if !source.parents.contains(p) {
                        return Err(fail("Invalid operation: The specified parent commit is not an immediate parent of the cherry-picked commit.".into()));
                    }
                }
                if source.type_ == MERGE && parent.is_none() {
                    return Err(fail("Incorrect usage of cherry-pick: If the source commit is a merge commit, an immediate parent commit must be specified.".into()));
                }
                if source.branch == st.current {
                    return Err(fail("Incorrect usage of \"cherryPick\". Source commit is already on current branch".into()));
                }
                let current = st.current.clone();
                let current_head = st.branches.get(&current).cloned().flatten();
                if !current_head.as_ref().is_some_and(|h| !h.is_empty() && st.commits.has(h)) {
                    return Err(fail(format!("Incorrect usage of \"cherry-pick\". Current branch ({})has no commits", current)));
                }
                let parents = match st.head_commit() {
                    None => Vec::new(),
                    Some(h) => vec![h.id.clone(), source.id.clone()],
                };
                let commit = GitCommit {
                    id: st.next_id(),
                    message: format!("cherry-picked {} into {}", source.message, current),
                    seq: st.seq,
                    parents,
                    branch: current,
                    type_: CHERRY_PICK,
                    tags: match tags {
                        Some(t) => t.into_iter().filter(|s| !s.is_empty()).collect(),
                        None => vec![format!(
                            "cherry-pick:{}{}",
                            source.id,
                            if source.type_ == MERGE { format!("|parent:{}", parent.clone().unwrap_or_else(|| "undefined".into())) } else { String::new() }
                        )],
                    },
                    custom_type: None,
                    custom_id: false,
                };
                st.seq += 1;
                st.add(commit);
            }
        }
    }

    // A branch made without an order sorts by creation among those of order zero, as in Mermaid.
    let mut lanes: Vec<(String, f64)> = orders.0;
    lanes.sort_by(|a, b| js::ord(a.1 - b.1));
    Ok(GitModel {
        title: ast.title.filter(|t| !t.is_empty()).or(title.map(str::to_string)),
        acc_title: ast.acc_title.filter(|t| !t.is_empty()).map(|t| js::trim_start(&t).to_string()),
        acc_descr: ast.acc_descr.filter(|t| !t.is_empty()).map(|d| newline_dedent(&d)),
        direction,
        commits: st.commits.0.into_iter().map(|(_, c)| c).collect(),
        lanes: lanes.into_iter().map(|(n, _)| n).collect(),
    })
}
