use std::fs;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tempfile::TempDir;
use nide::app::{App, Focus, MiddleMode, Msg};
use nide::ui;

fn key_press(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

#[test]
fn test_undo_redo_multiple_edits_restores_exact_buffer_and_cursor() {
    let temp = TempDir::new().expect("failed to create temp dir");
    let file_path = temp.path().join("undo_test.txt");
    fs::write(&file_path, "initial text").expect("failed to write test file");

    let mut app = App::new(temp.path().to_path_buf());
    app.open_file(&file_path).expect("failed to open file");
    assert_eq!(app.focus, Focus::Middle);
    assert_eq!(app.middle_mode, MiddleMode::Edit);

    // Initial state: cursor at (0, 0), "initial text", dirty = false
    assert_eq!(app.buf.as_ref().unwrap().lines, vec!["initial text"]);
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 0);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 0);
    assert!(!app.is_dirty());

    // Move cursor to end of line: (0, 12)
    app.handle_msg(Msg::Key(key_press(KeyCode::End, KeyModifiers::NONE)));
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 12);

    // Edit 1: Insert '!'
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('!'), KeyModifiers::NONE)));
    assert_eq!(app.buf.as_ref().unwrap().lines, vec!["initial text!"]);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 13);
    assert!(app.is_dirty());

    // Edit 2: Enter (newline)
    app.handle_msg(Msg::Key(key_press(KeyCode::Enter, KeyModifiers::NONE)));
    assert_eq!(
        app.buf.as_ref().unwrap().lines,
        vec!["initial text!", ""]
    );
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 1);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 0);

    // Edit 3: Insert "hi"
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('h'), KeyModifiers::NONE)));
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('i'), KeyModifiers::NONE)));
    assert_eq!(
        app.buf.as_ref().unwrap().lines,
        vec!["initial text!", "hi"]
    );
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 1);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 2);

    // Edit 4: Backspace removes 'i'
    app.handle_msg(Msg::Key(key_press(KeyCode::Backspace, KeyModifiers::NONE)));
    assert_eq!(
        app.buf.as_ref().unwrap().lines,
        vec!["initial text!", "h"]
    );
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 1);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 1);

    // --- Undo step by step ---
    // Undo 4: Backspace undone -> restores "hi" at (1, 2)
    app.handle_msg(Msg::Key(key_press(
        KeyCode::Char('z'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(
        app.buf.as_ref().unwrap().lines,
        vec!["initial text!", "hi"]
    );
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 1);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 2);

    // Undo 3b: 'i' insert undone -> restores "h" at (1, 1)
    app.handle_msg(Msg::Key(key_press(
        KeyCode::Char('z'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(
        app.buf.as_ref().unwrap().lines,
        vec!["initial text!", "h"]
    );
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 1);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 1);

    // Undo 3a: 'h' insert undone -> restores "" at (1, 0)
    app.handle_msg(Msg::Key(key_press(
        KeyCode::Char('z'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(
        app.buf.as_ref().unwrap().lines,
        vec!["initial text!", ""]
    );
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 1);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 0);

    // Undo 2: Enter newline undone -> restores "initial text!" at (0, 13)
    app.handle_msg(Msg::Key(key_press(
        KeyCode::Char('z'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(app.buf.as_ref().unwrap().lines, vec!["initial text!"]);
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 0);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 13);

    // Undo 1: '!' insert undone -> restores "initial text" at (0, 12)
    app.handle_msg(Msg::Key(key_press(
        KeyCode::Char('z'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(app.buf.as_ref().unwrap().lines, vec!["initial text"]);
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 0);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 12);
    // When restored to base snapshot, dirty flag becomes false
    assert!(!app.is_dirty());

    // --- Redo step by step ---
    // Redo 1: restores "initial text!" at (0, 13)
    app.handle_msg(Msg::Key(key_press(
        KeyCode::Char('y'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(app.buf.as_ref().unwrap().lines, vec!["initial text!"]);
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 0);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 13);
    assert!(app.is_dirty());

    // Redo 2: restores ["initial text!", ""] at (1, 0)
    app.handle_msg(Msg::Key(key_press(
        KeyCode::Char('y'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(
        app.buf.as_ref().unwrap().lines,
        vec!["initial text!", ""]
    );
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 1);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 0);

    // Redo 3a: restores "h" at (1, 1)
    app.handle_msg(Msg::Key(key_press(
        KeyCode::Char('y'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(
        app.buf.as_ref().unwrap().lines,
        vec!["initial text!", "h"]
    );
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 1);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 1);

    // Redo 3b: restores "hi" at (1, 2)
    app.handle_msg(Msg::Key(key_press(
        KeyCode::Char('y'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(
        app.buf.as_ref().unwrap().lines,
        vec!["initial text!", "hi"]
    );
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 1);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 2);

    // Redo 4: restores backspace to "h" at (1, 1)
    app.handle_msg(Msg::Key(key_press(
        KeyCode::Char('y'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(
        app.buf.as_ref().unwrap().lines,
        vec!["initial text!", "h"]
    );
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 1);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 1);
}

#[test]
fn test_shift_right_select_cut_paste_and_osc52_formatting() {
    let temp = TempDir::new().expect("failed to create temp dir");
    let file_path = temp.path().join("clip_test.txt");
    fs::write(&file_path, "Hello World").expect("failed to write test file");

    let mut app = App::new(temp.path().to_path_buf());
    app.open_file(&file_path).expect("failed to open file");
    assert_eq!(app.focus, Focus::Middle);
    assert_eq!(app.middle_mode, MiddleMode::Edit);

    // Cursor starts at (0, 0)
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 0);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 0);
    assert!(app.selection.is_none());

    // Select "Hello" with Shift+Right (5 times)
    for _ in 0..5 {
        app.handle_msg(Msg::Key(key_press(KeyCode::Right, KeyModifiers::SHIFT)));
    }

    assert!(app.selection.is_some());
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 5);
    let sel = app.selection.unwrap();
    assert_eq!(sel.range(0, 5), ((0, 0), (0, 5)));

    // Cut selected text with Ctrl+X
    app.handle_msg(Msg::Key(key_press(
        KeyCode::Char('x'),
        KeyModifiers::CONTROL,
    )));

    // Verify buffer has " World" remaining, cursor at (0, 0), selection cleared
    assert_eq!(app.buf.as_ref().unwrap().lines, vec![" World"]);
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 0);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 0);
    assert!(app.selection.is_none());
    assert!(app.is_dirty());

    // Verify OSC 52 sequence formatting: \x1b]52;c;<base64>\x07
    // Base64 of "Hello" is "SGVsbG8="
    let expected_osc52 = "\x1b]52;c;SGVsbG8=\x07";
    assert_eq!(app.last_osc52.as_deref(), Some(expected_osc52));
    assert_eq!(app.clipboard.as_deref(), Some("Hello"));

    // Move cursor to the end of the line: (0, 6) after " World"
    for _ in 0..6 {
        app.handle_msg(Msg::Key(key_press(KeyCode::Right, KeyModifiers::NONE)));
    }
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 6);

    // Paste with Msg::Paste
    app.handle_msg(Msg::Paste("Hello".to_string()));

    // Verify resulting text: " WorldHello"
    assert_eq!(app.buf.as_ref().unwrap().lines, vec![" WorldHello"]);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 11);
}

#[test]
fn test_normal_mode_v_selection_and_yank_copy() {
    let temp = TempDir::new().expect("failed to create temp dir");
    let file_path = temp.path().join("normal_sel.txt");
    fs::write(&file_path, "alpha beta gamma").expect("failed to write test file");

    let mut app = App::new(temp.path().to_path_buf());
    app.open_file(&file_path).expect("failed to open file");

    // Press Esc to switch to MiddleMode::Normal
    app.handle_msg(Msg::Key(key_press(KeyCode::Esc, KeyModifiers::NONE)));
    assert_eq!(app.middle_mode, MiddleMode::Normal);
    assert_eq!(app.focus, Focus::Middle);

    // Cursor at (0, 0). Press 'v' to begin visual selection
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('v'), KeyModifiers::NONE)));
    assert!(app.selection.is_some());
    assert_eq!(app.selection.unwrap().anchor_row, 0);
    assert_eq!(app.selection.unwrap().anchor_col, 0);

    // Move right 5 times using 'l'
    for _ in 0..5 {
        app.handle_msg(Msg::Key(key_press(KeyCode::Char('l'), KeyModifiers::NONE)));
    }
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 5);

    // Yank / copy with 'y'
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('y'), KeyModifiers::NONE)));

    // Selection should be cleared, text unchanged
    assert!(app.selection.is_none());
    assert_eq!(app.buf.as_ref().unwrap().lines, vec!["alpha beta gamma"]);
    assert_eq!(app.clipboard.as_deref(), Some("alpha"));

    // Expected OSC 52: base64("alpha") = "YWxwaGE="
    let expected_osc52 = "\x1b]52;c;YWxwaGE=\x07";
    assert_eq!(app.last_osc52.as_deref(), Some(expected_osc52));
}

#[test]
fn test_mouse_drag_selection_in_middle_pane() {
    let temp = TempDir::new().expect("failed to create temp dir");
    let file_path = temp.path().join("mouse_sel.txt");
    fs::write(&file_path, "quick brown fox jumps over").expect("failed to write test file");

    let mut app = App::new(temp.path().to_path_buf());
    app.open_file(&file_path).expect("failed to open file");

    // Set middle pane layout: x=10, y=2, width=40, height=10
    app.layout.middle = Some(ratatui::layout::Rect::new(10, 2, 40, 10));

    // inner_x = 11, inner_y = 3, gutter_width = 4 ("  1 ")
    // text starts at column 11 + 4 = 15
    // Mouse Down at col 15, row 3 (character 0 of line 0)
    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 15,
        row: 3,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 0);

    // Mouse Drag to col 20, row 3 (character 5)
    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: 20,
        row: 3,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 5);
    assert!(app.selection.is_some());
    assert_eq!(app.selection.unwrap().anchor_col, 0);

    // Mouse Up at col 20, row 3: selection preserved!
    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 20,
        row: 3,
        modifiers: KeyModifiers::NONE,
    }));
    assert!(app.selection.is_some());
    assert_eq!(
        app.selection.unwrap().range(0, 5),
        ((0, 0), (0, 5))
    );

    // Now a simple click without drag (Down + Up at same cell): clears selection
    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 17,
        row: 3,
        modifiers: KeyModifiers::NONE,
    }));
    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 17,
        row: 3,
        modifiers: KeyModifiers::NONE,
    }));
    assert!(app.selection.is_none());
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 2);
}

