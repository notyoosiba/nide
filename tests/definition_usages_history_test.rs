use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tempfile::TempDir;
use nide::{App, Focus, MiddleMode, Msg, PickerKind};

fn send_key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    let key = KeyEvent::new(code, modifiers);
    app.handle_msg(Msg::Key(key));
}

fn send_char(app: &mut App, c: char) {
    send_key(app, KeyCode::Char(c), KeyModifiers::NONE);
}

#[test]
fn test_goto_definition_jump_and_history_stack_recording() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let main_rs = root.join("main.rs");
    let lib_rs = root.join("lib.rs");

    std::fs::write(
        &main_rs,
        "fn main() {\n    helper();\n}\n",
    )
    .unwrap();

    std::fs::write(
        &lib_rs,
        "pub fn helper() {\n    println!(\"hello\");\n}\n",
    )
    .unwrap();

    let mut app = App::new(root.clone());
    app.open_file_at(&main_rs, 1, 4).unwrap(); // line 1 (0-indexed), col 4 is 'h' in "helper();"
    app.middle_mode = MiddleMode::Normal;
    app.set_focus(Focus::Middle);

    assert_eq!(app.line, 1);
    assert_eq!(app.col, 4);
    assert_eq!(app.current_path().unwrap(), main_rs.as_path());
    assert!(app.history.is_empty());

    // Press 'd' in Normal mode to go to definition
    send_char(&mut app, 'd');

    // Asserts the editor jumps to `fn helper()` in `lib.rs`
    assert_eq!(app.current_path().unwrap(), lib_rs.as_path());
    assert_eq!(app.line, 0); // line 0 is "pub fn helper() {"
    assert_eq!(app.middle_mode, MiddleMode::Normal);

    // Asserts main.rs is recorded in app.history
    assert_eq!(app.history.len(), 1);
    assert_eq!(app.history[0].path, main_rs);
    assert_eq!(app.history[0].line, 1);
    assert_eq!(app.history[0].col, 4);
}

#[test]
fn test_history_navigation_backward_and_forward() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let main_rs = root.join("main.rs");
    let lib_rs = root.join("lib.rs");

    std::fs::write(
        &main_rs,
        "fn main() {\n    helper();\n}\n",
    )
    .unwrap();

    std::fs::write(
        &lib_rs,
        "pub fn helper() {\n    println!(\"hello\");\n}\n",
    )
    .unwrap();

    let mut app = App::new(root.clone());
    app.open_file_at(&main_rs, 1, 4).unwrap();
    app.middle_mode = MiddleMode::Normal;
    app.set_focus(Focus::Middle);

    // Jump from main.rs to lib.rs using 'd'
    send_char(&mut app, 'd');
    assert_eq!(app.current_path().unwrap(), lib_rs.as_path());

    // Press '[' to navigate back in history to main.rs
    send_char(&mut app, '[');
    assert_eq!(app.current_path().unwrap(), main_rs.as_path());
    assert_eq!(app.line, 1);
    assert_eq!(app.col, 4);
    assert_eq!(app.middle_mode, MiddleMode::Normal);

    // Press ']' to navigate forward in history to lib.rs
    send_char(&mut app, ']');
    assert_eq!(app.current_path().unwrap(), lib_rs.as_path());
    assert_eq!(app.line, 0);
    assert_eq!(app.middle_mode, MiddleMode::Normal);

    // Navigate back once more
    send_char(&mut app, '[');
    assert_eq!(app.current_path().unwrap(), main_rs.as_path());
}

