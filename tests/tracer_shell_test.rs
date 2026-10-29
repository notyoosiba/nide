use std::path::PathBuf;

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use nide::app::{App, Focus, Msg};
use nide::ui;

fn key_press(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn mouse_down(column: u16, row: u16, button: MouseButton) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(button),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

#[test]
fn test_3pane_layout_and_geometry() {
    let width = 100;
    let height = 30;
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("failed to create TestBackend terminal");
    let mut app = App::new(PathBuf::from("/workspace/sample"));

    terminal
        .draw(|f| ui::draw(f, &mut app))
        .expect("draw failed");

    let tree = app.layout.tree.expect("tree rect must be present");
    let middle = app.layout.middle.expect("middle rect must be present");
    let right = app.layout.right.expect("right rect must be present");

    // 1. Non-overlapping verification:
    // Tree starts at x = 0
    assert_eq!(tree.x, 0, "Tree pane should start at x = 0");
    // Tree right edge matches Middle left edge
    assert_eq!(
        tree.x + tree.width,
        middle.x,
        "Tree right edge must meet Middle left edge without overlap"
    );
    // Middle right edge matches Right left edge
    assert_eq!(
        middle.x + middle.width,
        right.x,
        "Middle right edge must meet Right left edge without overlap"
    );
    // Right pane ends at total terminal width
    assert_eq!(
        right.x + right.width,
        width,
        "Right pane must end at total terminal width"
    );

    // 2. Widths sum to terminal width:
    let sum_widths = tree.width + middle.width + right.width;
    assert_eq!(
        sum_widths, width,
        "Panes width sum ({sum_widths}) must equal terminal width ({width})"
    );

    // 3. Vertical layout:
    assert_eq!(tree.y, 0);
    assert_eq!(middle.y, 0);
    assert_eq!(right.y, 0);

    let expected_height = height - 1; // 1 row reserved for status bar
    assert_eq!(tree.height, expected_height);
    assert_eq!(middle.height, expected_height);
    assert_eq!(right.height, expected_height);

    // Status bar layout:
    assert_eq!(app.layout.status.x, 0);
    assert_eq!(app.layout.status.y, expected_height);
    assert_eq!(app.layout.status.width, width);
    assert_eq!(app.layout.status.height, 1);
}

#[test]
fn test_tab_cycles_focus() {
    let mut app = App::new(PathBuf::from("/workspace/sample"));

    // Default focus starts in Tree
    assert_eq!(app.focus, Focus::Tree);

    // Tab -> Middle
    app.handle_msg(Msg::Key(key_press(KeyCode::Tab, KeyModifiers::NONE)));
    assert_eq!(app.focus, Focus::Middle);

    // Tab -> RightTerm
    app.handle_msg(Msg::Key(key_press(KeyCode::Tab, KeyModifiers::NONE)));
    assert_eq!(app.focus, Focus::RightTerm);

    // In RightTerm, Tab is forwarded to terminal (does not switch focus)
    app.handle_msg(Msg::Key(key_press(KeyCode::Tab, KeyModifiers::NONE)));
    assert_eq!(app.focus, Focus::RightTerm);

    // Ctrl+\ escapes terminal focus back to Tree
    app.handle_msg(Msg::Key(key_press(
        KeyCode::Char('\\'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(app.focus, Focus::Tree);
}

#[test]
fn test_mouse_clicks_change_focus() {
    let width = 100;
    let height = 30;
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("failed to create terminal");
    let mut app = App::new(PathBuf::from("/workspace/sample"));

    // Render once to populate layout geometry
    terminal
        .draw(|f| ui::draw(f, &mut app))
        .expect("draw failed");

    let tree = app.layout.tree.unwrap();
    let middle = app.layout.middle.unwrap();
    let right = app.layout.right.unwrap();

    assert_eq!(app.focus, Focus::Tree);

    // Click inside Middle pane
    let click_middle = mouse_down(middle.x + 2, middle.y + 2, MouseButton::Left);
    app.handle_msg(Msg::Mouse(click_middle));
    assert_eq!(app.focus, Focus::Middle);

    // Click inside Right pane
    let click_right = mouse_down(right.x + 2, right.y + 2, MouseButton::Left);
    app.handle_msg(Msg::Mouse(click_right));
    assert_eq!(app.focus, Focus::RightTerm);

    // Click inside Tree pane
    let click_tree = mouse_down(tree.x + 2, tree.y + 2, MouseButton::Left);
    app.handle_msg(Msg::Mouse(click_tree));
    assert_eq!(app.focus, Focus::Tree);

    // Right clicks do NOT change focus
    let right_click_middle = mouse_down(middle.x + 2, middle.y + 2, MouseButton::Right);
    app.handle_msg(Msg::Mouse(right_click_middle));
    assert_eq!(app.focus, Focus::Tree, "Right click must not change focus");
}

#[test]
fn test_q_triggers_quit_only_outside_terminal() {
    let mut app = App::new(PathBuf::from("/workspace/sample"));

    // 1. In Tree focus, 'q' quits
    app.focus = Focus::Tree;
    assert!(!app.should_quit);
    app.handle_msg(Msg::Key(key_press(
        KeyCode::Char('q'),
        KeyModifiers::NONE,
    )));
    assert!(app.should_quit, "'q' in Tree should trigger quit");

    // Reset should_quit
    app.should_quit = false;

    // 2. In Middle focus, 'q' quits
    app.focus = Focus::Middle;
    app.handle_msg(Msg::Key(key_press(
        KeyCode::Char('q'),
        KeyModifiers::NONE,
    )));
    assert!(app.should_quit, "'q' in Middle should trigger quit");

    // Reset should_quit
    app.should_quit = false;

    // 3. In RightTerm focus, 'q' does NOT quit (it will be forwarded to the shell)
    app.focus = Focus::RightTerm;
    app.handle_msg(Msg::Key(key_press(
        KeyCode::Char('q'),
        KeyModifiers::NONE,
    )));
    assert!(
        !app.should_quit,
        "'q' in RightTerm must NOT trigger quit"
    );

    // 4. Msg::Quit always sets should_quit
    app.handle_msg(Msg::Quit);
    assert!(app.should_quit, "Msg::Quit should always set should_quit");
}

#[test]
fn test_ctrl_c_quits() {
    let mut app = App::new(PathBuf::from("/workspace/sample"));
    // Ctrl+C in Tree or Middle triggers quit
    app.focus = Focus::Tree;
    assert!(!app.should_quit);
    app.handle_msg(Msg::Key(key_press(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    )));
    assert!(app.should_quit, "Ctrl+C outside terminal should trigger quit");

    // In RightTerm, Ctrl+C sends SIGINT to the terminal and does NOT quit
    app.should_quit = false;
    app.focus = Focus::RightTerm;
    app.handle_msg(Msg::Key(key_press(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    )));
    assert!(!app.should_quit, "Ctrl+C in terminal must NOT quit application");
}

#[test]
fn test_ui_renders_titles_and_status() {
    let width = 100;
    let height = 30;
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("failed to create terminal");
    let mut app = App::new(PathBuf::from("/workspace/myproject"));

    terminal
        .draw(|f| ui::draw(f, &mut app))
        .expect("draw failed");

    let buffer = terminal.backend().buffer();
    let content = format!("{buffer:?}");

    // Buffer should contain titles for Tree, Editor, Terminal, and status info
    assert!(content.contains("Tree"), "Buffer should contain 'Tree'");
    assert!(content.contains("Editor"), "Buffer should contain 'Editor'");
    assert!(
        content.contains("Terminal"),
        "Buffer should contain 'Terminal'"
    );
    assert!(content.contains("TREE"), "Status bar should show 'TREE'");
    assert!(
        content.contains("/workspace/myproject"),
        "Status bar should show root path"
    );
}