#[test]
fn test_soft_line_wrapping_ui_rendering_with_test_backend() {
    let width = 45;
    let height = 15;
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("failed to create test terminal");

    let temp = TempDir::new().expect("failed to create temp dir");
    let file_path = temp.path().join("wrap_test.txt");
    let long_line = "The quick brown fox jumps over the lazy dog and runs across the wide field.";
    fs::write(&file_path, long_line).expect("failed to write test file");

    let mut app = App::new(temp.path().to_path_buf());
    app.open_file(&file_path).expect("failed to open file");

    terminal
        .draw(|f| ui::draw(f, &mut app))
        .expect("failed to render UI");

    let buffer = terminal.backend().buffer();
    let content: String = (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // The line is wrapped: continuation rows show "·" in the gutter
    assert!(
        content.contains('·'),
        "Rendered middle pane must contain continuation marker '·' in gutter"
    );
    assert!(
        content.contains("quick brown"),
        "First wrapped row content must be visible"
    );
    assert!(
        content.contains("wide field"),
        "Continuation wrapped row content must be visible"
    );
}

#[test]
fn test_selection_highlight_rendering_in_catppuccin_latte() {
    let width = 60;
    let height = 15;
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("failed to create test terminal");

    let temp = TempDir::new().expect("failed to create temp dir");
    let file_path = temp.path().join("sel_render.txt");
    fs::write(&file_path, "highlight selection").expect("failed to write test file");

    let mut app = App::new(temp.path().to_path_buf());
    app.open_file(&file_path).expect("failed to open file");

    // Select "highlight" (9 chars)
    for _ in 0..9 {
        app.handle_msg(Msg::Key(key_press(KeyCode::Right, KeyModifiers::SHIFT)));
    }

    terminal
        .draw(|f| ui::draw(f, &mut app))
        .expect("failed to render UI");

    let buffer = terminal.backend().buffer();

    // Find cells for "highlight"
    let mut selected_cells_found = 0;
    let mut unselected_cells_found = 0;

    for y in 0..height {
        for x in 0..width {
            let cell = &buffer[(x, y)];
            if cell.bg == nide::theme::SELECTION_BG {
                selected_cells_found += 1;
            } else if cell.symbol() == "s" && cell.bg == nide::theme::BASE {
                unselected_cells_found += 1;
            }
        }
    }

    assert_eq!(
        selected_cells_found, 9,
        "Exactly 9 characters should be rendered with Catppuccin Latte SELECTION_BG"
    );
    assert!(
        unselected_cells_found >= 1,
        "Unselected character 's' should have base background"
    );
}
