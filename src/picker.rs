use std::path::{Path, PathBuf};

use nucleo::pattern::{CaseMatching, Normalization, Pattern};
use nucleo::{Config, Matcher, Utf32Str};

use crate::buffer::Buffer;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerKind {
    Files,
    Symbols,
    Grep,
    Usages,
    Definitions,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickItem {
    pub path: PathBuf,
    pub display_text: String,
    pub score: u32,
    pub match_indices: Vec<u32>,
    pub line: Option<usize>,
    pub col: Option<usize>,
}

pub struct Picker {
    pub kind: PickerKind,
    pub query: String,
    pub all_files: Vec<PathBuf>,
    pub all_symbols: Vec<PickItem>,
    pub items: Vec<PickItem>,
    pub selected_idx: usize,
    pub scroll: usize,
    pub preview: Option<Buffer>,
    pub preview_path: Option<PathBuf>,
    pub root_dir: PathBuf,
}

impl Picker {
    #[must_use]
    pub fn new(root_dir: PathBuf, all_files: Vec<PathBuf>, recent_files: &[PathBuf]) -> Self {
        let mut picker = Self {
            kind: PickerKind::Files,
            query: String::new(),
            all_files,
            all_symbols: Vec::new(),
            items: Vec::new(),
            selected_idx: 0,
            scroll: 0,
            preview: None,
            preview_path: None,
            root_dir,
        };
        picker.refilter(recent_files);
        picker
    }

    #[must_use]
    pub fn new_symbols(root_dir: PathBuf, symbols: Vec<PickItem>) -> Self {
        let mut picker = Self {
            kind: PickerKind::Symbols,
            query: String::new(),
            all_files: Vec::new(),
            all_symbols: symbols.clone(),
            items: symbols,
            selected_idx: 0,
            scroll: 0,
            preview: None,
            preview_path: None,
            root_dir,
        };
        picker.update_preview();
        picker
    }

    #[must_use]
    pub fn new_usages(root_dir: PathBuf, items: Vec<PickItem>) -> Self {
        let mut picker = Self {
            kind: PickerKind::Usages,
            query: String::new(),
            all_files: Vec::new(),
            all_symbols: items.clone(),
            items,
            selected_idx: 0,
            scroll: 0,
            preview: None,
            preview_path: None,
            root_dir,
        };
        picker.update_preview();
        picker
    }

    #[must_use]
    pub fn new_definitions(root_dir: PathBuf, items: Vec<PickItem>) -> Self {
        let mut picker = Self {
            kind: PickerKind::Definitions,
            query: String::new(),
            all_files: Vec::new(),
            all_symbols: items.clone(),
            items,
            selected_idx: 0,
            scroll: 0,
            preview: None,
            preview_path: None,
            root_dir,
        };
        picker.update_preview();
        picker
    }

    #[must_use]
    pub fn new_grep(root_dir: PathBuf) -> Self {
        Self {
            kind: PickerKind::Grep,
            query: String::new(),
            all_files: Vec::new(),
            all_symbols: Vec::new(),
            items: Vec::new(),
            selected_idx: 0,
            scroll: 0,
            preview: None,
            preview_path: None,
            root_dir,
        }
    }

    pub fn refilter(&mut self, recent_files: &[PathBuf]) {
        match self.kind {
            PickerKind::Files => self.refilter_files(recent_files),
            PickerKind::Symbols | PickerKind::Usages | PickerKind::Definitions => {
                self.refilter_symbols();
            }
            PickerKind::Grep => {
                if self.query.is_empty() {
                    self.items.clear();
                    self.selected_idx = 0;
                    self.scroll = 0;
                    self.preview = None;
                    self.preview_path = None;
                }
            }
        }
    }

    fn refilter_files(&mut self, recent_files: &[PathBuf]) {
        let is_in_all = |rf: &Path| {
            self.all_files
                .iter()
                .any(|f| f == rf || self.root_dir.join(f) == *rf || rf.ends_with(f))
        };

        if self.query.is_empty() {
            let mut items = Vec::new();
            let mut seen = std::collections::HashSet::new();

            for rf in recent_files {
                if is_in_all(rf) {
                    let rel = rf.strip_prefix(&self.root_dir).unwrap_or(rf);
                    let display_text = rel.to_string_lossy().replace('\\', "/");
                    if seen.insert(display_text.clone()) {
                        items.push(PickItem {
                            path: rf.clone(),
                            display_text,
                            score: 0,
                            match_indices: Vec::new(),
                            line: None,
                            col: None,
                        });
                    }
                }
            }

            for file in &self.all_files {
                let rel = file.strip_prefix(&self.root_dir).unwrap_or(file);
                let display_text = rel.to_string_lossy().replace('\\', "/");
                if seen.insert(display_text.clone()) {
                    items.push(PickItem {
                        path: file.clone(),
                        display_text,
                        score: 0,
                        match_indices: Vec::new(),
                        line: None,
                        col: None,
                    });
                }
            }

            self.items = items;
        } else {
            let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
            let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
            let mut matched_items = Vec::new();

            for file in &self.all_files {
                let rel = file.strip_prefix(&self.root_dir).unwrap_or(file);
                let display_text = rel.to_string_lossy().replace('\\', "/");

                let mut char_buf = Vec::new();
                let haystack = Utf32Str::new(&display_text, &mut char_buf);
                let mut match_indices = Vec::new();

                if let Some(mut score) = pattern.indices(haystack, &mut matcher, &mut match_indices)
                {
                    if let Some(pos) = recent_files.iter().position(|rf| {
                        rf == file || self.root_dir.join(file) == *rf || rf.ends_with(file)
                    }) {
                        let bonus = (recent_files.len().saturating_sub(pos) as u32) * 50;
                        score = score.saturating_add(bonus);
                    }

                    matched_items.push(PickItem {
                        path: file.clone(),
                        display_text,
                        score,
                        match_indices,
                        line: None,
                        col: None,
                    });
                }
            }

            matched_items.sort_by(|a, b| {
                b.score
                    .cmp(&a.score)
                    .then_with(|| a.display_text.len().cmp(&b.display_text.len()))
                    .then_with(|| a.display_text.cmp(&b.display_text))
            });

            self.items = matched_items;
        }

        if self.items.is_empty() {
            self.selected_idx = 0;
            self.scroll = 0;
        } else if self.selected_idx >= self.items.len() {
            self.selected_idx = self.items.len() - 1;
        }

        self.update_preview();
    }

    fn refilter_symbols(&mut self) {
        if self.query.is_empty() {
            self.items = self.all_symbols.clone();
        } else {
            let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
            let mut matcher = Matcher::new(Config::DEFAULT);
            let mut matched_items = Vec::new();

            for item in &self.all_symbols {
                let mut char_buf = Vec::new();
                let haystack = Utf32Str::new(&item.display_text, &mut char_buf);
                let mut match_indices = Vec::new();

                if let Some(score) = pattern.indices(haystack, &mut matcher, &mut match_indices) {
                    matched_items.push(PickItem {
                        path: item.path.clone(),
                        display_text: item.display_text.clone(),
                        score,
                        match_indices,
                        line: item.line,
                        col: item.col,
                    });
                }
            }

            matched_items.sort_by(|a, b| {
                b.score
                    .cmp(&a.score)
                    .then_with(|| a.display_text.len().cmp(&b.display_text.len()))
                    .then_with(|| a.display_text.cmp(&b.display_text))
            });

            self.items = matched_items;
        }

        if self.items.is_empty() {
            self.selected_idx = 0;
            self.scroll = 0;
        } else if self.selected_idx >= self.items.len() {
            self.selected_idx = self.items.len() - 1;
        }

        self.update_preview();
    }

    pub fn update_preview(&mut self) {
        if self.items.is_empty() {
            self.preview = None;
            self.preview_path = None;
            return;
        }

        let selected_path = &self.items[self.selected_idx].path;
        if self.preview_path.as_ref() == Some(selected_path) {
            return;
        }

        let full_path = if selected_path.is_absolute() {
            selected_path.clone()
        } else {
            self.root_dir.join(selected_path)
        };

        if full_path.is_file() {
            self.preview = Buffer::load(&full_path).ok();
            self.preview_path = Some(selected_path.clone());
        } else {
            self.preview = None;
            self.preview_path = None;
        }
    }

    pub fn select_next(&mut self) {
        if !self.items.is_empty() && self.selected_idx + 1 < self.items.len() {
            self.selected_idx += 1;
            self.update_preview();
        }
    }

    pub fn select_prev(&mut self) {
        if self.selected_idx > 0 {
            self.selected_idx -= 1;
            self.update_preview();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_picker_empty_query_ranks_recent_files_first() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();

        let f1 = PathBuf::from("a_file.rs");
        let f2 = PathBuf::from("b_file.rs");
        let f3 = PathBuf::from("c_file.rs");
        let all_files = vec![f1.clone(), f2.clone(), f3.clone()];
        let recent_files = vec![f3.clone(), f1.clone()];

        let picker = Picker::new(root, all_files, &recent_files);
        assert_eq!(picker.items.len(), 3);
        assert_eq!(picker.items[0].path, f3);
        assert_eq!(picker.items[1].path, f1);
        assert_eq!(picker.items[2].path, f2);
    }

    #[test]
    fn test_picker_fuzzy_matching_and_recent_boost() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();

        let f1 = PathBuf::from("src/editor.rs");
        let f2 = PathBuf::from("tests/editor_test.rs");
        let all_files = vec![f1.clone(), f2.clone()];

        let mut picker = Picker::new(root.clone(), all_files.clone(), &[]);
        picker.query = "editor".to_string();
        picker.refilter(&[]);
        assert_eq!(picker.items.len(), 2);

        let mut picker_boost = Picker::new(root, all_files, std::slice::from_ref(&f2));
        picker_boost.query = "editor".to_string();
        picker_boost.refilter(std::slice::from_ref(&f2));
        assert_eq!(picker_boost.items.len(), 2);
        assert_eq!(picker_boost.items[0].path, f2);
        assert!(!picker_boost.items[0].match_indices.is_empty());
    }

    #[test]
    fn test_picker_preview_loading() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();
        let test_file = temp.path().join("preview.rs");
        std::fs::write(&test_file, "fn preview_function() {}\n").unwrap();

        let all_files = vec![PathBuf::from("preview.rs")];
        let picker = Picker::new(root, all_files, &[]);

        assert_eq!(picker.items.len(), 1);
        assert!(picker.preview.is_some());
        assert_eq!(
            picker.preview.as_ref().unwrap().lines,
            vec!["fn preview_function() {}"]
        );
    }

    #[test]
    fn test_symbol_picker_filtering() {
        let item1 = PickItem {
            path: PathBuf::from("src/a.rs"),
            display_text: "alpha_fn (fn)  src/a.rs:1".to_string(),
            score: 0,
            match_indices: Vec::new(),
            line: Some(0),
            col: Some(0),
        };
        let item2 = PickItem {
            path: PathBuf::from("src/b.rs"),
            display_text: "beta_struct (struct)  src/b.rs:10".to_string(),
            score: 0,
            match_indices: Vec::new(),
            line: Some(9),
            col: Some(0),
        };

        let mut picker = Picker::new_symbols(PathBuf::from("."), vec![item1.clone(), item2.clone()]);
        assert_eq!(picker.items.len(), 2);

        picker.query = "beta".to_string();
        picker.refilter(&[]);
        assert_eq!(picker.items.len(), 1);
        assert_eq!(picker.items[0].path, item2.path);
    }
}
