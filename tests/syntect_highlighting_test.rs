use std::fs;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tempfile::TempDir;
use nide::app::{App, Focus, MiddleMode, Msg};
use nide::buffer::{Buffer, MAX_HL_BYTES, MAX_HL_LINES, MAX_SHOWN_BYTES};
use nide::theme;

fn plain_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn test_rust_file_syntax_highlighting_distinct_catppuccin_latte_styles() {
    let tmp = TempDir::new().expect("tempdir");
    let rust_file = tmp.path().join("main.rs");
    let code = "fn main() {\n    let val = 42;\n    // comment\n}\n";
    fs::write(&rust_file, code).expect("write main.rs");

    let mut app = App::new(tmp.path().to_path_buf());
    app.open_file(&rust_file).expect("open main.rs");

    assert_eq!(app.focus, Focus::Middle);
    assert_eq!(app.middle_mode, MiddleMode::Edit);

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|f| {
            nide::ui::draw(f, &mut app);
        })
        .expect("draw");

    let buffer = terminal.backend().buffer();

    // Look for cells in the middle pane
    // In our Catppuccin Latte theme:
    // "fn" keyword receives MAUVE (Rgb(136, 57, 239))
    // "main" function identifier receives BLUE (Rgb(30, 102, 245))
    // "42" number receives PEACH (Rgb(254, 100, 11))
    // "// comment" receives OVERLAY2 (Rgb(124, 127, 147))
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
            if cell.fg == theme::BLUE && (cell.symbol() == "m" || cell.symbol() == "a" || cell.symbol() == "i" || cell.symbol() == "n") {
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

    assert!(
        found_fn_mauve,
        "Expected 'fn' keyword to be styled with theme::MAUVE"
    );
    assert!(
        found_main_blue,
        "Expected 'main' function to be styled with theme::BLUE"
    );
    assert!(
        found_num_peach,
        "Expected '42' numeric constant to be styled with theme::PEACH"
    );
    assert!(
        found_comment_overlay2,
        "Expected '// comment' to be styled with theme::OVERLAY2"
    );

    // Verify all four styles are distinct Catppuccin Latte colors
    assert_ne!(theme::MAUVE, theme::BLUE);
    assert_ne!(theme::MAUVE, theme::PEACH);
    assert_ne!(theme::MAUVE, theme::OVERLAY2);
    assert_ne!(theme::BLUE, theme::PEACH);
    assert_ne!(theme::BLUE, theme::OVERLAY2);
    assert_ne!(theme::PEACH, theme::OVERLAY2);
}

#[test]
fn test_markdown_file_syntax_highlighting_distinct_catppuccin_latte_styles() {
    let tmp = TempDir::new().expect("tempdir");
    let md_file = tmp.path().join("README.md");
    let content = "# Header\n\n**bold text**\n";
    fs::write(&md_file, content).expect("write README.md");

    let mut app = App::new(tmp.path().to_path_buf());
    app.open_file(&md_file).expect("open README.md");

    assert_eq!(app.focus, Focus::Middle);
    assert_eq!(app.middle_mode, MiddleMode::Edit);

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|f| {
            nide::ui::draw(f, &mut app);
        })
        .expect("draw");

    let buffer = terminal.backend().buffer();

    let mut found_header_teal = false;
    let mut found_marker_overlay2 = false;
    let mut found_bold_red = false;

    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let cell = buffer.cell((x, y)).unwrap();
            if cell.fg == theme::TEAL && (cell.symbol() == "H" || cell.symbol() == "e") {
                found_header_teal = true;
            }
            if cell.fg == theme::OVERLAY2 && (cell.symbol() == "#" || cell.symbol() == "*") {
                found_marker_overlay2 = true;
            }
            if cell.fg == theme::RED && (cell.symbol() == "b" || cell.symbol() == "o") {
                found_bold_red = true;
            }
        }
    }

    assert!(
        found_header_teal,
        "Expected Markdown header text to be styled with theme::TEAL"
    );
    assert!(
        found_marker_overlay2,
        "Expected Markdown punctuation/markers (#, *) to be styled with theme::OVERLAY2"
    );
    assert!(
        found_bold_red,
        "Expected Markdown bold text to be styled with theme::RED"
    );

    assert_ne!(theme::TEAL, theme::OVERLAY2);
    assert_ne!(theme::TEAL, theme::RED);
    assert_ne!(theme::OVERLAY2, theme::RED);
}

