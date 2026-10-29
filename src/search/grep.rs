use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

use grep_matcher::Matcher;
use grep_regex::RegexMatcherBuilder;
use grep_searcher::{Searcher, SearcherBuilder, Sink, SinkMatch};

use crate::app::Msg;
use crate::picker::PickItem;

struct GrepSink<'a> {
    current_path: PathBuf,
    root_dir: &'a Path,
    matches: Vec<PickItem>,
    max_results: usize,
}

impl<'a> Sink for GrepSink<'a> {
    type Error = io::Error;

    fn matched(&mut self, _searcher: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        let line_1_based = mat.line_number().unwrap_or(1);
        let line_0_based = (line_1_based.saturating_sub(1)) as usize;

        let bytes = mat.bytes();
        let raw_line = String::from_utf8_lossy(bytes);
        let trimmed_line = raw_line.trim_end_matches(&['\r', '\n'][..]);

        let rel_path = self
            .current_path
            .strip_prefix(self.root_dir)
            .unwrap_or(&self.current_path);
        let display_rel = rel_path.to_string_lossy().replace('\\', "/");
        let display_text = format!(
            "{}:{}: {}",
            display_rel,
            line_1_based,
            trimmed_line.trim_start()
        );

        self.matches.push(PickItem {
            path: self.current_path.clone(),
            display_text,
            score: 0,
            match_indices: Vec::new(),
            line: Some(line_0_based),
            col: None,
        });

        if self.matches.len() >= self.max_results {
            Ok(false) // stop search
        } else {
            Ok(true)
        }
    }
}

pub fn run_grep(
    query: &str,
    root_dir: &Path,
    files: &[PathBuf],
    max_results: usize,
) -> Vec<PickItem> {
    if query.trim().is_empty() {
        return Vec::new();
    }

    let matcher = match RegexMatcherBuilder::new().case_smart(true).build(query) {
        Ok(m) => m,
        Err(_) => {
            let escaped = regex::escape(query);
            match RegexMatcherBuilder::new().case_smart(true).build(&escaped) {
                Ok(m) => m,
                Err(_) => return Vec::new(),
            }
        }
    };

    let mut searcher = SearcherBuilder::new().line_number(true).build();
    let mut sink = GrepSink {
        current_path: PathBuf::new(),
        root_dir,
        matches: Vec::new(),
        max_results,
    };

    for file_path in files {
        let full_path = if file_path.is_absolute() {
            file_path.clone()
        } else {
            root_dir.join(file_path)
        };

        if !full_path.is_file() {
            continue;
        }

        // Skip files exceeding 5MB
        if let Ok(meta) = full_path.metadata() {
            if meta.len() > 5 * 1024 * 1024 {
                continue;
            }
        }

        sink.current_path = full_path.clone();
        let _ = searcher.search_path(&matcher, &full_path, &mut sink);

        if sink.matches.len() >= max_results {
            break;
        }
    }

    // Populate match_indices in display_text for visual highlighting
    for item in &mut sink.matches {
        if let Ok(Some(mat)) = matcher.find(item.display_text.as_bytes()) {
            let start = mat.start();
            let end = mat.end();
            for (ch_idx, (byte_idx, _)) in item.display_text.char_indices().enumerate() {
                if byte_idx >= start && byte_idx < end {
                    item.match_indices.push(ch_idx as u32);
                }
            }
        }
    }

    sink.matches
}

pub fn spawn_grep(
    query: String,
    seq: u64,
    root_dir: PathBuf,
    files: Vec<PathBuf>,
    tx: mpsc::Sender<Msg>,
) {
    thread::spawn(move || {
        let rows = run_grep(&query, &root_dir, &files, 1000);
        let _ = tx.send(Msg::SearchDone { seq, rows });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_grep_basic_matching() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();

        let file1 = root.join("hello.rs");
        let file2 = root.join("world.py");
        std::fs::write(&file1, "fn main() {\n    println!(\"TARGET_MATCH\");\n}\n").unwrap();
        std::fs::write(&file2, "def test():\n    print('TARGET_MATCH')\n").unwrap();

        let files = vec![file1.clone(), file2.clone()];
        let rows = run_grep("TARGET_MATCH", &root, &files, 100);

        assert_eq!(rows.len(), 2);
        assert!(rows[0].display_text.contains("TARGET_MATCH"));
        assert!(!rows[0].match_indices.is_empty());
        assert_eq!(rows[0].line, Some(1)); // line 2 in file = 0-indexed line 1
    }

    #[test]
    fn test_grep_smart_case() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();

        let file = root.join("sample.txt");
        std::fs::write(&file, "apple\nApple\nBANANA\n").unwrap();
        let files = vec![file];

        // Lowercase query matches case-insensitively
        let lower_rows = run_grep("apple", &root, &files, 10);
        assert_eq!(lower_rows.len(), 2);

        // Mixed-case query matches case-sensitively
        let upper_rows = run_grep("Apple", &root, &files, 10);
        assert_eq!(upper_rows.len(), 1);
    }
}
