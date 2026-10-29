use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::Hasher;
use std::io;
use std::path::{Path, PathBuf};

use crate::theme;

pub const NUL_SNIFF_BYTES: usize = 8192;
pub const MAX_HL_LINES: usize = 30_000;
pub const MAX_HL_BYTES: usize = 4_000_000;
pub const MAX_SHOWN_BYTES: usize = 20_000;
pub const SYNTECT_CHECKPOINT_INTERVAL: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UndoSnapshot {
    pub lines: Vec<String>,
    pub cursor_row: usize,
    pub cursor_col: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntectCheckpoint {
    pub line_idx: usize,
    pub parse_state: syntect::parsing::ParseState,
    pub highlight_state: syntect::highlighting::HighlightState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Fold {
    pub start_row: usize,
    pub end_row: usize,
}

pub struct Buffer {
    pub path: PathBuf,
    pub lines: Vec<String>,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub scroll_row: usize,
    pub scroll_col: usize,
    pub last_cursor: Option<(usize, usize)>,
    pub dirty: bool,
    pub bom: bool,
    pub crlf: bool,
    pub trailing_newline: bool,
    pub base_lines: Vec<String>,
    pub disk_hash: u64,
    pub is_binary: bool,
    pub total_bytes: usize,
    pub checkpoints: Vec<SyntectCheckpoint>,
    pub undo_stack: Vec<UndoSnapshot>,
    pub redo_stack: Vec<UndoSnapshot>,
    pub ts_highlighter: Option<crate::treesitter::TsHighlighter>,
    pub folds: Vec<Fold>,
}

impl Buffer {
    pub fn load(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let bytes = fs::read(&path)?;
        let total_bytes = bytes.len();
        let disk_hash = compute_hash(&bytes);
        let (lines, bom, crlf, trailing_newline, is_binary) = parse_buffer_content(&bytes);
        let base_lines = lines.clone();

        let mut buf = Self {
            path,
            lines,
            cursor_row: 0,
            cursor_col: 0,
            scroll_row: 0,
            scroll_col: 0,
            last_cursor: None,
            dirty: false,
            bom,
            crlf,
            trailing_newline,
            base_lines,
            disk_hash,
            is_binary,
            total_bytes,
            checkpoints: Vec::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            ts_highlighter: None,
            folds: Vec::new(),
        };
        buf.reset_highlighter();
        Ok(buf)
    }

    pub fn reload(&mut self) -> io::Result<()> {
        let bytes = fs::read(&self.path)?;
        self.disk_hash = compute_hash(&bytes);
        self.total_bytes = bytes.len();
        self.folds.clear();
        let (lines, bom, crlf, trailing_newline, is_binary) = parse_buffer_content(&bytes);
        self.base_lines = lines.clone();
        self.lines = lines;
        self.bom = bom;
        self.crlf = crlf;
        self.trailing_newline = trailing_newline;
        self.is_binary = is_binary;
        self.dirty = false;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.last_cursor = None;
        self.clamp_cursor();
        self.reset_highlighter();
        Ok(())
    }

    pub fn new_empty(path: PathBuf) -> Self {
        let mut buf = Self {
            path,
            lines: vec![String::new()],
            cursor_row: 0,
            cursor_col: 0,
            scroll_row: 0,
            scroll_col: 0,
            last_cursor: None,
            dirty: false,
            bom: false,
            crlf: false,
            trailing_newline: false,
            base_lines: vec![String::new()],
            disk_hash: compute_hash(b""),
            is_binary: false,
            total_bytes: 0,
            checkpoints: Vec::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            ts_highlighter: None,
            folds: Vec::new(),
        };
        buf.reset_highlighter();
        buf
    }

    #[must_use]
    pub fn is_syntax_highlight_disabled(&self) -> bool {
        if self.is_binary || self.lines.len() > MAX_HL_LINES || self.total_bytes > MAX_HL_BYTES {
            return true;
        }
        for line in &self.lines {
            if line.len() > MAX_SHOWN_BYTES {
                return true;
            }
        }
        false
    }

    pub fn invalidate_checkpoints_after(&mut self, row: usize) {
        self.checkpoints.retain(|cp| cp.line_idx <= row);
    }

    pub fn reset_highlighter(&mut self) {
        self.checkpoints.clear();
        self.ts_highlighter = None;
        if !self.is_syntax_highlight_disabled() {
            let syntax = find_syntax_for_path(&self.path, self.lines.first().map(|s| s.as_str()));
            self.checkpoints.push(initial_checkpoint(syntax));

            if let Some(lang_id) = crate::treesitter::LanguageId::for_path(&self.path) {
                if let Some(mut ts) = crate::treesitter::TsHighlighter::new(lang_id) {
                    let text = self.content_bytes();
                    ts.parse_slice(&text, std::time::Duration::from_micros(500));
                    self.ts_highlighter = Some(ts);
                }
            }
        }
    }

    #[must_use]
    pub fn content_bytes(&self) -> Vec<u8> {
        let line_sep = if self.crlf { "\r\n" } else { "\n" };
        let mut content = Vec::with_capacity(self.total_bytes);
        if self.bom {
            content.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
        }
        for (i, line) in self.lines.iter().enumerate() {
            if i > 0 {
                content.extend_from_slice(line_sep.as_bytes());
            }
            content.extend_from_slice(line.as_bytes());
        }
        if self.trailing_newline {
            content.extend_from_slice(line_sep.as_bytes());
        }
        content
    }

    #[must_use]
    pub fn needs_parse(&self) -> bool {
        self.ts_highlighter.as_ref().is_some_and(|ts| ts.needs_parse)
    }

    pub fn parse_more(&mut self, timeout: std::time::Duration) -> bool {
        let needs_parse = self
            .ts_highlighter
            .as_ref()
            .is_some_and(|ts| ts.needs_parse);
        if needs_parse {
            let text = self.content_bytes();
            if let Some(ts) = &mut self.ts_highlighter {
                return ts.parse_slice(&text, timeout);
            }
        }
        false
    }

    pub fn apply_ts_edit(&mut self, edit: tree_sitter::InputEdit) {
        if let Some(ts) = &mut self.ts_highlighter {
            ts.apply_edit(&edit);
        }
    }

    pub fn recompute_total_bytes(&mut self) {
        let sep_len = if self.crlf { 2 } else { 1 };
        self.total_bytes = self.lines.iter().map(|l| l.len() + sep_len).sum();
    }

    pub fn highlight_lines(
        &mut self,
        start_line: usize,
        end_line: usize,
    ) -> Vec<(usize, Vec<(ratatui::style::Style, String)>)> {
        if self.lines.is_empty() {
            return Vec::new();
        }
        let start_line = start_line.min(self.lines.len().saturating_sub(1));
        let end_line = end_line.min(self.lines.len().saturating_sub(1));
        if start_line > end_line {
            return Vec::new();
        }

        if self.is_syntax_highlight_disabled() {
            return (start_line..=end_line)
                .map(|idx| {
                    let text = self.lines[idx].clone();
                    let spans = if text.is_empty() {
                        Vec::new()
                    } else {
                        vec![(ratatui::style::Style::default().fg(theme::TEXT), text)]
                    };
                    (idx, spans)
                })
                .collect();
        }

        let syntax = find_syntax_for_path(&self.path, self.lines.first().map(|s| s.as_str()));
        let highlighter = syntect::highlighting::Highlighter::new(theme::catppuccin_latte_theme());

        if self.checkpoints.is_empty() {
            self.checkpoints.push(initial_checkpoint(syntax));
        }

        let cp_idx = self
            .checkpoints
            .iter()
            .rposition(|cp| cp.line_idx <= start_line)
            .unwrap_or(0);

        let mut cur_line = self.checkpoints[cp_idx].line_idx;
        let mut cur_parse = self.checkpoints[cp_idx].parse_state.clone();
        let mut cur_hl = self.checkpoints[cp_idx].highlight_state.clone();

        while cur_line < start_line {
            let line_content = &self.lines[cur_line];
            let line_with_nl = format!("{line_content}\n");
            if let Ok(ops) = cur_parse.parse_line(&line_with_nl, &theme::SYNTAX_SET) {
                for _ in syntect::highlighting::HighlightIterator::new(
                    &mut cur_hl,
                    &ops[..],
                    &line_with_nl,
                    &highlighter,
                ) {}
            }
            cur_line += 1;
            if cur_line.is_multiple_of(SYNTECT_CHECKPOINT_INTERVAL)
                && !self.checkpoints.iter().any(|cp| cp.line_idx == cur_line)
            {
                self.checkpoints.push(SyntectCheckpoint {
                    line_idx: cur_line,
                    parse_state: cur_parse.clone(),
                    highlight_state: cur_hl.clone(),
                });
                self.checkpoints.sort_by_key(|cp| cp.line_idx);
            }
        }

        let mut result = Vec::with_capacity(end_line - start_line + 1);
        while cur_line <= end_line {
            let line_content = &self.lines[cur_line];
            let line_with_nl = format!("{line_content}\n");
            let spans = if let Ok(ops) = cur_parse.parse_line(&line_with_nl, &theme::SYNTAX_SET) {
                let iter = syntect::highlighting::HighlightIterator::new(
                    &mut cur_hl,
                    &ops[..],
                    &line_with_nl,
                    &highlighter,
                );
                collect_line_spans(iter)
            } else {
                vec![(
                    ratatui::style::Style::default().fg(theme::TEXT),
                    line_content.clone(),
                )]
            };
            result.push((cur_line, spans));
            cur_line += 1;
            if cur_line.is_multiple_of(SYNTECT_CHECKPOINT_INTERVAL)
                && !self.checkpoints.iter().any(|cp| cp.line_idx == cur_line)
            {
                self.checkpoints.push(SyntectCheckpoint {
                    line_idx: cur_line,
                    parse_state: cur_parse.clone(),
                    highlight_state: cur_hl.clone(),
                });
                self.checkpoints.sort_by_key(|cp| cp.line_idx);
            }
        }

        let ts_spans_map = if self.ts_highlighter.is_some() {
            let text = self.content_bytes();
            if let Some(ts) = &mut self.ts_highlighter {
                ts.query_line_spans(&self.lines, &text, start_line, end_line)
            } else {
                None
            }
        } else {
            None
        };

        if let Some(ts_map) = ts_spans_map {
            for (line_idx, spans) in &mut result {
                if let Some(ts_spans) = ts_map.get(line_idx) {
                    if let Some(line_str) = self.lines.get(*line_idx) {
                        *spans = crate::treesitter::overlay_treesitter_spans(
                            line_str,
                            std::mem::take(spans),
                            ts_spans,
                        );
                    }
                }
            }
        }

        result
    }

    pub fn save(&mut self) -> io::Result<()> {
        if self.is_binary {
            self.dirty = false;
            return Ok(());
        }

        let line_sep = if self.crlf { "\r\n" } else { "\n" };
        let mut content = Vec::new();
        if self.bom {
            content.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
        }

        if self.lines.len() == 1 && self.lines[0].is_empty() && !self.trailing_newline {
            // Empty file without trailing newline
        } else {
            for (i, line) in self.lines.iter().enumerate() {
                if i > 0 {
                    content.extend_from_slice(line_sep.as_bytes());
                }
                content.extend_from_slice(line.as_bytes());
            }
            if self.trailing_newline {
                content.extend_from_slice(line_sep.as_bytes());
            }
        }

        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        let parent = if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        };

        let temp_filename = format!(".nide-save-{}", std::process::id());
        let temp_path = parent.join(temp_filename);

        fs::write(&temp_path, &content)?;

        if let Err(e) = fs::rename(&temp_path, &self.path) {
            let _ = fs::remove_file(&temp_path);
            return Err(e);
        }

        self.disk_hash = compute_hash(&content);
        self.base_lines = self.lines.clone();
        self.dirty = false;
        self.total_bytes = content.len();

        Ok(())
    }

    pub fn push_undo(&mut self) {
        let snapshot = UndoSnapshot {
            lines: self.lines.clone(),
            cursor_row: self.cursor_row,
            cursor_col: self.cursor_col,
        };
        if self.undo_stack.last() == Some(&snapshot) {
            return;
        }
        self.undo_stack.push(snapshot);
        if self.undo_stack.len() > 1000 {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    pub fn undo(&mut self) -> bool {
        let Some(prev) = self.undo_stack.pop() else {
            return false;
        };
        let current = UndoSnapshot {
            lines: self.lines.clone(),
            cursor_row: self.cursor_row,
            cursor_col: self.cursor_col,
        };
        self.redo_stack.push(current);
        self.lines = prev.lines;
        self.cursor_row = prev.cursor_row;
        self.cursor_col = prev.cursor_col;
        self.dirty = self.lines != self.base_lines;
        self.clamp_cursor();
        self.invalidate_checkpoints_after(0);
        self.recompute_total_bytes();
        if let Some(ts) = &mut self.ts_highlighter {
            ts.reparse_full();
        }
        self.parse_more(std::time::Duration::from_micros(500));
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo_stack.pop() else {
            return false;
        };
        let current = UndoSnapshot {
            lines: self.lines.clone(),
            cursor_row: self.cursor_row,
            cursor_col: self.cursor_col,
        };
        self.undo_stack.push(current);
        self.lines = next.lines;
        self.cursor_row = next.cursor_row;
        self.cursor_col = next.cursor_col;
        self.dirty = self.lines != self.base_lines;
        self.clamp_cursor();
        self.invalidate_checkpoints_after(0);
        self.recompute_total_bytes();
        if let Some(ts) = &mut self.ts_highlighter {
            ts.reparse_full();
        }
        self.parse_more(std::time::Duration::from_micros(500));
        true
    }

    pub fn clamp_cursor(&mut self) {
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        if self.cursor_row >= self.lines.len() {
            self.cursor_row = self.lines.len().saturating_sub(1);
        }
        if let Some(fold) = self
            .folds
            .iter()
            .find(|f| self.cursor_row > f.start_row && self.cursor_row <= f.end_row)
        {
            self.cursor_row = fold.start_row;
        }
        let line_len = self.lines[self.cursor_row].chars().count();
        if self.cursor_col > line_len {
            self.cursor_col = line_len;
        }
    }

    pub fn insert_char(&mut self, c: char) {
        if self.is_binary {
            return;
        }
        self.clamp_cursor();
        let row = self.cursor_row;
        let col = self.cursor_col;
        let col_byte = char_to_byte_idx(&self.lines[row], col);
        let start_byte = line_col_to_byte_offset(&self.lines, row, col_byte, self.crlf, self.bom);
        let char_len = c.len_utf8();

        let edit = tree_sitter::InputEdit {
            start_byte,
            old_end_byte: start_byte,
            new_end_byte: start_byte + char_len,
            start_position: tree_sitter::Point {
                row,
                column: col_byte,
            },
            old_end_position: tree_sitter::Point {
                row,
                column: col_byte,
            },
            new_end_position: tree_sitter::Point {
                row,
                column: col_byte + char_len,
            },
        };
        self.apply_ts_edit(edit);

        self.lines[row].insert(col_byte, c);
        self.cursor_col += 1;
        self.dirty = true;
        self.total_bytes += char_len;
        self.invalidate_checkpoints_after(row);
        self.parse_more(std::time::Duration::from_micros(500));
    }

    pub fn insert_str(&mut self, s: &str) {
        if self.is_binary {
            return;
        }
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\r' {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                self.insert_newline();
            } else if c == '\n' {
                self.insert_newline();
            } else {
                self.insert_char(c);
            }
        }
    }

    pub fn insert_newline(&mut self) {
        if self.is_binary {
            return;
        }
        self.clamp_cursor();
        let row = self.cursor_row;
        let col = self.cursor_col;
        let col_byte = char_to_byte_idx(&self.lines[row], col);
        let start_byte = line_col_to_byte_offset(&self.lines, row, col_byte, self.crlf, self.bom);
        let sep_len = if self.crlf { 2 } else { 1 };

        let edit = tree_sitter::InputEdit {
            start_byte,
            old_end_byte: start_byte,
            new_end_byte: start_byte + sep_len,
            start_position: tree_sitter::Point {
                row,
                column: col_byte,
            },
            old_end_position: tree_sitter::Point {
                row,
                column: col_byte,
            },
            new_end_position: tree_sitter::Point {
                row: row + 1,
                column: 0,
            },
        };
        self.apply_ts_edit(edit);

        let remainder = self.lines[row].split_off(col_byte);
        self.lines.insert(row + 1, remainder);
        self.cursor_row += 1;
        self.cursor_col = 0;
        self.dirty = true;
        self.total_bytes += sep_len;
        self.invalidate_checkpoints_after(row);
        self.parse_more(std::time::Duration::from_micros(500));
    }

    pub fn backspace(&mut self) {
        if self.is_binary {
            return;
        }
        self.clamp_cursor();
        if self.cursor_col > 0 {
            let row = self.cursor_row;
            let byte_idx_start = char_to_byte_idx(&self.lines[row], self.cursor_col - 1);
            let byte_idx_end = char_to_byte_idx(&self.lines[row], self.cursor_col);
            let removed_len = byte_idx_end - byte_idx_start;
            let start_byte =
                line_col_to_byte_offset(&self.lines, row, byte_idx_start, self.crlf, self.bom);

            let edit = tree_sitter::InputEdit {
                start_byte,
                old_end_byte: start_byte + removed_len,
                new_end_byte: start_byte,
                start_position: tree_sitter::Point {
                    row,
                    column: byte_idx_start,
                },
                old_end_position: tree_sitter::Point {
                    row,
                    column: byte_idx_end,
                },
                new_end_position: tree_sitter::Point {
                    row,
                    column: byte_idx_start,
                },
            };
            self.apply_ts_edit(edit);

            self.lines[row].drain(byte_idx_start..byte_idx_end);
            self.cursor_col -= 1;
            self.dirty = true;
            self.total_bytes = self.total_bytes.saturating_sub(removed_len);
            self.invalidate_checkpoints_after(row);
            self.parse_more(std::time::Duration::from_micros(500));
        } else if self.cursor_row > 0 {
            let row = self.cursor_row;
            let prev_row = row - 1;
            let prev_len = self.lines[prev_row].chars().count();
            let prev_byte_len = self.lines[prev_row].len();
            let sep_len = if self.crlf { 2 } else { 1 };
            let start_byte =
                line_col_to_byte_offset(&self.lines, prev_row, prev_byte_len, self.crlf, self.bom);

            let edit = tree_sitter::InputEdit {
                start_byte,
                old_end_byte: start_byte + sep_len,
                new_end_byte: start_byte,
                start_position: tree_sitter::Point {
                    row: prev_row,
                    column: prev_byte_len,
                },
                old_end_position: tree_sitter::Point {
                    row,
                    column: 0,
                },
                new_end_position: tree_sitter::Point {
                    row: prev_row,
                    column: prev_byte_len,
                },
            };
            self.apply_ts_edit(edit);

            let current_line = self.lines.remove(row);
            self.lines[prev_row].push_str(&current_line);
            self.cursor_row = prev_row;
            self.cursor_col = prev_len;
            self.dirty = true;
            self.total_bytes = self.total_bytes.saturating_sub(sep_len);
            self.invalidate_checkpoints_after(self.cursor_row);
            self.parse_more(std::time::Duration::from_micros(500));
        }
    }

    pub fn delete(&mut self) {
        if self.is_binary {
            return;
        }
        self.clamp_cursor();
        let row = self.cursor_row;
        let line_len = self.lines[row].chars().count();
        if self.cursor_col < line_len {
            let byte_idx_start = char_to_byte_idx(&self.lines[row], self.cursor_col);
            let byte_idx_end = char_to_byte_idx(&self.lines[row], self.cursor_col + 1);
            let removed_len = byte_idx_end - byte_idx_start;
            let start_byte =
                line_col_to_byte_offset(&self.lines, row, byte_idx_start, self.crlf, self.bom);

            let edit = tree_sitter::InputEdit {
                start_byte,
                old_end_byte: start_byte + removed_len,
                new_end_byte: start_byte,
                start_position: tree_sitter::Point {
                    row,
                    column: byte_idx_start,
                },
                old_end_position: tree_sitter::Point {
                    row,
                    column: byte_idx_end,
                },
                new_end_position: tree_sitter::Point {
                    row,
                    column: byte_idx_start,
                },
            };
            self.apply_ts_edit(edit);

            self.lines[row].drain(byte_idx_start..byte_idx_end);
            self.dirty = true;
            self.total_bytes = self.total_bytes.saturating_sub(removed_len);
            self.invalidate_checkpoints_after(row);
            self.parse_more(std::time::Duration::from_micros(500));
        } else if self.cursor_row + 1 < self.lines.len() {
            let sep_len = if self.crlf { 2 } else { 1 };
            let line_byte_len = self.lines[row].len();
            let start_byte =
                line_col_to_byte_offset(&self.lines, row, line_byte_len, self.crlf, self.bom);

            let edit = tree_sitter::InputEdit {
                start_byte,
                old_end_byte: start_byte + sep_len,
                new_end_byte: start_byte,
                start_position: tree_sitter::Point {
                    row,
                    column: line_byte_len,
                },
                old_end_position: tree_sitter::Point {
                    row: row + 1,
                    column: 0,
                },
                new_end_position: tree_sitter::Point {
                    row,
                    column: line_byte_len,
                },
            };
            self.apply_ts_edit(edit);

            let next_line = self.lines.remove(row + 1);
            self.lines[row].push_str(&next_line);
            self.dirty = true;
            self.total_bytes = self.total_bytes.saturating_sub(sep_len);
            self.invalidate_checkpoints_after(row);
            self.parse_more(std::time::Duration::from_micros(500));
        }
    }

    pub fn move_up(&mut self) {
        if self.cursor_row > 0 {
            let mut target = self.cursor_row - 1;
            while let Some(fold) = self
                .folds
                .iter()
                .find(|f| target > f.start_row && target <= f.end_row)
            {
                target = fold.start_row;
                if target == 0 {
                    break;
                }
            }
            self.cursor_row = target;
            self.clamp_cursor();
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor_row + 1 < self.lines.len() {
            let mut target = self.cursor_row + 1;
            while target < self.lines.len() {
                if let Some(fold) = self
                    .folds
                    .iter()
                    .find(|f| target > f.start_row && target <= f.end_row)
                {
                    target = fold.end_row + 1;
                } else {
                    break;
                }
            }
            if target < self.lines.len() {
                self.cursor_row = target;
                self.clamp_cursor();
            }
        }
    }

    #[must_use]
    pub fn is_line_folded(&self, row: usize) -> bool {
        self.folds
            .iter()
            .any(|f| row > f.start_row && row <= f.end_row)
    }

    #[must_use]
    pub fn get_fold_at_start(&self, row: usize) -> Option<&Fold> {
        self.folds.iter().find(|f| f.start_row == row)
    }

    pub fn add_fold(&mut self, start_row: usize, end_row: usize) {
        if end_row > start_row
            && !self
                .folds
                .iter()
                .any(|f| f.start_row == start_row && f.end_row == end_row)
        {
            self.folds.push(Fold { start_row, end_row });
            self.folds.sort_by_key(|f| f.start_row);
        }
    }

    pub fn remove_fold_at(&mut self, row: usize) -> bool {
        if let Some(pos) = self.folds.iter().position(|f| f.start_row == row) {
            self.folds.remove(pos);
            true
        } else if let Some(pos) = self
            .folds
            .iter()
            .position(|f| row > f.start_row && row <= f.end_row)
        {
            self.folds.remove(pos);
            true
        } else {
            false
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        }
    }

    pub fn move_right(&mut self) {
        self.clamp_cursor();
        let line_len = self.lines[self.cursor_row].chars().count();
        if self.cursor_col < line_len {
            self.cursor_col += 1;
        }
    }

    pub fn move_home(&mut self) {
        self.cursor_col = 0;
    }

    pub fn move_end(&mut self) {
        self.clamp_cursor();
        self.cursor_col = self.lines[self.cursor_row].chars().count();
    }

    pub fn page_up(&mut self, page_size: usize) {
        self.cursor_row = self.cursor_row.saturating_sub(page_size);
        self.clamp_cursor();
    }

    pub fn page_down(&mut self, page_size: usize) {
        self.cursor_row = (self.cursor_row + page_size).min(self.lines.len().saturating_sub(1));
        self.clamp_cursor();
    }

    pub fn adjust_scroll(&mut self, visible_height: usize) {
        if visible_height == 0 {
            return;
        }
        if self.cursor_row < self.scroll_row {
            self.scroll_row = self.cursor_row;
        } else if self.cursor_row >= self.scroll_row + visible_height {
            self.scroll_row = self.cursor_row - visible_height + 1;
        }
    }
}

#[must_use]
pub fn find_syntax_for_path(
    path: &Path,
    first_line: Option<&str>,
) -> &'static syntect::parsing::SyntaxReference {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        if let Some(syntax) = theme::SYNTAX_SET.find_syntax_by_extension(ext) {
            return syntax;
        }
    }
    if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
        if let Some(syntax) = theme::SYNTAX_SET.find_syntax_by_extension(file_name) {
            return syntax;
        }
    }
    if let Some(first_line) = first_line {
        if let Some(syntax) = theme::SYNTAX_SET.find_syntax_by_first_line(first_line) {
            return syntax;
        }
    }
    theme::SYNTAX_SET.find_syntax_plain_text()
}

