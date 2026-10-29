use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tempfile::TempDir;
use nide::app::{App, Focus, MiddleMode, Msg};
use nide::ui;

fn press(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
        state: KeyEventState::empty(),
    }
}

fn create_test_app(temp_dir: &TempDir) -> (App, PathBuf) {
    let file_path = temp_dir.path().join("sample.txt");
    std::fs::write(&file_path, "Hello world\nSecond line\n").expect("write test file");
    let app = App::new(temp_dir.path().to_path_buf());
    (app, file_path)
}

#[test]
fn test_macos_keys_toggle_panes_in_all_modes() {
    let temp_dir = TempDir::new().expect("tempdir");
    let (mut app, file_path) = create_test_app(&temp_dir);

    // 1. Open file into MiddleMode::Edit
    app.open_file(&file_path).expect("open_file");
    assert_eq!(app.focus, Focus::Middle);
    assert_eq!(app.middle_mode, MiddleMode::Edit);
    assert!(app.show_tree);
    assert!(app.show_middle);
    assert!(app.show_right);

    let initial_content = app.buf.as_ref().unwrap().lines.join("\n");

    // Press '¡' (Option+1 on macOS) in MiddleMode::Edit -> toggles show_tree to false
    app.handle_msg(Msg::Key(press(KeyCode::Char('¡'), KeyModifiers::NONE)));
    assert!(!app.show_tree, "'¡' should toggle show_tree to false");
    // Ensure '¡' was NOT inserted into the buffer text
    assert_eq!(
        app.buf.as_ref().unwrap().lines.join("\n"),
        initial_content,
        "'¡' must not be inserted into editor buffer"
    );

    // Press '¡' again -> toggles show_tree back to true
    app.handle_msg(Msg::Key(press(KeyCode::Char('¡'), KeyModifiers::NONE)));
    assert!(app.show_tree, "'¡' should toggle show_tree back to true");

    // Press '£' (Option+3 on macOS) in MiddleMode::Edit -> toggles show_right to false
    app.handle_msg(Msg::Key(press(KeyCode::Char('£'), KeyModifiers::NONE)));
    assert!(!app.show_right, "'£' should toggle show_right to false");
    assert_eq!(
        app.buf.as_ref().unwrap().lines.join("\n"),
        initial_content,
        "'£' must not be inserted into editor buffer"
    );

    // Press '£' again -> toggles show_right back to true
    app.handle_msg(Msg::Key(press(KeyCode::Char('£'), KeyModifiers::NONE)));
    assert!(app.show_right, "'£' should toggle show_right back to true");

    // Press '™' (Option+2 on macOS) in MiddleMode::Edit -> toggles show_middle to false
    app.handle_msg(Msg::Key(press(KeyCode::Char('™'), KeyModifiers::NONE)));
    assert!(!app.show_middle, "'™' should toggle show_middle to false");
    // Since Middle was focused and is now hidden, focus must fall back to visible pane
    assert_ne!(app.focus, Focus::Middle);

    // Press '€' (European macOS Option+2) -> toggles show_middle back to true
    app.handle_msg(Msg::Key(press(KeyCode::Char('€'), KeyModifiers::NONE)));
    assert!(app.show_middle, "'€' should toggle show_middle back to true");

    // 2. Now test in Focus::RightTerm
    app.set_focus(Focus::RightTerm);
    assert_eq!(app.focus, Focus::RightTerm);

    // Press '¡' in RightTerm -> toggles show_tree
    app.handle_msg(Msg::Key(press(KeyCode::Char('¡'), KeyModifiers::NONE)));
    assert!(!app.show_tree, "'¡' in RightTerm must toggle show_tree");

    app.handle_msg(Msg::Key(press(KeyCode::Char('¡'), KeyModifiers::NONE)));
    assert!(app.show_tree, "'¡' in RightTerm must toggle show_tree back");

    // Press '™' in RightTerm -> toggles show_middle
    app.handle_msg(Msg::Key(press(KeyCode::Char('™'), KeyModifiers::NONE)));
    assert!(!app.show_middle, "'™' in RightTerm must toggle show_middle");

    app.handle_msg(Msg::Key(press(KeyCode::Char('™'), KeyModifiers::NONE)));
    assert!(app.show_middle, "'™' in RightTerm must toggle show_middle back");

    // Press '£' in RightTerm -> toggles show_right to false (and focus falls back)
    app.handle_msg(Msg::Key(press(KeyCode::Char('£'), KeyModifiers::NONE)));
    assert!(!app.show_right, "'£' in RightTerm must toggle show_right to false");
    assert_ne!(app.focus, Focus::RightTerm, "Focus must fall back when right pane is hidden");

    // Press '£' again to restore
    app.handle_msg(Msg::Key(press(KeyCode::Char('£'), KeyModifiers::NONE)));
    assert!(app.show_right, "'£' must toggle show_right back to true");

    // 3. Test Ctrl+1/2/3 fallbacks
    app.handle_msg(Msg::Key(press(KeyCode::Char('1'), KeyModifiers::CONTROL)));
    assert!(!app.show_tree, "Ctrl+1 should toggle tree");
    app.handle_msg(Msg::Key(press(KeyCode::Char('1'), KeyModifiers::CONTROL)));
    assert!(app.show_tree, "Ctrl+1 should toggle tree back");

    app.handle_msg(Msg::Key(press(KeyCode::Char('2'), KeyModifiers::CONTROL)));
    assert!(!app.show_middle, "Ctrl+2 should toggle middle");
    app.handle_msg(Msg::Key(press(KeyCode::Char('2'), KeyModifiers::CONTROL)));
    assert!(app.show_middle, "Ctrl+2 should toggle middle back");

    app.handle_msg(Msg::Key(press(KeyCode::Char('3'), KeyModifiers::CONTROL)));
    assert!(!app.show_right, "Ctrl+3 should toggle right");
    app.handle_msg(Msg::Key(press(KeyCode::Char('3'), KeyModifiers::CONTROL)));
    assert!(app.show_right, "Ctrl+3 should toggle right back");

    // 4. Test standard Alt+1/2/3
    app.handle_msg(Msg::Key(press(KeyCode::Char('1'), KeyModifiers::ALT)));
    assert!(!app.show_tree, "Alt+1 should toggle tree");
    app.handle_msg(Msg::Key(press(KeyCode::Char('1'), KeyModifiers::ALT)));
    assert!(app.show_tree, "Alt+1 should toggle tree back");
}

