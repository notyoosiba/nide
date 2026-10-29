use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;
use tempfile::TempDir;
use nide::app::{App, MiddleMode, Msg};
use nide::git::{list_marks, parse_status, rollup_marks};
use nide::theme;
use nide::tree::Tree;
use nide::ui;

fn key_press(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn find_cells_matching(
    buffer: &Buffer,
    text: &str,
) -> Vec<(u16, u16, Color, Modifier)> {
    let mut results = Vec::new();
    let text_chars: Vec<char> = text.chars().collect();
    for y in 0..buffer.area.height {
        let cells: Vec<_> = (0..buffer.area.width)
            .map(|x| buffer.cell((x, y)).unwrap())
            .collect();
        for start_x in 0..=cells.len().saturating_sub(text_chars.len()) {
            let matches = text_chars.iter().enumerate().all(|(i, &ch)| {
                cells[start_x + i].symbol().starts_with(ch)
            });
            if matches {
                for (i, _) in text_chars.iter().enumerate() {
                    let x = (start_x + i) as u16;
                    let cell = cells[start_x + i];
                    results.push((x, y, cell.fg, cell.modifier));
                }
                return results;
            }
        }
    }
    results
}

#[test]
fn test_git_parse_status_unit() {
    let raw = "\
?? untracked.txt
 M modified_worktree.rs
M  modified_index.rs
MM modified_both.rs
A  added_index.rs
AM added_and_modified.rs
 D deleted_worktree.txt
D  deleted_index.txt
R  old_name.rs -> new_name.rs
?? \"quoted name with spaces.txt\"
?? ./relative_path.txt
";

    let marks = parse_status(raw);

    assert_eq!(marks.get(Path::new("untracked.txt")), Some(&'?'));
    assert_eq!(marks.get(Path::new("modified_worktree.rs")), Some(&'M'));
    assert_eq!(marks.get(Path::new("modified_index.rs")), Some(&'M'));
    assert_eq!(marks.get(Path::new("modified_both.rs")), Some(&'M'));
    assert_eq!(marks.get(Path::new("added_index.rs")), Some(&'A'));
    assert_eq!(marks.get(Path::new("added_and_modified.rs")), Some(&'M'));
    assert_eq!(marks.get(Path::new("deleted_worktree.txt")), Some(&'D'));
    assert_eq!(marks.get(Path::new("deleted_index.txt")), Some(&'D'));
    assert_eq!(marks.get(Path::new("new_name.rs")), Some(&'M'));
    assert_eq!(
        marks.get(Path::new("quoted name with spaces.txt")),
        Some(&'?')
    );
    assert_eq!(marks.get(Path::new("relative_path.txt")), Some(&'?'));
}

#[test]
fn test_git_rollup_marks_unit() {
    let mut marks = HashMap::new();
    marks.insert(PathBuf::from("nested/deep/file.rs"), 'M');
    marks.insert(PathBuf::from("nested/deep/untracked.txt"), '?');
    marks.insert(PathBuf::from("deleted_dir/sub/gone.txt"), 'D');
    marks.insert(PathBuf::from("root_file.txt"), 'M');

    let rolled = rollup_marks(&marks);

    // Multi-level parent rollup
    assert_eq!(rolled.get(Path::new("nested/deep")), Some(&'M'));
    assert_eq!(rolled.get(Path::new("nested")), Some(&'M'));

    // Priority: 'M' (4) > '?' (2)
    assert_eq!(rolled.get(Path::new("nested/deep")), Some(&'M'));

    // Deleted folder rollup
    assert_eq!(rolled.get(Path::new("deleted_dir/sub")), Some(&'D'));
    assert_eq!(rolled.get(Path::new("deleted_dir")), Some(&'D'));

    // Root file should not add empty or root path
    assert_eq!(rolled.get(Path::new("")), None);
}

#[test]
fn test_tree_lazy_expand_ignored_unit() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    fs::write(root.join(".gitignore"), "target/\n").unwrap();
    fs::create_dir_all(root.join("target/debug/build")).unwrap();
    fs::write(root.join("target/debug/build/artifact.bin"), "bin").unwrap();
    fs::write(root.join("target/debug/app"), "app").unwrap();
    fs::write(root.join("target/summary.txt"), "summary").unwrap();
    fs::write(root.join("main.rs"), "fn main() {}").unwrap();

    let (mut tree, _files) = Tree::walk(root);

    // Initially target is present, 1 level deep, not expanded
    let target_idx = tree
        .nodes
        .iter()
        .position(|n| n.path == Path::new("target"))
        .expect("target present");
    assert!(tree.nodes[target_idx].ignored);
    assert!(tree.nodes[target_idx].is_dir);
    assert!(!tree.nodes[target_idx].expanded);
    assert_eq!(
        tree.nodes
            .iter()
            .filter(|n| n.path.starts_with("target"))
            .count(),
        1
    );

    // Lazily expand target
    tree.lazy_expand_ignored(target_idx);

    // Children of target are now inserted
    let target_debug_idx = tree
        .nodes
        .iter()
        .position(|n| n.path == Path::new("target/debug"))
        .expect("target/debug present after lazy expansion");
    assert!(tree.nodes[target_debug_idx].ignored);
    assert!(tree.nodes[target_debug_idx].is_dir);
    assert_eq!(tree.nodes[target_debug_idx].depth, 1);

    let summary_idx = tree
        .nodes
        .iter()
        .position(|n| n.path == Path::new("target/summary.txt"))
        .expect("target/summary.txt present");
    assert!(tree.nodes[summary_idx].ignored);
    assert!(!tree.nodes[summary_idx].is_dir);
    assert_eq!(tree.nodes[summary_idx].depth, 1);

    // Nested target/debug/build is not yet expanded
    assert_eq!(
        tree.nodes
            .iter()
            .filter(|n| n.path != Path::new("target/debug") && n.path.starts_with("target/debug"))
            .count(),
        0
    );

    // Now lazily expand target/debug
    let target_debug_idx = tree
        .nodes
        .iter()
        .position(|n| n.path == Path::new("target/debug"))
        .unwrap();
    tree.lazy_expand_ignored(target_debug_idx);

    let build_idx = tree
        .nodes
        .iter()
        .position(|n| n.path == Path::new("target/debug/build"))
        .expect("target/debug/build present");
    assert!(tree.nodes[build_idx].ignored);
    assert!(tree.nodes[build_idx].is_dir);
    assert_eq!(tree.nodes[build_idx].depth, 2);

    // Expanding again is idempotent (does not duplicate)
    let len_before = tree.nodes.len();
    tree.lazy_expand_ignored(target_debug_idx);
    assert_eq!(tree.nodes.len(), len_before);
}

