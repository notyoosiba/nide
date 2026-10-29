use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tempfile::TempDir;
use nide::app::{App, DragBorder, Focus, MiddleMode, Msg, TreeClipboardOp};
use nide::config::Config;
use nide::ui;

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn type_str(app: &mut App, s: &str) {
    for ch in s.chars() {
        app.handle_msg(Msg::Key(press(KeyCode::Char(ch))));
    }
}

// -----------------------------------------------------------------------------
// Acceptance Criterion 1: Debounced Autosave while typing in MiddleMode::Edit
// -----------------------------------------------------------------------------

#[test]
fn test_debounced_autosave_in_middle_mode_edit() {
    let temp = TempDir::new().expect("tempdir");
    let file_path = temp.path().join("code.rs");
    fs::write(&file_path, "fn initial() {}\n").expect("write file");

    let mut app = App::new(temp.path().to_path_buf());
    app.autosave_delay = Duration::from_millis(50);

    // Open file into MiddleMode::Edit
    app.open_file(&file_path).expect("open_file");
    assert_eq!(app.focus, Focus::Middle);
    assert_eq!(app.middle_mode, MiddleMode::Edit);
    assert!(!app.is_dirty());

    // Type a character into the buffer
    app.handle_msg(Msg::Key(press(KeyCode::Char('!'))));
    assert!(app.is_dirty());
    assert!(app.last_edit.is_some());

    // Immediate tick before delay elapsed -> must NOT save yet
    app.tick();
    assert!(app.is_dirty(), "buffer should still be dirty before autosave delay");
    let on_disk_before = fs::read_to_string(&file_path).expect("read file");
    assert_eq!(on_disk_before, "fn initial() {}\n");

    // Advance past autosave_delay
    std::thread::sleep(Duration::from_millis(60));

    // Call app.tick() -> atomic save completes while staying in MiddleMode::Edit
    app.tick();
    assert!(!app.is_dirty(), "buffer must be clean after debounced autosave");
    assert_eq!(app.middle_mode, MiddleMode::Edit, "must stay in MiddleMode::Edit after autosave");
    assert_eq!(app.focus, Focus::Middle);

    // Verify atomic save wrote to disk
    let on_disk_after = fs::read_to_string(&file_path).expect("read file");
    assert_eq!(on_disk_after, "!fn initial() {}\n");
}

#[test]
fn test_autosave_blocked_in_conflict_mode() {
    let temp = TempDir::new().expect("tempdir");
    let file_path = temp.path().join("conflict.rs");
    fs::write(&file_path, "original\n").expect("write file");

    let mut app = App::new(temp.path().to_path_buf());
    app.autosave_delay = Duration::from_millis(10);
    app.open_file(&file_path).expect("open_file");

    // Manually edit buffer and put in Conflict mode
    app.handle_msg(Msg::Key(press(KeyCode::Char('x'))));
    assert!(app.is_dirty());
    app.middle_mode = MiddleMode::Conflict;

    // Advance past autosave_delay
    app.last_edit = Some(Instant::now() - Duration::from_secs(5));

    // Tick should NOT autosave while in Conflict mode
    app.tick();
    assert!(app.is_dirty(), "conflict mode must block autosave");
    assert_eq!(fs::read_to_string(&file_path).unwrap(), "original\n");
}

// -----------------------------------------------------------------------------
// Acceptance Criterion 2: Tree Rename ('r'), Copy ('y' + 'p'), Move ('x' + 'p')
// -----------------------------------------------------------------------------

