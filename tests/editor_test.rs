use std::fs;
use std::path::Path;

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tempfile::TempDir;
use nide::app::{edit::MiddleMode, App, Focus, Msg};
use nide::buffer::Buffer;
use nide::ui;

fn key_press(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn plain_key(code: KeyCode) -> KeyEvent {
    key_press(code, KeyModifiers::NONE)
}

fn mouse_down(column: u16, row: u16, button: MouseButton) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(button),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn mouse_scroll_down(column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn mouse_scroll_up(column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn has_temp_save_files(dir: &Path) -> bool {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with(".nide-save-") {
                        return true;
                    }
                }
            } else if path.is_dir() && has_temp_save_files(&path) {
                return true;
            }
        }
    }
    false
}

#[test]
fn test_tree_enter_open_edit_esc_save_esc_tree() {
    let tmp = TempDir::new().expect("tempdir");
    let file_path = tmp.path().join("doc.txt");
    fs::write(&file_path, "initial content\n").expect("write doc.txt");

    let mut app = App::new(tmp.path().to_path_buf());
    assert_eq!(app.focus, Focus::Tree);

    // Tree cursor starts on doc.txt (only file in root)
    let selected = app.tree.selected_path().expect("selected file");
    assert_eq!(selected, Path::new("doc.txt"));

    // 1. Press Enter in tree -> opens in MiddleMode::Edit
    app.handle_msg(Msg::Key(plain_key(KeyCode::Enter)));
    assert_eq!(app.focus, Focus::Middle);
    assert_eq!(app.middle_mode, MiddleMode::Edit);
    assert!(app.buf.is_some());
    assert_eq!(app.buf.as_ref().unwrap().lines, vec!["initial content"]);

    // Move to end of line 0
    app.handle_msg(Msg::Key(plain_key(KeyCode::End)));

    // Type " hello"
    for c in " hello".chars() {
        app.handle_msg(Msg::Key(plain_key(KeyCode::Char(c))));
    }

    assert!(app.is_dirty());
    assert_eq!(
        app.buf.as_ref().unwrap().lines[0],
        "initial content hello"
    );

    // Before Esc, disk file still has original content
    let on_disk_before = fs::read_to_string(&file_path).expect("read before save");
    assert_eq!(on_disk_before, "initial content\n");

    // 2. Press Esc -> saves atomically to disk & transitions to MiddleMode::Normal
    app.handle_msg(Msg::Key(plain_key(KeyCode::Esc)));
    assert_eq!(app.focus, Focus::Middle);
    assert_eq!(app.middle_mode, MiddleMode::Normal);
    assert!(!app.is_dirty());

    let on_disk_after = fs::read_to_string(&file_path).expect("read after save");
    assert!(
        on_disk_after.contains("hello"),
        "Disk file must contain 'hello'"
    );
    assert_eq!(on_disk_after, "initial content hello\n");

    // No leftover .nide-save-* temp files
    assert!(!has_temp_save_files(tmp.path()));

    // 3. Press Esc again in Normal mode -> returns focus to Focus::Tree
    app.handle_msg(Msg::Key(plain_key(KeyCode::Esc)));
    assert_eq!(app.focus, Focus::Tree);
}