#[test]
fn test_git_repo_status_dimmed_ignored_and_status_coloring_in_testbackend() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    // 1. Initialize git repo
    let run_git = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("git failed");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    };

    run_git(&["init"]);
    run_git(&["config", "user.name", "Tester"]);
    run_git(&["config", "user.email", "tester@example.com"]);

    // 2. Set up initial files & commit
    fs::write(root.join(".gitignore"), "target/\n*.log\n").unwrap();

    fs::create_dir_all(root.join("tracked_dir")).unwrap();
    fs::write(root.join("tracked_dir/committed.txt"), "original\n").unwrap();

    fs::create_dir_all(root.join("clean_dir")).unwrap();
    fs::write(root.join("clean_dir/clean.txt"), "clean\n").unwrap();

    run_git(&[
        "add",
        ".gitignore",
        "tracked_dir/committed.txt",
        "clean_dir/clean.txt",
    ]);
    run_git(&["commit", "-m", "initial commit"]);

    // 3. Make working-tree changes:
    // - Modify committed file
    fs::write(root.join("tracked_dir/committed.txt"), "modified\n").unwrap();

    // - Create untracked file in untracked dir
    fs::create_dir_all(root.join("untracked_dir")).unwrap();
    fs::write(root.join("untracked_dir/new.txt"), "new\n").unwrap();

    // - Create ignored folder with nested content & ignored file
    fs::create_dir_all(root.join("target/debug")).unwrap();
    fs::write(root.join("target/debug/bin"), "bin\n").unwrap();
    fs::write(root.join("error.log"), "error log\n").unwrap();

    // 4. Verify git::list_marks and git::parse_status
    let marks = list_marks(root);
    assert_eq!(marks.get(Path::new("tracked_dir/committed.txt")), Some(&'M'));
    assert_eq!(marks.get(Path::new("untracked_dir/new.txt")), Some(&'?'));
    assert_eq!(marks.get(Path::new("clean_dir/clean.txt")), None);
    assert_eq!(marks.get(Path::new("error.log")), None);
    assert_eq!(marks.get(Path::new("target")), None);

    // 5. Build App and render with TestBackend
    let mut app = App::new(root.to_path_buf());
    app.refresh_git_marks_sync();
    app.tree.expand_all();

    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|f| ui::draw(f, &mut app))
        .expect("ui draw failed");

    let buffer = terminal.backend().buffer();

    // Verification 1: Dimmed rendering of ignored nodes in theme::OVERLAY0 with Modifier::DIM
    let target_cells = find_cells_matching(buffer, "target");
    assert!(!target_cells.is_empty(), "target should be rendered");
    for (x, y, fg, modifier) in &target_cells {
        assert_eq!(
            *fg,
            theme::GIT_IGNORED,
            "target at ({x}, {y}) should have OVERLAY0 fg"
        );
        assert!(
            modifier.contains(Modifier::DIM),
            "target at ({x}, {y}) should be DIM"
        );
    }

    let error_log_cells = find_cells_matching(buffer, "error.log");
    assert!(!error_log_cells.is_empty(), "error.log should be rendered");
    for (x, y, fg, modifier) in &error_log_cells {
        assert_eq!(
            *fg,
            theme::GIT_IGNORED,
            "error.log at ({x}, {y}) should have OVERLAY0 fg"
        );
        assert!(
            modifier.contains(Modifier::DIM),
            "error.log at ({x}, {y}) should be DIM"
        );
    }

    // Verification 2: Git status coloring on both modified file and its parent directory (theme::GIT_MODIFIED / YELLOW)
    let tracked_dir_cells = find_cells_matching(buffer, "tracked_dir");
    assert!(
        !tracked_dir_cells.is_empty(),
        "tracked_dir should be rendered"
    );
    for (x, y, fg, modifier) in &tracked_dir_cells {
        assert_eq!(
            *fg,
            theme::GIT_MODIFIED,
            "tracked_dir at ({x}, {y}) should have GIT_MODIFIED (YELLOW) fg"
        );
        assert!(
            modifier.contains(Modifier::BOLD),
            "tracked_dir at ({x}, {y}) should be BOLD"
        );
    }


    let committed_cells = find_cells_matching(buffer, "committed.txt");
    assert!(
        !committed_cells.is_empty(),
        "committed.txt should be rendered"
    );
    for (x, y, fg, _) in &committed_cells {
        assert_eq!(
            *fg,
            theme::GIT_MODIFIED,
            "committed.txt at ({x}, {y}) should have GIT_MODIFIED (YELLOW) fg"
        );
    }

    // Verification 3: Git status coloring on both untracked file and its parent directory (theme::GIT_UNTRACKED / GREEN)
    let untracked_dir_cells = find_cells_matching(buffer, "untracked_dir");
    assert!(
        !untracked_dir_cells.is_empty(),
        "untracked_dir should be rendered"
    );
    for (x, y, fg, modifier) in &untracked_dir_cells {
        assert_eq!(
            *fg,
            theme::GIT_UNTRACKED,
            "untracked_dir at ({x}, {y}) should have GIT_UNTRACKED (GREEN) fg"
        );
        assert!(
            modifier.contains(Modifier::BOLD),
            "untracked_dir at ({x}, {y}) should be BOLD"
        );
    }

    let new_txt_cells = find_cells_matching(buffer, "new.txt");
    assert!(!new_txt_cells.is_empty(), "new.txt should be rendered");
    for (x, y, fg, _) in &new_txt_cells {
        assert_eq!(
            *fg,
            theme::GIT_UNTRACKED,
            "new.txt at ({x}, {y}) should have GIT_UNTRACKED (GREEN) fg"
        );
    }

    // Verification 4: Clean directory and file have normal theme styles
    let clean_dir_cells = find_cells_matching(buffer, "clean_dir");
    assert!(!clean_dir_cells.is_empty(), "clean_dir should be rendered");
    for (x, y, fg, modifier) in &clean_dir_cells {
        assert_eq!(
            *fg,
            theme::BLUE,
            "clean_dir at ({x}, {y}) should have BLUE fg"
        );
        assert!(
            modifier.contains(Modifier::BOLD),
            "clean_dir at ({x}, {y}) should be BOLD"
        );
    }

    let clean_txt_cells = find_cells_matching(buffer, "clean.txt");
    assert!(!clean_txt_cells.is_empty(), "clean.txt should be rendered");
    for (x, y, fg, _) in &clean_txt_cells {
        assert_eq!(
            *fg,
            theme::TEXT,
            "clean.txt at ({x}, {y}) should have TEXT fg"
        );
    }
}