#[test]
fn test_tree_rename_copy_move_in_tempdir() {
    let temp = TempDir::new().expect("tempdir");
    let root = temp.path();

    // Setup initial tree:
    // root/
    //   file1.txt
    //   sub/
    //     nested.txt
    let file1 = root.join("file1.txt");
    fs::write(&file1, "file1 contents").expect("write file1");
    let sub_dir = root.join("sub");
    fs::create_dir_all(&sub_dir).expect("create sub");
    let nested = sub_dir.join("nested.txt");
    fs::write(&nested, "nested contents").expect("write nested");

    let mut app = App::new(root.to_path_buf());
    assert_eq!(app.focus, Focus::Tree);

    // -------------------------------------------------------------------------
    // 1. RENAME ('r'): rename "file1.txt" to "renamed.txt"
    // -------------------------------------------------------------------------
    assert!(app.tree.reveal(&PathBuf::from("file1.txt")));
    assert_eq!(app.tree.selected_path(), Some(Path::new("file1.txt")));

    // Press 'r' to open rename prompt
    app.handle_msg(Msg::Key(press(KeyCode::Char('r'))));
    assert!(app.tree_prompt.is_some());
    assert_eq!(app.tree_prompt.as_ref().unwrap().input.text(), "file1.txt");

    // Clear line and type "renamed.txt"
    for _ in 0..15 {
        app.handle_msg(Msg::Key(press(KeyCode::Backspace)));
    }
    type_str(&mut app, "renamed.txt");
    assert_eq!(app.tree_prompt.as_ref().unwrap().input.text(), "renamed.txt");

    // Confirm rename with Enter
    app.handle_msg(Msg::Key(press(KeyCode::Enter)));
    assert!(app.tree_prompt.is_none());

    // Verify on disk: file1.txt gone, renamed.txt present
    assert!(!file1.exists(), "old file must not exist");
    let renamed = root.join("renamed.txt");
    assert!(renamed.exists(), "new renamed file must exist");
    assert_eq!(fs::read_to_string(&renamed).unwrap(), "file1 contents");

    // Verify tree selection revealed the renamed file
    assert_eq!(app.tree.selected_path(), Some(Path::new("renamed.txt")));

    // -------------------------------------------------------------------------
    // 2. COPY ('y' + 'p'): copy "renamed.txt" into "sub/" directory
    // -------------------------------------------------------------------------
    // Press 'y' on renamed.txt
    app.handle_msg(Msg::Key(press(KeyCode::Char('y'))));
    assert_eq!(
        app.tree_clipboard,
        Some(TreeClipboardOp::Copy(renamed.clone()))
    );

    // Navigate to and select "sub" folder
    assert!(app.tree.reveal(&PathBuf::from("sub")));
    assert_eq!(app.tree.selected_path(), Some(Path::new("sub")));

    // Press 'p' to paste copy
    app.handle_msg(Msg::Key(press(KeyCode::Char('p'))));

    // Verify both original and copied files exist on disk
    let copied_in_sub = sub_dir.join("renamed.txt");
    assert!(renamed.exists(), "original file must still exist after copy");
    assert!(copied_in_sub.exists(), "copied file in sub must exist");
    assert_eq!(fs::read_to_string(&copied_in_sub).unwrap(), "file1 contents");
    assert_eq!(app.tree.selected_path(), Some(Path::new("sub/renamed.txt")));

    // -------------------------------------------------------------------------
    // 3. MOVE ('x' + 'p'): cut "sub/nested.txt" and paste into new folder "dest/"
    // -------------------------------------------------------------------------
    let dest_dir = root.join("dest");
    fs::create_dir_all(&dest_dir).expect("create dest");
    app.refresh_tree_sync();

    // Select "sub/nested.txt"
    assert!(app.tree.reveal(&PathBuf::from("sub/nested.txt")));
    assert_eq!(app.tree.selected_path(), Some(Path::new("sub/nested.txt")));

    // Press 'x' to cut/move
    app.handle_msg(Msg::Key(press(KeyCode::Char('x'))));
    assert_eq!(
        app.tree_clipboard,
        Some(TreeClipboardOp::Move(nested.clone()))
    );

    // Navigate to "dest" directory
    assert!(app.tree.reveal(&PathBuf::from("dest")));
    assert_eq!(app.tree.selected_path(), Some(Path::new("dest")));

    // Press 'p' to paste/move
    app.handle_msg(Msg::Key(press(KeyCode::Char('p'))));

    // Verify source removed, destination created, clipboard cleared
    assert!(!nested.exists(), "source file must no longer exist after move");
    let moved_file = dest_dir.join("nested.txt");
    assert!(moved_file.exists(), "moved file in dest must exist");
    assert_eq!(fs::read_to_string(&moved_file).unwrap(), "nested contents");
    assert!(app.tree_clipboard.is_none(), "clipboard must be cleared after move");
    assert_eq!(app.tree.selected_path(), Some(Path::new("dest/nested.txt")));
}

// -----------------------------------------------------------------------------
// Acceptance Criterion 3: Mouse Drag Border Resizing
// -----------------------------------------------------------------------------

