use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::app::{rect_contains, App, Focus};
use crate::buffer::Buffer;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MiddleMode {
    #[default]
    Normal,
    Edit,
    Conflict,
    Find,
    View,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor_row: usize,
    pub anchor_col: usize,
}

impl Selection {
    #[must_use]
    pub fn new(anchor_row: usize, anchor_col: usize) -> Self {
        Self {
            anchor_row,
            anchor_col,
        }
    }

    /// Returns `((start_row, start_col), (end_row, end_col))` with start <= end.
    #[must_use]
    pub fn range(&self, cursor_row: usize, cursor_col: usize) -> ((usize, usize), (usize, usize)) {
        if (self.anchor_row, self.anchor_col) <= (cursor_row, cursor_col) {
            ((self.anchor_row, self.anchor_col), (cursor_row, cursor_col))
        } else {
            ((cursor_row, cursor_col), (self.anchor_row, self.anchor_col))
        }
    }

    #[must_use]
    pub fn is_empty(&self, cursor_row: usize, cursor_col: usize) -> bool {
        self.anchor_row == cursor_row && self.anchor_col == cursor_col
    }
}

#[must_use]
pub fn format_osc52_sequence(text: &str) -> String {
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    format!("\x1b]52;c;{encoded}\x07")
}

pub fn osc52_copy(text: &str) {
    let osc52 = format_osc52_sequence(text);
    use std::io::Write;
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(osc52.as_bytes());
    let _ = stdout.flush();

    if std::env::var_os("TMUX").is_some() {
        if let Ok(mut child) = std::process::Command::new("tmux")
            .args(["load-buffer", "-"])
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
        }
    }
}

pub fn copy_to_clipboard(app: &mut App, text: String) {
    app.clipboard = Some(text.clone());
    let osc52 = format_osc52_sequence(&text);
    app.last_osc52 = Some(osc52);
    osc52_copy(&text);
}

pub fn get_selected_text(buf: &Buffer, selection: Selection) -> Option<String> {
    let ((s_row, s_col), (e_row, e_col)) = selection.range(buf.cursor_row, buf.cursor_col);
    if (s_row, s_col) == (e_row, e_col) {
        return None;
    }
    if s_row >= buf.lines.len() {
        return None;
    }
    let e_row = e_row.min(buf.lines.len().saturating_sub(1));
    let line_sep = if buf.crlf { "\r\n" } else { "\n" };

    if s_row == e_row {
        let line = &buf.lines[s_row];
        let chars: Vec<char> = line.chars().collect();
        let s = s_col.min(chars.len());
        let e = e_col.min(chars.len());
        if s >= e {
            return None;
        }
        Some(chars[s..e].iter().collect())
    } else {
        let mut result = String::new();
        for r in s_row..=e_row {
            let line = &buf.lines[r];
            let chars: Vec<char> = line.chars().collect();
            if r == s_row {
                let s = s_col.min(chars.len());
                let sub: String = chars[s..].iter().collect();
                result.push_str(&sub);
                result.push_str(line_sep);
            } else if r == e_row {
                let e = e_col.min(chars.len());
                let sub: String = chars[..e].iter().collect();
                result.push_str(&sub);
            } else {
                result.push_str(line);
                result.push_str(line_sep);
            }
        }
        Some(result)
    }
}

