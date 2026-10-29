use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tempfile::TempDir;
use nide::app::{App, MiddleMode, Msg};

fn key(code: KeyCode) -> Msg {
    Msg::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn backend_to_string(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let mut result = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let cell = buffer.cell((x, y)).unwrap();
            result.push_str(cell.symbol());
        }
        result.push('\n');
    }
    result
}

#[test]
fn test_markdown_preview_rendering_and_enter_jump() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();

    let md_content = r#"# Project Docs

Introduction paragraph text.

## Features

- [ ] Task Unchecked
- [x] Task Checked

| Tool | Language | Purpose |
| ---- | -------- | ------- |
| Cargo | Rust | Build tool |
| Pytest | Python | Testing framework |

```mermaid
flowchart TD
  Start --> Stop
```
"#;
    let md_path = root.join("docs.md");
    std::fs::write(&md_path, md_content).unwrap();

    let mut app = App::new(root);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();

    // 1. Open Markdown file in Middle pane (starts in MiddleMode::Edit)
    app.open_file_at(&md_path, 0, 0).unwrap();
    assert_eq!(app.middle_mode, MiddleMode::Edit);
    assert!(!app.preview_mode);

    // 2. Press Esc -> MiddleMode::Normal
    app.handle_msg(key(KeyCode::Esc));
    assert_eq!(app.middle_mode, MiddleMode::Normal);
    assert!(!app.preview_mode);

    // 3. Press 'p' to toggle preview mode
    app.handle_msg(key(KeyCode::Char('p')));
    assert_eq!(app.middle_mode, MiddleMode::Normal);
    assert!(app.preview_mode);

    // 4. Render to TestBackend and verify preview content
    terminal.draw(|f| nide::ui::draw(f, &mut app)).unwrap();
    let text = backend_to_string(&terminal);

    assert!(text.contains("[PREVIEW]"), "Title should show [PREVIEW]");
    assert!(text.contains("# Project Docs"), "Heading 1 should render");
    assert!(text.contains("## Features"), "Heading 2 should render");
    assert!(text.contains("☐"), "Unchecked task item should render");
    assert!(text.contains("Task Unchecked"));
    assert!(text.contains("☑"), "Checked task item should render");
    assert!(text.contains("Task Checked"));

    // Table elements
    assert!(text.contains('┌'), "Table top border should render");
    assert!(text.contains('├'), "Table mid border should render");
    assert!(text.contains('└'), "Table bot border should render");
    assert!(text.contains("Cargo"));
    assert!(text.contains("Pytest"));
    assert!(text.contains("Testing framework"));

    // Mermaid block
    assert!(text.contains("Mermaid Diagram"), "Mermaid block should render");

    // 5. Navigate with 'j' to "Pytest" table row
    let pytest_preview_idx = app
        .preview_lines
        .iter()
        .position(|l| l.text().contains("Pytest"))
        .expect("Pytest line must exist in preview");

    while app.preview_cursor < pytest_preview_idx {
        app.handle_msg(key(KeyCode::Char('j')));
    }
    assert_eq!(app.preview_cursor, pytest_preview_idx);

    // 6. Press Enter to jump to mapped source line in MiddleMode::Edit
    app.handle_msg(key(KeyCode::Enter));

    assert_eq!(app.middle_mode, MiddleMode::Edit, "Enter must switch to MiddleMode::Edit");
    assert!(!app.preview_mode, "Enter must exit preview mode");

    let buf = app.buf.as_ref().unwrap();
    let current_source_line = &buf.lines[buf.cursor_row];
    assert!(
        current_source_line.contains("Pytest"),
        "Cursor must jump to line with Pytest, got: {current_source_line}"
    );
}