#[test]
fn test_binary_detection_and_circuit_breaker_bypass_under_50ms() {
    let tmp = TempDir::new().expect("tempdir");

    // Case 1: Byte buffer containing NUL bytes
    let bin_path = tmp.path().join("data.bin");
    let mut binary_bytes = vec![0x41, 0x42, 0x43, 0x00, 0x44, 0x45];
    binary_bytes.extend(vec![0xFF; 1000]);
    fs::write(&bin_path, &binary_bytes).expect("write data.bin");

    let start_bin = Instant::now();
    let mut app = App::new(tmp.path().to_path_buf());
    app.open_file(&bin_path).expect("open data.bin");

    let buf = app.buf.as_ref().expect("buf exists");
    assert!(buf.is_binary, "Expected buf.is_binary == true");
    assert!(
        buf.is_syntax_highlight_disabled(),
        "Expected syntax highlighting disabled for binary"
    );
    assert_eq!(
        app.middle_mode,
        MiddleMode::Normal,
        "Binary files must open in MiddleMode::Normal"
    );

    // Verify pressing 'i' does not enter Edit mode
    app.handle_msg(Msg::Key(plain_key(KeyCode::Char('i'))));
    assert_eq!(
        app.middle_mode,
        MiddleMode::Normal,
        "Binary files must remain read-only"
    );

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|f| {
            nide::ui::draw(f, &mut app);
        })
        .expect("draw");

    let bin_elapsed = start_bin.elapsed();
    assert!(
        bin_elapsed.as_millis() < 50,
        "Binary detection & rendering took {:?}, expected < 50ms",
        bin_elapsed
    );

    // Verify binary banner is rendered
    let buffer = terminal.backend().buffer();
    let mut found_binary_banner = false;
    for y in 0..buffer.area.height {
        let mut line_str = String::new();
        for x in 0..buffer.area.width {
            let cell = buffer.cell((x, y)).unwrap();
            line_str.push_str(cell.symbol());
        }
        if line_str.contains("Binary file: editing disabled") {
            found_binary_banner = true;
            break;
        }
    }
    assert!(
        found_binary_banner,
        "Expected '[Binary file: editing disabled]' banner to be rendered"
    );

    // Case 2: >30,000-line buffer
    let large_file = tmp.path().join("large.rs");
    let line_count = MAX_HL_LINES + 100; // 30,100 lines
    let mut large_content = String::with_capacity(line_count * 20);
    for i in 0..line_count {
        large_content.push_str(&format!("let x_{i} = {i};\n"));
    }
    fs::write(&large_file, &large_content).expect("write large.rs");

    let start_large = Instant::now();
    let mut app_large = App::new(tmp.path().to_path_buf());
    app_large.open_file(&large_file).expect("open large.rs");

    let large_buf = app_large.buf.as_mut().expect("large buf exists");
    assert_eq!(large_buf.lines.len(), line_count);
    assert!(
        large_buf.is_syntax_highlight_disabled(),
        "Expected syntax highlighting disabled for >30,000 lines"
    );

    // Verify highlight_lines returns unhighlighted text immediately
    let hl_lines = large_buf.highlight_lines(0, 50);
    assert_eq!(hl_lines.len(), 51);
    assert_eq!(
        hl_lines[0].1[0].0.fg,
        Some(theme::TEXT),
        "Circuit-breaker bypass must yield plain theme::TEXT"
    );

    let backend_large = TestBackend::new(80, 24);
    let mut term_large = Terminal::new(backend_large).expect("terminal");
    term_large
        .draw(|f| {
            nide::ui::draw(f, &mut app_large);
        })
        .expect("draw large");

    let large_elapsed = start_large.elapsed();
    assert!(
        large_elapsed.as_millis() < 50,
        "Circuit-breaker bypass for >30,000 lines took {:?}, expected < 50ms",
        large_elapsed
    );
}