#[must_use]
pub fn initial_checkpoint(syntax: &'static syntect::parsing::SyntaxReference) -> SyntectCheckpoint {
    let highlighter = syntect::highlighting::Highlighter::new(theme::catppuccin_latte_theme());
    let parse_state = syntect::parsing::ParseState::new(syntax);
    let highlight_state = syntect::highlighting::HighlightState::new(
        &highlighter,
        syntect::parsing::ScopeStack::new(),
    );
    SyntectCheckpoint {
        line_idx: 0,
        parse_state,
        highlight_state,
    }
}

pub fn collect_line_spans<'a>(
    iter: impl Iterator<Item = (syntect::highlighting::Style, &'a str)>,
) -> Vec<(ratatui::style::Style, String)> {
    let mut raw_spans: Vec<(ratatui::style::Style, String)> = Vec::new();
    for (syn_style, text) in iter {
        let rat_style = theme::syntect_style_to_ratatui(syn_style);
        raw_spans.push((rat_style, text.to_string()));
    }

    if let Some((_, last_text)) = raw_spans.last_mut() {
        if last_text.ends_with("\r\n") {
            last_text.truncate(last_text.len() - 2);
        } else if last_text.ends_with('\n') || last_text.ends_with('\r') {
            last_text.truncate(last_text.len() - 1);
        }
    }

    raw_spans
        .into_iter()
        .filter(|(_, text)| !text.is_empty())
        .collect()
}

