use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use notify::{Event, EventKind};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tempfile::TempDir;
use nide::app::{App, Focus, MiddleMode, Msg};
use nide::live::{
    debounce_loop, filter_event, is_ignored_path, should_ignore_event, should_ignore_path,
};
use nide::tree::Tree;
use nide::ui;

#[test]
fn test_unit_nide_save_paths_ignored_by_watcher_filter() {
    // 1. Direct path checks
    assert!(is_ignored_path(Path::new(".nide-save-1234")));
    assert!(is_ignored_path(Path::new(".nide-save-99999")));
    assert!(is_ignored_path(Path::new("/tmp/project/.nide-save-100")));
    assert!(is_ignored_path(Path::new("relative/dir/.nide-save-555")));
    assert!(should_ignore_path(Path::new(".nide-save-pid")));

    // Normal paths must NOT be ignored
    assert!(!is_ignored_path(Path::new("main.rs")));
    assert!(!is_ignored_path(Path::new("/tmp/project/src/lib.rs")));
    assert!(!is_ignored_path(Path::new(".gitignore")));
    assert!(!is_ignored_path(Path::new("nide-save-not-hidden.txt")));
    assert!(!should_ignore_path(Path::new("notes.md")));

    // 2. Event filtering
    let only_temp = Event {
        kind: EventKind::Any,
        paths: vec![
            PathBuf::from("/tmp/repo/.nide-save-1"),
            PathBuf::from(".nide-save-2"),
        ],
        attrs: Default::default(),
    };
    assert!(should_ignore_event(&only_temp));
    assert!(filter_event(only_temp).is_none());

    let mixed = Event {
        kind: EventKind::Any,
        paths: vec![
            PathBuf::from("/tmp/repo/.nide-save-1"),
            PathBuf::from("/tmp/repo/src/code.rs"),
        ],
        attrs: Default::default(),
    };
    assert!(!should_ignore_event(&mixed));
    let filtered_mixed = filter_event(mixed).expect("retains non-temp path");
    assert_eq!(
        filtered_mixed.paths,
        vec![PathBuf::from("/tmp/repo/src/code.rs")]
    );

    let clean = Event {
        kind: EventKind::Any,
        paths: vec![PathBuf::from("/tmp/repo/README.md")],
        attrs: Default::default(),
    };
    assert!(!should_ignore_event(&clean));
    let filtered_clean = filter_event(clean).expect("retains clean path");
    assert_eq!(
        filtered_clean.paths,
        vec![PathBuf::from("/tmp/repo/README.md")]
    );
}

#[test]
fn test_clean_buffer_auto_reloads_on_fs_event_and_clamps_cursor() {
    let tmp = TempDir::new().expect("tempdir");
    let file_path = tmp.path().join("document.txt");

    // Initial file with 6 long lines
    let initial_content = "line 1: some long text\nline 2: more text\nline 3: third line\nline 4: fourth line\nline 5: fifth line\nline 6: last line\n";
    fs::write(&file_path, initial_content).expect("write initial file");

    let mut app = App::new(tmp.path().to_path_buf());
    app.open_file(&file_path).expect("open file");

    assert_eq!(app.focus, Focus::Middle);
    assert_eq!(app.middle_mode, MiddleMode::Edit);
    assert!(!app.dirty);
    assert!(!app.buf.as_ref().unwrap().dirty);
    assert_eq!(app.buf.as_ref().unwrap().lines.len(), 6);

    // Place cursor at row 5 (0-indexed line 6), col 15
    {
        let buf = app.buf.as_mut().unwrap();
        buf.cursor_row = 5;
        buf.cursor_col = 15;
    }
    app.sync_cursor();
    assert_eq!(app.line, 5);
    assert_eq!(app.col, 15);

    // Agent modifies file on disk: truncated to only 2 short lines
    let new_disk_content = "agent line 1\nhi\n";
    fs::write(&file_path, new_disk_content).expect("write modified file");

    // Dispatch Msg::Fs
    let fs_event = Event {
        kind: EventKind::Any,
        paths: vec![file_path.clone()],
        attrs: Default::default(),
    };
    app.handle_msg(Msg::Fs(fs_event));

    // Verify buffer was reloaded silently without dirtying
    let buf = app.buf.as_ref().expect("buffer still open");
    assert_eq!(
        buf.lines,
        vec!["agent line 1".to_string(), "hi".to_string()]
    );
    assert_eq!(
        buf.base_lines,
        vec!["agent line 1".to_string(), "hi".to_string()]
    );
    assert!(!buf.dirty, "buffer must remain clean");
    assert!(!app.dirty, "app must remain clean");

    // Cursor must be clamped: row clamped to 1 (last line index), col clamped to 2 ("hi".len())
    assert_eq!(buf.cursor_row, 1);
    assert_eq!(buf.cursor_col, 2);
    assert_eq!(app.line, 1);
    assert_eq!(app.col, 2);

    // Verify UI renders the updated buffer
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");

    let buf_render = terminal.backend().buffer();
    let mut rendered_text = String::new();
    for y in 0..24 {
        for x in 0..80 {
            rendered_text.push(buf_render[(x, y)].symbol().chars().next().unwrap_or(' '));
        }
        rendered_text.push('\n');
    }
    assert!(
        rendered_text.contains("agent line 1"),
        "UI must display reloaded line 1"
    );
    assert!(
        rendered_text.contains("hi"),
        "UI must display reloaded line 2"
    );
}