#[test]
fn test_circuit_breakers_file_bytes_and_line_length() {
    let tmp = TempDir::new().expect("tempdir");

    // Exceeds MAX_HL_BYTES (4MB)
    let path_4mb = tmp.path().join("big_bytes.rs");
    let mut big_bytes = Buffer::new_empty(path_4mb.clone());
    big_bytes.total_bytes = MAX_HL_BYTES + 10;
    assert!(
        big_bytes.is_syntax_highlight_disabled(),
        "Expected disabled syntax highlighting when total_bytes > 4MB"
    );

    // Exceeds MAX_SHOWN_BYTES (20,000 bytes in a single line)
    let path_long_line = tmp.path().join("long_line.rs");
    let mut long_line_buf = Buffer::new_empty(path_long_line.clone());
    long_line_buf.lines = vec!["a".repeat(MAX_SHOWN_BYTES + 1)];
    long_line_buf.recompute_total_bytes();
    assert!(
        long_line_buf.is_syntax_highlight_disabled(),
        "Expected disabled syntax highlighting when a line exceeds 20,000 bytes"
    );
}

#[test]
fn test_syntect_checkpoints_every_64_lines_and_invalidation() {
    let tmp = TempDir::new().expect("tempdir");
    let file_path = tmp.path().join("test_checkpoints.rs");

    // Create 300 lines of Rust code
    let mut content = String::new();
    for i in 0..300 {
        content.push_str(&format!("fn func_{i}() {{ let val = {i}; }}\n"));
    }
    fs::write(&file_path, &content).expect("write test_checkpoints.rs");

    let mut buf = Buffer::load(&file_path).expect("load buffer");
    assert!(!buf.is_syntax_highlight_disabled());

    // Initially checkpoint 0 exists
    assert_eq!(buf.checkpoints.len(), 1);
    assert_eq!(buf.checkpoints[0].line_idx, 0);

    // Highlighting up to line 200 should populate checkpoints at 64, 128, 192
    let _ = buf.highlight_lines(0, 200);

    let checkpoint_lines: Vec<usize> = buf.checkpoints.iter().map(|cp| cp.line_idx).collect();
    assert!(
        checkpoint_lines.contains(&0),
        "Expected checkpoint at 0"
    );
    assert!(
        checkpoint_lines.contains(&64),
        "Expected checkpoint at 64"
    );
    assert!(
        checkpoint_lines.contains(&128),
        "Expected checkpoint at 128"
    );
    assert!(
        checkpoint_lines.contains(&192),
        "Expected checkpoint at 192"
    );

    // Highlighting lines 195..205 starts from checkpoint 192 (nearest checkpoint <= 195)
    let start_time = Instant::now();
    let lines_195_205 = buf.highlight_lines(195, 205);
    let elapsed = start_time.elapsed();
    assert_eq!(lines_195_205.len(), 11);
    assert!(
        elapsed.as_millis() < 10,
        "Highlighting from checkpoint should take < 10ms, took {:?}",
        elapsed
    );

    // Editing line 130 invalidates checkpoints > 130 (i.e. line 192 removed)
    buf.cursor_row = 130;
    buf.cursor_col = 0;
    buf.insert_char('/');
    assert!(
        buf.checkpoints.iter().all(|cp| cp.line_idx <= 130),
        "Checkpoints after line 130 must be invalidated"
    );
    assert!(
        buf.checkpoints.iter().any(|cp| cp.line_idx == 128),
        "Checkpoint at 128 must be preserved"
    );
    assert!(
        !buf.checkpoints.iter().any(|cp| cp.line_idx == 192),
        "Checkpoint at 192 must have been invalidated"
    );
}
