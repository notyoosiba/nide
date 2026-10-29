use std::fs;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use notify::event::Event;
use notify::EventKind;
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tempfile::TempDir;

use nide::app::conflict::{diff_lines, reconcile, DiffHunk, ReconcileOutcome};
use nide::app::{App, MiddleMode, Msg};

fn make_key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::empty(),
        kind: KeyEventKind::Press,
        state: KeyEventState::empty(),
    }
}

fn make_shift_key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::SHIFT,
        kind: KeyEventKind::Press,
        state: KeyEventState::empty(),
    }
}

#[test]
fn test_unit_diff_lines_and_reconcile() {
    let base: Vec<String> = (1..=5).map(|i| format!("line {i}")).collect();
    let yours: Vec<String> = vec![
        "line 1 - modified".into(),
        "line 2".into(),
        "line 3".into(),
        "line 4".into(),
        "line 5".into(),
    ];
    let hunks = diff_lines(&base, &yours);
    assert_eq!(hunks.len(), 1);
    assert_eq!(
        hunks[0],
        DiffHunk {
            base_range: 0..1,
            lines: vec!["line 1 - modified".into()],
        }
    );

    // Identical changes do not conflict
    let agent = yours.clone();
    let outcome = reconcile(&base, &yours, &agent, 12345);
    match outcome {
        ReconcileOutcome::SilentMerge { merged_lines } => {
            assert_eq!(merged_lines, yours);
        }
        ReconcileOutcome::Conflict { .. } => {
            panic!("Identical changes should silently merge");
        }
    }
}

#[test]
fn test_silent_auto_merge_non_overlapping_edits_line_1_and_line_15() {
    let tmp = TempDir::new().expect("tempdir");
    let file_path = tmp.path().join("code.txt");

    // File with 20 lines
    let initial_content: String = (1..=20)
        .map(|i| format!("line {i}\n"))
        .collect();
    fs::write(&file_path, &initial_content).expect("write initial code");

    let mut app = App::new(tmp.path().to_path_buf());
    app.open_file(&file_path).expect("open file");
    assert_eq!(app.middle_mode, MiddleMode::Edit);
    assert!(!app.is_dirty());

    // Edit line 1 in memory (0-indexed row 0)
    {
        let buf = app.buf.as_mut().unwrap();
        buf.lines[0] = "line 1 - user dirty edit".into();
        buf.dirty = true;
    }
    app.dirty = true;
    assert!(app.is_dirty());

    // Agent edits line 15 on disk (0-indexed row 14)
    let mut disk_lines: Vec<String> = (1..=20)
        .map(|i| format!("line {i}"))
        .collect();
    disk_lines[14] = "line 15 - agent external edit".into();
    let disk_content = disk_lines.join("\n") + "\n";
    fs::write(&file_path, &disk_content).expect("write agent edit to disk");

    // Trigger reconciliation via Msg::Fs
    let fs_event = Event {
        kind: EventKind::Modify(notify::event::ModifyKind::Data(
            notify::event::DataChange::Content,
        )),
        paths: vec![file_path.clone()],
        attrs: Default::default(),
    };
    app.handle_msg(Msg::Fs(fs_event));

    // Assert silent auto-merge without entering MiddleMode::Conflict
    assert_eq!(
        app.middle_mode,
        MiddleMode::Edit,
        "Mode must remain MiddleMode::Edit"
    );
    assert!(
        app.conflict.is_none(),
        "Must not create a ConflictSession on non-overlapping edits"
    );

    let buf = app.buf.as_ref().unwrap();
    assert_eq!(
        buf.lines[0], "line 1 - user dirty edit",
        "Line 1 should retain user's in-memory edit"
    );
    assert_eq!(
        buf.lines[14], "line 15 - agent external edit",
        "Line 15 should have agent's clean edit applied from disk"
    );
    assert!(
        app.is_dirty(),
        "Buffer remains dirty because user's edit is not yet saved to disk"
    );
    assert_eq!(
        buf.base_lines[14], "line 15 - agent external edit",
        "base_lines should now match disk_lines"
    );
}