pub fn delete_selection(buf: &mut Buffer, selection: Selection) -> Option<String> {
    let selected_text = get_selected_text(buf, selection)?;
    let ((s_row, s_col), (e_row, e_col)) = selection.range(buf.cursor_row, buf.cursor_col);
    if s_row >= buf.lines.len() {
        return None;
    }
    let e_row = e_row.min(buf.lines.len().saturating_sub(1));

    if s_row == e_row {
        let (start_byte, end_byte, removed_len, offset) = {
            let line = &buf.lines[s_row];
            let sb = crate::buffer::char_to_byte_idx(line, s_col);
            let eb = crate::buffer::char_to_byte_idx(line, e_col);
            let off = crate::buffer::line_col_to_byte_offset(
                &buf.lines, s_row, sb, buf.crlf, buf.bom,
            );
            (sb, eb, eb - sb, off)
        };

        let edit = tree_sitter::InputEdit {
            start_byte: offset,
            old_end_byte: offset + removed_len,
            new_end_byte: offset,
            start_position: tree_sitter::Point {
                row: s_row,
                column: start_byte,
            },
            old_end_position: tree_sitter::Point {
                row: s_row,
                column: end_byte,
            },
            new_end_position: tree_sitter::Point {
                row: s_row,
                column: start_byte,
            },
        };
        buf.apply_ts_edit(edit);

        buf.lines[s_row].drain(start_byte..end_byte);
    } else {
        let start_byte = crate::buffer::char_to_byte_idx(&buf.lines[s_row], s_col);
        let end_byte = crate::buffer::char_to_byte_idx(&buf.lines[e_row], e_col);
        let start_offset = crate::buffer::line_col_to_byte_offset(
            &buf.lines, s_row, start_byte, buf.crlf, buf.bom,
        );
        let end_offset = crate::buffer::line_col_to_byte_offset(
            &buf.lines, e_row, end_byte, buf.crlf, buf.bom,
        );

        let edit = tree_sitter::InputEdit {
            start_byte: start_offset,
            old_end_byte: end_offset,
            new_end_byte: start_offset,
            start_position: tree_sitter::Point {
                row: s_row,
                column: start_byte,
            },
            old_end_position: tree_sitter::Point {
                row: e_row,
                column: end_byte,
            },
            new_end_position: tree_sitter::Point {
                row: s_row,
                column: start_byte,
            },
        };
        buf.apply_ts_edit(edit);

        let prefix: String = buf.lines[s_row][..start_byte].to_string();
        let suffix: String = buf.lines[e_row][end_byte..].to_string();

        buf.lines.drain((s_row + 1)..=e_row);
        buf.lines[s_row] = format!("{prefix}{suffix}");
    }

    buf.cursor_row = s_row;
    buf.cursor_col = s_col;
    buf.clamp_cursor();
    buf.dirty = true;
    buf.invalidate_checkpoints_after(s_row);
    buf.recompute_total_bytes();
    buf.parse_more(std::time::Duration::from_micros(500));

    Some(selected_text)
}

pub fn copy_selection_or_line(app: &mut App) {
    let text_to_copy = {
        let Some(buf) = &app.buf else { return };
        if let Some(sel) = app.selection {
            get_selected_text(buf, sel)
        } else if buf.cursor_row < buf.lines.len() {
            let line = &buf.lines[buf.cursor_row];
            let line_sep = if buf.crlf { "\r\n" } else { "\n" };
            Some(format!("{line}{line_sep}"))
        } else {
            None
        }
    };

    if let Some(text) = text_to_copy {
        copy_to_clipboard(app, text);
    }
}

pub fn cut_selection_or_line(app: &mut App) {
    let text_to_copy = {
        let Some(buf) = &mut app.buf else { return };
        buf.push_undo();

        if let Some(sel) = app.selection.take() {
            delete_selection(buf, sel)
        } else if buf.cursor_row < buf.lines.len() {
            let row = buf.cursor_row;
            let line = buf.lines[row].clone();
            let line_sep = if buf.crlf { "\r\n" } else { "\n" };
            if buf.lines.len() > 1 {
                let start_byte = crate::buffer::line_col_to_byte_offset(
                    &buf.lines, row, 0, buf.crlf, buf.bom,
                );
                let sep_len = if buf.crlf { 2 } else { 1 };
                let removed_len = line.len() + sep_len;
                let edit = tree_sitter::InputEdit {
                    start_byte,
                    old_end_byte: start_byte + removed_len,
                    new_end_byte: start_byte,
                    start_position: tree_sitter::Point { row, column: 0 },
                    old_end_position: tree_sitter::Point {
                        row: row + 1,
                        column: 0,
                    },
                    new_end_position: tree_sitter::Point { row, column: 0 },
                };
                buf.apply_ts_edit(edit);
                buf.lines.remove(row);
                buf.clamp_cursor();
            } else {
                let start_byte = crate::buffer::line_col_to_byte_offset(
                    &buf.lines, 0, 0, buf.crlf, buf.bom,
                );
                let removed_len = line.len();
                let edit = tree_sitter::InputEdit {
                    start_byte,
                    old_end_byte: start_byte + removed_len,
                    new_end_byte: start_byte,
                    start_position: tree_sitter::Point { row: 0, column: 0 },
                    old_end_position: tree_sitter::Point {
                        row: 0,
                        column: removed_len,
                    },
                    new_end_position: tree_sitter::Point { row: 0, column: 0 },
                };
                buf.apply_ts_edit(edit);
                buf.lines[0].clear();
                buf.cursor_col = 0;
            }
            buf.dirty = true;
            buf.invalidate_checkpoints_after(row);
            buf.recompute_total_bytes();
            buf.parse_more(std::time::Duration::from_micros(500));
            Some(format!("{line}{line_sep}"))
        } else {
            None
        }
    };

    if let Some(text) = text_to_copy {
        copy_to_clipboard(app, text);
        if let Some(buf) = &app.buf {
            if buf.dirty {
                app.mark_edited();
            } else {
                app.dirty = false;
            }
        }
    }
}

