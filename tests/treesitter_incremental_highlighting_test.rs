use std::fs;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tempfile::TempDir;
use nide::app::{App, Focus, MiddleMode, Msg};
use nide::theme;
use nide::treesitter::{get_or_compile_query, LanguageId, TsHighlighter};

fn plain_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn test_treesitter_registry_and_queries_for_core_languages() {
    let languages = [
        ("test.rs", LanguageId::Rust),
        ("test.py", LanguageId::Python),
        ("test.js", LanguageId::JavaScript),
        ("test.ts", LanguageId::TypeScript),
        ("test.tsx", LanguageId::Tsx),
        ("test.go", LanguageId::Go),
        ("test.json", LanguageId::Json),
        ("test.toml", LanguageId::Toml),
        ("test.sh", LanguageId::Bash),
        ("test.c", LanguageId::C),
        ("test.cpp", LanguageId::Cpp),
        ("test.html", LanguageId::Html),
        ("test.css", LanguageId::Css),
    ];

    for (filename, expected_lang) in languages {
        let path = std::path::Path::new(filename);
        let lang = LanguageId::for_path(path).expect("language should be recognized");
        assert_eq!(lang, expected_lang);

        let query = get_or_compile_query(lang);
        assert!(
            query.is_ok(),
            "Query for {filename} should compile cleanly: {:?}",
            query.err()
        );
        let q = query.unwrap();
        assert!(q.pattern_count() > 0, "Query should have patterns");
    }
}

#[test]
fn test_treesitter_direct_parsing_and_capture_mapping_rust_and_python() {
    // 1. Rust direct parse via treesitter.rs
    let mut ts_rust = TsHighlighter::new(LanguageId::Rust).expect("ts highlighter for rust");
    let rust_code = b"fn compute_sum(val: i32) -> i32 {\n    let num = 42;\n    // calculate\n    num + val\n}\n";
    let parsed = ts_rust.parse_slice(rust_code, Duration::from_millis(50));
    assert!(parsed, "Rust parse should complete");
    assert!(!ts_rust.needs_parse);
    assert!(ts_rust.tree.is_some());

    let lines: Vec<String> = String::from_utf8_lossy(rust_code)
        .lines()
        .map(|s| s.to_string())
        .collect();
    let spans = ts_rust
        .query_line_spans(&lines, rust_code, 0, lines.len() - 1)
        .expect("line spans for rust");

    // Line 0: "fn compute_sum(val: i32) -> i32 {"
    // fn should be keyword (MAUVE), compute_sum should be function (BLUE), i32 should be type (YELLOW)
    let line0_spans = spans.get(&0).expect("spans for line 0");
    let has_keyword = line0_spans
        .iter()
        .any(|(s, e, style)| *s == 0 && *e == 2 && style.fg == Some(theme::MAUVE));
    let has_func = line0_spans
        .iter()
        .any(|(s, e, style)| *s == 3 && *e == 14 && style.fg == Some(theme::BLUE));
    assert!(has_keyword, "Expected 'fn' keyword with MAUVE style");
    assert!(has_func, "Expected 'compute_sum' function with BLUE style");

    // Line 1: "    let num = 42;"
    let line1_spans = spans.get(&1).expect("spans for line 1");
    let has_num_peach = line1_spans
        .iter()
        .any(|(s, e, style)| *s == 14 && *e == 16 && style.fg == Some(theme::PEACH));
    assert!(has_num_peach, "Expected '42' constant with PEACH style");

    // 2. Python direct parse via treesitter.rs
    let mut ts_py = TsHighlighter::new(LanguageId::Python).expect("ts highlighter for python");
    let py_code = b"def greet_user(name: str):\n    # greeting\n    msg = \"hello\"\n    return 42\n";
    let parsed_py = ts_py.parse_slice(py_code, Duration::from_millis(50));
    assert!(parsed_py, "Python parse should complete");
    assert!(!ts_py.needs_parse);
    assert!(ts_py.tree.is_some());

    let py_lines: Vec<String> = String::from_utf8_lossy(py_code)
        .lines()
        .map(|s| s.to_string())
        .collect();
    let py_spans = ts_py
        .query_line_spans(&py_lines, py_code, 0, py_lines.len() - 1)
        .expect("line spans for python");

    // Line 0: "def greet_user(name: str):"
    let py_line0 = py_spans.get(&0).expect("spans for py line 0");
    let has_def = py_line0
        .iter()
        .any(|(s, e, style)| *s == 0 && *e == 3 && style.fg == Some(theme::MAUVE));
    let has_greet = py_line0
        .iter()
        .any(|(s, e, style)| *s == 4 && *e == 14 && style.fg == Some(theme::BLUE));
    assert!(has_def, "Expected 'def' keyword with MAUVE style");
    assert!(has_greet, "Expected 'greet_user' function with BLUE style");

    // Line 1: "    # greeting"
    let py_line1 = py_spans.get(&1).expect("spans for py line 1");
    let has_comment = py_line1
        .iter()
        .any(|(s, e, style)| *s == 4 && *e == 14 && style.fg == Some(theme::OVERLAY2));
    assert!(has_comment, "Expected '# greeting' comment with OVERLAY2 style");

    // Line 3: "    return 42"
    let py_line3 = py_spans.get(&3).expect("spans for py line 3");
    let has_return = py_line3
        .iter()
        .any(|(s, e, style)| *s == 4 && *e == 10 && style.fg == Some(theme::MAUVE));
    let has_42 = py_line3
        .iter()
        .any(|(s, e, style)| *s == 11 && *e == 13 && style.fg == Some(theme::PEACH));
    assert!(has_return, "Expected 'return' keyword with MAUVE style");
    assert!(has_42, "Expected '42' constant with PEACH style");
}

