use std::fs;
use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tempfile::TempDir;
use nide::app::{App, Focus, MiddleMode, Msg};
use nide::pty::TermId;
use nide::ui;

fn buffer_to_string(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let area = buffer.area;
    let mut out = String::new();
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            out.push_str(buffer[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

// ---------------------------------------------------------------------------
// Acceptance Criterion 1: Alt+` Quake overlay, dimensions unchanged, PtyExited
// ---------------------------------------------------------------------------

#[test]
fn test_quake_console_toggle_focus_dimensions_and_pty_exited() {
    let tmp = TempDir::new().expect("tempdir failed");
    let root = tmp.path();

    let mut app = App::new(root.to_path_buf());
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");

    // Initial render pass to calculate layout geometry
    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");

    assert!(!app.show_quake, "Quake overlay must initially be hidden");
    assert_eq!(app.focus, Focus::Tree);
    assert_eq!(app.layout.quake, None);

    let orig_right = app
        .layout
        .right
        .expect("app.layout.right must exist before toggling Quake");

    // 1. Press Alt+` to open Quake overlay
    app.handle_msg(Msg::Key(KeyEvent::new(
        KeyCode::Char('`'),
        KeyModifiers::ALT,
    )));

    assert!(app.show_quake, "show_quake must be true after Alt+`");
    assert_eq!(
        app.focus,
        Focus::QuakeTerm,
        "Focus must switch to Focus::QuakeTerm"
    );
    assert!(
        app.quake_term.is_some(),
        "quake_term must be lazily initialized"
    );

    // Redraw and inspect layout
    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");

    // CRITICAL SPEC REQUIREMENT: app.layout.right dimensions remain strictly unchanged!
    let right_after_quake = app
        .layout
        .right
        .expect("app.layout.right must exist when Quake is shown");
    assert_eq!(
        right_after_quake, orig_right,
        "app.layout.right dimensions must remain unchanged when Quake is open"
    );

    // Check Quake layout geometry: covers bottom 35% of main_area
    let quake_rect = app
        .layout
        .quake
        .expect("app.layout.quake must be present when show_quake is true");
    // Main area height is 29 (30 - 1 status line). 35% of 29 = 10.
    assert_eq!(quake_rect.width, 120);
    assert_eq!(quake_rect.height, 10);
    assert_eq!(quake_rect.y, 19);

    // Verify UI output in TestBackend contains "Quake Console" title and status bar badge
    let text = buffer_to_string(&terminal);
    assert!(
        text.contains("Quake Console"),
        "Buffer should contain Quake Console title"
    );
    assert!(
        text.contains("[QUAKE]") || text.contains("QUAKE"),
        "Status bar should reflect QUAKE focus"
    );
    assert!(
        text.contains("[`:Quake]"),
        "Status bar should include [`:Quake] badge"
    );

    // 2. Press Alt+` again to toggle Quake off
    app.handle_msg(Msg::Key(KeyEvent::new(
        KeyCode::Char('`'),
        KeyModifiers::ALT,
    )));

    assert!(!app.show_quake, "show_quake must be false after toggling");
    assert_eq!(
        app.focus,
        Focus::Tree,
        "Focus must be restored to previous focus (Tree)"
    );

    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");
    assert_eq!(app.layout.quake, None);
    assert_eq!(app.layout.right.expect("right exists"), orig_right);

    // 3. Open Quake via Ctrl+` fallback
    app.handle_msg(Msg::Key(KeyEvent::new(
        KeyCode::Char('`'),
        KeyModifiers::CONTROL,
    )));
    assert!(app.show_quake, "show_quake must be true after Ctrl+`");
    assert_eq!(app.focus, Focus::QuakeTerm);

    // 4. Simulate PTY child process exit: Msg::PtyExited { pane: TermId::Quake }
    app.handle_msg(Msg::PtyExited {
        pane: TermId::Quake,
    });

    assert!(
        !app.show_quake,
        "show_quake must be hidden on PtyExited {{ pane: Quake }}"
    );
    assert!(
        app.quake_term.is_none(),
        "quake_term must be dropped on exit so it respawns fresh next open"
    );
    assert_eq!(
        app.focus,
        Focus::Tree,
        "Focus must restore to previous focus after Quake PTY exit"
    );

    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");
    assert_eq!(app.layout.quake, None);
    assert_eq!(app.layout.right.expect("right exists"), orig_right);
}

// ---------------------------------------------------------------------------
// Acceptance Criterion 2: Quake Ctrl+Click Link Jump into Middle Editor
// ---------------------------------------------------------------------------

#[test]
fn test_quake_ctrl_click_link_resolution_opens_middle_editor() {
    let tmp = TempDir::new().expect("tempdir failed");
    let root = tmp.path();

    // Setup target source file: src/lib.rs
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).expect("create src dir");
    let lib_content = "\
// Line 1
// Line 2
pub fn quake_target() {
    println!(\"from quake\");
}
";
    let lib_path = src_dir.join("lib.rs");
    fs::write(&lib_path, lib_content).expect("write src/lib.rs");

    let mut app = App::new(root.to_path_buf());
    // Toggle Quake open
    app.toggle_quake();
    assert!(app.show_quake);
    assert_eq!(app.focus, Focus::QuakeTerm);

    // Feed terminal output referencing "src/lib.rs:3:5"
    let quake_term = app.quake_term.as_mut().expect("quake_term present");
    quake_term.process_output(b"Compiled src/lib.rs:3:5\r\n");

    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");

    let quake_rect = app.layout.quake.expect("quake_rect present");
    let inner_x = quake_rect.x + 1;
    let inner_y = quake_rect.y + 1;

    // "Compiled " is 9 chars. Click at inner_x + 12 on row inner_y (inside "src/lib.rs")
    let click_col = inner_x + 12;
    let click_row = inner_y;

    let mouse_event = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: click_col,
        row: click_row,
        modifiers: KeyModifiers::CONTROL,
    };
    app.handle_msg(Msg::Mouse(mouse_event));

    // Verify acceptance criteria:
    // 1. File loaded into Middle buffer
    assert!(
        app.buf.is_some(),
        "Expected buffer to be loaded in Middle pane"
    );
    let buf = app.buf.as_ref().unwrap();
    assert!(
        buf.path.ends_with("src/lib.rs"),
        "Expected app.buf.path to be src/lib.rs, got: {:?}",
        buf.path
    );

    // 2. Line 3 -> 0-indexed 2
    assert_eq!(
        app.line, 2,
        "Expected app.line to be 2 (line 3 in 1-indexed), got {}",
        app.line
    );
    // Col 5 -> 0-indexed 4
    assert_eq!(
        app.col, 4,
        "Expected app.col to be 4 (col 5 in 1-indexed), got {}",
        app.col
    );

    // 3. Focus switches to Middle editor
    assert_eq!(
        app.focus,
        Focus::Middle,
        "Expected focus to switch to Middle pane"
    );

    // 4. Middle mode is Edit
    assert_eq!(
        app.middle_mode,
        MiddleMode::Edit,
        "Expected middle mode to be MiddleMode::Edit"
    );

    // 5. Revealed and selected in tree
    assert_eq!(
        app.tree.selected_path(),
        Some(Path::new("src/lib.rs")),
        "Expected left tree to highlight src/lib.rs"
    );
}

#[test]
fn test_quake_cmd_click_super_modifier_also_opens_file() {
    let tmp = TempDir::new().expect("tempdir failed");
    let root = tmp.path();

    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).expect("create src dir");
    let main_path = src_dir.join("main.rs");
    fs::write(&main_path, "fn main() {}\n// line 2\n").expect("write main.rs");

    let mut app = App::new(root.to_path_buf());
    app.toggle_quake();

    let quake_term = app.quake_term.as_mut().expect("quake_term present");
    quake_term.process_output(b"Check src/main.rs:2:1\r\n");

    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");

    let quake_rect = app.layout.quake.expect("quake_rect");
    let inner_x = quake_rect.x + 1;
    let inner_y = quake_rect.y + 1;

    let mouse_event = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: inner_x + 8,
        row: inner_y,
        modifiers: KeyModifiers::SUPER,
    };
    app.handle_msg(Msg::Mouse(mouse_event));

    assert!(app.buf.is_some());
    assert!(app.buf.as_ref().unwrap().path.ends_with("src/main.rs"));
    assert_eq!(app.line, 1); // Line 2 -> 0-indexed 1
    assert_eq!(app.focus, Focus::Middle);
    assert_eq!(app.middle_mode, MiddleMode::Edit);
}

