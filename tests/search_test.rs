use std::fs;
use std::sync::mpsc;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tempfile::TempDir;
use nide::app::{App, Focus, MiddleMode, Msg};
use nide::picker::{PickItem, PickerKind};
use nide::ui;

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::empty(),
    }
}

fn press_shift(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::SHIFT,
        kind: KeyEventKind::Press,
        state: KeyEventState::empty(),
    }
}

fn press_ctrl(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::CONTROL,
        kind: KeyEventKind::Press,
        state: KeyEventState::empty(),
    }
}

#[test]
fn test_symbol_search_rust_and_python_filtering_and_jump() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let rs_path = root.join("calculator.rs");
    let py_path = root.join("service.py");

    let rs_content = r#"// Line 0
// Line 1
pub fn compute_sum(a: i32, b: i32) -> i32 {
    a + b
}

pub struct CalcConfig {
    pub max_val: i32,
}
"#;

    let py_content = r#"# Line 0
# Line 1
# Line 2
def handle_request(req):
    return req

class ServiceWorker:
    def __init__(self):
        pass
"#;

    fs::write(&rs_path, rs_content).unwrap();
    fs::write(&py_path, py_content).unwrap();

    let (tx, _rx) = mpsc::channel();
    let mut app = App::with_sender(root.clone(), Some(tx));
    app.set_focus(Focus::Middle);
    app.middle_mode = MiddleMode::Normal;

    // Press 'D' in Normal mode to open symbol search
    app.handle_msg(Msg::Key(press(KeyCode::Char('D'))));

    assert!(app.picker.is_some(), "Symbol picker should be open");
    let picker = app.picker.as_ref().unwrap();
    assert_eq!(picker.kind, PickerKind::Symbols);

    // Verify symbols from both files are present
    let items = &picker.items;
    assert!(
        items.iter().any(|it| it.display_text.contains("compute_sum")),
        "Should extract Rust fn compute_sum"
    );
    assert!(
        items.iter().any(|it| it.display_text.contains("CalcConfig")),
        "Should extract Rust struct CalcConfig"
    );
    assert!(
        items.iter().any(|it| it.display_text.contains("handle_request")),
        "Should extract Python def handle_request"
    );
    assert!(
        items.iter().any(|it| it.display_text.contains("ServiceWorker")),
        "Should extract Python class ServiceWorker"
    );

    // Filter by symbol name "ServiceWorker"
    for c in "ServiceWorker".chars() {
        app.handle_msg(Msg::Key(press(KeyCode::Char(c))));
    }

    let picker = app.picker.as_ref().unwrap();
    assert!(!picker.items.is_empty(), "Filtered items should not be empty");
    assert!(
        picker.items[picker.selected_idx]
            .display_text
            .contains("ServiceWorker"),
        "Top selected item should be ServiceWorker"
    );
    assert_eq!(
        picker.items[picker.selected_idx].line,
        Some(6), // line index 6 in service.py (line 7, 1-based)
        "Symbol definition line should match class definition line"
    );

    // Press Enter to accept and jump to the definition
    app.handle_msg(Msg::Key(press(KeyCode::Enter)));

    assert!(app.picker.is_none(), "Picker should close on accept");
    assert_eq!(
        app.current_path(),
        Some(py_path.as_path()),
        "Editor should open service.py"
    );
    assert_eq!(
        app.line, 6,
        "Editor cursor row should jump to the exact definition line"
    );
    assert_eq!(
        app.middle_mode,
        MiddleMode::Edit,
        "Editor should be in Edit mode"
    );
}

#[test]
fn test_symbol_search_global_ctrl_t_from_edit_mode() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let rs_path = root.join("main.rs");
    fs::write(&rs_path, "pub fn entrypoint() {}\n").unwrap();

    let (tx, _rx) = mpsc::channel();
    let mut app = App::with_sender(root.clone(), Some(tx));
    let _ = app.open_file(&rs_path);
    app.middle_mode = MiddleMode::Edit;

    // Press Ctrl+T from Edit mode
    app.handle_msg(Msg::Key(press_ctrl(KeyCode::Char('t'))));

    assert!(app.picker.is_some(), "Ctrl+T should open symbol picker");
    let picker = app.picker.as_ref().unwrap();
    assert_eq!(picker.kind, PickerKind::Symbols);
    assert!(picker.items.iter().any(|it| it.display_text.contains("entrypoint")));

    // Esc cancels picker
    app.handle_msg(Msg::Key(press(KeyCode::Esc)));
    assert!(app.picker.is_none());
    assert_eq!(app.middle_mode, MiddleMode::Edit);
}