#[test]
fn test_rust_and_python_files_render_catppuccin_latte_styles_in_testbackend() {
    let tmp = TempDir::new().expect("tempdir");

    // 1. Rust file in App
    let rust_file = tmp.path().join("main.rs");
    let rust_code = "fn main() {\n    let val = 42;\n    // comment\n}\n";
    fs::write(&rust_file, rust_code).expect("write main.rs");

    let mut app = App::new(tmp.path().to_path_buf());
    app.open_file(&rust_file).expect("open main.rs");

    assert_eq!(app.focus, Focus::Middle);
    assert_eq!(app.middle_mode, MiddleMode::Edit);
    assert!(
        app.buf.as_ref().unwrap().ts_highlighter.is_some(),
        "Rust buffer should have Tree-sitter highlighter initialized"
    );

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|f| {
            nide::ui::draw(f, &mut app);
        })
        .expect("draw rust");

    let buffer = terminal.backend().buffer();

    let mut found_fn_mauve = false;
    let mut found_main_blue = false;
    let mut found_num_peach = false;
    let mut found_comment_overlay2 = false;

    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let cell = buffer.cell((x, y)).unwrap();
            if cell.fg == theme::MAUVE && (cell.symbol() == "f" || cell.symbol() == "n") {
                found_fn_mauve = true;
            }
            if cell.fg == theme::BLUE && (cell.symbol() == "m" || cell.symbol() == "a" || cell.symbol() == "i") {
                found_main_blue = true;
            }
            if cell.fg == theme::PEACH && (cell.symbol() == "4" || cell.symbol() == "2") {
                found_num_peach = true;
            }
            if cell.fg == theme::OVERLAY2 && cell.symbol() == "/" {
                found_comment_overlay2 = true;
            }
        }
    }

    assert!(found_fn_mauve, "Expected 'fn' keyword styled with theme::MAUVE");
    assert!(found_main_blue, "Expected 'main' function styled with theme::BLUE");
    assert!(found_num_peach, "Expected '42' styled with theme::PEACH");
    assert!(found_comment_overlay2, "Expected '// comment' styled with theme::OVERLAY2");

    // 2. Python file in App
    let py_file = tmp.path().join("script.py");
    let py_code = "def compute(x):\n    # py comment\n    return 42\n";
    fs::write(&py_file, py_code).expect("write script.py");

    app.open_file(&py_file).expect("open script.py");
    assert!(
        app.buf.as_ref().unwrap().ts_highlighter.is_some(),
        "Python buffer should have Tree-sitter highlighter initialized"
    );

    terminal
        .draw(|f| {
            nide::ui::draw(f, &mut app);
        })
        .expect("draw python");

    let py_buffer = terminal.backend().buffer();

    let mut found_def_mauve = false;
    let mut found_compute_blue = false;
    let mut found_py_comment_overlay2 = false;
    let mut found_py_num_peach = false;

    for y in 0..py_buffer.area.height {
        for x in 0..py_buffer.area.width {
            let cell = py_buffer.cell((x, y)).unwrap();
            if cell.fg == theme::MAUVE && (cell.symbol() == "d" || cell.symbol() == "e" || cell.symbol() == "f") {
                found_def_mauve = true;
            }
            if cell.fg == theme::BLUE && (cell.symbol() == "c" || cell.symbol() == "o" || cell.symbol() == "m") {
                found_compute_blue = true;
            }
            if cell.fg == theme::OVERLAY2 && cell.symbol() == "#" {
                found_py_comment_overlay2 = true;
            }
            if cell.fg == theme::PEACH && (cell.symbol() == "4" || cell.symbol() == "2") {
                found_py_num_peach = true;
            }
        }
    }

    assert!(found_def_mauve, "Expected 'def' keyword styled with theme::MAUVE");
    assert!(found_compute_blue, "Expected 'compute' function styled with theme::BLUE");
    assert!(found_py_comment_overlay2, "Expected '# py comment' styled with theme::OVERLAY2");
    assert!(found_py_num_peach, "Expected '42' numeric constant styled with theme::PEACH");
}

