use std::fs;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tempfile::TempDir;
use nide::app::{App, Focus, MiddleMode, Msg};
use nide::theme;
use nide::ui;

fn key_press(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

#[test]
fn test_fuzzy_file_picker_modal_open_query_preview_and_accept() {
    let width = 100;
    let height = 30;
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("failed to create test terminal");

    let temp = TempDir::new().expect("failed to create temp dir");
    let root = temp.path();

    // Populate multiple files in workspace
    fs::create_dir_all(root.join("src")).expect("mkdir src");
    fs::create_dir_all(root.join("docs")).expect("mkdir docs");

    fs::write(
        root.join("src/main.rs"),
        "fn main() {\n    println!(\"hello main\");\n}\n",
    )
    .expect("write main.rs");
    fs::write(
        root.join("src/utils.rs"),
        "pub fn calculate_magic() -> i32 {\n    42\n}\n",
    )
    .expect("write utils.rs");
    fs::write(
        root.join("docs/readme.txt"),
        "Documentation guide for project.\n",
    )
    .expect("write readme.txt");

    let mut app = App::new(root.to_path_buf());
    assert_eq!(app.focus, Focus::Tree);
    assert_eq!(app.files.len(), 3);

    // Initial render in Tree mode
    terminal
        .draw(|f| ui::draw(f, &mut app))
        .expect("render UI");

    // 1. Press 'o' in Tree mode to open fuzzy file picker modal
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('o'), KeyModifiers::NONE)));
    assert!(
        app.picker.is_some(),
        "Pressing 'o' in Focus::Tree should open the Picker modal"
    );

    let picker = app.picker.as_ref().unwrap();
    assert_eq!(picker.items.len(), 3);
    assert_eq!(picker.query, "");

    // 2. Type fuzzy query "util"
    for c in "util".chars() {
        app.handle_msg(Msg::Key(key_press(KeyCode::Char(c), KeyModifiers::NONE)));
    }

    assert_eq!(app.picker.as_ref().unwrap().query, "util");
    let picker = app.picker.as_ref().unwrap();
    assert!(
        !picker.items.is_empty(),
        "Query 'util' should match candidates"
    );
    // The top candidate should be src/utils.rs
    assert!(
        picker.items[0].display_text.contains("utils.rs"),
        "Top candidate should be utils.rs, got: {}",
        picker.items[0].display_text
    );

    // 3. Render via TestBackend to verify the overlay and preview
    terminal
        .draw(|f| ui::draw(f, &mut app))
        .expect("render UI with Picker");

    let buffer = terminal.backend().buffer();
    let screen_text = format!("{buffer:?}");

    // Verify modal overlay title "Files" is rendered
    assert!(
        screen_text.contains("Files"),
        "Overlay should render 'Files' title"
    );

    // Verify candidate text "utils.rs" is rendered
    assert!(
        screen_text.contains("utils.rs"),
        "Candidate list should display 'utils.rs'"
    );

    // Verify preview content from src/utils.rs is rendered
    assert!(
        screen_text.contains("calculate_magic"),
        "Preview pane should display content 'calculate_magic' from utils.rs"
    );

    // Verify matched-character highlight color (PEACH) is used in the candidate list
    let mut matched_highlight_cells = 0;
    for y in 0..height {
        for x in 0..width {
            let cell = &buffer[(x, y)];
            if cell.fg == theme::PEACH {
                matched_highlight_cells += 1;
            }
        }
    }
    assert!(
        matched_highlight_cells >= 4,
        "Matched characters ('u', 't', 'i', 'l') should be highlighted in PEACH, found: {matched_highlight_cells}"
    );

    // 4. Press Enter to confirm selection
    app.handle_msg(Msg::Key(key_press(KeyCode::Enter, KeyModifiers::NONE)));

    // Picker should be closed
    assert!(
        app.picker.is_none(),
        "Picker should close on Enter confirmation"
    );

    // Selected file (src/utils.rs) should be open in Editor in MiddleMode::Edit
    assert_eq!(app.focus, Focus::Middle);
    assert_eq!(app.middle_mode, MiddleMode::Edit);
    let buf = app.buf.as_ref().expect("buffer should be open");
    assert!(
        buf.path.ends_with("src/utils.rs") || buf.path.ends_with("utils.rs"),
        "Opened buffer should be utils.rs, got: {:?}",
        buf.path
    );

    // Left tree should have revealed src/utils.rs
    let selected_tree = app.tree.selected_path().expect("tree should have a selection");
    assert!(
        selected_tree.ends_with("src/utils.rs") || selected_tree.ends_with("utils.rs"),
        "Tree should reveal utils.rs, got: {:?}",
        selected_tree
    );
}