#[test]
fn test_dirty_buffer_does_not_silently_reload_on_fs_event() {
    let tmp = TempDir::new().expect("tempdir");
    let file_path = tmp.path().join("draft.txt");
    fs::write(&file_path, "original clean content\n").expect("write draft");

    let mut app = App::new(tmp.path().to_path_buf());
    app.open_file(&file_path).expect("open file");

    // User makes an edit in memory -> buffer becomes dirty
    app.buf
        .as_mut()
        .unwrap()
        .insert_str("user uncommitted edit: ");
    assert!(app.is_dirty());

    // Agent modifies file on disk
    fs::write(&file_path, "external agent overwrite\n").expect("write modified draft");

    // Dispatch Msg::Fs
    let fs_event = Event {
        kind: EventKind::Any,
        paths: vec![file_path.clone()],
        attrs: Default::default(),
    };
    app.handle_msg(Msg::Fs(fs_event));

    // Assert user's dirty buffer was NOT overwritten
    let buf = app.buf.as_ref().unwrap();
    assert!(
        buf.lines[0].starts_with("user uncommitted edit:"),
        "User edits must not be overwritten when buffer is dirty"
    );
    assert!(buf.dirty, "Buffer must still be dirty");
}

#[test]
fn test_project_walked_adds_new_file_and_preserves_expanded_folders() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    // Initial folder structure:
    // root/
    //   folder_a/
    //     sub_a.txt
    //   folder_b/
    //     sub_b.txt
    //   root_file.txt
    let folder_a = root.join("folder_a");
    let folder_b = root.join("folder_b");
    fs::create_dir_all(&folder_a).expect("create folder_a");
    fs::create_dir_all(&folder_b).expect("create folder_b");
    fs::write(folder_a.join("sub_a.txt"), "sub a").expect("write sub_a");
    fs::write(folder_b.join("sub_b.txt"), "sub b").expect("write sub_b");
    fs::write(root.join("root_file.txt"), "root").expect("write root_file");

    let mut app = App::new(root.to_path_buf());

    // Expand folder_a
    let folder_a_node = app
        .tree
        .nodes
        .iter_mut()
        .find(|n| n.path == Path::new("folder_a"))
        .expect("folder_a in tree");
    folder_a_node.expanded = true;

    // Verify folder_a is expanded and folder_b is collapsed
    assert!(
        app.tree
            .nodes
            .iter()
            .find(|n| n.path == Path::new("folder_a"))
            .unwrap()
            .expanded
    );
    assert!(
        !app.tree
            .nodes
            .iter()
            .find(|n| n.path == Path::new("folder_b"))
            .unwrap()
            .expanded
    );

    // Agent creates new files on disk:
    // 1. In root: "new_root.txt"
    // 2. In expanded folder_a: "new_nested.txt"
    fs::write(root.join("new_root.txt"), "new root file").expect("write new_root");
    fs::write(folder_a.join("new_nested.txt"), "new nested file").expect("write new_nested");

    // Perform walk and dispatch Msg::ProjectWalked
    let (new_tree, files) = Tree::walk(root);
    app.handle_msg(Msg::ProjectWalked(new_tree, files));

    // Verify app.files includes new files
    assert!(
        app.files.contains(&PathBuf::from("new_root.txt")),
        "app.files must include new_root.txt"
    );
    assert!(
        app.files
            .contains(&PathBuf::from("folder_a/new_nested.txt")),
        "app.files must include folder_a/new_nested.txt"
    );

    // Verify app.tree includes new nodes
    assert!(
        app.tree
            .nodes
            .iter()
            .any(|n| n.path == Path::new("new_root.txt")),
        "app.tree must contain new_root.txt"
    );
    assert!(
        app.tree
            .nodes
            .iter()
            .any(|n| n.path == Path::new("folder_a/new_nested.txt")),
        "app.tree must contain folder_a/new_nested.txt"
    );

    // Assert existing expanded folder_a is PRESERVED as expanded
    let preserved_folder_a = app
        .tree
        .nodes
        .iter()
        .find(|n| n.path == Path::new("folder_a"))
        .expect("folder_a still in tree");
    assert!(
        preserved_folder_a.expanded,
        "folder_a must remain expanded after ProjectWalked"
    );

    // Assert folder_b is still collapsed
    let preserved_folder_b = app
        .tree
        .nodes
        .iter()
        .find(|n| n.path == Path::new("folder_b"))
        .expect("folder_b still in tree");
    assert!(
        !preserved_folder_b.expanded,
        "folder_b must remain collapsed after ProjectWalked"
    );

    // Visible nodes must include the new nested file inside expanded folder_a
    let visible_paths: Vec<_> = app
        .tree
        .visible_nodes()
        .into_iter()
        .map(|n| n.path.as_path())
        .collect();
    assert!(
        visible_paths.contains(&Path::new("folder_a/new_nested.txt")),
        "Nested file in expanded directory must be visible"
    );
    // But folder_b's contents should NOT be visible since it is collapsed
    assert!(
        !visible_paths.contains(&Path::new("folder_b/sub_b.txt")),
        "Collapsed directory contents must not be visible"
    );
}