#[must_use]
pub fn parse_buffer_content(bytes: &[u8]) -> (Vec<String>, bool, bool, bool, bool) {
    let (bom, text_bytes) = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        (true, &bytes[3..])
    } else {
        (false, bytes)
    };

    let is_binary = text_bytes.iter().take(NUL_SNIFF_BYTES).any(|&b| b == 0);

    let (lines, crlf, trailing_newline) = if is_binary {
        (vec!["[Binary file: editing disabled]".to_string()], false, false)
    } else {
        let text = String::from_utf8_lossy(text_bytes);
        let crlf = text.contains("\r\n");
        let trailing_newline = text.ends_with("\r\n") || text.ends_with('\n');

        let trimmed_text = if let Some(stripped) = text.strip_suffix("\r\n") {
            stripped
        } else if let Some(stripped) = text.strip_suffix('\n') {
            stripped
        } else {
            &text[..]
        };

        let lines = if trimmed_text.is_empty() {
            vec![String::new()]
        } else {
            trimmed_text
                .split('\n')
                .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
                .collect()
        };

        (lines, crlf, trailing_newline)
    };

    (lines, bom, crlf, trailing_newline, is_binary)
}

#[must_use]
pub fn char_to_byte_idx(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(idx, _)| idx)
        .unwrap_or(s.len())
}

#[must_use]
pub fn line_col_to_byte_offset(
    lines: &[String],
    row: usize,
    col_byte: usize,
    crlf: bool,
    bom: bool,
) -> usize {
    let sep_len = if crlf { 2 } else { 1 };
    let mut offset = if bom { 3 } else { 0 };
    for line in lines.iter().take(row) {
        offset += line.len() + sep_len;
    }
    offset + col_byte
}

#[must_use]
pub fn compute_hash(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    hasher.write(bytes);
    hasher.finish()
}
