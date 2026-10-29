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
fn test_in_buffer_search_incremental_highlight_cycle_and_esc_restore() {
    let width = 80;
    let height = 24;
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("failed to create test terminal");

    let temp = TempDir::new().expect("failed to create temp dir");
    let file_path = temp.path().join("search_test.rs");
    let content = "\
fn first() {
    let target = 1;
}
fn second() {
    let target = 2;
}
fn third() {
    let target = 3;
}
";
    fs::write(&file_path, content).expect("failed to write test file");

    let mut app = App::new(temp.path().to_path_buf());
    app.open_file(&file_path).expect("failed to open file");

    // Exit Edit mode to Normal mode with Esc
    app.handle_msg(Msg::Key(key_press(KeyCode::Esc, KeyModifiers::NONE)));
    assert_eq!(app.middle_mode, MiddleMode::Normal);
    assert_eq!(app.focus, Focus::Middle);

    // Initial cursor is at (0, 0)
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 0);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 0);

    // Step 1: Press '/' to enter MiddleMode::Find
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('/'), KeyModifiers::NONE)));
    assert_eq!(app.middle_mode, MiddleMode::Find);
    assert!(app.find.is_some());
    assert_eq!(app.find.as_ref().unwrap().anchor, (0, 0));

    // Step 2: Type search query "target" incrementally
    for c in "target".chars() {
        app.handle_msg(Msg::Key(key_press(KeyCode::Char(c), KeyModifiers::NONE)));
    }

    assert_eq!(app.find.as_ref().unwrap().query, "target");
    assert_eq!(app.find.as_ref().unwrap().matches.len(), 3);

    // Assert cursor jumps to the first match at (1, 8) ("let target = 1;")
    let buf = app.buf.as_ref().unwrap();
    assert_eq!(buf.cursor_row, 1);
    assert_eq!(buf.cursor_col, 8);

    // Step 3: Render in TestBackend and assert live match highlighting and status bar
    terminal
        .draw(|f| ui::draw(f, &mut app))
        .expect("failed to render UI");

    let term_buf = terminal.backend().buffer();

    // Check editor cells for Catppuccin Latte search match highlights:
    // First match is current (PEACH), others are YELLOW
    let mut current_match_cells = 0;
    let mut other_match_cells = 0;

    // Scan middle editor pane area (excluding bottom status line)
    for y in 0..(height - 1) {
        for x in 0..width {
            let cell = &term_buf[(x, y)];
            if cell.bg == theme::SEARCH_MATCH_CURRENT_BG {
                current_match_cells += 1;
            } else if cell.bg == theme::SEARCH_MATCH_BG {
                other_match_cells += 1;
            }
        }
    }

    assert_eq!(
        current_match_cells, 6,
        "Current match 'target' (6 chars) should be highlighted with Catppuccin Latte PEACH"
    );
    assert_eq!(
        other_match_cells, 12,
        "Other matches (2 occurrences of 'target', 12 chars) should be highlighted with Catppuccin Latte YELLOW"
    );

    // Verify status bar shows "/target" and "[FIND]" badge
    let mut status_bar_text = String::new();
    let status_y = height - 1;
    for x in 0..width {
        status_bar_text.push_str(term_buf[(x, status_y)].symbol());
    }
    assert!(
        status_bar_text.contains("[FIND]"),
        "Status bar should render [FIND] mode badge, got: {status_bar_text}"
    );
    assert!(
        status_bar_text.contains("/target"),
        "Status bar should render search query /target, got: {status_bar_text}"
    );

    // Step 4: Confirm match with Enter to enter MiddleMode::Normal
    app.handle_msg(Msg::Key(key_press(KeyCode::Enter, KeyModifiers::NONE)));
    assert_eq!(app.middle_mode, MiddleMode::Normal);
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 1);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 8);

    // Step 5: Press 'n' to step forward to second match
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('n'), KeyModifiers::NONE)));
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 4);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 8);

    // Press 'n' to step forward to third match
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('n'), KeyModifiers::NONE)));
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 7);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 8);

    // Press 'n' to wraparound back to first match
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('n'), KeyModifiers::NONE)));
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 1);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 8);

    // Step 6: Press 'N' to step backward (wraparound to third match)
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('N'), KeyModifiers::NONE)));
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 7);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 8);

    // Press 'N' to step backward to second match
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('N'), KeyModifiers::NONE)));
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 4);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 8);

    // Step 7: Press Esc to restore original pre-search cursor position (0, 0)
    app.handle_msg(Msg::Key(key_press(KeyCode::Esc, KeyModifiers::NONE)));
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 0);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 0);
    assert!(
        app.find.is_none(),
        "Esc should clear active find state after restoring anchor"
    );
}