#[test]
fn test_layout_redistributes_100_percent_width_and_refuses_to_hide_last_pane() {
    let width = 100;
    let height = 30;
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    let mut app = App::new(PathBuf::from("/workspace/sample"));

    // 1. Initial draw: all 3 panes visible
    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");

    let tree = app.layout.tree.expect("tree layout");
    let middle = app.layout.middle.expect("middle layout");
    let right = app.layout.right.expect("right layout");

    assert_eq!(tree.x, 0);
    assert_eq!(tree.x + tree.width, middle.x);
    assert_eq!(middle.x + middle.width, right.x);
    assert_eq!(tree.width + middle.width + right.width, width);

    // 2. Hide RightTerm -> remaining panes redistribute 100% of width
    app.toggle_right();
    assert!(!app.show_right);

    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");
    assert!(app.layout.right.is_none());
    let tree = app.layout.tree.expect("tree layout");
    let middle = app.layout.middle.expect("middle layout");

    assert_eq!(tree.x, 0);
    assert_eq!(tree.x + tree.width, middle.x);
    assert_eq!(tree.width + middle.width, width, "Tree + Middle must span 100% of terminal width");

    // 3. Hide Middle -> Tree gets 100% of width
    app.toggle_middle();
    assert!(!app.show_middle);

    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");
    assert!(app.layout.middle.is_none());
    assert!(app.layout.right.is_none());
    let tree = app.layout.tree.expect("tree layout");

    assert_eq!(tree.x, 0);
    assert_eq!(tree.width, width, "Tree alone must span 100% of terminal width");

    // 4. Try to hide Tree -> Tree is the LAST visible pane, so it MUST refuse to hide!
    app.toggle_tree();
    assert!(app.show_tree, "Must refuse to hide the last visible pane");

    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");
    let tree = app.layout.tree.expect("tree layout");
    assert_eq!(tree.width, width);

    // Also test refusal via key dispatch
    app.handle_msg(Msg::Key(press(KeyCode::Char('¡'), KeyModifiers::NONE)));
    assert!(app.show_tree, "'¡' must also refuse to hide the last visible pane");
    app.handle_msg(Msg::Key(press(KeyCode::Char('1'), KeyModifiers::ALT)));
    assert!(app.show_tree, "Alt+1 must also refuse to hide the last visible pane");

    // 5. Test refusal when only Middle is visible
    app.toggle_middle(); // Show middle
    app.toggle_tree();   // Hide tree
    assert!(!app.show_tree);
    assert!(app.show_middle);
    assert!(!app.show_right);

    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");
    let middle = app.layout.middle.expect("middle layout");
    assert_eq!(middle.x, 0);
    assert_eq!(middle.width, width, "Middle alone must span 100% of terminal width");

    app.toggle_middle(); // Try to hide last pane
    assert!(app.show_middle, "Must refuse to hide middle when it is the last visible pane");

    // 6. Test refusal when only Right is visible
    app.toggle_right();  // Show right
    app.toggle_middle(); // Hide middle
    assert!(!app.show_tree);
    assert!(!app.show_middle);
    assert!(app.show_right);

    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");
    let right = app.layout.right.expect("right layout");
    assert_eq!(right.x, 0);
    assert_eq!(right.width, width, "Right alone must span 100% of terminal width");

    app.toggle_right(); // Try to hide last pane
    assert!(app.show_right, "Must refuse to hide right when it is the last visible pane");

    // 7. Test width redistribution on 120-col terminal with Tree and Right visible
    let width_120 = 120;
    let backend_120 = TestBackend::new(width_120, height);
    let mut terminal_120 = Terminal::new(backend_120).expect("create terminal");

    app.show_tree = true;
    app.show_middle = false;
    app.show_right = true;

    terminal_120.draw(|f| ui::draw(f, &mut app)).expect("draw");
    let tree = app.layout.tree.expect("tree layout");
    let right = app.layout.right.expect("right layout");

    assert_eq!(tree.x, 0);
    assert_eq!(tree.x + tree.width, right.x);
    assert_eq!(tree.width + right.width, width_120, "Tree + Right must span 100% of width 120");
}