#[test]
fn test_conflict_overlapping_edits_line_5_blocks_switch_and_quit_and_resolves_yours() {
    let tmp = TempDir::new().expect("tempdir");
    let file_path = tmp.path().join("conflict.txt");
    let other_file = tmp.path().join("other.txt");

    let initial_content: String = (1..=10)
        .map(|i| format!("line {i}\n"))
        .collect();
    fs::write(&file_path, &initial_content).expect("write file");
    fs::write(&other_file, "other file content\n").expect("write other");

    let mut app = App::new(tmp.path().to_path_buf());
    app.open_file(&file_path).expect("open file");
    assert_eq!(app.middle_mode, MiddleMode::Edit);

    // Edit line 5 in memory (0-indexed line 4)
    {
        let buf = app.buf.as_mut().unwrap();
        buf.lines[4] = "line 5 - in-memory edit (Yours)".into();
        buf.dirty = true;
    }
    app.dirty = true;
    assert!(app.is_dirty());

    // Edit line 5 on disk (0-indexed line 4)
    let mut disk_lines: Vec<String> = (1..=10)
        .map(|i| format!("line {i}"))
        .collect();
    disk_lines[4] = "line 5 - on-disk edit (Agent)".into();
    fs::write(&file_path, disk_lines.join("\n") + "\n").expect("write agent edit");

    // Trigger reconciliation via Msg::Fs
    let fs_event = Event {
        kind: EventKind::Modify(notify::event::ModifyKind::Data(
            notify::event::DataChange::Content,
        )),
        paths: vec![file_path.clone()],
        attrs: Default::default(),
    };
    app.handle_msg(Msg::Fs(fs_event));

    // Assert MiddleMode::Conflict
    assert_eq!(
        app.middle_mode,
        MiddleMode::Conflict,
        "Must transition to MiddleMode::Conflict on overlapping hunks"
    );
    assert!(
        app.conflict.is_some(),
        "ConflictSession must be active in app"
    );

    // 1. Verify file switching is blocked
    let open_res = app.open_file(&other_file);
    assert!(open_res.is_ok());
    assert_eq!(
        app.current_path().unwrap(),
        file_path.as_path(),
        "File switching must be blocked while conflict is active"
    );
    assert!(
        app.status_message.is_some(),
        "Status message should warn about unresolved conflict"
    );

    // 2. Verify quitting with 'q' is blocked
    app.handle_msg(Msg::Key(make_key(KeyCode::Char('q'))));
    assert!(
        !app.should_quit,
        "Quitting via 'q' must be blocked while conflict is active"
    );

    // Also verify Msg::Quit is blocked
    app.handle_msg(Msg::Quit);
    assert!(
        !app.should_quit,
        "Msg::Quit must be blocked while conflict is active"
    );

    // 3. Resolve hunk with 'l' (Yours)
    app.handle_msg(Msg::Key(make_key(KeyCode::Char('l'))));

    // Assert merged buffer saves to disk and returns to MiddleMode::Edit
    assert_eq!(
        app.middle_mode,
        MiddleMode::Edit,
        "Must return to MiddleMode::Edit after all conflicts resolved"
    );
    assert!(app.conflict.is_none(), "ConflictSession should be cleared");
    assert!(!app.is_dirty(), "App must be clean (saved to disk)");

    // Verify disk content contains user's edit
    let saved_disk = fs::read_to_string(&file_path).expect("read saved file");
    assert!(
        saved_disk.contains("line 5 - in-memory edit (Yours)"),
        "Disk content must contain user's resolved edit"
    );
    assert!(
        !saved_disk.contains("line 5 - on-disk edit (Agent)"),
        "Disk content must not contain rejected agent edit"
    );
}

#[test]
fn test_conflict_resolve_agent_r() {
    let tmp = TempDir::new().expect("tempdir");
    let file_path = tmp.path().join("conflict_r.txt");

    let initial: String = (1..=5).map(|i| format!("row {i}\n")).collect();
    fs::write(&file_path, &initial).expect("write file");

    let mut app = App::new(tmp.path().to_path_buf());
    app.open_file(&file_path).expect("open file");

    // Edit row 3 in memory
    app.buf.as_mut().unwrap().lines[2] = "row 3 - yours".into();
    app.buf.as_mut().unwrap().dirty = true;
    app.dirty = true;

    // Edit row 3 on disk
    fs::write(&file_path, "row 1\nrow 2\nrow 3 - agent\nrow 4\nrow 5\n").expect("write disk");

    // Trigger fs event
    app.handle_msg(Msg::Fs(Event {
        kind: EventKind::Any,
        paths: vec![file_path.clone()],
        attrs: Default::default(),
    }));

    assert_eq!(app.middle_mode, MiddleMode::Conflict);

    // Resolve with 'r' (Agent)
    app.handle_msg(Msg::Key(make_key(KeyCode::Char('r'))));

    assert_eq!(app.middle_mode, MiddleMode::Edit);
    assert!(!app.is_dirty());

    let saved = fs::read_to_string(&file_path).expect("read file");
    assert!(saved.contains("row 3 - agent"));
    assert!(!saved.contains("row 3 - yours"));
}