#[test]
fn test_fuzzy_picker_global_ctrl_e_and_esc_cancel() {
    let temp = TempDir::new().expect("failed to create temp dir");
    let root = temp.path();

    fs::write(root.join("file1.txt"), "hello file 1\n").unwrap();
    fs::write(root.join("file2.txt"), "hello file 2\n").unwrap();

    let mut app = App::new(root.to_path_buf());
    app.open_file(root.join("file1.txt")).unwrap();
    assert_eq!(app.focus, Focus::Middle);
    assert_eq!(app.middle_mode, MiddleMode::Edit);

    // 1. Press Ctrl+E while in Edit mode -> opens picker
    app.handle_msg(Msg::Key(key_press(
        KeyCode::Char('e'),
        KeyModifiers::CONTROL,
    )));
    assert!(app.picker.is_some(), "Ctrl+E should open picker modal");

    // 2. Press Esc -> closes picker, leaves file1 open in Edit mode
    app.handle_msg(Msg::Key(key_press(KeyCode::Esc, KeyModifiers::NONE)));
    assert!(app.picker.is_none(), "Esc should close picker modal");
    assert_eq!(app.focus, Focus::Middle);
    assert_eq!(app.middle_mode, MiddleMode::Edit);
    assert!(app.buf.as_ref().unwrap().path.ends_with("file1.txt"));
}

#[test]
fn test_fuzzy_picker_recent_files_ranking_and_boost() {
    let temp = TempDir::new().expect("failed to create temp dir");
    let root = temp.path();

    fs::write(root.join("alpha.rs"), "// alpha\n").unwrap();
    fs::write(root.join("beta.rs"), "// beta\n").unwrap();
    fs::write(root.join("gamma.rs"), "// gamma\n").unwrap();

    let mut app = App::new(root.to_path_buf());

    // Visit gamma.rs first, then alpha.rs
    app.open_file(root.join("gamma.rs")).unwrap();
    app.open_file(root.join("alpha.rs")).unwrap();

    // Drop to Normal mode
    app.handle_msg(Msg::Key(key_press(KeyCode::Esc, KeyModifiers::NONE)));

    // Press 'o' in Normal mode
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('o'), KeyModifiers::NONE)));
    assert!(app.picker.is_some());

    let picker = app.picker.as_ref().unwrap();
    // With empty query, recent files should be first in MRU order: alpha.rs, gamma.rs, beta.rs
    assert_eq!(picker.items[0].display_text, "alpha.rs");
    assert_eq!(picker.items[1].display_text, "gamma.rs");
    assert_eq!(picker.items[2].display_text, "beta.rs");

    // Close picker
    app.handle_msg(Msg::Key(key_press(KeyCode::Esc, KeyModifiers::NONE)));
}

#[test]
fn test_fuzzy_picker_navigation_updates_preview() {
    let temp = TempDir::new().expect("failed to create temp dir");
    let root = temp.path();

    fs::write(root.join("file_a.txt"), "content_a\n").unwrap();
    fs::write(root.join("file_b.txt"), "content_b\n").unwrap();

    let mut app = App::new(root.to_path_buf());
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('o'), KeyModifiers::NONE)));
    assert!(app.picker.is_some());

    assert_eq!(app.picker.as_ref().unwrap().selected_idx, 0);
    let preview_a = app
        .picker
        .as_ref()
        .unwrap()
        .preview
        .as_ref()
        .unwrap()
        .lines[0]
        .clone();
    assert_eq!(preview_a, "content_a");

    // Move Down
    app.handle_msg(Msg::Key(key_press(KeyCode::Down, KeyModifiers::NONE)));
    assert_eq!(app.picker.as_ref().unwrap().selected_idx, 1);
    let preview_b = app
        .picker
        .as_ref()
        .unwrap()
        .preview
        .as_ref()
        .unwrap()
        .lines[0]
        .clone();
    assert_eq!(preview_b, "content_b");

    // Move Up
    app.handle_msg(Msg::Key(key_press(KeyCode::Up, KeyModifiers::NONE)));
    assert_eq!(app.picker.as_ref().unwrap().selected_idx, 0);
    let preview_back = app
        .picker
        .as_ref()
        .unwrap()
        .preview
        .as_ref()
        .unwrap()
        .lines[0]
        .clone();
    assert_eq!(preview_back, "content_a");
}
