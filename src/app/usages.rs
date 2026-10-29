use std::path::{Path, PathBuf};

use regex::Regex;

use crate::app::App;
use crate::buffer::Buffer;
use crate::picker::{PickItem, Picker};

#[must_use]
pub fn is_comment_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("//")
        || trimmed.starts_with('#')
        || trimmed.starts_with("/*")
        || trimmed.starts_with('*')
        || trimmed.starts_with("--")
        || trimmed.starts_with(';')
        || trimmed.starts_with("<!--")
}

#[must_use]
fn find_inline_comment_start(line: &str) -> Option<usize> {
    if let Some(idx) = line.find("//") {
        // Ensure not part of "://" (URL protocol)
        if idx == 0 || line.as_bytes().get(idx - 1) != Some(&b':') {
            return Some(idx);
        }
    }
    if let Some(idx) = line.find('#') {
        // Ensure preceded by whitespace or at line start
        if idx == 0
            || line.as_bytes().get(idx - 1) == Some(&b' ')
            || line.as_bytes().get(idx - 1) == Some(&b'\t')
        {
            return Some(idx);
        }
    }
    None
}

#[must_use]
pub fn scan_usages(
    root_dir: &Path,
    files: &[PathBuf],
    current_buf: Option<&Buffer>,
    ident: &str,
) -> Vec<PickItem> {
    if ident.is_empty() {
        return Vec::new();
    }

    let pattern = format!(r"\b{}\b", regex::escape(ident));
    let Ok(re) = Regex::new(&pattern) else {
        return Vec::new();
    };

    let mut items = Vec::new();
    let current_path = current_buf.map(|b| b.path.as_path());

    let mut process_file_lines = |file_path: &Path, lines: &[String]| {
        let rel_path = file_path.strip_prefix(root_dir).unwrap_or(file_path);
        let display_rel = rel_path.to_string_lossy().replace('\\', "/");

        for (line_idx, line) in lines.iter().enumerate() {
            if is_comment_line(line) {
                continue;
            }

            let comment_offset = find_inline_comment_start(line);

            for mat in re.find_iter(line) {
                if let Some(comm_start) = comment_offset {
                    if mat.start() >= comm_start {
                        continue;
                    }
                }

                let line_1_based = line_idx + 1;
                let trimmed = line.trim();
                let display_text = format!("{}:{}: {}", display_rel, line_1_based, trimmed);

                let mut match_indices = Vec::new();
                if let Some(prefix_len) = display_text.find(trimmed) {
                    let trim_leading_spaces = line.len() - line.trim_start().len();
                    if mat.start() >= trim_leading_spaces {
                        let rel_mat_start = mat.start() - trim_leading_spaces;
                        let byte_start = prefix_len + rel_mat_start;
                        let byte_end = byte_start + mat.as_str().len();
                        for (ch_idx, (b_idx, _)) in display_text.char_indices().enumerate() {
                            if b_idx >= byte_start && b_idx < byte_end {
                                match_indices.push(ch_idx as u32);
                            }
                        }
                    }
                }

                let char_col = line[..mat.start()].chars().count();

                items.push(PickItem {
                    path: file_path.to_path_buf(),
                    display_text,
                    score: 0,
                    match_indices,
                    line: Some(line_idx),
                    col: Some(char_col),
                });
            }
        }
    };

    // Process current buffer lines first if loaded
    if let Some(buf) = current_buf {
        process_file_lines(&buf.path, &buf.lines);
    }

    // Process all other files
    for file in files {
        let full_path = if file.is_absolute() {
            file.clone()
        } else {
            root_dir.join(file)
        };

        if current_path == Some(&full_path) {
            continue;
        }

        if !full_path.is_file() {
            continue;
        }

        if let Ok(meta) = full_path.metadata() {
            if meta.len() > 1024 * 1024 {
                continue;
            }
        } else {
            continue;
        }

        if let Ok(content) = std::fs::read_to_string(&full_path) {
            if !content.contains(ident) {
                continue;
            }
            let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
            process_file_lines(&full_path, &lines);
        }
    }

    items
}

pub fn find_usages(app: &mut App) {
    let Some(ident) = crate::app::definition::extract_ident_at_cursor(app) else {
        return;
    };

    let items = scan_usages(&app.root_dir, &app.files, app.buf.as_ref(), &ident);

    if items.is_empty() {
        app.status_message = Some(format!("No usages found for '{ident}'"));
    }

    let picker = Picker::new_usages(app.root_dir.clone(), items);
    app.picker = Some(picker);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_is_comment_line() {
        assert!(is_comment_line("// simple comment"));
        assert!(is_comment_line("   /// doc comment"));
        assert!(is_comment_line("# python comment"));
        assert!(is_comment_line("/* block comment */"));
        assert!(is_comment_line(" * continuation"));
        assert!(is_comment_line("-- sql comment"));
        assert!(is_comment_line("; ini comment"));
        assert!(is_comment_line("<!-- html comment -->"));
        assert!(!is_comment_line("let x = 1;"));
        assert!(!is_comment_line("fn helper() {}"));
    }

    #[test]
    fn test_scan_usages_filters_comments_and_matches_boundaries() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();

        let file1 = root.join("main.rs");
        let file2 = root.join("lib.rs");

        std::fs::write(
            &file1,
            "fn main() {\n    // helper(); in comment\n    helper();\n    helper_other();\n}\n",
        )
        .unwrap();

        std::fs::write(&file2, "pub fn helper() {\n    let val = helper(); // inline comment\n}\n")
            .unwrap();

        let files = vec![file1.clone(), file2.clone()];
        let usages = scan_usages(&root, &files, None, "helper");

        // Should find:
        // 1. main.rs:3: helper();
        // 2. lib.rs:1: pub fn helper()
        // 3. lib.rs:2: let val = helper();
        // Should NOT find:
        // - main.rs:2 (comment line)
        // - main.rs:4 (helper_other)
        assert_eq!(usages.len(), 3);
        assert!(usages.iter().any(|u| u.path == file1 && u.line == Some(2))); // 0-indexed line 2 is line 3
        assert!(usages.iter().any(|u| u.path == file2 && u.line == Some(0)));
        assert!(usages.iter().any(|u| u.path == file2 && u.line == Some(1)));
    }
}
