use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::app::edit::MiddleMode;
use crate::app::{rect_contains, App, Focus};
use crate::line_edit::LineEdit;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreePromptKind {
    Create,
    Rename { orig_path: std::path::PathBuf },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeClipboardOp {
    Copy(std::path::PathBuf),
    Move(std::path::PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreePrompt {
    pub kind: TreePromptKind,
    pub input: LineEdit,
}

impl TreePrompt {
    #[must_use]
    pub fn new_create(initial_text: impl Into<String>) -> Self {
        Self {
            kind: TreePromptKind::Create,
            input: LineEdit::with_text(initial_text),
        }
    }

    #[must_use]
    pub fn new_rename(orig_path: std::path::PathBuf, initial_text: impl Into<String>) -> Self {
        Self {
            kind: TreePromptKind::Rename { orig_path },
            input: LineEdit::with_text(initial_text),
        }
    }
}

pub fn handle_key(app: &mut App, key: KeyEvent) {
    if let Some(mut prompt) = app.tree_prompt.take() {
        match key.code {
            KeyCode::Enter => {
                confirm_prompt(app, &prompt);
            }
            KeyCode::Esc => {
                // Prompt cancelled, app.tree_prompt remains None
            }
            _ => {
                prompt.input.handle_key(&key);
                app.tree_prompt = Some(prompt);
            }
        }
        return;
    }

    let is_create = (key.code == KeyCode::Char('a')
        && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::NONE))
        || (key.code == KeyCode::Char('n') && key.modifiers.contains(KeyModifiers::CONTROL))
        || key.code == KeyCode::Char('\u{e}');

    if is_create {
        let prefix = get_selected_dir_prefix(app);
        app.tree_prompt = Some(TreePrompt::new_create(prefix));
        return;
    }

    let is_plain = key.modifiers.is_empty() || key.modifiers == KeyModifiers::NONE;

    if key.code == KeyCode::Char('r') && is_plain {
        if let Some(node) = app.tree.selected_node() {
            let orig_path = if node.path.is_absolute() {
                node.path.clone()
            } else {
                app.root_dir.join(&node.path)
            };
            let rel_path = node.path.strip_prefix(&app.root_dir).unwrap_or(&node.path);
            let initial_text = rel_path.to_string_lossy().to_string();
            app.tree_prompt = Some(TreePrompt::new_rename(orig_path, initial_text));
            return;
        }
    }

    if key.code == KeyCode::Char('y') && is_plain {
        if let Some(node) = app.tree.selected_node() {
            let path = if node.path.is_absolute() {
                node.path.clone()
            } else {
                app.root_dir.join(&node.path)
            };
            app.tree_clipboard = Some(TreeClipboardOp::Copy(path));
            app.status_message = Some(format!("Copied: {}", node.path.display()));
            return;
        }
    }

    if key.code == KeyCode::Char('x') && is_plain {
        if let Some(node) = app.tree.selected_node() {
            let path = if node.path.is_absolute() {
                node.path.clone()
            } else {
                app.root_dir.join(&node.path)
            };
            app.tree_clipboard = Some(TreeClipboardOp::Move(path));
            app.status_message = Some(format!("Cut: {}", node.path.display()));
            return;
        }
    }

    if key.code == KeyCode::Char('p') && is_plain {
        paste_selected(app);
        return;
    }

    if key.code == KeyCode::Delete || key.code == KeyCode::Backspace {
        delete_selected(app);
        return;
    }

    match key.code {
        KeyCode::Char('o')
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT) =>
        {
            crate::app::picker::open_file_picker(app);
        }
        KeyCode::Char('D') | KeyCode::Char('d')
            if (key.code == KeyCode::Char('D') || key.modifiers.contains(KeyModifiers::SHIFT))
                && !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT) =>
        {
            crate::app::picker::open_symbol_picker(app);
        }
        KeyCode::Char('s')
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT) =>
        {
            crate::app::picker::open_grep_picker(app);
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.tree.move_down();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.tree.move_up();
        }
        KeyCode::Enter => {
            if let Some(node) = app.tree.selected_node().cloned() {
                if node.is_dir {
                    app.tree.toggle_expanded();
                } else {
                    let path = if node.path.is_absolute() {
                        node.path
                    } else {
                        app.tree.root.join(&node.path)
                    };
                    let _ = app.open_file(&path);
                }
            }
        }
        KeyCode::Right | KeyCode::Char('l') => {
            app.tree.toggle_expanded();
        }
        KeyCode::Left | KeyCode::Char('h') => {
            app.tree.collapse_or_parent();
        }
        _ => {}
    }
}