pub fn paste_text(app: &mut App, text: &str) {
    let Some(buf) = &mut app.buf else { return };
    buf.push_undo();

    if let Some(sel) = app.selection.take() {
        delete_selection(buf, sel);
    }

    buf.insert_str(text);
    app.mark_edited();
    app.selection = None;
    app.sync_cursor();
}

pub fn jump_next_hunk(app: &mut App) {
    let Some(diff) = &app.diff else { return };
    if diff.hunks.is_empty() {
        return;
    }
    let Some(buf) = &mut app.buf else { return };

    app.selection = None;
    let current_row = buf.cursor_row;
    let next = diff.hunks.iter().find(|h| h.target_line() > current_row);
    let target = match next {
        Some(h) => h.target_line(),
        None => diff.hunks[0].target_line(),
    };

    buf.cursor_row = target;
    buf.cursor_col = 0;
    buf.clamp_cursor();
    buf.last_cursor = None;
    let visible_height = app
        .layout
        .middle
        .map_or(24, |r| r.height.saturating_sub(2) as usize);
    buf.adjust_scroll(visible_height);
    app.sync_cursor();
}

pub fn jump_prev_hunk(app: &mut App) {
    let Some(diff) = &app.diff else { return };
    if diff.hunks.is_empty() {
        return;
    }
    let Some(buf) = &mut app.buf else { return };

    app.selection = None;
    let current_row = buf.cursor_row;
    let prev = diff.hunks.iter().rev().find(|h| h.target_line() < current_row);
    let target = match prev {
        Some(h) => h.target_line(),
        None => diff.hunks.last().unwrap().target_line(),
    };

    buf.cursor_row = target;
    buf.cursor_col = 0;
    buf.clamp_cursor();
    buf.last_cursor = None;
    let visible_height = app
        .layout
        .middle
        .map_or(24, |r| r.height.saturating_sub(2) as usize);
    buf.adjust_scroll(visible_height);
    app.sync_cursor();
}

pub fn handle_key(app: &mut App, key: KeyEvent) {
    match app.middle_mode {
        MiddleMode::Normal => handle_normal_key(app, key),
        MiddleMode::Edit => handle_edit_key(app, key),
        MiddleMode::Conflict => crate::app::conflict::handle_key(app, key),
        MiddleMode::Find => crate::app::find::handle_key(app, key),
        MiddleMode::View => handle_view_key(app, key),
    }
}

fn handle_view_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            if app.is_visible(Focus::Tree) {
                app.set_focus(Focus::Tree);
            }
        }
        KeyCode::Char('?') => {
            app.toggle_help();
        }
        _ => {}
    }
}