#[test]
fn test_project_grep_stale_seq_drop_and_jump() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let f1 = root.join("alpha.rs");
    let f2 = root.join("beta.rs");

    fs::write(
        &f1,
        "// line 0\n// line 1\nlet target_variable = 42;\n// line 3\n",
    )
    .unwrap();
    fs::write(&f2, "// other file content\n").unwrap();

    let (tx, _rx) = mpsc::channel();
    let mut app = App::with_sender(root.clone(), Some(tx));
    app.set_focus(Focus::Middle);
    app.middle_mode = MiddleMode::Normal;

    // Press 's' in Normal mode to open project grep
    app.handle_msg(Msg::Key(press(KeyCode::Char('s'))));

    assert!(app.picker.is_some());
    let picker = app.picker.as_ref().unwrap();
    assert_eq!(picker.kind, PickerKind::Grep);
    assert_eq!(picker.items.len(), 0);

    // Type query "target_variable"
    for c in "target_variable".chars() {
        app.handle_msg(Msg::Key(press(KeyCode::Char(c))));
    }

    let active_seq = app.search_seq;
    assert!(active_seq > 0, "search_seq should increment with typing");

    // Simulate stale SearchDone message with seq = active_seq - 1
    let stale_items = vec![PickItem {
        path: f2.clone(),
        display_text: "beta.rs:1: stale result".to_string(),
        score: 0,
        match_indices: vec![],
        line: Some(0),
        col: None,
    }];
    app.handle_msg(Msg::SearchDone {
        seq: active_seq - 1,
        rows: stale_items,
    });

    // Stale message must be ignored: picker items remain empty
    let picker = app.picker.as_ref().unwrap();
    assert_eq!(
        picker.items.len(),
        0,
        "Stale search results must be dropped when seq does not match"
    );

    // Simulate current SearchDone message with matching seq
    let correct_items = vec![PickItem {
        path: f1.clone(),
        display_text: "alpha.rs:3: let target_variable = 42;".to_string(),
        score: 0,
        match_indices: vec![16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30],
        line: Some(2), // 0-indexed line 2 (line 3)
        col: Some(4),
    }];
    app.handle_msg(Msg::SearchDone {
        seq: active_seq,
        rows: correct_items,
    });

    let picker = app.picker.as_ref().unwrap();
    assert_eq!(picker.items.len(), 1, "Picker should update with current results");
    assert_eq!(picker.items[0].path, f1);
    assert_eq!(picker.items[0].line, Some(2));

    // Press Enter to accept and open file at matched line
    app.handle_msg(Msg::Key(press(KeyCode::Enter)));

    assert!(app.picker.is_none());
    assert_eq!(app.current_path(), Some(f1.as_path()));
    assert_eq!(app.line, 2, "Editor cursor row should match grep line");
    assert_eq!(app.middle_mode, MiddleMode::Edit);
}

#[test]
fn test_project_grep_background_thread_execution() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let f1 = root.join("src").join("lib.rs");
    fs::create_dir_all(f1.parent().unwrap()).unwrap();
    fs::write(
        &f1,
        "fn helper() {}\n// UNIQUE_GREP_TOKEN_1234\nfn another() {}\n",
    )
    .unwrap();

    let (tx, rx) = mpsc::channel();
    let mut app = App::with_sender(root.clone(), Some(tx));
    app.set_focus(Focus::Middle);
    app.middle_mode = MiddleMode::Normal;

    // Open grep picker
    app.handle_msg(Msg::Key(press(KeyCode::Char('s'))));

    // Type query
    for c in "UNIQUE_GREP_TOKEN".chars() {
        app.handle_msg(Msg::Key(press(KeyCode::Char(c))));
    }

    // Receive messages from background worker until latest seq is processed
    let mut received_done = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if let Ok(Msg::SearchDone { seq, rows }) = rx.recv_timeout(Duration::from_millis(50)) {
            let is_latest = seq == app.search_seq;
            app.handle_msg(Msg::SearchDone { seq, rows });
            if is_latest {
                received_done = true;
                break;
            }
        }
    }

    assert!(received_done, "Background worker should send SearchDone");
    let picker = app.picker.as_ref().unwrap();
    assert!(!picker.items.is_empty(), "Picker items should be populated by grep");
    assert_eq!(picker.items[0].line, Some(1));
}

#[test]
fn test_search_ui_rendering_test_backend() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let f1 = root.join("app.rs");
    fs::write(&f1, "pub fn start_server() {}\n").unwrap();

    let (tx, _rx) = mpsc::channel();
    let mut app = App::with_sender(root.clone(), Some(tx));
    app.set_focus(Focus::Middle);
    app.middle_mode = MiddleMode::Normal;

    // Open symbol search
    app.handle_msg(Msg::Key(press_shift(KeyCode::Char('D'))));

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|f| ui::draw(f, &mut app)).unwrap();

    let buffer = terminal.backend().buffer();
    let mut rendered_text = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let cell = &buffer[(x, y)];
            rendered_text.push_str(cell.symbol());
        }
        rendered_text.push('\n');
    }

    assert!(
        rendered_text.contains("Symbols"),
        "UI should render Symbols modal title"
    );
    assert!(
        rendered_text.contains("start_server"),
        "UI should render extracted symbol in candidate list"
    );
}
