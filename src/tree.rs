use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::WalkBuilder;
use unicase::UniCase;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub path: PathBuf,
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
    pub ignored: bool,
}

impl Node {
    #[must_use]
    pub fn name(&self) -> &str {
        self.path
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or("")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tree {
    pub root: PathBuf,
    pub nodes: Vec<Node>,
    pub cursor: usize,
    pub scroll: usize,
}

struct RawNode {
    path: PathBuf,
    name: String,
    is_dir: bool,
    ignored: bool,
    children: Vec<RawNode>,
}

impl RawNode {
    fn new_root() -> Self {
        Self {
            path: PathBuf::new(),
            name: String::new(),
            is_dir: true,
            ignored: false,
            children: Vec::new(),
        }
    }

    fn new_dir(path: PathBuf, name: String, ignored: bool) -> Self {
        Self {
            path,
            name,
            is_dir: true,
            ignored,
            children: Vec::new(),
        }
    }

    fn new_file(path: PathBuf, name: String, ignored: bool) -> Self {
        Self {
            path,
            name,
            is_dir: false,
            ignored,
            children: Vec::new(),
        }
    }

    fn insert(&mut self, rel_path: &Path, is_dir: bool, ignored: bool) {
        let components: Vec<_> = rel_path.iter().collect();
        if components.is_empty() {
            return;
        }
        self.insert_components(&components, 0, PathBuf::new(), is_dir, ignored);
    }

    fn insert_components(
        &mut self,
        components: &[&OsStr],
        idx: usize,
        current_path: PathBuf,
        is_dir: bool,
        ignored: bool,
    ) {
        if idx >= components.len() {
            return;
        }

        let comp_str = components[idx].to_string_lossy().to_string();
        let next_path = current_path.join(components[idx]);
        let is_last = idx == components.len() - 1;

        if is_last {
            if let Some(child) = self.children.iter_mut().find(|c| c.name == comp_str) {
                child.is_dir = is_dir;
                child.ignored = ignored;
            } else {
                let node = if is_dir {
                    RawNode::new_dir(next_path, comp_str, ignored)
                } else {
                    RawNode::new_file(next_path, comp_str, ignored)
                };
                self.children.push(node);
            }
        } else {
            let child_idx = if let Some(pos) = self
                .children
                .iter()
                .position(|c| c.name == comp_str && c.is_dir)
            {
                pos
            } else {
                let dir_node = RawNode::new_dir(next_path.clone(), comp_str, false);
                self.children.push(dir_node);
                self.children.len() - 1
            };
            self.children[child_idx].insert_components(
                components,
                idx + 1,
                next_path,
                is_dir,
                ignored,
            );
        }
    }

    fn sort_recursive(&mut self) {
        self.children.sort_by(|a, b| {
            (!a.is_dir)
                .cmp(&(!b.is_dir))
                .then_with(|| UniCase::new(&a.name).cmp(&UniCase::new(&b.name)))
                .then_with(|| a.name.cmp(&b.name))
        });
        for child in &mut self.children {
            child.sort_recursive();
        }
    }

    fn flatten_into(&self, depth: usize, out_nodes: &mut Vec<Node>, out_files: &mut Vec<PathBuf>) {
        for child in &self.children {
            out_nodes.push(Node {
                path: child.path.clone(),
                depth,
                is_dir: child.is_dir,
                expanded: false,
                ignored: child.ignored,
            });
            if !child.is_dir && !child.ignored {
                out_files.push(child.path.clone());
            }
            if child.is_dir {
                child.flatten_into(depth + 1, out_nodes, out_files);
            }
        }
    }
}

fn build_gitignore(root: &Path) -> Gitignore {
    let mut builder = GitignoreBuilder::new(root);
    let root_gi = root.join(".gitignore");
    if root_gi.is_file() {
        let _ = builder.add(&root_gi);
    }

    // Discover any nested .gitignore files (skipping .git)
    for entry in WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(false)
        .filter_entry(|e| e.file_name() != ".git")
        .build()
        .flatten()
    {
        if entry.file_name() == ".gitignore" && entry.path() != root_gi {
            let _ = builder.add(entry.path());
        }
    }

    builder.build().unwrap_or_else(|_| Gitignore::empty())
}

impl Tree {
    #[must_use]
    pub fn walk(root: impl AsRef<Path>) -> (Self, Vec<PathBuf>) {
        let root = root.as_ref().canonicalize().unwrap_or_else(|_| root.as_ref().to_path_buf());
        let gitignore = build_gitignore(&root);

        let mut raw_root = RawNode::new_root();

        let mut wb = WalkBuilder::new(&root);
        wb.hidden(false)
            .git_ignore(false)
            .filter_entry({
                let root_path = root.clone();
                let gi = gitignore.clone();
                move |entry| {
                    if entry.file_name() == ".git" {
                        return false;
                    }
                    if let Ok(rel) = entry.path().strip_prefix(&root_path) {
                        if let Some(parent) = rel.parent() {
                            if !parent.as_os_str().is_empty()
                                && gi
                                    .matched_path_or_any_parents(root_path.join(parent), true)
                                    .is_ignore()
                            {
                                return false;
                            }
                        }
                    }
                    true
                }
            });

        for result in wb.build() {
            let entry = match result {
                Ok(e) => e,
                Err(_) => continue,
            };

            if entry.depth() == 0 {
                continue;
            }

            let rel_path = match entry.path().strip_prefix(&root) {
                Ok(p) => p.to_path_buf(),
                Err(_) => continue,
            };

            let is_dir = entry.file_type().is_some_and(|ft| ft.is_dir());
            let ignored = gitignore
                .matched_path_or_any_parents(entry.path(), is_dir)
                .is_ignore();

            raw_root.insert(&rel_path, is_dir, ignored);
        }

        raw_root.sort_recursive();

        let mut nodes = Vec::new();
        let mut files = Vec::new();
        raw_root.flatten_into(0, &mut nodes, &mut files);

        let tree = Self {
            root,
            nodes,
            cursor: 0,
            scroll: 0,
        };

        (tree, files)
    }

    #[must_use]
    pub fn visible_indices(&self) -> Vec<usize> {
        let mut visible = Vec::new();
        let mut hide_depth: Option<usize> = None;

        for (idx, node) in self.nodes.iter().enumerate() {
            if let Some(hide) = hide_depth {
                if node.depth > hide {
                    continue;
                }
                hide_depth = None;
            }

            visible.push(idx);

            if node.is_dir && !node.expanded {
                hide_depth = Some(node.depth);
            }
        }

        visible
    }

    #[must_use]
    pub fn visible_nodes(&self) -> Vec<&Node> {
        let visible = self.visible_indices();
        visible.into_iter().map(|idx| &self.nodes[idx]).collect()
    }

    #[must_use]
    pub fn selected_node(&self) -> Option<&Node> {
        let visible = self.visible_indices();
        visible.get(self.cursor).map(|&idx| &self.nodes[idx])
    }

    #[must_use]
    pub fn selected_path(&self) -> Option<&Path> {
        self.selected_node().map(|node| node.path.as_path())
    }

    pub fn move_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn move_down(&mut self) {
        let count = self.visible_indices().len();
        if count > 0 && self.cursor + 1 < count {
            self.cursor += 1;
        }
    }

    pub fn toggle_expanded(&mut self) {
        let visible = self.visible_indices();
        if visible.is_empty() {
            return;
        }
        let node_idx = visible[self.cursor.min(visible.len() - 1)];
        if self.nodes[node_idx].is_dir {
            if !self.nodes[node_idx].expanded {
                if self.nodes[node_idx].ignored {
                    self.lazy_expand_ignored(node_idx);
                }
                self.nodes[node_idx].expanded = true;
            } else {
                self.nodes[node_idx].expanded = false;
            }
            let new_visible = self.visible_indices();
            if self.cursor >= new_visible.len() {
                self.cursor = new_visible.len().saturating_sub(1);
            }
        }
    }

    pub fn expand_dir(&mut self, node_idx: usize) {
        if node_idx < self.nodes.len() && self.nodes[node_idx].is_dir {
            if self.nodes[node_idx].ignored {
                self.lazy_expand_ignored(node_idx);
            }
            self.nodes[node_idx].expanded = true;
        }
    }

    pub fn lazy_expand_ignored(&mut self, node_idx: usize) {
        if node_idx >= self.nodes.len() {
            return;
        }
        let node = &self.nodes[node_idx];
        if !node.is_dir || !node.ignored {
            return;
        }

        // Check if children are already loaded
        if node_idx + 1 < self.nodes.len() {
            let next = &self.nodes[node_idx + 1];
            if next.depth > node.depth && next.path.starts_with(&node.path) {
                return;
            }
        }

        let dir_path = self.root.join(&node.path);
        let Ok(entries) = std::fs::read_dir(&dir_path) else {
            return;
        };

        let depth = node.depth + 1;
        let mut raw_children = Vec::new();

        for entry in entries.flatten() {
            if entry.file_name() == ".git" {
                continue;
            }
            let file_type = entry.file_type().ok();
            let is_dir = file_type.is_some_and(|ft| ft.is_dir());
            let rel_path = match entry.path().strip_prefix(&self.root) {
                Ok(p) => p.to_path_buf(),
                Err(_) => continue,
            };
            let name = entry.file_name().to_string_lossy().to_string();

            raw_children.push((rel_path, name, is_dir));
        }

        raw_children.sort_by(|a, b| {
            (!a.2)
                .cmp(&(!b.2))
                .then_with(|| UniCase::new(&a.1).cmp(&UniCase::new(&b.1)))
                .then_with(|| a.1.cmp(&b.1))
        });

        let child_nodes: Vec<Node> = raw_children
            .into_iter()
            .map(|(path, _name, is_dir)| Node {
                path,
                depth,
                is_dir,
                expanded: false,
                ignored: true,
            })
            .collect();

        self.nodes.splice(node_idx + 1..node_idx + 1, child_nodes);
    }

    pub fn collapse_or_parent(&mut self) {
        let visible = self.visible_indices();
        if visible.is_empty() {
            return;
        }
        let node_idx = visible[self.cursor.min(visible.len() - 1)];
        let node = &self.nodes[node_idx];

        if node.is_dir && node.expanded {
            self.nodes[node_idx].expanded = false;
            let new_visible = self.visible_indices();
            if self.cursor >= new_visible.len() {
                self.cursor = new_visible.len().saturating_sub(1);
            }
        } else if let Some(parent_path) = node.path.parent() {
            if !parent_path.as_os_str().is_empty() {
                for (vis_idx, &idx) in visible.iter().enumerate() {
                    if self.nodes[idx].is_dir && self.nodes[idx].path == parent_path {
                        self.cursor = vis_idx;
                        break;
                    }
                }
            }
        }
    }

    pub fn reveal(&mut self, path: &Path) -> bool {
        let canon = path.canonicalize().ok();
        let rel_path = canon
            .as_deref()
            .and_then(|p| p.strip_prefix(&self.root).ok())
            .or_else(|| path.strip_prefix(&self.root).ok())
            .unwrap_or(path);
        let rel_path = match rel_path.strip_prefix(".") {
            Ok(p) => p,
            Err(_) => rel_path,
        };

        let components: Vec<_> = rel_path.iter().collect();
        if components.is_empty() {
            return false;
        }

        // Expand all ancestor directories
        let mut current_ancestor = PathBuf::new();
        for comp in &components[..components.len().saturating_sub(1)] {
            current_ancestor.push(comp);
            let mut idx = 0;
            while idx < self.nodes.len() {
                if self.nodes[idx].is_dir && self.nodes[idx].path == current_ancestor {
                    if self.nodes[idx].ignored {
                        self.lazy_expand_ignored(idx);
                    }
                    self.nodes[idx].expanded = true;
                    break;
                }
                idx += 1;
            }
        }

        // Locate target in visible indices
        let visible = self.visible_indices();
        for (vis_idx, &node_idx) in visible.iter().enumerate() {
            if self.nodes[node_idx].path == rel_path {
                self.cursor = vis_idx;
                return true;
            }
        }

        false
    }

    pub fn expand_all(&mut self) {
        for node in &mut self.nodes {
            if node.is_dir {
                node.expanded = true;
            }
        }
    }

    pub fn collapse_all(&mut self) {
        for node in &mut self.nodes {
            if node.is_dir {
                node.expanded = false;
            }
        }
        self.cursor = 0;
        self.scroll = 0;
    }

    pub fn adjust_scroll(&mut self, visible_height: usize) {
        if visible_height == 0 {
            return;
        }
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if self.cursor >= self.scroll + visible_height {
            self.scroll = self.cursor - visible_height + 1;
        }
    }
}