// ---------------------------------------------------------------------------
// Acceptance Criterion 3: Help Overlay (?) Opens and Closes in TestBackend
// ---------------------------------------------------------------------------

#[test]
fn test_help_overlay_open_and_close_in_test_backend() {
    let tmp = TempDir::new().expect("tempdir failed");
    let root = tmp.path();

    let mut app = App::new(root.to_path_buf());
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");

    // Initially in Focus::Tree, help is closed
    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");
    assert!(!app.show_help);
    let initial_text = buffer_to_string(&terminal);
    assert!(
        !initial_text.contains("Help — Keybindings"),
        "Help modal should not be visible initially"
    );

    // 1. Press '?' in Focus::Tree to open Help overlay
    app.handle_msg(Msg::Key(KeyEvent::new(
        KeyCode::Char('?'),
        KeyModifiers::NONE,
    )));
    assert!(app.show_help, "show_help should be true after '?'");

    // Draw and verify Help modal content rendered on screen
    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");
    let help_text = buffer_to_string(&terminal);
    assert!(
        help_text.contains("Help — Keybindings"),
        "Help modal title must be rendered in buffer"
    );
    assert!(
        help_text.contains("Toggle Quake Console Overlay"),
        "Help modal should list Quake console shortcut"
    );
    assert!(
        help_text.contains("Toggle Tree / Middle / Right Panes"),
        "Help modal should list pane toggles"
    );
    assert!(
        help_text.contains("Press ? or Esc to close"),
        "Help modal should display close prompt"
    );

    // 2. Press '?' again to toggle Help overlay off
    app.handle_msg(Msg::Key(KeyEvent::new(
        KeyCode::Char('?'),
        KeyModifiers::NONE,
    )));
    assert!(!app.show_help, "show_help should be false after second '?'");

    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");
    let closed_text = buffer_to_string(&terminal);
    assert!(
        !closed_text.contains("Help — Keybindings"),
        "Help modal should disappear after closing"
    );

    // 3. Test opening from MiddleMode::Normal and closing with Esc
    let test_file = root.join("test.txt");
    fs::write(&test_file, "content").expect("write test file");
    app.open_file(&test_file).expect("open file");
    app.middle_mode = MiddleMode::Normal;
    assert_eq!(app.focus, Focus::Middle);

    app.handle_msg(Msg::Key(KeyEvent::new(
        KeyCode::Char('?'),
        KeyModifiers::NONE,
    )));
    assert!(
        app.show_help,
        "show_help should be openable from MiddleMode::Normal"
    );

    app.handle_msg(Msg::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
    assert!(
        !app.show_help,
        "Esc should close the help overlay"
    );

    // 4. Test that '?' in MiddleMode::Edit inserts char instead of opening Help
    app.middle_mode = MiddleMode::Edit;
    app.handle_msg(Msg::Key(KeyEvent::new(
        KeyCode::Char('?'),
        KeyModifiers::NONE,
    )));
    assert!(
        !app.show_help,
        "'?' in MiddleMode::Edit must not open help modal"
    );
    let buf_content = app.buf.as_ref().unwrap().lines.join("\n");
    assert!(
        buf_content.contains('?'),
        "'?' should be inserted into the buffer in edit mode"
    );
}

// ---------------------------------------------------------------------------
// Additional: Quake terminal inputs, tab, and paste
// ---------------------------------------------------------------------------

#[test]
fn test_quake_terminal_input_and_paste() {
    let tmp = TempDir::new().expect("tempdir failed");
    let root = tmp.path();

    let mut app = App::new(root.to_path_buf());
    app.toggle_quake();
    assert_eq!(app.focus, Focus::QuakeTerm);

    // Tab key in QuakeTerm is forwarded to shell
    app.handle_msg(Msg::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
    let quake = app.quake_term.as_ref().unwrap();
    assert_eq!(quake.last_written, b"\t");

    // Paste in QuakeTerm is forwarded
    app.handle_msg(Msg::Paste("ls -la\n".to_string()));
    let quake = app.quake_term.as_ref().unwrap();
    assert_eq!(quake.last_written, b"ls -la\n");

    // 'q' in QuakeTerm does NOT quit the application
    app.handle_msg(Msg::Key(KeyEvent::new(
        KeyCode::Char('q'),
        KeyModifiers::NONE,
    )));
    assert!(!app.should_quit, "'q' in QuakeTerm must not quit the app");
}