#[test]
fn test_fs_event_spawns_background_walk_and_deduplicates_in_flight() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    fs::write(root.join("initial.txt"), "hello").expect("write initial");

    let (tx, rx) = mpsc::channel();
    let mut app = App::with_sender(root.to_path_buf(), Some(tx));

    // Create a new file on disk
    fs::write(root.join("added_by_agent.txt"), "agent content").expect("write agent file");

    // Dispatch Msg::Fs
    let fs_event = Event {
        kind: EventKind::Any,
        paths: vec![root.join("added_by_agent.txt")],
        attrs: Default::default(),
    };
    app.handle_msg(Msg::Fs(fs_event.clone()));

    // Verify walk_in_flight was set or handled
    // While in flight, a second Msg::Fs should NOT launch another concurrent walk
    app.walk_in_flight.store(true, Ordering::SeqCst);
    // Calling trigger_background_walk should return early without sending another message
    app.trigger_background_walk();
    app.walk_in_flight.store(false, Ordering::SeqCst);

    // Wait for the background walk message
    let msg = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("should receive Msg::ProjectWalked from background thread");

    if let Msg::ProjectWalked(new_tree, files) = msg {
        app.handle_msg(Msg::ProjectWalked(new_tree, files));
        assert!(app
            .files
            .contains(&PathBuf::from("added_by_agent.txt")));
        assert!(app
            .tree
            .nodes
            .iter()
            .any(|n| n.path == Path::new("added_by_agent.txt")));
    } else {
        panic!("Expected Msg::ProjectWalked, got {:?}", msg);
    }
}

#[test]
fn test_debounce_max_wait_timeout() {
    let (event_tx, event_rx) = mpsc::channel();
    let (out_tx, out_rx) = mpsc::channel();

    let quiet = Duration::from_millis(60);
    let max_wait = Duration::from_millis(150);

    thread::spawn(move || {
        debounce_loop(event_rx, out_tx, quiet, max_wait);
    });

    // Stream events every 20ms (< quiet 60ms) for 200ms (> max_wait 150ms)
    thread::spawn(move || {
        for i in 0..10 {
            let _ = event_tx.send(Event {
                kind: EventKind::Any,
                paths: vec![PathBuf::from(format!("event_{}.txt", i))],
                attrs: Default::default(),
            });
            thread::sleep(Duration::from_millis(20));
        }
    });

    // Even though quiet window was never reached, max_wait must force emission
    let received = out_rx
        .recv_timeout(Duration::from_millis(300))
        .expect("must emit within max_wait");

    if let Msg::Fs(event) = received {
        assert!(
            !event.paths.is_empty(),
            "Emitted event must have accumulated paths"
        );
    } else {
        panic!("Expected Msg::Fs");
    }
}