#[test]
fn test_incremental_edit_updates_spans_after_inserting_code() {
    let tmp = TempDir::new().expect("tempdir");
    let rust_file = tmp.path().join("main.rs");
    // Start with empty main function
    let initial_code = "fn main() {\n}\n";
    fs::write(&rust_file, initial_code).expect("write main.rs");

    let mut app = App::new(tmp.path().to_path_buf());
    app.open_file(&rust_file).expect("open main.rs");

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|f| {
            nide::ui::draw(f, &mut app);
        })
        .expect("draw initial");

    // Initially there is no numeric literal 99 or string "hello"
    let initial_buffer = terminal.backend().buffer();
    let mut found_99 = false;
    let mut found_string_quote = false;
    for y in 0..initial_buffer.area.height {
        for x in 0..initial_buffer.area.width {
            let cell = initial_buffer.cell((x, y)).unwrap();
            if cell.fg == theme::PEACH && cell.symbol() == "9" {
                found_99 = true;
            }
            if cell.symbol() == "\"" {
                found_string_quote = true;
            }
        }
    }
    assert!(!found_99, "Should not contain '9' initially");
    assert!(!found_string_quote, "Should not contain '\"' initially");

    // Now navigate to inside main(): row 0, col 11 (after '{')
    // and press Enter to insert newline, then type:
    // let s = "magic";
    {
        let buf = app.buf.as_mut().unwrap();
        buf.cursor_row = 0;
        buf.cursor_col = 11;
    }

    // Insert newline
    app.handle_msg(Msg::Key(plain_key(KeyCode::Enter)));

    // Type `let s = 99;`
    for c in "    let s = 99;".chars() {
        app.handle_msg(Msg::Key(plain_key(KeyCode::Char(c))));
    }

    // Verify buffer lines
    let buf = app.buf.as_ref().unwrap();
    assert_eq!(buf.lines.len(), 3);
    assert_eq!(buf.lines[0], "fn main() {");
    assert_eq!(buf.lines[1], "    let s = 99;");
    assert_eq!(buf.lines[2], "}");

    // Verify tree-sitter tree is valid and updated incrementally
    assert!(buf.ts_highlighter.as_ref().unwrap().tree.is_some());

    // Redraw with TestBackend
    terminal
        .draw(|f| {
            nide::ui::draw(f, &mut app);
        })
        .expect("draw after insert");

    let updated_buffer = terminal.backend().buffer();

    let mut found_inserted_let_mauve = false;
    let mut found_inserted_num_peach = false;

    for y in 0..updated_buffer.area.height {
        for x in 0..updated_buffer.area.width {
            let cell = updated_buffer.cell((x, y)).unwrap();
            if cell.fg == theme::MAUVE && (cell.symbol() == "l" || cell.symbol() == "e" || cell.symbol() == "t") {
                found_inserted_let_mauve = true;
            }
            if cell.fg == theme::PEACH && cell.symbol() == "9" {
                found_inserted_num_peach = true;
            }
        }
    }

    assert!(
        found_inserted_let_mauve,
        "Inserted 'let' keyword must be styled with theme::MAUVE after incremental edit"
    );
    assert!(
        found_inserted_num_peach,
        "Inserted '99' constant must be styled with theme::PEACH after incremental edit"
    );

    // Now test incremental edit with Backspace: delete the '99' and insert `"str"`
    // Cursor is after ';' at col 15 on row 1
    // Backspace ';'
    app.handle_msg(Msg::Key(plain_key(KeyCode::Backspace)));
    // Backspace '9'
    app.handle_msg(Msg::Key(plain_key(KeyCode::Backspace)));
    // Backspace '9'
    app.handle_msg(Msg::Key(plain_key(KeyCode::Backspace)));

    // Type `"ok";`
    for c in "\"ok\";".chars() {
        app.handle_msg(Msg::Key(plain_key(KeyCode::Char(c))));
    }

    terminal
        .draw(|f| {
            nide::ui::draw(f, &mut app);
        })
        .expect("draw after replace with string");

    let str_buffer = terminal.backend().buffer();
    let mut found_str_green = false;

    for y in 0..str_buffer.area.height {
        for x in 0..str_buffer.area.width {
            let cell = str_buffer.cell((x, y)).unwrap();
            if cell.fg == theme::GREEN && (cell.symbol() == "o" || cell.symbol() == "k" || cell.symbol() == "\"") {
                found_str_green = true;
            }
        }
    }

    assert!(
        found_str_green,
        "Replaced string \"ok\" must be styled with theme::GREEN after incremental edit"
    );
}

#[test]
fn test_time_sliced_parsing_bounded_slices() {
    let mut ts = TsHighlighter::new(LanguageId::Rust).expect("highlighter");

    // Very large synthetic input
    let mut large_text = String::from("fn root() {\n");
    for i in 0..15_000 {
        large_text.push_str(&format!("    let var_{i} = {i};\n"));
    }
    large_text.push_str("}\n");
    let bytes = large_text.as_bytes();

    // 1. With an extremely small slice (100 nanoseconds), parsing must yield without blocking
    let finished_early = ts.parse_slice(bytes, Duration::from_nanos(100));
    // Either finished or yielded cleanly (yielded expected on 15,000 lines)
    if !finished_early {
        assert!(ts.needs_parse);
    }

    // 2. With adequate slice (1 second), parsing completes
    let finished = ts.parse_slice(bytes, Duration::from_secs(1));
    assert!(finished);
    assert!(!ts.needs_parse);
    assert!(ts.tree.is_some());
}