#[test]
fn test_git_marks_message_and_save_triggers() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    fs::create_dir_all(root.join("sub")).unwrap();
    let file = root.join("sub/test.txt");
    fs::write(&file, "hello\n").unwrap();

    let (tx, rx) = std::sync::mpsc::channel();
    let mut app = App::with_sender(root.to_path_buf(), Some(tx));

    // Dispatch synthetic Msg::Marks
    let mut marks = HashMap::new();
    marks.insert(PathBuf::from("sub/test.txt"), 'M');
    app.handle_msg(Msg::Marks(marks));

    assert_eq!(
        app.git_status_for(Path::new("sub/test.txt"), false),
        Some('M')
    );
    assert_eq!(app.git_status_for(Path::new("sub"), true), Some('M'));

    // Open file, edit, Esc saves and triggers git marks
    app.open_file(&file).unwrap();
    assert_eq!(app.middle_mode, MiddleMode::Edit);
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('!'))));
    assert!(app.is_dirty());

    // Drain channel before Esc
    while rx.try_recv().is_ok() {}

    // Esc triggers save and background git marks
    app.handle_msg(Msg::Key(key_press(KeyCode::Esc)));
    assert_eq!(app.middle_mode, MiddleMode::Normal);
    assert!(!app.is_dirty());

    // Expect Msg::Marks sent over tx
    let start = std::time::Instant::now();
    let mut got_marks = false;
    while start.elapsed() < std::time::Duration::from_secs(2) {
        if let Ok(Msg::Marks(_)) = rx.recv_timeout(std::time::Duration::from_millis(200)) {
            got_marks = true;
            break;
        }
    }
    assert!(got_marks, "Expected Msg::Marks sent over tx");
}