#[test]
fn test_find_usages_lists_call_sites_across_project() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let main_rs = root.join("main.rs");
    let lib_rs = root.join("lib.rs");
    let test_rs = root.join("test.rs");

    std::fs::write(
        &main_rs,
        "fn main() {\n    // helper(); inside comment\n    helper();\n}\n",
    )
    .unwrap();

    std::fs::write(
        &lib_rs,
        "pub fn helper() {\n    println!(\"helper called\");\n}\n",
    )
    .unwrap();

    std::fs::write(
        &test_rs,
        "fn run_test() {\n    helper();\n    helper_not_match();\n}\n",
    )
    .unwrap();

    let mut app = App::new(root.clone());
    app.open_file_at(&lib_rs, 0, 7).unwrap(); // on "helper" in "pub fn helper() {"
    app.middle_mode = MiddleMode::Normal;
    app.set_focus(Focus::Middle);

    // Press 'u' to find usages
    send_char(&mut app, 'u');

    // Picker must be open with Usages
    assert!(app.picker.is_some());
    let picker = app.picker.as_ref().unwrap();
    assert_eq!(picker.kind, PickerKind::Usages);

    // Items must contain call sites across the project:
    // main.rs:3: helper();
    // test.rs:2: helper();
    // lib.rs:1: pub fn helper()
    // but NOT main.rs comment, and NOT test.rs helper_not_match
    let paths: Vec<PathBuf> = picker.items.iter().map(|it| it.path.clone()).collect();
    assert!(paths.contains(&main_rs));
    assert!(paths.contains(&test_rs));

    // Verify main.rs call site is line 2 (0-indexed line 3 in 1-based editor)
    let main_item = picker.items.iter().find(|it| it.path == main_rs).unwrap();
    assert_eq!(main_item.line, Some(2));
    assert!(main_item.display_text.contains("helper();"));

    // Verify test.rs call site is line 1
    let test_item = picker.items.iter().find(|it| it.path == test_rs).unwrap();
    assert_eq!(test_item.line, Some(1));
    assert!(test_item.display_text.contains("helper();"));

    // Ensure no items have helper_not_match or comment line
    for item in &picker.items {
        assert!(!item.display_text.contains("inside comment"));
        assert!(!item.display_text.contains("helper_not_match"));
    }
}

#[test]
fn test_goto_definition_ambiguous_opens_picker() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let main_rs = root.join("main.rs");
    let file_a = root.join("a.rs");
    let file_b = root.join("b.rs");

    std::fs::write(&main_rs, "fn main() {\n    helper();\n}\n").unwrap();
    std::fs::write(&file_a, "pub fn helper() {}\n").unwrap();
    std::fs::write(&file_b, "pub fn helper() {}\n").unwrap();

    let mut app = App::new(root.clone());
    app.open_file_at(&main_rs, 1, 4).unwrap();
    app.middle_mode = MiddleMode::Normal;
    app.set_focus(Focus::Middle);

    send_char(&mut app, 'd');

    // With multiple definitions and no single winner, Picker opens with Definitions
    assert!(app.picker.is_some());
    let picker = app.picker.as_ref().unwrap();
    assert_eq!(picker.kind, PickerKind::Definitions);
    assert_eq!(picker.items.len(), 2);

    // Accepting picker navigates to chosen definition and records history
    send_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.picker.is_none());
    assert_eq!(app.history.len(), 1);
    assert_eq!(app.history[0].path, main_rs);
}

#[test]
fn test_goto_definition_local_buffer_binding() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let file = root.join("main.rs");
    std::fs::write(
        &file,
        "fn helper() {}\n\nfn main() {\n    helper();\n}\n",
    )
    .unwrap();

    let mut app = App::new(root.clone());
    app.open_file_at(&file, 3, 4).unwrap(); // line 3: helper();
    app.middle_mode = MiddleMode::Normal;
    app.set_focus(Focus::Middle);

    send_char(&mut app, 'd');

    // Jumps to line 0: fn helper() {}
    assert_eq!(app.line, 0);
    assert_eq!(app.history.len(), 1);
    assert_eq!(app.history[0].line, 3);

    // Jump back
    send_char(&mut app, '[');
    assert_eq!(app.line, 3);
}

#[test]
fn test_ui_rendering_usages_and_definitions_picker() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let file = root.join("sample.rs");
    std::fs::write(&file, "fn test() {\n    test();\n}\n").unwrap();

    let mut app = App::new(root.clone());
    app.open_file_at(&file, 1, 4).unwrap();
    app.middle_mode = MiddleMode::Normal;
    app.set_focus(Focus::Middle);

    send_char(&mut app, 'u');
    assert!(app.picker.is_some());

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            nide::ui::draw(f, &mut app);
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let mut found_usages_title = false;
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push(buffer[(x, y)].symbol().chars().next().unwrap_or(' '));
        }
        if line.contains("Usages") {
            found_usages_title = true;
            break;
        }
    }

    assert!(found_usages_title, "Rendered UI should display 'Usages' in picker title");
}