#[test]
fn test_mouse_drag_vertical_pane_border_resizing() {
    let temp = TempDir::new().expect("tempdir");
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut app = App::new(temp.path().to_path_buf());

    // 1. Initial draw pass: widths = [20, 50, 30]
    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");

    assert_eq!(app.widths, [20, 50, 30]);
    let initial_tree = app.layout.tree.expect("tree layout");
    let initial_middle = app.layout.middle.expect("middle layout");
    let initial_right = app.layout.right.expect("right layout");

    assert_eq!(initial_tree.x, 0);
    assert_eq!(initial_tree.width, 20);
    assert_eq!(initial_middle.x, 20);
    assert_eq!(initial_middle.width, 50);
    assert_eq!(initial_right.x, 70);
    assert_eq!(initial_right.width, 30);

    // 2. Mouse Down on vertical border between Tree and Middle (col 20, row 10)
    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 20,
        row: 10,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(app.drag_border, Some(DragBorder::TreeMiddle));

    // 3. Mouse Drag to column 26
    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: 26,
        row: 10,
        modifiers: KeyModifiers::NONE,
    }));

    // Assert app.widths updated: tree expanded by 6 cols (20 -> 26), middle shrank by 6 (50 -> 44)
    assert_eq!(app.widths, [26, 44, 30]);
    assert_eq!(app.layout.tree.unwrap().width, 26);
    assert_eq!(app.layout.middle.unwrap().x, 26);
    assert_eq!(app.layout.middle.unwrap().width, 44);
    assert_eq!(app.layout.right.unwrap().width, 30);

    // Redraw and verify PaneLayout Rects match
    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");
    assert_eq!(app.layout.tree.unwrap().width, 26);
    assert_eq!(app.layout.middle.unwrap().width, 44);

    // 4. Test min 15 col clamping: drag Tree border all the way left to col 5
    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: 5,
        row: 10,
        modifiers: KeyModifiers::NONE,
    }));

    // Must clamp to min 15 cols
    assert_eq!(app.widths[0], 15, "tree width must be clamped to min 15 cols");
    assert_eq!(app.widths[1], 55, "middle width must absorb the difference");
    assert_eq!(app.layout.tree.unwrap().width, 15);
    assert_eq!(app.layout.middle.unwrap().width, 55);

    // Mouse Up releases drag
    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 5,
        row: 10,
        modifiers: KeyModifiers::NONE,
    }));
    assert!(app.drag_border.is_none());

    // 5. Drag border between Middle and Right (border is at middle.x + middle.width = 15 + 55 = 70)
    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 70,
        row: 12,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(app.drag_border, Some(DragBorder::MiddleRight));

    // Drag to col 75
    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: 75,
        row: 12,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(app.widths[1], 60);
    assert_eq!(app.widths[2], 25);
    assert_eq!(app.layout.middle.unwrap().width, 60);
    assert_eq!(app.layout.right.unwrap().width, 25);

    // Test min 15 col clamping on right border: drag to col 85 (right would be 100 - 85 = 15)
    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: 95,
        row: 12,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(app.widths[2], 15, "right pane must be clamped to min 15 cols");
    assert_eq!(app.layout.right.unwrap().width, 15);

    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 95,
        row: 12,
        modifiers: KeyModifiers::NONE,
    }));
    assert!(app.drag_border.is_none());
}

#[test]
fn test_mouse_drag_quake_console_border_resizing() {
    let temp = TempDir::new().expect("tempdir");
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut app = App::new(temp.path().to_path_buf());

    // Open Quake
    app.toggle_quake();
    assert!(app.show_quake);

    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");
    let quake_initial = app.layout.quake.expect("quake rect");

    // Click on top border of Quake
    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 50,
        row: quake_initial.y,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(app.drag_border, Some(DragBorder::Quake));

    // Drag up to row 12 (expanding quake height)
    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: 50,
        row: 12,
        modifiers: KeyModifiers::NONE,
    }));
    let new_quake = app.layout.quake.expect("new quake rect");
    assert_eq!(new_quake.y, 12);
    assert!(new_quake.height > quake_initial.height);
    assert_eq!(app.quake_height, Some(new_quake.height));

    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 50,
        row: 12,
        modifiers: KeyModifiers::NONE,
    }));
    assert!(app.drag_border.is_none());
}

#[test]
fn test_config_loader_initializes_app_widths_and_autosave() {
    let toml_str = r#"
        autosave_delay_ms = 400
        widths = [22, 48, 30]
    "#;
    let cfg = Config::from_toml(toml_str).expect("parse config");
    let temp = TempDir::new().expect("tempdir");
    let app = App::new(temp.path().to_path_buf()).with_config(cfg);

    assert_eq!(app.autosave_delay, Duration::from_millis(400));
    assert_eq!(app.widths, [22, 48, 30]);
    assert_eq!(app.tree_weight, 22);
    assert_eq!(app.middle_weight, 48);
    assert_eq!(app.right_weight, 30);
}