pub fn handle_mouse(app: &mut App, mouse: MouseEvent) {
    if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
        if let Some(tree_rect) = app.layout.tree {
            if rect_contains(tree_rect, mouse.column, mouse.row) {
                app.set_focus(Focus::Tree);
                app.tree_prompt = None;
                if mouse.row > tree_rect.y
                    && mouse.row < tree_rect.y + tree_rect.height.saturating_sub(1)
                {
                    let clicked_row = (mouse.row - (tree_rect.y + 1)) as usize;
                    let visible_idx = clicked_row + app.tree.scroll;
                    let visible_count = app.tree.visible_indices().len();
                    if visible_idx < visible_count {
                        app.tree.cursor = visible_idx;
                    }
                }
            }
        }
    }
}

fn get_selected_dir_prefix(app: &App) -> String {
    let Some(node) = app.tree.selected_node() else {
        return String::new();
    };

    let rel_path = node.path.strip_prefix(&app.root_dir).unwrap_or(&node.path);

    if node.is_dir {
        let p_str = rel_path.to_string_lossy();
        if p_str.is_empty() || p_str == "." {
            String::new()
        } else {
            format!("{p_str}/")
        }
    } else if let Some(parent) = rel_path.parent() {
        let p_str = parent.to_string_lossy();
        if p_str.is_empty() || p_str == "." {
            String::new()
        } else {
            format!("{p_str}/")
        }
    } else {
        String::new()
    }
}

fn confirm_prompt(app: &mut App, prompt: &TreePrompt) {
    match prompt.kind {
        TreePromptKind::Create => {
            let text = prompt.input.text().trim();
            if text.is_empty() {
                return;
            }
            let is_dir = text.ends_with('/');
            let clean = text.trim_start_matches('/');
            let clean = clean.strip_prefix("./").unwrap_or(clean);
            if clean.is_empty() {
                return;
            }

            let target_path = app.root_dir.join(clean);
            if is_dir {
                if let Err(e) = std::fs::create_dir_all(&target_path) {
                    app.status_message = Some(format!("Failed to create directory: {e}"));
                    return;
                }
                app.refresh_tree_sync();
                app.tree.reveal(&target_path);
                app.trigger_git_marks();
            } else {
                if let Some(parent) = target_path.parent() {
                    if let Err(e) = std::fs::create_dir_all(parent) {
                        app.status_message = Some(format!("Failed to create directory: {e}"));
                        return;
                    }
                }
                if let Err(e) = std::fs::File::create(&target_path) {
                    app.status_message = Some(format!("Failed to create file: {e}"));
                    return;
                }
                app.refresh_tree_sync();
                app.tree.reveal(&target_path);
                let _ = app.open_file(&target_path);
                app.trigger_git_marks();
            }
        }
        TreePromptKind::Rename { ref orig_path } => {
            let text = prompt.input.text().trim();
            if text.is_empty() {
                return;
            }
            let clean = text.strip_prefix("./").unwrap_or(text);
            let target_path = if clean.contains('/') {
                let clean = clean.trim_start_matches('/');
                app.root_dir.join(clean)
            } else {
                orig_path.parent().unwrap_or(&app.root_dir).join(clean)
            };

            if target_path == *orig_path {
                return;
            }

            if let Some(parent) = target_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }

            if let Err(e) = std::fs::rename(orig_path, &target_path) {
                app.status_message = Some(format!("Failed to rename: {e}"));
                return;
            }

            if let Some(buf) = &mut app.buf {
                if crate::app::paths_refer_to_same_file(orig_path, &buf.path, &app.root_dir) {
                    buf.path = target_path.clone();
                } else if let Ok(rel) = buf.path.strip_prefix(orig_path) {
                    buf.path = target_path.join(rel);
                }
            }

            app.refresh_tree_sync();
            app.tree.reveal(&target_path);
            app.trigger_git_marks();
        }
    }
}

fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dst_path)?;
        } else {
            std::fs::copy(entry.path(), dst_path)?;
        }
    }
    Ok(())
}