#[test]
fn test_csv_preview_rendering_and_enter_jump() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();

    let csv_content = "ID,Name,Department,Salary\n\
101,Alice Smith,Engineering,95000\n\
102,Bob Jones,Design,85000\n\
103,Charlie Brown,Marketing,78000\n";
    let csv_path = root.join("employees.csv");
    std::fs::write(&csv_path, csv_content).unwrap();

    let mut app = App::new(root);
    let mut terminal = Terminal::new(TestBackend::new(140, 30)).unwrap();

    // 1. Open CSV file
    app.open_file_at(&csv_path, 0, 0).unwrap();
    assert_eq!(app.middle_mode, MiddleMode::Edit);

    // 2. Press Esc -> Normal mode
    app.handle_msg(key(KeyCode::Esc));
    assert_eq!(app.middle_mode, MiddleMode::Normal);

    // 3. Press 'p' -> Preview mode
    app.handle_msg(key(KeyCode::Char('p')));
    assert!(app.preview_mode);

    // 4. Render to TestBackend and verify aligned table
    terminal.draw(|f| nide::ui::draw(f, &mut app)).unwrap();
    let text = backend_to_string(&terminal);

    assert!(text.contains("[PREVIEW]"));
    assert!(text.contains("Alice Smith"));
    assert!(text.contains("Bob Jones"));
    assert!(text.contains("Charlie Brown"));
    assert!(text.contains("Engineering"));
    assert!(text.contains('┌'));
    assert!(text.contains('┬'));
    assert!(text.contains('┐'));
    assert!(text.contains('├'));
    assert!(text.contains('┼'));
    assert!(text.contains('┤'));
    assert!(text.contains('└'));
    assert!(text.contains('┴'));
    assert!(text.contains('┘'));

    // 5. Navigate to Bob Jones (row 2 in source file)
    let bob_preview_idx = app
        .preview_lines
        .iter()
        .position(|l| l.text().contains("Bob Jones"))
        .expect("Bob Jones line must exist in preview");

    while app.preview_cursor < bob_preview_idx {
        app.handle_msg(key(KeyCode::Char('j')));
    }
    assert_eq!(app.preview_cursor, bob_preview_idx);

    // 6. Press Enter to jump to Bob Jones in MiddleMode::Edit
    app.handle_msg(key(KeyCode::Enter));

    assert_eq!(app.middle_mode, MiddleMode::Edit);
    assert!(!app.preview_mode);

    let buf = app.buf.as_ref().unwrap();
    assert_eq!(buf.cursor_row, 2, "Bob Jones is line 2 in CSV file");
    assert!(buf.lines[buf.cursor_row].contains("Bob Jones"));
}

#[test]
fn test_tsv_and_jsonl_preview_rendering() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();

    // Test TSV
    let tsv_content = "Code\tCity\tCountry\nFRA\tParis\tFrance\nJPN\tTokyo\tJapan\n";
    let tsv_path = root.join("cities.tsv");
    std::fs::write(&tsv_path, tsv_content).unwrap();

    let mut app = App::new(root.clone());
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();

    app.open_file_at(&tsv_path, 0, 0).unwrap();
    app.handle_msg(key(KeyCode::Esc));
    app.handle_msg(key(KeyCode::Char('p')));
    assert!(app.preview_mode);

    terminal.draw(|f| nide::ui::draw(f, &mut app)).unwrap();
    let text = backend_to_string(&terminal);
    assert!(text.contains("Paris"));
    assert!(text.contains("Tokyo"));
    assert!(text.contains('┌'));

    // Test JSONL
    let jsonl_content = "{\"id\": 1, \"user\": \"Ada\", \"active\": true}\n\
{\"id\": 2, \"user\": \"Alan\", \"active\": false}\n";
    let jsonl_path = root.join("records.jsonl");
    std::fs::write(&jsonl_path, jsonl_content).unwrap();

    app.open_file_at(&jsonl_path, 0, 0).unwrap();
    app.handle_msg(key(KeyCode::Esc));
    app.handle_msg(key(KeyCode::Char('p')));
    assert!(app.preview_mode);

    terminal.draw(|f| nide::ui::draw(f, &mut app)).unwrap();
    let text = backend_to_string(&terminal);
    assert!(text.contains("Ada"));
    assert!(text.contains("Alan"));
    assert!(text.contains("active"));
    assert!(text.contains('┌'));

    // Jump to Alan on Enter
    let alan_idx = app
        .preview_lines
        .iter()
        .position(|l| l.text().contains("Alan"))
        .unwrap();
    while app.preview_cursor < alan_idx {
        app.handle_msg(key(KeyCode::Char('j')));
    }
    app.handle_msg(key(KeyCode::Enter));
    assert_eq!(app.middle_mode, MiddleMode::Edit);
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 1);
}

#[test]
fn test_preview_mode_toggle_and_esc() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();

    let md_path = root.join("test.md");
    std::fs::write(&md_path, "# Title\n\nContent here.\n").unwrap();

    let mut app = App::new(root);

    app.open_file_at(&md_path, 0, 0).unwrap();
    app.handle_msg(key(KeyCode::Esc)); // Normal mode
    assert!(!app.preview_mode);

    // Toggle on with 'p'
    app.handle_msg(key(KeyCode::Char('p')));
    assert!(app.preview_mode);

    // Toggle off with 'p'
    app.handle_msg(key(KeyCode::Char('p')));
    assert!(!app.preview_mode);

    // Toggle on again
    app.handle_msg(key(KeyCode::Char('p')));
    assert!(app.preview_mode);

    // Toggle off with 'Esc'
    app.handle_msg(key(KeyCode::Esc));
    assert!(!app.preview_mode);
}

#[test]
fn test_non_previewable_file_press_p() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();

    let rs_path = root.join("main.rs");
    std::fs::write(&rs_path, "fn main() {}\n").unwrap();

    let mut app = App::new(root);

    app.open_file_at(&rs_path, 0, 0).unwrap();
    app.handle_msg(key(KeyCode::Esc)); // Normal mode

    app.handle_msg(key(KeyCode::Char('p')));
    assert!(!app.preview_mode, "Non-previewable file should not enter preview mode");
    assert!(app.status_message.is_some(), "Status message should explain preview is unsupported");
}