#[test]
fn test_preserve_crlf_and_utf8_bom() {
    let tmp = TempDir::new().expect("tempdir");
    let file_path = tmp.path().join("bom_crlf.txt");

    // Write file with UTF-8 BOM and CRLF line endings
    let mut original_bytes = vec![0xEF, 0xBB, 0xBF];
    original_bytes.extend_from_slice(b"alpha\r\nbeta\r\n");
    fs::write(&file_path, &original_bytes).expect("write bom_crlf.txt");

    let mut app = App::new(tmp.path().to_path_buf());
    assert!(app.tree.reveal(Path::new("bom_crlf.txt")));

    // Open file
    app.handle_msg(Msg::Key(plain_key(KeyCode::Enter)));
    assert_eq!(app.focus, Focus::Middle);
    assert_eq!(app.middle_mode, MiddleMode::Edit);

    let buf = app.buf.as_ref().expect("buffer loaded");
    assert!(buf.bom, "Buffer should recognize BOM");
    assert!(buf.crlf, "Buffer should recognize CRLF");
    assert!(
        buf.trailing_newline,
        "Buffer should recognize trailing newline"
    );
    assert_eq!(buf.lines, vec!["alpha", "beta"]);

    // Move cursor down to second line
    app.handle_msg(Msg::Key(plain_key(KeyCode::Down)));
    // Move to end of second line
    app.handle_msg(Msg::Key(plain_key(KeyCode::End)));
    // Type " gamma"
    for c in " gamma".chars() {
        app.handle_msg(Msg::Key(plain_key(KeyCode::Char(c))));
    }

    // Save with Esc
    app.handle_msg(Msg::Key(plain_key(KeyCode::Esc)));
    assert_eq!(app.middle_mode, MiddleMode::Normal);
    assert!(!app.is_dirty());

    // Verify on disk: BOM + CRLF preserved byte-for-byte
    let saved_bytes = fs::read(&file_path).expect("read file after save");
    let mut expected_bytes = vec![0xEF, 0xBB, 0xBF];
    expected_bytes.extend_from_slice(b"alpha\r\nbeta gamma\r\n");

    assert_eq!(
        saved_bytes, expected_bytes,
        "File must match expected bytes with BOM and CRLF preserved"
    );

    // Verify no leftover .nide-save-* temp files
    assert!(!has_temp_save_files(tmp.path()));
}

#[test]
fn test_switch_file_auto_saves_dirty_buffer() {
    let tmp = TempDir::new().expect("tempdir");
    let file_a = tmp.path().join("a.txt");
    let file_b = tmp.path().join("b.txt");

    fs::write(&file_a, "content A\n").expect("write a.txt");
    fs::write(&file_b, "content B\n").expect("write b.txt");

    let mut app = App::new(tmp.path().to_path_buf());

    // 1. Open file A
    assert!(app.tree.reveal(Path::new("a.txt")));
    app.handle_msg(Msg::Key(plain_key(KeyCode::Enter)));
    assert_eq!(app.focus, Focus::Middle);
    assert_eq!(app.middle_mode, MiddleMode::Edit);
    assert_eq!(
        app.buf.as_ref().unwrap().path,
        file_a.canonicalize().unwrap()
    );

    // Modify file A in MiddleMode::Edit without saving
    for c in "modified ".chars() {
        app.handle_msg(Msg::Key(plain_key(KeyCode::Char(c))));
    }
    assert!(app.is_dirty());

    // Ensure file A on disk is still original before switch
    assert_eq!(fs::read_to_string(&file_a).unwrap(), "content A\n");

    // 2. Switch focus to Tree (e.g. mouse click or focus switch) without pressing Esc
    app.focus = Focus::Tree;
    assert_eq!(app.focus, Focus::Tree);

    // Select file B and press Enter
    assert!(app.tree.reveal(Path::new("b.txt")));
    app.handle_msg(Msg::Key(plain_key(KeyCode::Enter)));

    // Now file B should be loaded in Middle pane
    assert_eq!(app.focus, Focus::Middle);
    assert_eq!(app.middle_mode, MiddleMode::Edit);
    assert_eq!(
        app.buf.as_ref().unwrap().path,
        file_b.canonicalize().unwrap()
    );

    // 3. File A must have been auto-saved to disk!
    let a_disk = fs::read_to_string(&file_a).expect("read file A");
    assert_eq!(
        a_disk, "modified content A\n",
        "File A must be auto-saved to disk on switch"
    );

    assert!(!has_temp_save_files(tmp.path()));
}

#[test]
fn test_modal_enter_i_reenters_edit_mode() {
    let tmp = TempDir::new().expect("tempdir");
    let file_path = tmp.path().join("modal.txt");
    fs::write(&file_path, "line 1\nline 2\n").expect("write modal.txt");

    let mut app = App::new(tmp.path().to_path_buf());
    assert!(app.tree.reveal(Path::new("modal.txt")));
    app.handle_msg(Msg::Key(plain_key(KeyCode::Enter)));

    assert_eq!(app.focus, Focus::Middle);
    assert_eq!(app.middle_mode, MiddleMode::Edit);

    // Esc drops to Normal
    app.handle_msg(Msg::Key(plain_key(KeyCode::Esc)));
    assert_eq!(app.middle_mode, MiddleMode::Normal);
    assert_eq!(app.focus, Focus::Middle);

    // 'i' re-enters Edit mode
    app.handle_msg(Msg::Key(plain_key(KeyCode::Char('i'))));
    assert_eq!(app.middle_mode, MiddleMode::Edit);

    // Esc drops to Normal
    app.handle_msg(Msg::Key(plain_key(KeyCode::Esc)));
    assert_eq!(app.middle_mode, MiddleMode::Normal);

    // 'Enter' in Normal mode also re-enters Edit mode
    app.handle_msg(Msg::Key(plain_key(KeyCode::Enter)));
    assert_eq!(app.middle_mode, MiddleMode::Edit);
}

