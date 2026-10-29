use std::fs;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tempfile::TempDir;
use nide::app::links::{
    extract_token_from_screen, parse_link_target, pick_best_candidate, resolve_file,
};
use nide::app::{App, Focus, MiddleMode, Msg};
use nide::pty::{TermId, TerminalPane};
use nide::ui;

fn mouse_click_with_modifiers(
    column: u16,
    row: u16,
    button: MouseButton,
    modifiers: KeyModifiers,
) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(button),
        column,
        row,
        modifiers,
    }
}

// ---------------------------------------------------------------------------
// Acceptance Criterion 1: Unit Tests for parsing, soft-wrap, and tie-breaking
// ---------------------------------------------------------------------------

#[test]
fn test_unit_parse_main_rs_with_line_and_col() {
    let target = parse_link_target("src/main.rs:25:5").expect("failed to parse");
    assert_eq!(target.path, "src/main.rs");
    assert_eq!(target.line, Some(25));
    assert_eq!(target.col, Some(5));
}

#[test]
fn test_unit_parse_diff_prefix_a_and_b() {
    let target_a = parse_link_target("a/src/foo.rs:12").expect("failed to parse a/");
    assert_eq!(target_a.path, "src/foo.rs");
    assert_eq!(target_a.line, Some(12));
    assert_eq!(target_a.col, None);

    let target_b = parse_link_target("b/src/foo.rs:12").expect("failed to parse b/");
    assert_eq!(target_b.path, "src/foo.rs");
    assert_eq!(target_b.line, Some(12));
    assert_eq!(target_b.col, None);
}

#[test]
fn test_unit_parse_hash_line_fragment() {
    let target = parse_link_target("foo.rs#L12").expect("failed to parse #L12");
    assert_eq!(target.path, "foo.rs");
    assert_eq!(target.line, Some(12));
    assert_eq!(target.col, None);

    let target_with_col = parse_link_target("foo.rs#L12C4").expect("failed to parse #L12C4");
    assert_eq!(target_with_col.path, "foo.rs");
    assert_eq!(target_with_col.line, Some(12));
    assert_eq!(target_with_col.col, Some(4));
}

#[test]
fn test_unit_parse_quoted_and_bracketed() {
    let target_double = parse_link_target("\"buffer.rs\"").expect("double quoted");
    assert_eq!(target_double.path, "buffer.rs");
    assert_eq!(target_double.line, None);
    assert_eq!(target_double.col, None);

    let target_single = parse_link_target("'buffer.rs'").expect("single quoted");
    assert_eq!(target_single.path, "buffer.rs");

    let target_parens = parse_link_target("(\"buffer.rs\"),").expect("parens + comma");
    assert_eq!(target_parens.path, "buffer.rs");

    let target_brackets = parse_link_target("[buffer.rs]").expect("brackets");
    assert_eq!(target_brackets.path, "buffer.rs");
}

#[test]
fn test_unit_soft_wrapped_paths_across_25_col_screen() {
    let r = std::num::NonZeroU16::new(5).unwrap();
    let c = std::num::NonZeroU16::new(25).unwrap();
    let mut parser = vt100::Parser::new(r, c, 0);
    // Write a command outputting a path that soft-wraps across the 25-col terminal
    parser.process(b"path/to/very/deeply/nested/source.rs:88:14");
    let screen = parser.screen();

    // Verify row 0 wrapped to row 1
    assert!(screen.row_wrapped(0), "Row 0 should be marked as wrapped");

    // Click on row 0 (before the wrap boundary)
    let link_from_row0 = extract_token_from_screen(screen, 0, 5).expect("extract from row 0");
    assert_eq!(link_from_row0.path, "path/to/very/deeply/nested/source.rs");
    assert_eq!(link_from_row0.line, Some(88));
    assert_eq!(link_from_row0.col, Some(14));

    // Click on row 1 (after the wrap boundary)
    let link_from_row1 = extract_token_from_screen(screen, 1, 3).expect("extract from row 1");
    assert_eq!(link_from_row1.path, "path/to/very/deeply/nested/source.rs");
    assert_eq!(link_from_row1.line, Some(88));
    assert_eq!(link_from_row1.col, Some(14));
}