fn unique_copy_path(target_dir: &std::path::Path, file_name: &std::ffi::OsStr) -> std::path::PathBuf {
    let dest = target_dir.join(file_name);
    if !dest.exists() {
        return dest;
    }
    let p = std::path::Path::new(file_name);
    let stem = p.file_stem().map(|s| s.to_string_lossy()).unwrap_or_default();
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();

    let mut candidate = target_dir.join(format!("{stem}_copy{ext}"));
    let mut idx = 2;
    while candidate.exists() {
        candidate = target_dir.join(format!("{stem}_copy_{idx}{ext}"));
        idx += 1;
    }
    candidate
}

fn paste_selected(app: &mut App) {
    let Some(op) = app.tree_clipboard.clone() else {
        return;
    };
    let (src_path, is_move) = match op {
        TreeClipboardOp::Copy(p) => (p, false),
        TreeClipboardOp::Move(p) => (p, true),
    };

    if !src_path.exists() {
        app.status_message = Some(format!("Source does not exist: {}", src_path.display()));
        if is_move {
            app.tree_clipboard = None;
        }
        return;
    }

    let target_dir = if let Some(node) = app.tree.selected_node() {
        let node_path = if node.path.is_absolute() {
            node.path.clone()
        } else {
            app.root_dir.join(&node.path)
        };
        if node.is_dir {
            node_path
        } else {
            node_path.parent().unwrap_or(&app.root_dir).to_path_buf()
        }
    } else {
        app.root_dir.clone()
    };

    let file_name = match src_path.file_name() {
        Some(name) => name,
        None => return,
    };

    let dest_path = if is_move {
        let candidate = target_dir.join(file_name);
        if candidate == src_path {
            return;
        }
        candidate
    } else {
        let candidate = target_dir.join(file_name);
        if candidate == src_path || candidate.exists() {
            unique_copy_path(&target_dir, file_name)
        } else {
            candidate
        }
    };

    if let Some(parent) = dest_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    if is_move {
        app.tree_clipboard = None;
        let move_res = std::fs::rename(&src_path, &dest_path).or_else(|_| {
            if src_path.is_dir() {
                copy_dir_all(&src_path, &dest_path)?;
                std::fs::remove_dir_all(&src_path)
            } else {
                std::fs::copy(&src_path, &dest_path)?;
                std::fs::remove_file(&src_path)
            }
        });

        if let Err(e) = move_res {
            app.status_message = Some(format!("Failed to move: {e}"));
            return;
        }

        if let Some(buf) = &mut app.buf {
            if crate::app::paths_refer_to_same_file(&src_path, &buf.path, &app.root_dir) {
                buf.path = dest_path.clone();
            } else if let Ok(rel) = buf.path.strip_prefix(&src_path) {
                buf.path = dest_path.join(rel);
            }
        }
    } else {
        let copy_res = if src_path.is_dir() {
            copy_dir_all(&src_path, &dest_path)
        } else {
            std::fs::copy(&src_path, &dest_path).map(|_| ())
        };

        if let Err(e) = copy_res {
            app.status_message = Some(format!("Failed to copy: {e}"));
            return;
        }
    }

    app.refresh_tree_sync();
    app.tree.reveal(&dest_path);
    app.trigger_git_marks();
}

fn delete_selected(app: &mut App) {
    let Some(node) = app.tree.selected_node().cloned() else {
        return;
    };

    let target_path = if node.path.is_absolute() {
        node.path.clone()
    } else {
        app.root_dir.join(&node.path)
    };

    // Check if open buffer needs to be cleared BEFORE deleting from disk,
    // so canonicalize() and path checks succeed.
    let should_clear_buf = app.buf.as_ref().is_some_and(|buf| {
        let same_file =
            crate::app::paths_refer_to_same_file(&target_path, &buf.path, &app.root_dir);
        let inside_deleted_dir = node.is_dir
            && (buf.path.starts_with(&target_path)
                || {
                    if let (Ok(bc), Ok(tc)) =
                        (buf.path.canonicalize(), target_path.canonicalize())
                    {
                        bc.starts_with(&tc)
                    } else {
                        false
                    }
                });
        same_file || inside_deleted_dir
    });

    let delete_result = if node.is_dir {
        std::fs::remove_dir_all(&target_path)
    } else {
        std::fs::remove_file(&target_path)
    };

    if let Err(e) = delete_result {
        app.status_message = Some(format!("Failed to delete: {e}"));
        return;
    }

    if should_clear_buf {
        app.buf = None;
        app.dirty = false;
        app.line = 0;
        app.col = 0;
        app.middle_mode = MiddleMode::Normal;
        app.diff = None;
        app.conflict = None;
    }

    app.refresh_tree_sync();
    app.trigger_git_marks();
}