#[test]
fn test_focus_fallback_on_hide() {
    let mut app = App::new(PathBuf::from("/workspace/sample"));
    assert_eq!(app.focus, Focus::Tree);

    // Scenario A: Hiding active pane moves focus to prev_focus if visible
    // Move focus from Tree -> Middle
    app.set_focus(Focus::Middle);
    assert_eq!(app.focus, Focus::Middle);
    assert_eq!(app.prev_focus, Some(Focus::Tree));

    // Hide Middle -> Focus must fall back to prev_focus (Tree)
    app.toggle_middle();
    assert_eq!(app.focus, Focus::Tree, "Hiding active Middle must move focus to prev_focus (Tree)");

    // Restore Middle
    app.toggle_middle();
    assert!(app.show_middle);

    // Move focus from Tree -> RightTerm
    app.set_focus(Focus::RightTerm);
    assert_eq!(app.focus, Focus::RightTerm);
    assert_eq!(app.prev_focus, Some(Focus::Tree));

    // Move focus from RightTerm -> Middle
    app.set_focus(Focus::Middle);
    assert_eq!(app.focus, Focus::Middle);
    assert_eq!(app.prev_focus, Some(Focus::RightTerm));

    // Hide Middle -> Focus must fall back to prev_focus (RightTerm)
    app.toggle_middle();
    assert_eq!(app.focus, Focus::RightTerm, "Hiding active Middle must move focus to prev_focus (RightTerm)");

    // Scenario B: Left-to-right fallback when prev_focus is NOT visible
    // Reset all visible
    app.show_tree = true;
    app.show_middle = true;
    app.show_right = true;
    app.set_focus(Focus::Tree);
    app.set_focus(Focus::Middle);
    assert_eq!(app.prev_focus, Some(Focus::Tree));

    // Hide Tree (the prev_focus target) while Middle is focused
    app.toggle_tree();
    assert!(!app.show_tree);
    assert_eq!(app.focus, Focus::Middle);

    // Now hide Middle -> prev_focus (Tree) is NOT visible!
    // Left-to-right fallback across [Tree (hidden), Middle (hidden), RightTerm (visible)]
    // -> Must fall back to RightTerm!
    app.toggle_middle();
    assert_eq!(
        app.focus,
        Focus::RightTerm,
        "When prev_focus is not visible, focus must fall back left-to-right to RightTerm"
    );

    // Scenario C: Left-to-right fallback when prev_focus is None
    let mut fresh_app = App::new(PathBuf::from("/workspace/sample"));
    assert_eq!(fresh_app.focus, Focus::Tree);
    assert_eq!(fresh_app.prev_focus, None);

    // Hide Tree -> Left-to-right fallback moves focus to Middle
    fresh_app.toggle_tree();
    assert_eq!(
        fresh_app.focus,
        Focus::Middle,
        "Hiding Tree with prev_focus=None must fall back left-to-right to Middle"
    );

    // Scenario D: Hiding an unfocused pane leaves focus intact
    let mut app2 = App::new(PathBuf::from("/workspace/sample"));
    app2.set_focus(Focus::Middle);
    assert_eq!(app2.focus, Focus::Middle);

    app2.toggle_right();
    assert_eq!(app2.focus, Focus::Middle, "Hiding Right while Middle is focused must not alter focus");
}

