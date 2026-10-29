use std::path::{Path, PathBuf};

use regex::Regex;

use crate::app::{App, Location, MiddleMode};
use crate::buffer::Buffer;
use crate::picker::PickItem;
use crate::search::symbols::{extract_symbols_from_source, is_code_file};

#[must_use]
pub fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[must_use]
pub fn extract_ident_at(line: &str, char_col: usize) -> Option<String> {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    if chars.is_empty() {
        return None;
    }

    let target_idx = if char_col < chars.len() && is_ident_char(chars[char_col].1) {
        char_col
    } else if char_col > 0 && char_col <= chars.len() && is_ident_char(chars[char_col - 1].1) {
        char_col - 1
    } else {
        return None;
    };

    let mut start = target_idx;
    while start > 0 && is_ident_char(chars[start - 1].1) {
        start -= 1;
    }

    let mut end = target_idx;
    while end + 1 < chars.len() && is_ident_char(chars[end + 1].1) {
        end += 1;
    }

    let byte_start = chars[start].0;
    let byte_end = if end + 1 < chars.len() {
        chars[end + 1].0
    } else {
        line.len()
    };

    let ident = &line[byte_start..byte_end];
    if ident.is_empty() || ident == "_" || ident.chars().all(|c| c.is_ascii_digit()) {
        None
    } else {
        Some(ident.to_string())
    }
}

#[must_use]
pub fn extract_ident_at_cursor(app: &App) -> Option<String> {
    let buf = app.buf.as_ref()?;
    if app.line >= buf.lines.len() {
        return None;
    }
    extract_ident_at(&buf.lines[app.line], app.col)
}

#[must_use]
pub fn extract_imported_names(lines: &[String]) -> Vec<String> {
    let mut names = Vec::new();
    for line in lines {
        let trimmed = line.trim();
        if trimmed.starts_with("use ")
            || trimmed.starts_with("mod ")
            || trimmed.starts_with("import ")
            || trimmed.starts_with("from ")
            || trimmed.starts_with("#include")
            || trimmed.starts_with("require(")
        {
            for part in trimmed.split(|c: char| !c.is_alphanumeric() && c != '_') {
                let p = part.trim();
                if !p.is_empty()
                    && p != "use"
                    && p != "mod"
                    && p != "import"
                    && p != "from"
                    && p != "include"
                    && p != "require"
                    && p != "crate"
                    && p != "super"
                    && p != "self"
                    && p != "pub"
                    && p != "default"
                    && p != "as"
                {
                    names.push(p.to_lowercase());
                }
            }
        }
    }
    names
}

#[must_use]
pub fn find_local_definitions(
    root_dir: &Path,
    buf: &Buffer,
    ident: &str,
    current_row: usize,
) -> Vec<PickItem> {
    let mut local_items = Vec::new();

    // 1. Check Tree-sitter / regex symbols in current buffer
    let text = buf.lines.join("\n");
    let file_symbols = extract_symbols_from_source(&buf.path, &text);
    for sym in file_symbols {
        if sym.name == ident && sym.line != current_row {
            local_items.push(crate::search::symbols::symbol_to_pick_item(root_dir, &sym));
        }
    }

    // 2. Check local variable bindings if not already found in symbols
    let escaped = regex::escape(ident);
    let binding_patterns = [
        format!(r"\blet\s+(?:mut\s+)?{escaped}\b"),
        format!(r"\bconst\s+{escaped}\b"),
        format!(r"\bvar\s+{escaped}\b"),
        format!(r"\b{escaped}\s*:="),
        format!(r"^\s*{escaped}\s*="),
    ];
    let regexes: Vec<Regex> = binding_patterns
        .iter()
        .filter_map(|p| Regex::new(p).ok())
        .collect();

    for (line_idx, line) in buf.lines.iter().enumerate() {
        if line_idx == current_row || crate::app::usages::is_comment_line(line) {
            continue;
        }
        for re in &regexes {
            if let Some(mat) = re.find(line) {
                if let Some(ident_offset) = mat.as_str().find(ident) {
                    let byte_offset = mat.start() + ident_offset;
                    let char_col = line[..byte_offset].chars().count();
                    if !local_items.iter().any(|item| item.line == Some(line_idx)) {
                        let rel_path = buf.path.strip_prefix(root_dir).unwrap_or(&buf.path);
                        let display_rel = rel_path.to_string_lossy().replace('\\', "/");
                        let display_text =
                            format!("{}:{}  {}", display_rel, line_idx + 1, line.trim());
                        local_items.push(PickItem {
                            path: buf.path.clone(),
                            display_text,
                            score: 0,
                            match_indices: Vec::new(),
                            line: Some(line_idx),
                            col: Some(char_col),
                        });
                    }
                }
                break;
            }
        }
    }

    local_items
}