#[test]
fn test_unit_same_basename_tie_breaking() {
    let root = Path::new("/workspace");
    let files = vec![
        PathBuf::from("src/app/mod.rs"),
        PathBuf::from("src/ui/mod.rs"),
        PathBuf::from("tests/common/mod.rs"),
    ];

    // Case 1: Active buffer is in src/app/ -> resolves to src/app/mod.rs
    let active_app = Path::new("/workspace/src/app/edit.rs");
    let cand_app = pick_best_candidate(&files.iter().collect::<Vec<_>>(), Some(active_app), root)
        .expect("best candidate");
    assert_eq!(cand_app, &PathBuf::from("src/app/mod.rs"));

    let resolved_app = resolve_file("mod.rs", root, &files, Some(active_app)).expect("resolve_file");
    assert_eq!(resolved_app, PathBuf::from("/workspace/src/app/mod.rs"));

    // Case 2: Active buffer is in src/ui/ -> resolves to src/ui/mod.rs
    let active_ui = Path::new("/workspace/src/ui/term.rs");
    let cand_ui = pick_best_candidate(&files.iter().collect::<Vec<_>>(), Some(active_ui), root)
        .expect("best candidate");
    assert_eq!(cand_ui, &PathBuf::from("src/ui/mod.rs"));

    let resolved_ui = resolve_file("mod.rs", root, &files, Some(active_ui)).expect("resolve_file");
    assert_eq!(resolved_ui, PathBuf::from("/workspace/src/ui/mod.rs"));

    // Case 3: Active buffer is in tests/ -> resolves to tests/common/mod.rs
    let active_test = Path::new("/workspace/tests/editor_test.rs");
    let cand_test = pick_best_candidate(&files.iter().collect::<Vec<_>>(), Some(active_test), root)
        .expect("best candidate");
    assert_eq!(cand_test, &PathBuf::from("tests/common/mod.rs"));
}

// ---------------------------------------------------------------------------
// Acceptance Criterion 2: End-to-End Test for Terminal Ctrl+Click Link Jump
// ---------------------------------------------------------------------------

#[test]
fn test_e2e_terminal_ctrl_click_opens_middle_editor_and_reveals_in_tree() {
    let tmp = TempDir::new().expect("tempdir failed");
    let root = tmp.path();

    // 1. Setup file tree: src/lib.rs with multiple lines
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).expect("create src dir");
    let lib_content = "\
line 1: header
line 2: imports
line 3: config
pub fn target_function() {
    println!(\"hello world\");
}
";
    let lib_path = src_dir.join("lib.rs");
    fs::write(&lib_path, lib_content).expect("write src/lib.rs");

    // Initialize App without spawning live background shell process
    let mut app = App::new(root.to_path_buf());
    // Replace right terminal with a synthetic pane
    app.right_term = TerminalPane::new(TermId::RightAgent, root.to_path_buf(), None);

    // 2. Feed "Modified src/lib.rs:4:2" into RightTerm
    app.right_term
        .process_output(b"Modified src/lib.rs:4:2\r\n");

    // Render with TestBackend (120x30) to compute layout geometry
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).expect("create test terminal");
    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");

    let right = app.layout.right.expect("app.layout.right rect");
    let inner_x = right.x + 1;
    let inner_y = right.y + 1;

    // "Modified src/lib.rs:4:2"
    // "Modified " has length 9. Click inside "src/lib.rs" at column inner_x + 12 on row inner_y
    let click_col = inner_x + 12;
    let click_row = inner_y;

    // 3. Send synthetic Ctrl+Click Msg::Mouse
    let mouse_event = mouse_click_with_modifiers(
        click_col,
        click_row,
        MouseButton::Left,
        KeyModifiers::CONTROL,
    );
    app.handle_msg(Msg::Mouse(mouse_event));

    // 4. Verify all acceptance criteria assertions:
    // - app.buf.path is src/lib.rs
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

    // - app.line == 3 (0-indexed 3 for 1-indexed line 4)
    assert_eq!(
        app.line, 3,
        "Expected app.line to be 3 (line 4 in 1-indexed), got {}",
        app.line
    );
    // col == 1 (0-indexed 1 for 1-indexed col 2)
    assert_eq!(
        app.col, 1,
        "Expected app.col to be 1 (col 2 in 1-indexed), got {}",
        app.col
    );

    // - app.focus == Focus::Middle
    assert_eq!(
        app.focus,
        Focus::Middle,
        "Expected focus to switch to Middle pane"
    );

    // - app.middle_mode == MiddleMode::Edit
    assert_eq!(
        app.middle_mode,
        MiddleMode::Edit,
        "Expected middle mode to be MiddleMode::Edit"
    );

    // - app.tree highlights src/lib.rs
    assert_eq!(
        app.tree.selected_path(),
        Some(Path::new("src/lib.rs")),
        "Expected left tree to highlight and select src/lib.rs"
    );
}