#[test]
fn test_q_in_edit_inserts_char_and_q_in_normal_saves_and_quits() {
    let tmp = TempDir::new().expect("tempdir");
    let file_path = tmp.path().join("q_test.txt");
    fs::write(&file_path, "hello\n").expect("write q_test.txt");

    let mut app = App::new(tmp.path().to_path_buf());
    assert!(app.tree.reveal(Path::new("q_test.txt")));
    app.handle_msg(Msg::Key(plain_key(KeyCode::Enter)));

    assert_eq!(app.middle_mode, MiddleMode::Edit);

    // Pressing 'q' in Edit mode must NOT quit; it types 'q'
    app.handle_msg(Msg::Key(plain_key(KeyCode::Char('q'))));
    assert!(
        !app.should_quit,
        "'q' in Edit mode must not trigger application quit"
    );
    assert_eq!(app.buf.as_ref().unwrap().lines[0], "qhello");
    assert!(app.is_dirty());

    // Press Esc to enter Normal mode (which saves the buffer)
    app.handle_msg(Msg::Key(plain_key(KeyCode::Esc)));
    assert_eq!(app.middle_mode, MiddleMode::Normal);
    assert!(!app.is_dirty());

    // Re-enter Edit mode and dirty it again
    app.handle_msg(Msg::Key(plain_key(KeyCode::Char('i'))));
    app.handle_msg(Msg::Key(plain_key(KeyCode::Char('!'))));
    assert!(app.is_dirty());

    // Switch to Normal mode WITHOUT Esc saving (simulate focus switch to tree or stay in normal)
    app.middle_mode = MiddleMode::Normal;
    // 'q' in Normal mode when dirty must auto-save and quit
    app.handle_msg(Msg::Key(plain_key(KeyCode::Char('q'))));
    assert!(app.should_quit, "'q' in Normal mode should quit");

    let content = fs::read_to_string(&file_path).expect("read file");
    assert_eq!(
        content, "q!hello\n",
        "Quit must auto-save dirty buffer before exiting"
    );
}

#[test]
fn test_editing_keys_newline_backspace_delete_tab() {
    let tmp = TempDir::new().expect("tempdir");
    let file_path = tmp.path().join("keys.txt");
    fs::write(&file_path, "ab").expect("write keys.txt");

    let mut app = App::new(tmp.path().to_path_buf());
    assert!(app.tree.reveal(Path::new("keys.txt")));
    app.handle_msg(Msg::Key(plain_key(KeyCode::Enter)));

    // Cursor at (0, 0). Insert Tab (4 spaces)
    app.handle_msg(Msg::Key(plain_key(KeyCode::Tab)));
    assert_eq!(app.buf.as_ref().unwrap().lines[0], "    ab");
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 4);

    // Insert Enter newline between 'a' and 'b'
    // Move right 1 to after 'a' -> col 5
    app.handle_msg(Msg::Key(plain_key(KeyCode::Right)));
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 5);
    app.handle_msg(Msg::Key(plain_key(KeyCode::Enter)));
    assert_eq!(
        app.buf.as_ref().unwrap().lines,
        vec!["    a".to_string(), "b".to_string()]
    );
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 1);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 0);

    // Backspace at row 1, col 0 joins back with line 0
    app.handle_msg(Msg::Key(plain_key(KeyCode::Backspace)));
    assert_eq!(app.buf.as_ref().unwrap().lines, vec!["    ab".to_string()]);
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 0);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 5);

    // Delete character after cursor ('b')
    app.handle_msg(Msg::Key(plain_key(KeyCode::Delete)));
    assert_eq!(app.buf.as_ref().unwrap().lines, vec!["    a".to_string()]);

    // Backspace removes 'a'
    app.handle_msg(Msg::Key(plain_key(KeyCode::Backspace)));
    assert_eq!(app.buf.as_ref().unwrap().lines, vec!["    ".to_string()]);
}