#[test]
fn test_alt_tab_and_ctrl_backslash_focus_cycling() {
    let temp_dir = TempDir::new().expect("tempdir");
    let (mut app, file_path) = create_test_app(&temp_dir);

    app.open_file(&file_path).expect("open_file");
    assert_eq!(app.focus, Focus::Middle);
    assert_eq!(app.middle_mode, MiddleMode::Edit);

    // 1. Regular Tab in MiddleMode::Edit inserts indent, does not switch focus
    let len_before = app.buf.as_ref().unwrap().lines[0].len();
    app.handle_msg(Msg::Key(press(KeyCode::Tab, KeyModifiers::NONE)));
    assert_eq!(app.focus, Focus::Middle, "Regular Tab in Edit must not switch focus");
    let len_after = app.buf.as_ref().unwrap().lines[0].len();
    assert!(len_after > len_before, "Regular Tab in Edit should insert indentation");

    // 2. Alt+Tab in MiddleMode::Edit escapes editor and cycles focus to RightTerm
    app.handle_msg(Msg::Key(press(KeyCode::Tab, KeyModifiers::ALT)));
    assert_eq!(app.focus, Focus::RightTerm, "Alt+Tab must escape MiddleMode::Edit to RightTerm");

    // 3. Regular Tab in RightTerm is forwarded to PTY, does not switch focus
    app.handle_msg(Msg::Key(press(KeyCode::Tab, KeyModifiers::NONE)));
    assert_eq!(app.focus, Focus::RightTerm, "Regular Tab in RightTerm must not switch focus");

    // 4. Ctrl+\ escapes RightTerm and cycles to Tree
    app.handle_msg(Msg::Key(press(KeyCode::Char('\\'), KeyModifiers::CONTROL)));
    assert_eq!(app.focus, Focus::Tree, "Ctrl+\\ in RightTerm must cycle focus to Tree");

    // 5. Ctrl+\ in Tree cycles to Middle
    app.handle_msg(Msg::Key(press(KeyCode::Char('\\'), KeyModifiers::CONTROL)));
    assert_eq!(app.focus, Focus::Middle, "Ctrl+\\ in Tree must cycle focus to Middle");

    // 6. Ctrl+\ in MiddleMode::Edit cycles to RightTerm
    app.handle_msg(Msg::Key(press(KeyCode::Char('\\'), KeyModifiers::CONTROL)));
    assert_eq!(app.focus, Focus::RightTerm, "Ctrl+\\ in MiddleMode::Edit must cycle focus to RightTerm");

    // 7. Alt+Tab from RightTerm cycles to Tree
    app.handle_msg(Msg::Key(press(KeyCode::Tab, KeyModifiers::ALT)));
    assert_eq!(app.focus, Focus::Tree, "Alt+Tab from RightTerm must cycle to Tree");

    // 8. Cycling skips hidden panes
    app.toggle_middle(); // Hide middle
    assert!(!app.show_middle);
    assert_eq!(app.focus, Focus::Tree);

    // Tab in Tree skips hidden Middle and goes to RightTerm
    app.handle_msg(Msg::Key(press(KeyCode::Tab, KeyModifiers::NONE)));
    assert_eq!(app.focus, Focus::RightTerm, "Tab in Tree must skip hidden Middle and focus RightTerm");

    // Alt+Tab in RightTerm skips hidden Middle and goes to Tree
    app.handle_msg(Msg::Key(press(KeyCode::Tab, KeyModifiers::ALT)));
    assert_eq!(app.focus, Focus::Tree, "Alt+Tab in RightTerm must skip hidden Middle and focus Tree");
}