#[test]
fn test_unit_parse_diff_ghost_anchoring_stale_seq_and_intraline() {
    use nide::git::{parse_diff, Mark};
    use nide::intraline::SegmentKind;

    let diff_text = "\
diff --git a/test.rs b/test.rs
--- a/test.rs
+++ b/test.rs
@@ -2,2 +2 @@ fn test()
-let x = 10;
-let y = 20;
+let x = 99;
@@ -10,0 +9,2 @@ fn mid()
+added1();
+added2();
@@ -15,2 +16,0 @@ fn end()
-deleted1();
-deleted2();
";

    let diff = parse_diff(diff_text);
    assert_eq!(diff.hunks.len(), 3);

    // Hunk 1: modification (2 deleted, 1 added)
    let h1 = &diff.hunks[0];
    assert_eq!(h1.old_start, 2);
    assert_eq!(h1.old_count, 2);
    assert_eq!(h1.new_start, 2);
    assert_eq!(h1.new_count, 1);
    assert_eq!(h1.target_line(), 1);
    assert_eq!(diff.marks.get(&1), Some(&Mark::Changed));
    // Ghost lines anchored at 1
    let h1_ghosts: Vec<_> = diff.ghosts.iter().filter(|g| g.anchor_line == 1).collect();
    assert_eq!(h1_ghosts.len(), 2);
    assert_eq!(h1_ghosts[0].text, "let x = 10;");
    assert_eq!(h1_ghosts[1].text, "let y = 20;");
    // Myers intraline diff for "let x = 10;" vs "let x = 99;"
    assert_eq!(
        h1_ghosts[0].spans,
        vec![
            nide::IntralineSpan::new("let x = ", SegmentKind::Unchanged),
            nide::IntralineSpan::new("10", SegmentKind::Deleted),
            nide::IntralineSpan::new(";", SegmentKind::Unchanged),
        ]
    );
    // Purely deleted line in hunk 1 has full deleted span
    assert_eq!(
        h1_ghosts[1].spans,
        vec![nide::IntralineSpan::new("let y = 20;", SegmentKind::Deleted)]
    );

    // Hunk 2: pure addition (0 old, 2 new)
    let h2 = &diff.hunks[1];
    assert_eq!(h2.old_start, 10);
    assert_eq!(h2.old_count, 0);
    assert_eq!(h2.new_start, 9);
    assert_eq!(h2.new_count, 2);
    assert_eq!(h2.target_line(), 8);
    assert_eq!(diff.marks.get(&8), Some(&Mark::Added));
    assert_eq!(diff.marks.get(&9), Some(&Mark::Added));

    // Hunk 3: pure deletion (2 old, 0 new)
    let h3 = &diff.hunks[2];
    assert_eq!(h3.old_start, 15);
    assert_eq!(h3.old_count, 2);
    assert_eq!(h3.new_start, 16);
    assert_eq!(h3.new_count, 0);
    assert_eq!(h3.target_line(), 15);
    assert_eq!(diff.marks.get(&15), Some(&Mark::DeletedBelow));
    let h3_ghosts: Vec<_> = diff.ghosts.iter().filter(|g| g.anchor_line == 16).collect();
    assert_eq!(h3_ghosts.len(), 2);
    assert_eq!(h3_ghosts[0].text, "deleted1();");
    assert_eq!(h3_ghosts[1].text, "deleted2();");

    // Test stale `seq` rejection in App
    let tmp = TempDir::new().expect("tempdir");
    let file = tmp.path().join("test.rs");
    fs::write(&file, "line1\nline2\n").unwrap();

    let mut app = App::new(tmp.path().to_path_buf());
    app.open_file(&file).unwrap();
    app.diff_seq = 10;

    // Stale seq: seq = 9 (!= 10) -> rejected
    app.handle_msg(Msg::DiffDone {
        seq: 9,
        path: file.clone(),
        diff: diff.clone(),
    });
    assert!(app.diff.is_none(), "Stale seq message should be rejected");

    // Stale path: seq = 10, but different path -> rejected
    app.handle_msg(Msg::DiffDone {
        seq: 10,
        path: tmp.path().join("other.rs"),
        diff: diff.clone(),
    });
    assert!(app.diff.is_none(), "Stale path message should be rejected");

    // Matching seq and path -> accepted
    app.handle_msg(Msg::DiffDone {
        seq: 10,
        path: file.clone(),
        diff: diff.clone(),
    });
    assert!(app.diff.is_some(), "Matching seq and path should be accepted");
    assert_eq!(app.diff.unwrap().hunks.len(), 3);
}