#[test]
fn test_binary_file_detection_and_safeguard() {
    let tmp = TempDir::new().expect("tempdir");
    let file_path = tmp.path().join("binary.bin");
    let bin_data = vec![0x47, 0x49, 0x46, 0x38, 0x00, 0x01, 0x02];
    fs::write(&file_path, &bin_data).expect("write binary");

    let mut buf = Buffer::load(&file_path).expect("load binary buffer");
    assert!(buf.is_binary, "Buffer should detect binary via NUL sniff");

    // Editing operations should be no-ops on binary
    buf.insert_char('x');
    buf.insert_newline();
    buf.backspace();
    buf.delete();

    // Save should not corrupt binary file
    buf.save().expect("save binary");
    let read_back = fs::read(&file_path).expect("read binary");
    assert_eq!(read_back, bin_data);
}

#[test]
fn test_ui_rendering_with_test_backend() {
    let tmp = TempDir::new().expect("tempdir");
    let file_path = tmp.path().join("hello.rs");
    fs::write(&file_path, "fn main() {\n    println!(\"hi\");\n}\n").expect("write hello.rs");

    let mut app = App::new(tmp.path().to_path_buf());
    assert!(app.tree.reveal(Path::new("hello.rs")));
    app.handle_msg(Msg::Key(plain_key(KeyCode::Enter)));

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");

    terminal
        .draw(|f| ui::draw(f, &mut app))
        .expect("draw frame");

    let buffer = terminal.backend().buffer();
    let content = format!("{buffer:?}");

    // Border title contains filename and [EDIT]
    assert!(content.contains("hello.rs"), "Must render filename in UI");
    assert!(content.contains("EDIT"), "Must render EDIT mode indicator");

    // Line numbers in gutter
    assert!(content.contains(" 1 "), "Must render line number 1");
    assert!(content.contains(" 2 "), "Must render line number 2");
    assert!(content.contains(" 3 "), "Must render line number 3");

    // Code lines rendered
    assert!(content.contains("fn main()"), "Must render code contents");

    // Check terminal cursor position
    let cursor = terminal.get_cursor_position().expect("cursor position");
    // Middle pane starts around x=20, inner x=21, gutter width=4 ("  1 ") -> x=25, y=1
    let middle = app.layout.middle.unwrap();
    assert_eq!(cursor.y, middle.y + 1);
    assert_eq!(cursor.x, middle.x + 1 + 4);
}

#[test]
fn test_mouse_click_in_middle_moves_cursor() {
    let tmp = TempDir::new().expect("tempdir");
    let file_path = tmp.path().join("lines.txt");
    fs::write(&file_path, "first line\nsecond line\nthird line\n").expect("write lines.txt");

    let mut app = App::new(tmp.path().to_path_buf());
    assert!(app.tree.reveal(Path::new("lines.txt")));
    app.handle_msg(Msg::Key(plain_key(KeyCode::Enter)));

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|f| ui::draw(f, &mut app))
        .expect("draw frame");

    let middle = app.layout.middle.unwrap();
    // Click on row 2 (second line: y = middle.y + 1 + 1)
    let click_y = middle.y + 2;
    // Gutter width is 4 ("  1 "), inner_x is middle.x + 1, so column for char 3 is middle.x + 1 + 4 + 3
    let click_x = middle.x + 1 + 4 + 3;

    app.handle_msg(Msg::Mouse(mouse_down(click_x, click_y, MouseButton::Left)));

    let buf = app.buf.as_ref().unwrap();
    assert_eq!(buf.cursor_row, 1, "Cursor row should move to row 1");
    assert_eq!(buf.cursor_col, 3, "Cursor col should move to col 3");
}