#[must_use]
pub fn find_project_definitions(
    root_dir: &Path,
    files: &[PathBuf],
    current_buf: Option<&Buffer>,
    ident: &str,
) -> Vec<PickItem> {
    let current_path = current_buf.map(|b| b.path.as_path());
    let current_dir = current_path.and_then(|p| p.parent());
    let imported_names = current_buf
        .map(|b| extract_imported_names(&b.lines))
        .unwrap_or_default();

    let mut candidates: Vec<(u32, PickItem)> = Vec::new();

    for file in files {
        let full_path = if file.is_absolute() {
            file.clone()
        } else {
            root_dir.join(file)
        };

        if current_path == Some(&full_path) {
            continue;
        }

        if !is_code_file(&full_path) {
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

            let file_symbols = extract_symbols_from_source(&full_path, &content);
            for sym in file_symbols {
                if sym.name == ident {
                    let item = crate::search::symbols::symbol_to_pick_item(root_dir, &sym);

                    let file_stem = full_path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .map(|s| s.to_lowercase())
                        .unwrap_or_default();
                    let file_name = full_path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .map(|s| s.to_lowercase())
                        .unwrap_or_default();

                    let is_imported = imported_names.contains(&file_stem)
                        || imported_names.contains(&file_name);
                    let is_same_dir = current_dir == full_path.parent();

                    let rank = if is_imported {
                        0
                    } else if is_same_dir {
                        1
                    } else {
                        2
                    };

                    candidates.push((rank, item));
                }
            }
        }
    }

    candidates.sort_by(|(r1, a), (r2, b)| {
        r1.cmp(r2)
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.line.cmp(&b.line))
    });

    candidates.into_iter().map(|(_, item)| item).collect()
}

#[must_use]
pub fn find_definitions(app: &App, ident: &str) -> Vec<PickItem> {
    if let Some(buf) = &app.buf {
        let local_defs = find_local_definitions(&app.root_dir, buf, ident, app.line);
        if !local_defs.is_empty() {
            return local_defs;
        }
    }

    find_project_definitions(&app.root_dir, &app.files, app.buf.as_ref(), ident)
}

pub fn goto_definition(app: &mut App) {
    let Some(ident) = extract_ident_at_cursor(app) else {
        return;
    };

    let defs = find_definitions(app, &ident);

    if defs.is_empty() {
        app.status_message = Some(format!("No definition found for '{ident}'"));
        return;
    }

    if defs.len() == 1 {
        let target = &defs[0];
        let path = target.path.clone();
        let line = target.line.unwrap_or(0);
        let col = target.col.unwrap_or(0);

        if let Some(buf) = &app.buf {
            app.history.push(Location::new(
                buf.path.clone(),
                buf.cursor_row,
                buf.cursor_col,
            ));
        }

        let _ = app.open_file_at(&path, line, col);
        app.middle_mode = MiddleMode::Normal;
    } else {
        crate::app::picker::open_definition_picker(app, defs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_ident_at() {
        let line = "let helper = foo_bar(123);";

        // Cursor on 'helper'
        assert_eq!(extract_ident_at(line, 4), Some("helper".to_string()));
        assert_eq!(extract_ident_at(line, 7), Some("helper".to_string()));
        assert_eq!(extract_ident_at(line, 9), Some("helper".to_string()));

        // Cursor on 'foo_bar'
        assert_eq!(extract_ident_at(line, 13), Some("foo_bar".to_string()));
        assert_eq!(extract_ident_at(line, 16), Some("foo_bar".to_string()));
        assert_eq!(extract_ident_at(line, 19), Some("foo_bar".to_string()));

        // Cursor on whitespace
        assert_eq!(extract_ident_at(line, 3), Some("let".to_string())); // adjacent to 'let'
        assert_eq!(extract_ident_at(line, 11), None); // '='

        // Cursor on number literal
        assert_eq!(extract_ident_at(line, 21), None); // '123'
    }

    #[test]
    fn test_extract_imported_names() {
        let lines = vec![
            "use crate::lib::helper;".to_string(),
            "mod my_module;".to_string(),
            "import { Component } from './utils';".to_string(),
        ];
        let imported = extract_imported_names(&lines);
        assert!(imported.contains(&"lib".to_string()));
        assert!(imported.contains(&"helper".to_string()));
        assert!(imported.contains(&"my_module".to_string()));
        assert!(imported.contains(&"utils".to_string()));
    }
}