#[test]
fn test_status_bar_badges_reflect_visibility() {
    let width = 120;
    let height = 30;
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    let mut app = App::new(PathBuf::from("/workspace/sample"));

    // 1. All 3 panes visible -> all 3 badges present
    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");
    let content = format!("{:?}", terminal.backend().buffer());
    assert!(content.contains("[1:Tree]"), "Status bar must show [1:Tree]");
    assert!(content.contains("[2:Editor]"), "Status bar must show [2:Editor]");
    assert!(content.contains("[3:Term]"), "Status bar must show [3:Term]");

    // 2. Hide Tree -> [1:Tree] disappears
    app.toggle_tree();
    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");
    let content = format!("{:?}", terminal.backend().buffer());
    assert!(!content.contains("[1:Tree]"), "Hidden tree badge must not appear");
    assert!(content.contains("[2:Editor]"), "Visible editor badge must appear");
    assert!(content.contains("[3:Term]"), "Visible term badge must appear");

    // 3. Hide Right -> only [2:Editor] appears
    app.toggle_right();
    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");
    let content = format!("{:?}", terminal.backend().buffer());
    assert!(!content.contains("[1:Tree]"));
    assert!(content.contains("[2:Editor]"));
    assert!(!content.contains("[3:Term]"));
}

#[test]
fn test_shift_number_and_tilde_pane_toggles_in_tree_mode() {
    let temp_dir = TempDir::new().expect("tempdir");
    let (mut app, _) = create_test_app(&temp_dir);
    assert_eq!(app.focus, Focus::Tree);
    assert!(app.show_tree);
    assert!(app.show_middle);
    assert!(app.show_right);
    assert!(!app.show_quake);

    // 1. Shift+1 (or '!') toggles Tree pane
    app.handle_msg(Msg::Key(press(KeyCode::Char('!'), KeyModifiers::NONE)));
    assert!(!app.show_tree, "'!' in Tree mode should toggle Tree off");
    // Since Tree is hidden, focus moved
    assert_ne!(app.focus, Focus::Tree);

    // Toggle Tree back on via Shift+1 from new focus
    app.handle_msg(Msg::Key(press(KeyCode::Char('1'), KeyModifiers::SHIFT)));
    assert!(app.show_tree, "Shift+1 should toggle Tree back on");

    // Return to Tree
    app.set_focus(Focus::Tree);

    // 2. Shift+2 (or '@') toggles Middle pane
    app.handle_msg(Msg::Key(press(KeyCode::Char('@'), KeyModifiers::NONE)));
    assert!(!app.show_middle, "'@' in Tree mode should toggle Middle off");
    app.handle_msg(Msg::Key(press(KeyCode::Char('2'), KeyModifiers::SHIFT)));
    assert!(app.show_middle, "Shift+2 should toggle Middle back on");

    // 3. Shift+3 (or '#') toggles Right pane
    app.handle_msg(Msg::Key(press(KeyCode::Char('#'), KeyModifiers::NONE)));
    assert!(!app.show_right, "'#' in Tree mode should toggle Right off");
    app.handle_msg(Msg::Key(press(KeyCode::Char('3'), KeyModifiers::SHIFT)));
    assert!(app.show_right, "Shift+3 should toggle Right back on");

    // 4. '~' (or Shift+`) toggles Quake console
    app.handle_msg(Msg::Key(press(KeyCode::Char('~'), KeyModifiers::NONE)));
    assert!(app.show_quake, "'~' in Tree mode should toggle Quake open");
    assert_eq!(app.focus, Focus::QuakeTerm);

    // Switch focus back to Tree and close Quake with '~'
    app.set_focus(Focus::Tree);
    app.handle_msg(Msg::Key(press(KeyCode::Char('~'), KeyModifiers::NONE)));
    assert!(!app.show_quake, "'~' should toggle Quake closed");

    // Test bare backtick and section sign in Tree mode
    app.handle_msg(Msg::Key(press(KeyCode::Char('`'), KeyModifiers::NONE)));
    assert!(app.show_quake, "bare '`' in Tree mode should toggle Quake open");
    app.set_focus(Focus::Tree);
    app.handle_msg(Msg::Key(press(KeyCode::Char('§'), KeyModifiers::NONE)));
    assert!(!app.show_quake, "'§' in Tree mode should toggle Quake closed");
}