#[test]
fn test_mouse_scroll_down_advances_and_does_not_clamp_to_zero() {
    let tmp = TempDir::new().expect("tempdir");
    let file_path = tmp.path().join("long.txt");
    let content = (0..500)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&file_path, content).expect("write long.txt");

    let mut app = App::new(tmp.path().to_path_buf());
    assert!(app.tree.reveal(Path::new("long.txt")));
    app.handle_msg(Msg::Key(plain_key(KeyCode::Enter)));

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|f| ui::draw(f, &mut app))
        .expect("draw frame");

    let middle = app.layout.middle.unwrap();
    assert_eq!(app.buf.as_ref().unwrap().scroll_row, 0);
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 0);

    // Dispatch ScrollDown in the middle pane
    app.handle_msg(Msg::Mouse(mouse_scroll_down(middle.x + 2, middle.y + 2)));
    assert_eq!(app.buf.as_ref().unwrap().scroll_row, 3);

    // Render frame via TestBackend: scroll_row must NOT be clamped back to 0
    terminal
        .draw(|f| ui::draw(f, &mut app))
        .expect("draw frame");
    assert_eq!(
        app.buf.as_ref().unwrap().scroll_row,
        3,
        "scroll_row must remain at 3 after rendering frame"
    );
    assert_eq!(
        app.buf.as_ref().unwrap().cursor_row,
        0,
        "cursor_row must remain at 0"
    );
}

#[test]
fn test_mouse_scroll_down_reaches_eof_and_scroll_up_without_freezing() {
    let tmp = TempDir::new().expect("tempdir");
    let file_path = tmp.path().join("long.txt");
    let content = (0..500)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&file_path, content).expect("write long.txt");

    let mut app = App::new(tmp.path().to_path_buf());
    assert!(app.tree.reveal(Path::new("long.txt")));
    app.handle_msg(Msg::Key(plain_key(KeyCode::Enter)));

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|f| ui::draw(f, &mut app))
        .expect("draw frame");

    let middle = app.layout.middle.unwrap();
    let visible_height = middle.height.saturating_sub(2) as usize;
    let expected_max_scroll = 500 - visible_height;

    // Repeatedly scroll down and render frames
    for _ in 0..200 {
        app.handle_msg(Msg::Mouse(mouse_scroll_down(middle.x + 2, middle.y + 2)));
        terminal
            .draw(|f| ui::draw(f, &mut app))
            .expect("draw frame");
    }

    assert_eq!(
        app.buf.as_ref().unwrap().scroll_row,
        expected_max_scroll,
        "scroll_row must smoothly reach EOF"
    );

    // Scroll up from EOF without freezing or being clamped by cursor position (which is still at 0)
    for _ in 0..200 {
        app.handle_msg(Msg::Mouse(mouse_scroll_up(middle.x + 2, middle.y + 2)));
        terminal
            .draw(|f| ui::draw(f, &mut app))
            .expect("draw frame");
    }

    assert_eq!(
        app.buf.as_ref().unwrap().scroll_row,
        0,
        "scroll_row must smoothly scroll all the way back up to 0"
    );
}

#[test]
fn test_cursor_snap_back_on_key_navigation() {
    let tmp = TempDir::new().expect("tempdir");
    let file_path = tmp.path().join("long.txt");
    let content = (0..500)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&file_path, content).expect("write long.txt");

    let mut app = App::new(tmp.path().to_path_buf());
    assert!(app.tree.reveal(Path::new("long.txt")));
    app.handle_msg(Msg::Key(plain_key(KeyCode::Enter)));

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|f| ui::draw(f, &mut app))
        .expect("draw frame");

    let middle = app.layout.middle.unwrap();

    // Scroll viewport down 5 times (15 rows) away from cursor at line 0
    for _ in 0..5 {
        app.handle_msg(Msg::Mouse(mouse_scroll_down(middle.x + 2, middle.y + 2)));
    }
    terminal
        .draw(|f| ui::draw(f, &mut app))
        .expect("draw frame");
    assert_eq!(app.buf.as_ref().unwrap().scroll_row, 15);
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 0);

    // Switch to normal mode and press 'j' (move down to line 1)
    app.handle_msg(Msg::Key(plain_key(KeyCode::Esc)));
    app.handle_msg(Msg::Key(plain_key(KeyCode::Char('j'))));
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 1);

    // Render frame: viewport must snap back to make line 1 visible
    terminal
        .draw(|f| ui::draw(f, &mut app))
        .expect("draw frame");
    assert_eq!(
        app.buf.as_ref().unwrap().scroll_row,
        1,
        "Viewport must snap back to make line 1 visible"
    );
}