#[test]
fn test_esc_during_find_mode_restores_anchor() {
    let temp = TempDir::new().expect("failed to create temp dir");
    let file_path = temp.path().join("esc_test.txt");
    fs::write(
        &file_path,
        "line zero\nline one\nline two\nline three target\nline four",
    )
    .expect("failed to write test file");

    let mut app = App::new(temp.path().to_path_buf());
    app.open_file(&file_path).expect("failed to open file");

    // Exit Edit mode to Normal mode
    app.handle_msg(Msg::Key(key_press(KeyCode::Esc, KeyModifiers::NONE)));

    // Position cursor at line 2, col 3
    if let Some(buf) = &mut app.buf {
        buf.cursor_row = 2;
        buf.cursor_col = 3;
    }

    // Press '/' to start find
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('/'), KeyModifiers::NONE)));
    assert_eq!(app.middle_mode, MiddleMode::Find);

    // Type "target"
    for c in "target".chars() {
        app.handle_msg(Msg::Key(key_press(KeyCode::Char(c), KeyModifiers::NONE)));
    }

    // Cursor jumped to line 3, col 11
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 3);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 11);

    // Press Esc while still in MiddleMode::Find
    app.handle_msg(Msg::Key(key_press(KeyCode::Esc, KeyModifiers::NONE)));
    assert_eq!(app.middle_mode, MiddleMode::Normal);
    assert!(app.find.is_none());

    // Cursor restored to (2, 3)
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 2);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 3);
}

#[test]
fn test_ctrl_f_from_edit_mode_and_backspace() {
    let temp = TempDir::new().expect("failed to create temp dir");
    let file_path = temp.path().join("ctrl_f_test.txt");
    fs::write(&file_path, "apple banana apple cherry").expect("failed to write test file");

    let mut app = App::new(temp.path().to_path_buf());
    app.open_file(&file_path).expect("failed to open file");
    assert_eq!(app.middle_mode, MiddleMode::Edit);

    // Initial cursor at (0, 0)
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 0);
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 0);

    // Move cursor to col 5 ("banana")
    if let Some(buf) = &mut app.buf {
        buf.cursor_col = 5;
    }

    // Press Ctrl+F in Edit mode -> enters MiddleMode::Find
    app.handle_msg(Msg::Key(key_press(
        KeyCode::Char('f'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(app.middle_mode, MiddleMode::Find);
    assert_eq!(app.find.as_ref().unwrap().anchor, (0, 5));

    // Type "apple"
    for c in "apple".chars() {
        app.handle_msg(Msg::Key(key_press(KeyCode::Char(c), KeyModifiers::NONE)));
    }

    // Anchor was at col 5, so nearest match at or after anchor is the second "apple" at col 13
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 13);
    assert_eq!(app.find.as_ref().unwrap().current_match_index, Some(1));

    // Backspace 5 times
    for _ in 0..5 {
        app.handle_msg(Msg::Key(key_press(KeyCode::Backspace, KeyModifiers::NONE)));
    }

    assert_eq!(app.find.as_ref().unwrap().query, "");
    assert!(app.find.as_ref().unwrap().matches.is_empty());
    // Cursor restored to anchor (0, 5)
    assert_eq!(app.buf.as_ref().unwrap().cursor_col, 5);

    // Press Esc to exit Find mode
    app.handle_msg(Msg::Key(key_press(KeyCode::Esc, KeyModifiers::NONE)));
    assert_eq!(app.middle_mode, MiddleMode::Normal);
}

#[test]
fn test_smart_case_and_regex_fallback_in_buffer() {
    let temp = TempDir::new().expect("failed to create temp dir");
    let file_path = temp.path().join("case_test.txt");
    fs::write(&file_path, "Result result RESULT result(").expect("failed to write test file");

    let mut app = App::new(temp.path().to_path_buf());
    app.open_file(&file_path).expect("failed to open file");
    app.handle_msg(Msg::Key(key_press(KeyCode::Esc, KeyModifiers::NONE)));

    // 1. Lowercase search "result" -> case-insensitive, matches all 4 occurrences
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('/'), KeyModifiers::NONE)));
    for c in "result".chars() {
        app.handle_msg(Msg::Key(key_press(KeyCode::Char(c), KeyModifiers::NONE)));
    }
    assert_eq!(app.find.as_ref().unwrap().matches.len(), 4);

    // Cancel find
    app.handle_msg(Msg::Key(key_press(KeyCode::Esc, KeyModifiers::NONE)));

    // 2. Uppercase search "Result" -> case-sensitive, matches only 1 occurrence
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('/'), KeyModifiers::NONE)));
    for c in "Result".chars() {
        app.handle_msg(Msg::Key(key_press(KeyCode::Char(c), KeyModifiers::NONE)));
    }
    assert_eq!(app.find.as_ref().unwrap().matches.len(), 1);

    // Cancel find
    app.handle_msg(Msg::Key(key_press(KeyCode::Esc, KeyModifiers::NONE)));

    // 3. Fallback search with unclosed paren "result(" -> matches literal "result("
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('/'), KeyModifiers::NONE)));
    for c in "result(".chars() {
        app.handle_msg(Msg::Key(key_press(KeyCode::Char(c), KeyModifiers::NONE)));
    }
    assert_eq!(app.find.as_ref().unwrap().matches.len(), 1);
    assert_eq!(app.find.as_ref().unwrap().matches[0].col_start, 21);
}