#[test]
fn test_shift_number_and_tilde_pane_toggles_in_normal_and_view_modes() {
    let temp_dir = TempDir::new().expect("tempdir");
    let (mut app, file_path) = create_test_app(&temp_dir);

    // Open file and enter Normal mode
    app.open_file(&file_path).expect("open_file");
    app.middle_mode = MiddleMode::Normal;
    assert_eq!(app.focus, Focus::Middle);
    assert_eq!(app.middle_mode, MiddleMode::Normal);

    // 1. Shift+1 in Normal mode toggles Tree
    app.handle_msg(Msg::Key(press(KeyCode::Char('!'), KeyModifiers::NONE)));
    assert!(!app.show_tree, "'!' in Normal mode should toggle Tree off");
    app.handle_msg(Msg::Key(press(KeyCode::Char('!'), KeyModifiers::NONE)));
    assert!(app.show_tree, "'!' in Normal mode should toggle Tree back on");

    // 2. Shift+2 in Normal mode toggles Middle
    app.handle_msg(Msg::Key(press(KeyCode::Char('@'), KeyModifiers::NONE)));
    assert!(!app.show_middle, "'@' in Normal mode should toggle Middle off");
    app.handle_msg(Msg::Key(press(KeyCode::Char('@'), KeyModifiers::NONE)));
    assert!(app.show_middle, "'@' should toggle Middle back on");
    app.set_focus(Focus::Middle);
    app.middle_mode = MiddleMode::Normal;

    // 3. Shift+3 in Normal mode toggles Right
    app.handle_msg(Msg::Key(press(KeyCode::Char('#'), KeyModifiers::NONE)));
    assert!(!app.show_right, "'#' in Normal mode should toggle Right off");
    app.handle_msg(Msg::Key(press(KeyCode::Char('#'), KeyModifiers::NONE)));
    assert!(app.show_right, "'#' in Normal mode should toggle Right back on");

    // 4. '~' in Normal mode toggles Quake
    app.handle_msg(Msg::Key(press(KeyCode::Char('~'), KeyModifiers::NONE)));
    assert!(app.show_quake, "'~' in Normal mode should toggle Quake open");
    assert_eq!(app.focus, Focus::QuakeTerm);

    // Close Quake from Normal mode
    app.set_focus(Focus::Middle);
    app.middle_mode = MiddleMode::Normal;
    app.handle_msg(Msg::Key(press(KeyCode::Char('~'), KeyModifiers::NONE)));
    assert!(!app.show_quake, "'~' should toggle Quake closed");

    // 5. Test in MiddleMode::View
    app.middle_mode = MiddleMode::View;
    app.handle_msg(Msg::Key(press(KeyCode::Char('!'), KeyModifiers::NONE)));
    assert!(!app.show_tree, "'!' in View mode should toggle Tree off");
    app.handle_msg(Msg::Key(press(KeyCode::Char('!'), KeyModifiers::NONE)));
    assert!(app.show_tree, "'!' in View mode should toggle Tree on");

    app.handle_msg(Msg::Key(press(KeyCode::Char('~'), KeyModifiers::NONE)));
    assert!(app.show_quake, "'~' in View mode should toggle Quake open");
}