#[test]
fn test_conflict_resolve_both_b() {
    let tmp = TempDir::new().expect("tempdir");
    let file_path = tmp.path().join("conflict_b.txt");

    let initial: String = (1..=5).map(|i| format!("row {i}\n")).collect();
    fs::write(&file_path, &initial).expect("write file");

    let mut app = App::new(tmp.path().to_path_buf());
    app.open_file(&file_path).expect("open file");

    // Edit row 3 in memory
    app.buf.as_mut().unwrap().lines[2] = "row 3 - yours".into();
    app.buf.as_mut().unwrap().dirty = true;
    app.dirty = true;

    // Edit row 3 on disk
    fs::write(&file_path, "row 1\nrow 2\nrow 3 - agent\nrow 4\nrow 5\n").expect("write disk");

    app.handle_msg(Msg::Fs(Event {
        kind: EventKind::Any,
        paths: vec![file_path.clone()],
        attrs: Default::default(),
    }));

    assert_eq!(app.middle_mode, MiddleMode::Conflict);

    // Resolve with 'b' (Keep Both)
    app.handle_msg(Msg::Key(make_key(KeyCode::Char('b'))));

    assert_eq!(app.middle_mode, MiddleMode::Edit);
    assert!(!app.is_dirty());

    let saved = fs::read_to_string(&file_path).expect("read file");
    assert!(saved.contains("row 3 - yours"));
    assert!(saved.contains("row 3 - agent"));
}

#[test]
fn test_conflict_multiple_hunks_stepping_c_and_resolve_all_l() {
    let tmp = TempDir::new().expect("tempdir");
    let file_path = tmp.path().join("multi_hunks.txt");

    let initial: String = (1..=10).map(|i| format!("line {i}\n")).collect();
    fs::write(&file_path, &initial).expect("write file");

    let mut app = App::new(tmp.path().to_path_buf());
    app.open_file(&file_path).expect("open file");

    // Edit line 2 and line 8 in memory
    {
        let buf = app.buf.as_mut().unwrap();
        buf.lines[1] = "line 2 - user".into();
        buf.lines[7] = "line 8 - user".into();
        buf.dirty = true;
    }
    app.dirty = true;

    // Edit line 2 and line 8 on disk differently
    let mut disk_lines: Vec<String> = (1..=10).map(|i| format!("line {i}")).collect();
    disk_lines[1] = "line 2 - agent".into();
    disk_lines[7] = "line 8 - agent".into();
    fs::write(&file_path, disk_lines.join("\n") + "\n").expect("write disk");

    app.handle_msg(Msg::Fs(Event {
        kind: EventKind::Any,
        paths: vec![file_path.clone()],
        attrs: Default::default(),
    }));

    assert_eq!(app.middle_mode, MiddleMode::Conflict);
    let conflict = app.conflict.as_ref().unwrap();
    assert_eq!(conflict.conflict_count(), 2);
    assert_eq!(conflict.active_hunk_idx, 0);

    // Step to next hunk with 'c'
    app.handle_msg(Msg::Key(make_key(KeyCode::Char('c'))));
    assert_eq!(app.conflict.as_ref().unwrap().active_hunk_idx, 1);

    // Step to prev hunk with 'C' (shift+c)
    app.handle_msg(Msg::Key(make_shift_key(KeyCode::Char('C'))));
    assert_eq!(app.conflict.as_ref().unwrap().active_hunk_idx, 0);

    // Resolve all with 'L'
    app.handle_msg(Msg::Key(make_shift_key(KeyCode::Char('L'))));

    assert_eq!(app.middle_mode, MiddleMode::Edit);
    assert!(!app.is_dirty());

    let saved = fs::read_to_string(&file_path).expect("read file");
    assert!(saved.contains("line 2 - user"));
    assert!(saved.contains("line 8 - user"));
    assert!(!saved.contains("line 2 - agent"));
    assert!(!saved.contains("line 8 - agent"));
}

#[test]
fn test_conflict_ui_rendering_in_test_backend() {
    let tmp = TempDir::new().expect("tempdir");
    let file_path = tmp.path().join("render_conflict.txt");

    let initial = "first\nmiddle\nlast\n";
    fs::write(&file_path, initial).expect("write file");

    let mut app = App::new(tmp.path().to_path_buf());
    app.open_file(&file_path).expect("open file");

    app.buf.as_mut().unwrap().lines[1] = "middle - user".into();
    app.buf.as_mut().unwrap().dirty = true;
    app.dirty = true;

    fs::write(&file_path, "first\nmiddle - agent\nlast\n").expect("write disk");

    app.handle_msg(Msg::Fs(Event {
        kind: EventKind::Any,
        paths: vec![file_path.clone()],
        attrs: Default::default(),
    }));

    assert_eq!(app.middle_mode, MiddleMode::Conflict);

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|f| {
            nide::ui::draw(f, &mut app);
        })
        .expect("draw");

    let buffer = terminal.backend().buffer();
    let content: String = buffer
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();

    assert!(
        content.contains("CONFLICT"),
        "Status bar or editor title must show CONFLICT badge"
    );
    assert!(
        content.contains("Yours"),
        "Buffer view must render 'Yours' conflict header"
    );
    assert!(
        content.contains("Agent"),
        "Buffer view must render 'Agent' conflict separator"
    );
    assert!(
        content.contains("middle - user"),
        "Must display user's conflicting line"
    );
    assert!(
        content.contains("middle - agent"),
        "Must display agent's conflicting line"
    );
}