#[test]
fn test_e2e_terminal_cmd_click_super_modifier_also_opens_file() {
    let tmp = TempDir::new().expect("tempdir failed");
    let root = tmp.path();

    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).expect("create src dir");
    let main_content = "fn main() {}\n// line 2\n// line 3\n// line 4\n// line 5\n";
    fs::write(src_dir.join("main.rs"), main_content).expect("write src/main.rs");

    let mut app = App::new(root.to_path_buf());
    app.right_term = TerminalPane::new(TermId::RightAgent, root.to_path_buf(), None);
    app.right_term.process_output(b"Check src/main.rs:5:1\r\n");

    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).expect("create test terminal");
    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");

    let right = app.layout.right.expect("right rect");
    let inner_x = right.x + 1;
    let inner_y = right.y + 1;

    // Click on "src/main.rs" with KeyModifiers::SUPER (macOS Cmd key)
    let mouse_event = mouse_click_with_modifiers(
        inner_x + 8,
        inner_y,
        MouseButton::Left,
        KeyModifiers::SUPER,
    );
    app.handle_msg(Msg::Mouse(mouse_event));

    assert!(app.buf.is_some());
    assert!(app.buf.as_ref().unwrap().path.ends_with("src/main.rs"));
    assert_eq!(app.line, 4); // Line 5 -> 0-indexed 4
    assert_eq!(app.focus, Focus::Middle);
    assert_eq!(app.middle_mode, MiddleMode::Edit);
    assert_eq!(app.tree.selected_path(), Some(Path::new("src/main.rs")));
}

#[test]
fn test_terminal_ctrl_click_auto_saves_dirty_buffer_before_jump() {
    let tmp = TempDir::new().expect("tempdir failed");
    let root = tmp.path();

    let file_a = root.join("file_a.txt");
    let file_b = root.join("file_b.txt");
    fs::write(&file_a, "original A").expect("write file A");
    fs::write(&file_b, "original B").expect("write file B");

    let mut app = App::new(root.to_path_buf());
    app.right_term = TerminalPane::new(TermId::RightAgent, root.to_path_buf(), None);

    // Open file_a in middle editor and make it dirty
    app.open_file(&file_a).expect("open file a");
    if let Some(buf) = &mut app.buf {
        buf.insert_char('!');
    }
    assert!(app.is_dirty(), "App should be dirty before jump");

    // Output reference to file_b.txt in terminal
    app.right_term.process_output(b"See file_b.txt:1:1\r\n");

    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).expect("create test terminal");
    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");

    let right = app.layout.right.expect("right rect");
    let inner_x = right.x + 1;
    let inner_y = right.y + 1;

    // Ctrl+Click file_b.txt
    let mouse_event = mouse_click_with_modifiers(
        inner_x + 6,
        inner_y,
        MouseButton::Left,
        KeyModifiers::CONTROL,
    );
    app.handle_msg(Msg::Mouse(mouse_event));

    // file_a must have been saved to disk with modified content
    let on_disk_a = fs::read_to_string(&file_a).expect("read file a");
    assert!(
        on_disk_a.contains('!'),
        "file_a must have been automatically saved to disk before jump"
    );

    // file_b is now open
    assert!(app.buf.as_ref().unwrap().path.ends_with("file_b.txt"));
    assert_eq!(app.focus, Focus::Middle);
    assert_eq!(app.middle_mode, MiddleMode::Edit);
}