fn handle_normal_key(app: &mut App, key: KeyEvent) {
    let has_ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let has_shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let has_super = key.modifiers.contains(KeyModifiers::SUPER);

    if has_ctrl || has_super {
        match key.code {
            KeyCode::Char('z') => {
                if let Some(buf) = &mut app.buf {
                    if buf.undo() {
                        app.dirty = buf.dirty;
                        if app.dirty {
                            app.last_edit = Some(std::time::Instant::now());
                        } else {
                            app.last_edit = None;
                        }
                        app.selection = None;
                    }
                }
                return;
            }
            KeyCode::Char('y') => {
                if let Some(buf) = &mut app.buf {
                    if buf.redo() {
                        app.dirty = buf.dirty;
                        if app.dirty {
                            app.last_edit = Some(std::time::Instant::now());
                        } else {
                            app.last_edit = None;
                        }
                        app.selection = None;
                    }
                }
                return;
            }
            KeyCode::Char('c') => {
                copy_selection_or_line(app);
                return;
            }
            KeyCode::Char('x') => {
                cut_selection_or_line(app);
                return;
            }
            KeyCode::Char('v') => {
                if let Some(text) = app.clipboard.clone() {
                    paste_text(app, &text);
                }
                return;
            }
            KeyCode::Char('f') => {
                crate::app::find::start_find(app);
                return;
            }
            _ => {}
        }
    }

    match key.code {
        KeyCode::Char('c') if !has_ctrl && !has_super => {
            if has_shift {
                jump_prev_hunk(app);
            } else {
                jump_next_hunk(app);
            }
        }
        KeyCode::Char('C') if !has_ctrl && !has_super => {
            jump_prev_hunk(app);
        }
        KeyCode::Char('o') if !has_ctrl && !has_super => {
            crate::app::picker::open_file_picker(app);
        }
        KeyCode::Char('d')
            if !has_shift
                && !has_ctrl
                && !has_super
                && app.selection.is_none() =>
        {
            crate::app::definition::goto_definition(app);
        }
        KeyCode::Char('D') | KeyCode::Char('d')
            if (key.code == KeyCode::Char('D') || has_shift)
                && !has_ctrl
                && !has_super
                && app.selection.is_none() =>
        {
            crate::app::picker::open_symbol_picker(app);
        }
        KeyCode::Char('u') if !has_shift && !has_ctrl && !has_super => {
            crate::app::usages::find_usages(app);
        }
        KeyCode::Char('[') if !has_ctrl && !has_super => {
            app.jump_back();
        }
        KeyCode::Char(']') if !has_ctrl && !has_super => {
            app.jump_forward();
        }
        KeyCode::Char('s') if !has_ctrl && !has_super => {
            crate::app::picker::open_grep_picker(app);
        }
        KeyCode::Char('/') if !has_ctrl && !has_super => {
            crate::app::find::start_find(app);
        }
        KeyCode::Char('n') if !has_shift && !has_ctrl && !has_super => {
            crate::app::find::step_next(app);
        }
        KeyCode::Char('n') if has_shift && !has_ctrl && !has_super => {
            crate::app::find::step_prev(app);
        }
        KeyCode::Char('N') if !has_ctrl && !has_super => {
            crate::app::find::step_prev(app);
        }
        KeyCode::Char('v') => {
            if app.selection.is_none() {
                if let Some(buf) = &app.buf {
                    app.selection = Some(Selection::new(buf.cursor_row, buf.cursor_col));
                }
            } else {
                app.selection = None;
            }
        }
        KeyCode::Esc => {
            if app.preview_mode {
                app.preview_mode = false;
                if let Some(line) = app.preview_lines.get(app.preview_cursor) {
                    if let Some(buf) = &mut app.buf {
                        buf.cursor_row = line.source_line;
                        buf.clamp_cursor();
                        app.sync_cursor();
                    }
                }
            } else if app.selection.is_some() {
                app.selection = None;
            } else if app.find.is_some() {
                crate::app::find::cancel_find(app);
            } else if app.show_tree {
                app.set_focus(Focus::Tree);
            }
        }
        KeyCode::Char('f') if !has_ctrl && !has_super => {
            if !app.preview_mode {
                crate::app::collapse::toggle_fold(app);
            }
        }
        KeyCode::Char('p') if !has_ctrl && !has_super => {
            if app.preview_mode {
                app.preview_mode = false;
                if let Some(line) = app.preview_lines.get(app.preview_cursor) {
                    if let Some(buf) = &mut app.buf {
                        buf.cursor_row = line.source_line;
                        buf.clamp_cursor();
                        app.sync_cursor();
                    }
                }
            } else if let Some(buf) = &app.buf {
                if crate::markdown::is_previewable(&buf.path) {
                    app.preview_mode = true;
                    app.update_preview();
                } else {
                    app.status_message = Some("Preview not supported for this file type (supported: .md, .csv, .tsv, .jsonl)".to_string());
                }
            }
        }
        KeyCode::Char('i') | KeyCode::Enter => {
            if app.preview_mode {
                if let Some(line) = app.preview_lines.get(app.preview_cursor) {
                    let target_line = line.source_line;
                    if let Some(buf) = &mut app.buf {
                        buf.cursor_row = target_line;
                        buf.cursor_col = 0;
                        buf.clamp_cursor();
                        let visible_height = app
                            .layout
                            .middle
                            .map_or(24, |r| r.height.saturating_sub(2) as usize);
                        buf.adjust_scroll(visible_height);
                        app.sync_cursor();
                    }
                }
                app.preview_mode = false;
                app.selection = None;
                app.find = None;
                app.middle_mode = MiddleMode::Edit;
                return;
            }
            if let Some(buf) = &app.buf {
                if !buf.is_binary {
                    app.selection = None;
                    app.find = None;
                    app.middle_mode = MiddleMode::Edit;
                }
            }
        }
        KeyCode::Char('y') if app.selection.is_some() => {
            copy_selection_or_line(app);
            app.selection = None;
        }
        KeyCode::Char('d') | KeyCode::Char('x') if app.selection.is_some() => {
            cut_selection_or_line(app);
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if app.preview_mode {
                if app.preview_cursor + 1 < app.preview_lines.len() {
                    app.preview_cursor += 1;
                    if let Some(line) = app.preview_lines.get(app.preview_cursor) {
                        if let Some(buf) = &mut app.buf {
                            buf.cursor_row = line.source_line;
                            app.sync_cursor();
                        }
                    }
                }
                return;
            }
            if has_shift && app.selection.is_none() {
                if let Some(buf) = &app.buf {
                    app.selection = Some(Selection::new(buf.cursor_row, buf.cursor_col));
                }
            }
            if let Some(buf) = &mut app.buf {
                buf.move_down();
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.preview_mode {
                app.preview_cursor = app.preview_cursor.saturating_sub(1);
                if let Some(line) = app.preview_lines.get(app.preview_cursor) {
                    if let Some(buf) = &mut app.buf {
                        buf.cursor_row = line.source_line;
                        app.sync_cursor();
                    }
                }
                return;
            }
            if has_shift && app.selection.is_none() {
                if let Some(buf) = &app.buf {
                    app.selection = Some(Selection::new(buf.cursor_row, buf.cursor_col));
                }
            }
            if let Some(buf) = &mut app.buf {
                buf.move_up();
            }
        }
        KeyCode::Char('h') | KeyCode::Left => {
            if has_shift && app.selection.is_none() {
                if let Some(buf) = &app.buf {
                    app.selection = Some(Selection::new(buf.cursor_row, buf.cursor_col));
                }
            }
            if let Some(buf) = &mut app.buf {
                buf.move_left();
            }
        }
        KeyCode::Char('l') | KeyCode::Right => {
            if has_shift && app.selection.is_none() {
                if let Some(buf) = &app.buf {
                    app.selection = Some(Selection::new(buf.cursor_row, buf.cursor_col));
                }
            }
            if let Some(buf) = &mut app.buf {
                buf.move_right();
            }
        }
        KeyCode::Char('G') => {
            if app.preview_mode {
                app.preview_cursor = app.preview_lines.len().saturating_sub(1);
                if let Some(line) = app.preview_lines.get(app.preview_cursor) {
                    if let Some(buf) = &mut app.buf {
                        buf.cursor_row = line.source_line;
                        buf.cursor_col = 0;
                        buf.last_cursor = None;
                        app.sync_cursor();
                    }
                }
                return;
            }
            if let Some(buf) = &mut app.buf {
                buf.cursor_row = buf.lines.len().saturating_sub(1);
                buf.cursor_col = 0;
                buf.clamp_cursor();
                buf.last_cursor = None;
                let visible_height = app
                    .layout
                    .middle
                    .map_or(24, |r| r.height.saturating_sub(2) as usize);
                buf.adjust_scroll(visible_height);
                app.sync_cursor();
            }
        }
        KeyCode::Char('g') => {
            if app.preview_mode {
                app.preview_cursor = 0;
                if let Some(line) = app.preview_lines.first() {
                    if let Some(buf) = &mut app.buf {
                        buf.cursor_row = line.source_line;
                        buf.cursor_col = 0;
                        buf.last_cursor = None;
                        app.sync_cursor();
                    }
                }
                return;
            }
            if let Some(buf) = &mut app.buf {
                buf.cursor_row = 0;
                buf.cursor_col = 0;
                buf.clamp_cursor();
                buf.last_cursor = None;
                let visible_height = app
                    .layout
                    .middle
                    .map_or(24, |r| r.height.saturating_sub(2) as usize);
                buf.adjust_scroll(visible_height);
                app.sync_cursor();
            }
        }
        KeyCode::Home | KeyCode::Char('0') => {
            if app.preview_mode {
                app.preview_cursor = 0;
                if let Some(line) = app.preview_lines.get(app.preview_cursor) {
                    if let Some(buf) = &mut app.buf {
                        buf.cursor_row = line.source_line;
                        app.sync_cursor();
                    }
                }
                return;
            }
            if has_shift && key.code == KeyCode::Home && app.selection.is_none() {
                if let Some(buf) = &app.buf {
                    app.selection = Some(Selection::new(buf.cursor_row, buf.cursor_col));
                }
            }
            if let Some(buf) = &mut app.buf {
                buf.move_home();
            }
        }
        KeyCode::End | KeyCode::Char('$') => {
            if app.preview_mode {
                app.preview_cursor = app.preview_lines.len().saturating_sub(1);
                if let Some(line) = app.preview_lines.get(app.preview_cursor) {
                    if let Some(buf) = &mut app.buf {
                        buf.cursor_row = line.source_line;
                        app.sync_cursor();
                    }
                }
                return;
            }
            if has_shift && key.code == KeyCode::End && app.selection.is_none() {
                if let Some(buf) = &app.buf {
                    app.selection = Some(Selection::new(buf.cursor_row, buf.cursor_col));
                }
            }
            if let Some(buf) = &mut app.buf {
                buf.move_end();
            }
        }
        KeyCode::PageUp => {
            if app.preview_mode {
                let page_size = app
                    .layout
                    .middle
                    .map(|r| r.height.saturating_sub(2) as usize)
                    .unwrap_or(20)
                    .max(1);
                app.preview_cursor = app.preview_cursor.saturating_sub(page_size);
                if let Some(line) = app.preview_lines.get(app.preview_cursor) {
                    if let Some(buf) = &mut app.buf {
                        buf.cursor_row = line.source_line;
                        app.sync_cursor();
                    }
                }
                return;
            }
            if let Some(buf) = &mut app.buf {
                let page_size = app
                    .layout
                    .middle
                    .map(|r| r.height.saturating_sub(2) as usize)
                    .unwrap_or(20)
                    .max(1);
                buf.page_up(page_size);
                buf.last_cursor = None;
            }
        }
        KeyCode::PageDown => {
            if app.preview_mode {
                let page_size = app
                    .layout
                    .middle
                    .map(|r| r.height.saturating_sub(2) as usize)
                    .unwrap_or(20)
                    .max(1);
                app.preview_cursor = (app.preview_cursor + page_size).min(app.preview_lines.len().saturating_sub(1));
                if let Some(line) = app.preview_lines.get(app.preview_cursor) {
                    if let Some(buf) = &mut app.buf {
                        buf.cursor_row = line.source_line;
                        app.sync_cursor();
                    }
                }
                return;
            }
            if let Some(buf) = &mut app.buf {
                let page_size = app
                    .layout
                    .middle
                    .map(|r| r.height.saturating_sub(2) as usize)
                    .unwrap_or(20)
                    .max(1);
                buf.page_down(page_size);
                buf.last_cursor = None;
            }
        }
        _ => {}
    }
}

fn handle_edit_key(app: &mut App, key: KeyEvent) {
    let has_ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let has_shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let has_super = key.modifiers.contains(KeyModifiers::SUPER);

    if has_ctrl || has_super {
        match key.code {
            KeyCode::Char('z') => {
                if let Some(buf) = &mut app.buf {
                    if buf.undo() {
                        app.dirty = buf.dirty;
                        if app.dirty {
                            app.last_edit = Some(std::time::Instant::now());
                        } else {
                            app.last_edit = None;
                        }
                        app.selection = None;
                    }
                }
                return;
            }
            KeyCode::Char('y') => {
                if let Some(buf) = &mut app.buf {
                    if buf.redo() {
                        app.dirty = buf.dirty;
                        if app.dirty {
                            app.last_edit = Some(std::time::Instant::now());
                        } else {
                            app.last_edit = None;
                        }
                        app.selection = None;
                    }
                }
                return;
            }
            KeyCode::Char('c') => {
                copy_selection_or_line(app);
                return;
            }
            KeyCode::Char('x') => {
                cut_selection_or_line(app);
                return;
            }
            KeyCode::Char('v') => {
                if let Some(text) = app.clipboard.clone() {
                    paste_text(app, &text);
                }
                return;
            }
            KeyCode::Char('f') => {
                crate::app::find::start_find(app);
                return;
            }
            _ => return,
        }
    }

    if key.modifiers.contains(KeyModifiers::ALT) {
        return;
    }

    match key.code {
        KeyCode::Esc => {
            app.selection = None;
            if app.is_dirty() {
                let _ = app.save_active_buffer();
            }
            app.trigger_git_marks();
            app.trigger_diff();
            app.middle_mode = MiddleMode::Normal;
        }
        KeyCode::Left => {
            if has_shift {
                if app.selection.is_none() {
                    if let Some(buf) = &app.buf {
                        app.selection = Some(Selection::new(buf.cursor_row, buf.cursor_col));
                    }
                }
            } else {
                app.selection = None;
            }
            if let Some(buf) = &mut app.buf {
                buf.move_left();
            }
        }
        KeyCode::Right => {
            if has_shift {
                if app.selection.is_none() {
                    if let Some(buf) = &app.buf {
                        app.selection = Some(Selection::new(buf.cursor_row, buf.cursor_col));
                    }
                }
            } else {
                app.selection = None;
            }
            if let Some(buf) = &mut app.buf {
                buf.move_right();
            }
        }
        KeyCode::Up => {
            if has_shift {
                if app.selection.is_none() {
                    if let Some(buf) = &app.buf {
                        app.selection = Some(Selection::new(buf.cursor_row, buf.cursor_col));
                    }
                }
            } else {
                app.selection = None;
            }
            if let Some(buf) = &mut app.buf {
                buf.move_up();
            }
        }
        KeyCode::Down => {
            if has_shift {
                if app.selection.is_none() {
                    if let Some(buf) = &app.buf {
                        app.selection = Some(Selection::new(buf.cursor_row, buf.cursor_col));
                    }
                }
            } else {
                app.selection = None;
            }
            if let Some(buf) = &mut app.buf {
                buf.move_down();
            }
        }
        KeyCode::Home => {
            if has_shift {
                if app.selection.is_none() {
                    if let Some(buf) = &app.buf {
                        app.selection = Some(Selection::new(buf.cursor_row, buf.cursor_col));
                    }
                }
            } else {
                app.selection = None;
            }
            if let Some(buf) = &mut app.buf {
                buf.move_home();
            }
        }
        KeyCode::End => {
            if has_shift {
                if app.selection.is_none() {
                    if let Some(buf) = &app.buf {
                        app.selection = Some(Selection::new(buf.cursor_row, buf.cursor_col));
                    }
                }
            } else {
                app.selection = None;
            }
            if let Some(buf) = &mut app.buf {
                buf.move_end();
            }
        }
        KeyCode::PageUp => {
            app.selection = None;
            if let Some(buf) = &mut app.buf {
                let page_size = app
                    .layout
                    .middle
                    .map(|r| r.height.saturating_sub(2) as usize)
                    .unwrap_or(20)
                    .max(1);
                buf.page_up(page_size);
                buf.last_cursor = None;
            }
        }
        KeyCode::PageDown => {
            app.selection = None;
            if let Some(buf) = &mut app.buf {
                let page_size = app
                    .layout
                    .middle
                    .map(|r| r.height.saturating_sub(2) as usize)
                    .unwrap_or(20)
                    .max(1);
                buf.page_down(page_size);
                buf.last_cursor = None;
            }
        }
        KeyCode::Char(c) => {
            if let Some(buf) = &mut app.buf {
                buf.push_undo();
                if let Some(sel) = app.selection.take() {
                    delete_selection(buf, sel);
                }
                buf.insert_char(c);
                app.mark_edited();
            }
        }
        KeyCode::Enter => {
            if let Some(buf) = &mut app.buf {
                buf.push_undo();
                if let Some(sel) = app.selection.take() {
                    delete_selection(buf, sel);
                }
                buf.insert_newline();
                app.mark_edited();
            }
        }
        KeyCode::Backspace => {
            if let Some(buf) = &mut app.buf {
                buf.push_undo();
                if let Some(sel) = app.selection.take() {
                    delete_selection(buf, sel);
                } else {
                    buf.backspace();
                }
                app.mark_edited();
            }
        }
        KeyCode::Delete => {
            if let Some(buf) = &mut app.buf {
                buf.push_undo();
                if let Some(sel) = app.selection.take() {
                    delete_selection(buf, sel);
                } else {
                    buf.delete();
                }
                app.mark_edited();
            }
        }
        KeyCode::Tab => {
            if let Some(buf) = &mut app.buf {
                buf.push_undo();
                if let Some(sel) = app.selection.take() {
                    delete_selection(buf, sel);
                }
                buf.insert_str("    ");
                app.mark_edited();
            }
        }
        _ => {}
    }
}

pub fn handle_mouse(app: &mut App, mouse: MouseEvent) {
    let Some(middle_rect) = app.layout.middle else {
        return;
    };
    if !rect_contains(middle_rect, mouse.column, mouse.row) {
        return;
    }
    let Some(buf) = &mut app.buf else {
        return;
    };

    let inner_x = middle_rect.x.saturating_add(1);
    let inner_y = middle_rect.y.saturating_add(1);
    let inner_h = middle_rect.height.saturating_sub(2);
    let inner_w = middle_rect.width.saturating_sub(2);

    if mouse.row < inner_y || mouse.row >= inner_y + inner_h {
        return;
    }

    if app.preview_mode {
        let clicked_row = app.preview_scroll + (mouse.row - inner_y) as usize;
        if clicked_row < app.preview_lines.len() {
            app.preview_cursor = clicked_row;
            if let Some(line) = app.preview_lines.get(clicked_row) {
                buf.cursor_row = line.source_line;
                buf.cursor_col = 0;
                app.sync_cursor();
            }
        }
        return;
    }

    let total_buffer_lines = buf.lines.len();
    let gutter_digits = format!("{total_buffer_lines}").len().max(3);
    let gutter_width = gutter_digits + 1;
    let content_width = (inner_w as usize).saturating_sub(gutter_width).max(1);

    let wrapped_rows = crate::wrap::wrap_buffer(&buf.lines, content_width);

    let clicked_v_row = buf.scroll_row + (mouse.row - inner_y) as usize;
    let text_x = inner_x + gutter_width as u16;

    let clicked_v_col = if mouse.column >= text_x {
        (mouse.column - text_x) as usize
    } else {
        0
    };

    let (target_row, target_col) =
        crate::wrap::visual_to_buffer(&wrapped_rows, clicked_v_row, clicked_v_col)
            .unwrap_or((buf.lines.len().saturating_sub(1), 0));

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if mouse.modifiers.contains(KeyModifiers::SHIFT) {
                if app.selection.is_none() {
                    app.selection = Some(Selection::new(buf.cursor_row, buf.cursor_col));
                }
            } else {
                app.selection = Some(Selection::new(target_row, target_col));
            }
            buf.cursor_row = target_row;
            buf.cursor_col = target_col;
            buf.clamp_cursor();
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if app.selection.is_none() {
                app.selection = Some(Selection::new(buf.cursor_row, buf.cursor_col));
            }
            buf.cursor_row = target_row;
            buf.cursor_col = target_col;
            buf.clamp_cursor();
        }
        MouseEventKind::Up(MouseButton::Left) => {
            if let Some(sel) = app.selection {
                if sel.is_empty(buf.cursor_row, buf.cursor_col) {
                    app.selection = None;
                }
            }
        }
        _ => {}
    }
}
