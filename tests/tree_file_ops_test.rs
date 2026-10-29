use std::fs;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tempfile::TempDir;
use nide::app::{App, Focus, MiddleMode, Msg};
use nide::ui;

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn press_with_mod(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn type_str(app: &mut App, s: &str) {
    for ch in s.chars() {
        app.handle_msg(Msg::Key(press(KeyCode::Char(ch))));
    }
}

#[test]
fn test_create_file_reveals_in_tree_and_opens_in_edit_mode() {
    let tmp = TempDir::new().expect("tempdir");
    let mut app = App::new(tmp.path().to_path_buf());
    assert_eq!(app.focus, Focus::Tree);

    // 1. Press 'a' to open creation prompt
    app.handle_msg(Msg::Key(press(KeyCode::Char('a'))));
    assert!(app.tree_prompt.is_some());

    // 2. Type "dir/hello.rs"
    type_str(&mut app, "dir/hello.rs");
    assert_eq!(
        app.tree_prompt.as_ref().unwrap().input.text(),
        "dir/hello.rs"
    );

    // 3. Press Enter to confirm
    app.handle_msg(Msg::Key(press(KeyCode::Enter)));

    // Verify file exists on disk
    let file_path = tmp.path().join("dir/hello.rs");
    assert!(file_path.exists(), "dir/hello.rs must exist on disk");

    // Verify node is revealed in app.tree
    assert_eq!(
        app.tree.selected_path(),
        Some(Path::new("dir/hello.rs")),
        "dir/hello.rs must be revealed and selected in app.tree"
    );

    // Verify file is open in MiddleMode::Edit
    assert_eq!(app.middle_mode, MiddleMode::Edit);
    assert_eq!(app.focus, Focus::Middle);
    assert!(app.buf.is_some());
    assert_eq!(
        app.buf.as_ref().unwrap().path,
        file_path
    );
    assert!(app.files.contains(&PathBuf::from("dir/hello.rs")));
}

#[test]
fn test_delete_selected_file_removes_from_disk_and_app_files() {
    let tmp = TempDir::new().expect("tempdir");
    let file_path = tmp.path().join("dir/hello.rs");
    fs::create_dir_all(tmp.path().join("dir")).unwrap();
    fs::write(&file_path, "fn hello() {}\n").unwrap();

    let mut app = App::new(tmp.path().to_path_buf());
    assert!(app.files.contains(&PathBuf::from("dir/hello.rs")));

    // Reveal and select dir/hello.rs
    assert!(app.tree.reveal(&PathBuf::from("dir/hello.rs")));
    assert_eq!(app.tree.selected_path(), Some(Path::new("dir/hello.rs")));

    // Open file in editor
    app.handle_msg(Msg::Key(press(KeyCode::Enter)));
    assert_eq!(app.middle_mode, MiddleMode::Edit);
    assert!(app.buf.is_some());

    // Switch focus back to Tree
    app.set_focus(Focus::Tree);
    assert_eq!(app.focus, Focus::Tree);
    assert_eq!(app.tree.selected_path(), Some(Path::new("dir/hello.rs")));

    // Press Delete
    app.handle_msg(Msg::Key(press(KeyCode::Delete)));

    // Verify file is removed from disk
    assert!(!file_path.exists(), "file must be deleted from disk");

    // Verify file is removed from app.files
    assert!(
        !app.files.iter().any(|p| p.ends_with("dir/hello.rs")),
        "file must be removed from app.files"
    );

    // Verify app.buf is cleared because open file was deleted
    assert!(app.buf.is_none(), "app.buf must be cleared when open file is deleted");
}

#[test]
fn test_backspace_deletes_selected_node() {
    let tmp = TempDir::new().expect("tempdir");
    let file_path = tmp.path().join("item.txt");
    fs::write(&file_path, "content").unwrap();

    let mut app = App::new(tmp.path().to_path_buf());
    assert_eq!(app.tree.selected_path(), Some(Path::new("item.txt")));

    // Press Backspace
    app.handle_msg(Msg::Key(press(KeyCode::Backspace)));

    assert!(!file_path.exists(), "item.txt must be removed from disk");
    assert!(!app.files.contains(&PathBuf::from("item.txt")));
}

#[test]
fn test_create_directory_with_trailing_slash() {
    let tmp = TempDir::new().expect("tempdir");
    let mut app = App::new(tmp.path().to_path_buf());

    // Press 'a' and type folder path ending with /
    app.handle_msg(Msg::Key(press(KeyCode::Char('a'))));
    type_str(&mut app, "components/widgets/");
    app.handle_msg(Msg::Key(press(KeyCode::Enter)));

    let dir_path = tmp.path().join("components/widgets");
    assert!(dir_path.is_dir(), "directory must exist on disk");

    // Must be revealed in tree
    assert_eq!(
        app.tree.selected_path(),
        Some(Path::new("components/widgets"))
    );

    // Directories should NOT be opened in edit mode
    assert!(app.buf.is_none());
    assert_eq!(app.focus, Focus::Tree);
}

#[test]
fn test_create_prompt_prefills_directory_prefix() {
    let tmp = TempDir::new().expect("tempdir");
    let dir_path = tmp.path().join("src/utils");
    fs::create_dir_all(&dir_path).unwrap();
    fs::write(dir_path.join("math.rs"), "// math").unwrap();

    let mut app = App::new(tmp.path().to_path_buf());

    // Navigate to and reveal src/utils
    assert!(app.tree.reveal(&PathBuf::from("src/utils")));
    assert_eq!(app.tree.selected_path(), Some(Path::new("src/utils")));

    // Press 'a' on selected directory
    app.handle_msg(Msg::Key(press(KeyCode::Char('a'))));
    assert!(app.tree_prompt.is_some());
    // Prefix must be "src/utils/"
    assert_eq!(
        app.tree_prompt.as_ref().unwrap().input.text(),
        "src/utils/"
    );

    // Type filename
    type_str(&mut app, "string.rs");
    assert_eq!(
        app.tree_prompt.as_ref().unwrap().input.text(),
        "src/utils/string.rs"
    );

    app.handle_msg(Msg::Key(press(KeyCode::Enter)));
    assert!(tmp.path().join("src/utils/string.rs").exists());
    assert_eq!(app.middle_mode, MiddleMode::Edit);
    assert!(app.buf.is_some());
}

#[test]
fn test_ctrl_n_opens_create_prompt_and_esc_cancels() {
    let tmp = TempDir::new().expect("tempdir");
    let mut app = App::new(tmp.path().to_path_buf());

    // Press Ctrl+N
    app.handle_msg(Msg::Key(press_with_mod(
        KeyCode::Char('n'),
        KeyModifiers::CONTROL,
    )));
    assert!(app.tree_prompt.is_some());

    type_str(&mut app, "never_created.txt");
    assert_eq!(
        app.tree_prompt.as_ref().unwrap().input.text(),
        "never_created.txt"
    );

    // Press Esc to cancel
    app.handle_msg(Msg::Key(press(KeyCode::Esc)));
    assert!(app.tree_prompt.is_none());
    assert!(!tmp.path().join("never_created.txt").exists());
}

#[test]
fn test_delete_directory_removes_children_and_clears_open_file_inside() {
    let tmp = TempDir::new().expect("tempdir");
    let dir = tmp.path().join("pkg");
    fs::create_dir_all(&dir).unwrap();
    let f1 = dir.join("mod.rs");
    let f2 = dir.join("lib.rs");
    fs::write(&f1, "mod").unwrap();
    fs::write(&f2, "lib").unwrap();

    let mut app = App::new(tmp.path().to_path_buf());

    // Open pkg/lib.rs in editor
    let _ = app.open_file(&f2);
    assert!(app.buf.is_some());

    // Switch focus to Tree and select pkg directory
    app.set_focus(Focus::Tree);
    assert!(app.tree.reveal(&PathBuf::from("pkg")));
    assert_eq!(app.tree.selected_path(), Some(Path::new("pkg")));

    // Press Delete on directory
    app.handle_msg(Msg::Key(press(KeyCode::Delete)));

    assert!(!dir.exists(), "directory must be deleted from disk");
    assert!(!app.files.iter().any(|p| p.starts_with("pkg")));
    assert!(app.buf.is_none(), "open file inside deleted dir must be cleared");
}

#[test]
fn test_q_key_in_create_prompt_does_not_quit() {
    let tmp = TempDir::new().expect("tempdir");
    let mut app = App::new(tmp.path().to_path_buf());

    app.handle_msg(Msg::Key(press(KeyCode::Char('a'))));
    assert!(app.tree_prompt.is_some());

    // Press 'q'
    app.handle_msg(Msg::Key(press(KeyCode::Char('q'))));
    assert!(!app.should_quit, "'q' in prompt must not quit app");
    assert_eq!(app.tree_prompt.as_ref().unwrap().input.text(), "q");
}

#[test]
fn test_ui_rendering_of_create_prompt_in_tree_and_status() {
    let tmp = TempDir::new().expect("tempdir");
    let mut app = App::new(tmp.path().to_path_buf());

    app.handle_msg(Msg::Key(press(KeyCode::Char('a'))));
    type_str(&mut app, "my_new_file.txt");

    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|f| ui::draw(f, &mut app))
        .expect("draw failed");

    let buffer = terminal.backend().buffer();
    let text = format!("{buffer:?}");

    // Tree header shows [New]
    assert!(text.contains("Tree [New]"), "Tree title should indicate [New] prompt");
    // Tree prompt row shows "New: my_new_file.txt"
    assert!(text.contains("New: my_new_file.txt"), "Tree must display prompt text");
    // Status bar shows [CREATE] mode
    assert!(text.contains("[CREATE]"), "Status bar should show [CREATE] badge");
}
