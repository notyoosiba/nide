use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::App;
use crate::picker::{Picker, PickerKind};

pub fn open_file_picker(app: &mut App) {
    let picker = Picker::new(app.root_dir.clone(), app.files.clone(), &app.recent_files);
    app.picker = Some(picker);
}

pub fn open_symbol_picker(app: &mut App) {
    let symbols = crate::search::symbols::extract_project_symbols(
        &app.root_dir,
        &app.files,
        app.buf.as_ref(),
    );
    let picker = Picker::new_symbols(app.root_dir.clone(), symbols);
    app.picker = Some(picker);
}

pub fn open_grep_picker(app: &mut App) {
    let picker = Picker::new_grep(app.root_dir.clone());
    app.picker = Some(picker);
}

pub fn open_usage_picker(app: &mut App, items: Vec<crate::picker::PickItem>) {
    let picker = Picker::new_usages(app.root_dir.clone(), items);
    app.picker = Some(picker);
}

pub fn open_definition_picker(app: &mut App, items: Vec<crate::picker::PickItem>) {
    let picker = Picker::new_definitions(app.root_dir.clone(), items);
    app.picker = Some(picker);
}

pub fn trigger_grep(app: &mut App) {
    app.search_seq = app.search_seq.wrapping_add(1);
    let seq = app.search_seq;
    let query = app.picker.as_ref().map(|p| p.query.clone()).unwrap_or_default();
    if query.trim().is_empty() {
        if let Some(p) = &mut app.picker {
            p.items.clear();
            p.selected_idx = 0;
            p.scroll = 0;
            p.update_preview();
        }
        return;
    }

    if let Some(tx) = app.tx.clone() {
        crate::search::grep::spawn_grep(query, seq, app.root_dir.clone(), app.files.clone(), tx);
    }
}

pub fn close_picker(app: &mut App) {
    app.picker = None;
}

pub fn accept_picker(app: &mut App) {
    if let Some(picker) = app.picker.take() {
        if let Some(item) = picker.items.get(picker.selected_idx) {
            let path = item.path.clone();
            let line = item.line.unwrap_or(0);
            let col = item.col.unwrap_or(0);
            if app.is_dirty() {
                let _ = app.save_active_buffer();
            }
            if let Some(buf) = &app.buf {
                if buf.path != path || buf.cursor_row != line {
                    app.history.push(crate::app::Location::new(
                        buf.path.clone(),
                        buf.cursor_row,
                        buf.cursor_col,
                    ));
                }
            }
            let _ = app.open_file_at(&path, line, col);
            if picker.kind == PickerKind::Usages || picker.kind == PickerKind::Definitions {
                app.middle_mode = crate::app::MiddleMode::Normal;
            }
        }
    }
}

pub fn handle_key(app: &mut App, key: KeyEvent) {
    let has_ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let has_alt = key.modifiers.contains(KeyModifiers::ALT);

    if has_alt {
        return;
    }

    if has_ctrl {
        match key.code {
            KeyCode::Char('c') | KeyCode::Char('e') => {
                close_picker(app);
                return;
            }
            KeyCode::Char('n') => {
                if let Some(p) = &mut app.picker {
                    p.select_next();
                }
                return;
            }
            KeyCode::Char('p') => {
                if let Some(p) = &mut app.picker {
                    p.select_prev();
                }
                return;
            }
            KeyCode::Char('u') => {
                if let Some(p) = &mut app.picker {
                    p.query.clear();
                    p.refilter(&app.recent_files);
                }
                if app.picker.as_ref().is_some_and(|p| p.kind == PickerKind::Grep) {
                    trigger_grep(app);
                }
                return;
            }
            KeyCode::Char('w') => {
                if let Some(p) = &mut app.picker {
                    while let Some(c) = p.query.pop() {
                        if c.is_whitespace() {
                            break;
                        }
                    }
                    p.refilter(&app.recent_files);
                }
                if app.picker.as_ref().is_some_and(|p| p.kind == PickerKind::Grep) {
                    trigger_grep(app);
                }
                return;
            }
            _ => return,
        }
    }

    match key.code {
        KeyCode::Esc => {
            close_picker(app);
        }
        KeyCode::Enter => {
            accept_picker(app);
        }
        KeyCode::Up => {
            if let Some(p) = &mut app.picker {
                p.select_prev();
            }
        }
        KeyCode::Down => {
            if let Some(p) = &mut app.picker {
                p.select_next();
            }
        }
        KeyCode::PageUp => {
            if let Some(p) = &mut app.picker {
                for _ in 0..10 {
                    p.select_prev();
                }
            }
        }
        KeyCode::PageDown => {
            if let Some(p) = &mut app.picker {
                for _ in 0..10 {
                    p.select_next();
                }
            }
        }
        KeyCode::Backspace => {
            if let Some(p) = &mut app.picker {
                p.query.pop();
                p.refilter(&app.recent_files);
            }
            if app.picker.as_ref().is_some_and(|p| p.kind == PickerKind::Grep) {
                trigger_grep(app);
            }
        }
        KeyCode::Char(c) => {
            if let Some(p) = &mut app.picker {
                p.query.push(c);
                p.refilter(&app.recent_files);
            }
            if app.picker.as_ref().is_some_and(|p| p.kind == PickerKind::Grep) {
                trigger_grep(app);
            }
        }
        _ => {}
    }
}