#[test]
fn test_e2e_git_diff_gutter_bars_ghost_lines_and_hunk_stepping_in_testbackend() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    let run_git = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("git failed");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    };

    run_git(&["init"]);
    run_git(&["config", "user.name", "Tester"]);
    run_git(&["config", "user.email", "tester@example.com"]);

    let initial_content = "\
line 01: fn alpha() {
line 02:     let x = 10;
line 03:     let y = 20;
line 04: }
line 05:
line 06: fn beta() {
line 07:     let name = \"hello\";
line 08: }
line 09:
line 10: fn gamma() {
line 11:     println!(\"gamma\");
line 12: }
line 13:
line 14: fn delta() {
line 15:     let val = 1;
line 16: }
line 17:
line 18: fn epsilon() {
line 19:     println!(\"epsilon\");
line 20: }
";
    let file = root.join("code.rs");
    fs::write(&file, initial_content).unwrap();

    run_git(&["add", "code.rs"]);
    run_git(&["commit", "-m", "initial commit"]);

    // Modify file:
    // Hunk 1 (line 2-3): modify line 2 to "let x = 99;", delete line 3 ("let y = 20;")
    // Hunk 2 (line 15): insert "    let added_extra = true;"
    let modified_content = "\
line 01: fn alpha() {
line 02:     let x = 99;
line 04: }
line 05:
line 06: fn beta() {
line 07:     let name = \"hello\";
line 08: }
line 09:
line 10: fn gamma() {
line 11:     println!(\"gamma\");
line 12: }
line 13:
line 14: fn delta() {
line 15:     let added_extra = true;
line 15:     let val = 1;
line 16: }
line 17:
line 18: fn epsilon() {
line 19:     println!(\"epsilon\");
line 20: }
";
    fs::write(&file, modified_content).unwrap();

    let (tx, rx) = std::sync::mpsc::channel();
    let mut app = App::with_sender(root.to_path_buf(), Some(tx));
    app.open_file(&file).unwrap();

    // Process Msg::DiffDone from background git diff
    let mut received_diff = false;
    for _ in 0..10 {
        if let Ok(msg) = rx.recv_timeout(std::time::Duration::from_millis(500)) {
            match msg {
                Msg::DiffDone { seq, path, diff } => {
                    app.handle_msg(Msg::DiffDone { seq, path, diff });
                    received_diff = true;
                    break;
                }
                other => {
                    app.handle_msg(other);
                }
            }
        }
    }
    assert!(received_diff, "Should receive Msg::DiffDone");
    assert!(app.diff.is_some(), "App should have diff loaded");
    assert_eq!(app.diff.as_ref().unwrap().hunks.len(), 2);

    // Switch to MiddleMode::Normal and layout middle pane
    app.set_focus(nide::Focus::Middle);
    app.middle_mode = MiddleMode::Normal;
    app.layout.middle = Some(ratatui::layout::Rect::new(0, 0, 80, 24));

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|f| ui::draw(f, &mut app))
        .expect("ui draw failed");

    let buffer = terminal.backend().buffer();

    // 1. Verify Gutter Diff Bars in TestBackend
    // Changed line has blue gutter bar
    let mut found_blue_gutter_bar = false;
    let mut found_green_gutter_bar = false;
    let mut found_red_gutter_bar = false;

    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let cell = buffer.cell((x, y)).unwrap();
            if cell.symbol() == theme::DIFF_GUTTER_BAR {
                if cell.fg == theme::DIFF_CHANGED {
                    found_blue_gutter_bar = true;
                } else if cell.fg == theme::DIFF_ADDED {
                    found_green_gutter_bar = true;
                } else if cell.fg == theme::DIFF_DELETED {
                    found_red_gutter_bar = true;
                }
            }
        }
    }

    assert!(
        found_blue_gutter_bar,
        "Should render blue gutter bar for changed line"
    );
    assert!(
        found_green_gutter_bar,
        "Should render green gutter bar for added line"
    );
    assert!(
        found_red_gutter_bar,
        "Should render red gutter bar for ghost/deleted line"
    );

    // 2. Verify Ghost Line rendering in TestBackend
    // The phantom deleted line "let y = 20;" should appear in the rendered buffer
    let ghost_cells = find_cells_matching(buffer, "let y = 20;");
    assert!(
        !ghost_cells.is_empty(),
        "Ghost line 'let y = 20;' should be rendered in viewport"
    );
    for (x, y, fg, modifier) in &ghost_cells {
        assert_eq!(
            *fg,
            theme::RED,
            "Ghost line at ({x}, {y}) should be rendered in red"
        );
        assert!(
            modifier.contains(Modifier::CROSSED_OUT),
            "Ghost line at ({x}, {y}) should be crossed out"
        );
    }

    // Also verify the ghost line gutter has '-' and red gutter bar
    let mid_rect = app.layout.middle.unwrap();
    let ghost_y = ghost_cells[0].1;
    let gutter_minus = buffer.cell((mid_rect.x + 3, ghost_y)).unwrap();
    assert_eq!(
        gutter_minus.symbol(),
        "-",
        "Ghost line gutter should show '-'"
    );
    assert_eq!(
        gutter_minus.fg,
        theme::DIFF_DELETED,
        "Ghost line gutter '-' should be red"
    );

    let ghost_gutter_bar = buffer.cell((mid_rect.x + 4, ghost_y)).unwrap();
    assert_eq!(
        ghost_gutter_bar.symbol(),
        theme::DIFF_GUTTER_BAR,
        "Ghost line gutter bar should show DIFF_GUTTER_BAR"
    );
    assert_eq!(
        ghost_gutter_bar.fg,
        theme::DIFF_DELETED,
        "Ghost line gutter bar should be red"
    );

    // 3. Verify `c` and `C` hunk stepping across hunks
    // Initially cursor is at row 0
    if let Some(buf) = &mut app.buf {
        buf.cursor_row = 0;
    }
    app.sync_cursor();
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 0);

    let hunk1_target = app.diff.as_ref().unwrap().hunks[0].target_line();
    let hunk2_target = app.diff.as_ref().unwrap().hunks[1].target_line();
    assert_eq!(hunk1_target, 1);
    assert_eq!(hunk2_target, 13);

    // Press 'c' -> steps to hunk 1
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('c'))));
    assert_eq!(
        app.buf.as_ref().unwrap().cursor_row,
        hunk1_target,
        "Pressing 'c' should jump to hunk 1"
    );

    // Press 'c' -> steps to hunk 2
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('c'))));
    assert_eq!(
        app.buf.as_ref().unwrap().cursor_row,
        hunk2_target,
        "Pressing 'c' again should jump to hunk 2"
    );

    // Press 'c' -> wraps back to hunk 1
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('c'))));
    assert_eq!(
        app.buf.as_ref().unwrap().cursor_row,
        hunk1_target,
        "Pressing 'c' at last hunk should wrap to hunk 1"
    );

    // Press 'C' (Shift+c) -> wraps to hunk 2
    app.handle_msg(Msg::Key(KeyEvent {
        code: KeyCode::Char('c'),
        modifiers: KeyModifiers::SHIFT,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }));
    assert_eq!(
        app.buf.as_ref().unwrap().cursor_row,
        hunk2_target,
        "Pressing 'C' should jump back to hunk 2"
    );

    // Press 'C' -> steps back to hunk 1
    app.handle_msg(Msg::Key(KeyEvent {
        code: KeyCode::Char('C'),
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }));
    assert_eq!(
        app.buf.as_ref().unwrap().cursor_row,
        hunk1_target,
        "Pressing 'C' again should jump back to hunk 1"
    );
}