#[test]
fn test_normal_mode_g_and_capital_g_file_boundary_navigation() {
    let tmp = TempDir::new().expect("tempdir");
    let file_path = tmp.path().join("long.txt");
    let content = (0..500)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&file_path, content).expect("write long.txt");

    let mut app = App::new(tmp.path().to_path_buf());
    assert!(app.tree.reveal(Path::new("long.txt")));
    app.handle_msg(Msg::Key(plain_key(KeyCode::Enter)));

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|f| ui::draw(f, &mut app))
        .expect("draw frame");

    let middle = app.layout.middle.unwrap();
    let visible_height = middle.height.saturating_sub(2) as usize;

    // Switch to Normal mode
    app.handle_msg(Msg::Key(plain_key(KeyCode::Esc)));
    assert_eq!(app.middle_mode, MiddleMode::Normal);
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 0);

    // Press 'G' (EOF)
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('G'), KeyModifiers::SHIFT)));
    assert_eq!(
        app.buf.as_ref().unwrap().cursor_row,
        499,
        "G must navigate cursor to EOF"
    );
    assert!(
        app.selection.is_none(),
        "G must not initiate selection"
    );

    terminal
        .draw(|f| ui::draw(f, &mut app))
        .expect("draw frame");
    let scroll_row = app.buf.as_ref().unwrap().scroll_row;
    assert!(
        scroll_row + visible_height >= 500,
        "Viewport must adjust so EOF is visible: scroll_row={scroll_row}, visible_height={visible_height}"
    );

    // Press 'g' (BOF)
    app.handle_msg(Msg::Key(plain_key(KeyCode::Char('g'))));
    assert_eq!(
        app.buf.as_ref().unwrap().cursor_row,
        0,
        "g must navigate cursor to BOF"
    );
    assert_eq!(
        app.buf.as_ref().unwrap().scroll_row,
        0,
        "scroll_row must adjust to 0 for BOF"
    );
    assert!(
        app.selection.is_none(),
        "g must not initiate selection"
    );

    terminal
        .draw(|f| ui::draw(f, &mut app))
        .expect("draw frame");
    assert_eq!(app.buf.as_ref().unwrap().scroll_row, 0);
}

#[test]
fn test_normal_mode_dollar_and_zero_line_navigation() {
    let tmp = TempDir::new().expect("tempdir");
    let file_path = tmp.path().join("lines.txt");
    fs::write(&file_path, "hello world 12345\nline 2\n").expect("write lines.txt");

    let mut app = App::new(tmp.path().to_path_buf());
    assert!(app.tree.reveal(Path::new("lines.txt")));
    app.handle_msg(Msg::Key(plain_key(KeyCode::Enter)));

    // Switch to Normal mode
    app.handle_msg(Msg::Key(plain_key(KeyCode::Esc)));
    assert_eq!(app.middle_mode, MiddleMode::Normal);
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 0);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 0);

    // Press '$' (with SHIFT)
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('$'), KeyModifiers::SHIFT)));
    assert_eq!(
        app.buf.as_ref().unwrap().cursor_row,
        0,
        "cursor_row must stay 0"
    );
    assert_eq!(
        app.buf.as_ref().unwrap().cursor_col,
        17,
        "cursor_col must move to end of line ('hello world 12345' is 17 chars)"
    );
    assert!(
        app.selection.is_none(),
        "'$' must not initiate text selection even though SHIFT was pressed"
    );

    // Press '0'
    app.handle_msg(Msg::Key(plain_key(KeyCode::Char('0'))));
    assert_eq!(
        app.buf.as_ref().unwrap().cursor_row,
        0,
        "cursor_row must stay 0"
    );
    assert_eq!(
        app.buf.as_ref().unwrap().cursor_col,
        0,
        "cursor_col must move to 0"
    );
    assert!(
        app.selection.is_none(),
        "'0' must not initiate text selection"
    );

    // Shift + End must initiate selection
    app.handle_msg(Msg::Key(key_press(KeyCode::End, KeyModifiers::SHIFT)));
    assert!(
        app.selection.is_some(),
        "Shift+End must initiate selection"
    );
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 17);

    // Shift + Home keeps/updates selection
    app.handle_msg(Msg::Key(key_press(KeyCode::Home, KeyModifiers::SHIFT)));
    assert!(
        app.selection.is_some(),
        "Shift+Home keeps selection"
    );
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 0);
}