#[test]
fn test_shift_number_and_tilde_safe_in_edit_prompt_and_terminal() {
    let temp_dir = TempDir::new().expect("tempdir");
    let (mut app, file_path) = create_test_app(&temp_dir);

    // 1. In MiddleMode::Edit, '!', '@', '#', '~' must insert text into buffer, NOT toggle panes!
    app.open_file(&file_path).expect("open_file");
    assert_eq!(app.focus, Focus::Middle);
    assert_eq!(app.middle_mode, MiddleMode::Edit);
    assert!(app.show_tree);
    assert!(app.show_middle);
    assert!(app.show_right);
    assert!(!app.show_quake);

    let _initial_line = app.buf.as_ref().unwrap().lines[0].clone();

    app.handle_msg(Msg::Key(press(KeyCode::Char('!'), KeyModifiers::NONE)));
    app.handle_msg(Msg::Key(press(KeyCode::Char('@'), KeyModifiers::NONE)));
    app.handle_msg(Msg::Key(press(KeyCode::Char('#'), KeyModifiers::NONE)));
    app.handle_msg(Msg::Key(press(KeyCode::Char('~'), KeyModifiers::NONE)));

    assert!(app.show_tree, "Tree must remain visible in Edit mode");
    assert!(app.show_middle, "Middle must remain visible in Edit mode");
    assert!(app.show_right, "Right must remain visible in Edit mode");
    assert!(!app.show_quake, "Quake must remain hidden in Edit mode");

    let modified_line = &app.buf.as_ref().unwrap().lines[0];
    assert!(
        modified_line.contains("!@#~"),
        "Characters !@#~ must be inserted into editor buffer, got: {}",
        modified_line
    );

    // 2. In Focus::RightTerm, '!', '@', '#', '~' must be forwarded to PTY, NOT toggle panes!
    app.set_focus(Focus::RightTerm);
    app.handle_msg(Msg::Key(press(KeyCode::Char('!'), KeyModifiers::NONE)));
    app.handle_msg(Msg::Key(press(KeyCode::Char('@'), KeyModifiers::NONE)));
    app.handle_msg(Msg::Key(press(KeyCode::Char('#'), KeyModifiers::NONE)));
    app.handle_msg(Msg::Key(press(KeyCode::Char('~'), KeyModifiers::NONE)));

    assert!(app.show_tree, "Tree must remain visible in RightTerm");
    assert!(app.show_middle, "Middle must remain visible in RightTerm");
    assert!(app.show_right, "Right must remain visible in RightTerm");
    assert!(!app.show_quake, "Quake must remain hidden in RightTerm");

    // 3. In Focus::Tree with tree_prompt, characters must go to LineEdit, NOT toggle panes!
    app.set_focus(Focus::Tree);
    app.handle_msg(Msg::Key(press(KeyCode::Char('a'), KeyModifiers::NONE)));
    assert!(app.tree_prompt.is_some());
    app.handle_msg(Msg::Key(press(KeyCode::Char('!'), KeyModifiers::NONE)));
    app.handle_msg(Msg::Key(press(KeyCode::Char('@'), KeyModifiers::NONE)));
    app.handle_msg(Msg::Key(press(KeyCode::Char('#'), KeyModifiers::NONE)));
    app.handle_msg(Msg::Key(press(KeyCode::Char('~'), KeyModifiers::NONE)));

    assert!(app.show_tree, "Tree must remain visible when prompt active");
    assert!(app.show_middle, "Middle must remain visible when prompt active");
    assert!(app.show_right, "Right must remain visible when prompt active");
    assert!(!app.show_quake, "Quake must remain hidden when prompt active");

    let prompt = app.tree_prompt.as_ref().unwrap();
    assert!(
        prompt.input.text().contains("!@#~"),
        "Characters must be inserted into tree prompt, got: {}",
        prompt.input.text()
    );
}
