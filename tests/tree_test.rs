use std::fs;
use std::path::{Path, PathBuf};

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tempfile::TempDir;
use nide::app::{App, Focus, Msg};
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

fn mouse_down(column: u16, row: u16, button: MouseButton) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(button),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

#[test]
fn test_walk_directories_before_files_and_rendering() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    // Create a hierarchy with mixed directory and file names
    fs::create_dir_all(root.join("z_dir")).unwrap();
    fs::write(root.join("z_dir/nested.txt"), "nested").unwrap();

    fs::create_dir_all(root.join("a_dir/sub_dir")).unwrap();
    fs::write(root.join("a_dir/sub_dir/deep.rs"), "deep").unwrap();
    fs::write(root.join("a_dir/file_b.txt"), "b").unwrap();
    fs::write(root.join("a_dir/file_a.txt"), "a").unwrap();

    fs::write(root.join("c_file.txt"), "c").unwrap();
    fs::write(root.join("b_file.txt"), "b").unwrap();

    let (mut tree, files) = Tree::walk(root);

    // 1. Verify tree.nodes ordering:
    // Root level directories: a_dir, z_dir
    // Root level files: b_file.txt, c_file.txt
    let top_level_nodes: Vec<_> = tree.nodes.iter().filter(|n| n.depth == 0).collect();
    assert_eq!(top_level_nodes.len(), 4);
    assert_eq!(top_level_nodes[0].path, Path::new("a_dir"));
    assert!(top_level_nodes[0].is_dir);
    assert_eq!(top_level_nodes[1].path, Path::new("z_dir"));
    assert!(top_level_nodes[1].is_dir);
    assert_eq!(top_level_nodes[2].path, Path::new("b_file.txt"));
    assert!(!top_level_nodes[2].is_dir);
    assert_eq!(top_level_nodes[3].path, Path::new("c_file.txt"));
    assert!(!top_level_nodes[3].is_dir);

    // 2. Expand all and verify UI rendering via TestBackend
    tree.expand_all();
    let mut app = App::new(root.to_path_buf());
    app.tree = tree;

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|f| ui::draw(f, &mut app))
        .expect("draw failed");

    let buffer = terminal.backend().buffer();
    let text = format!("{buffer:?}");

    // Buffer contains folder arrows
    assert!(text.contains("▾ a_dir") || text.contains("▾ a_dir"));
    assert!(text.contains("▾ sub_dir"));
    assert!(text.contains("deep.rs"));

    // Find row positions in the rendered buffer to assert directories appear before files
    let pos_a_dir = text.find("a_dir").expect("a_dir present");
    let pos_z_dir = text.find("z_dir").expect("z_dir present");
    let pos_b_file = text.find("b_file.txt").expect("b_file present");
    let pos_c_file = text.find("c_file.txt").expect("c_file present");

    assert!(pos_a_dir < pos_z_dir, "a_dir before z_dir");
    assert!(pos_z_dir < pos_b_file, "directories before files at top level");
    assert!(pos_b_file < pos_c_file, "b_file before c_file");

    // 3. Verify app.files contains only non-dir files
    assert_eq!(files.len(), 6);
    assert!(files.contains(&PathBuf::from("a_dir/file_a.txt")));
    assert!(files.contains(&PathBuf::from("a_dir/file_b.txt")));
    assert!(files.contains(&PathBuf::from("a_dir/sub_dir/deep.rs")));
    assert!(files.contains(&PathBuf::from("b_file.txt")));
    assert!(files.contains(&PathBuf::from("c_file.txt")));
    assert!(files.contains(&PathBuf::from("z_dir/nested.txt")));
}

#[test]
fn test_keyboard_navigation_expand_collapse_and_parent_jump() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    fs::create_dir_all(root.join("folder_a/folder_b")).unwrap();
    fs::write(root.join("folder_a/folder_b/child.txt"), "child").unwrap();
    fs::write(root.join("folder_a/sibling.txt"), "sib").unwrap();
    fs::write(root.join("root_file.txt"), "root").unwrap();

    let mut app = App::new(root.to_path_buf());
    assert_eq!(app.focus, Focus::Tree);

    // Initial state: folders are collapsed by default
    // Visible rows: folder_a (0), root_file.txt (1)
    let visible = app.tree.visible_nodes();
    assert_eq!(visible.len(), 2);
    assert_eq!(visible[0].path, Path::new("folder_a"));
    assert_eq!(visible[1].path, Path::new("root_file.txt"));
    assert_eq!(app.tree.cursor, 0);

    // 'j' moves cursor down to root_file.txt
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('j'))));
    assert_eq!(app.tree.cursor, 1);
    assert_eq!(
        app.tree.selected_node().unwrap().path,
        Path::new("root_file.txt")
    );

    // 'k' moves cursor back up to folder_a
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('k'))));
    assert_eq!(app.tree.cursor, 0);
    assert_eq!(
        app.tree.selected_node().unwrap().path,
        Path::new("folder_a")
    );

    // 'Enter' on folder_a expands it
    app.handle_msg(Msg::Key(key_press(KeyCode::Enter)));
    assert!(app.tree.selected_node().unwrap().expanded);
    // Visible rows: folder_a (0), folder_b (1), sibling.txt (2), root_file.txt (3)
    let visible = app.tree.visible_nodes();
    assert_eq!(visible.len(), 4);
    assert_eq!(visible[1].path, Path::new("folder_a/folder_b"));
    assert_eq!(visible[2].path, Path::new("folder_a/sibling.txt"));
    assert_eq!(visible[3].path, Path::new("root_file.txt"));
    assert_eq!(app.tree.cursor, 0);

    // 'j' moves cursor to folder_b
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('j'))));
    assert_eq!(app.tree.cursor, 1);
    assert_eq!(
        app.tree.selected_node().unwrap().path,
        Path::new("folder_a/folder_b")
    );

    // 'Enter' expands folder_b
    app.handle_msg(Msg::Key(key_press(KeyCode::Enter)));
    assert!(app.tree.selected_node().unwrap().expanded);
    // Visible rows: folder_a (0), folder_b (1), child.txt (2), sibling.txt (3), root_file.txt (4)
    let visible = app.tree.visible_nodes();
    assert_eq!(visible.len(), 5);
    assert_eq!(visible[2].path, Path::new("folder_a/folder_b/child.txt"));
    assert_eq!(app.tree.cursor, 1);

    // 'j' moves cursor to child.txt
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('j'))));
    assert_eq!(app.tree.cursor, 2);
    assert_eq!(
        app.tree.selected_node().unwrap().path,
        Path::new("folder_a/folder_b/child.txt")
    );

    // 'h' on child.txt (file) jumps to its parent directory folder_b
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('h'))));
    assert_eq!(app.tree.cursor, 1);
    assert_eq!(
        app.tree.selected_node().unwrap().path,
        Path::new("folder_a/folder_b")
    );

    // 'h' on expanded folder_b collapses it
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('h'))));
    assert!(!app.tree.selected_node().unwrap().expanded);
    assert_eq!(app.tree.visible_nodes().len(), 4);
    assert_eq!(app.tree.cursor, 1);

    // 'h' on collapsed folder_b jumps to parent folder_a
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('h'))));
    assert_eq!(app.tree.cursor, 0);
    assert_eq!(
        app.tree.selected_node().unwrap().path,
        Path::new("folder_a")
    );

    // 'h' on expanded folder_a collapses it
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('h'))));
    assert!(!app.tree.selected_node().unwrap().expanded);
    assert_eq!(app.tree.visible_nodes().len(), 2);
    assert_eq!(app.tree.cursor, 0);
}

#[test]
fn test_reveal_on_collapsed_tree() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    fs::create_dir_all(root.join("a/b")).unwrap();
    fs::write(root.join("a/b/c.rs"), "fn main() {}").unwrap();
    fs::write(root.join("a/x.rs"), "// x").unwrap();
    fs::create_dir_all(root.join("d/e")).unwrap();
    fs::write(root.join("d/e/f.rs"), "// f").unwrap();

    let (mut tree, _) = Tree::walk(root);

    // Initially all directories are collapsed
    for node in &tree.nodes {
        if node.is_dir {
            assert!(!node.expanded, "node {} should start collapsed", node.path.display());
        }
    }

    // Call tree.reveal(Path::new("a/b/c.rs"))
    let found = tree.reveal(Path::new("a/b/c.rs"));
    assert!(found, "reveal should succeed for existing file");

    // Assert ancestor folders 'a' and 'a/b' expand
    let node_a = tree.nodes.iter().find(|n| n.path == Path::new("a")).expect("node a");
    assert!(node_a.expanded, "ancestor 'a' must be expanded");

    let node_ab = tree.nodes.iter().find(|n| n.path == Path::new("a/b")).expect("node a/b");
    assert!(node_ab.expanded, "ancestor 'a/b' must be expanded");

    // Unrelated directory 'd' remains collapsed
    let node_d = tree.nodes.iter().find(|n| n.path == Path::new("d")).expect("node d");
    assert!(!node_d.expanded, "unrelated 'd' must stay collapsed");

    // Assert cursor selects 'c.rs'
    let selected = tree.selected_node().expect("selected node");
    assert_eq!(selected.path, Path::new("a/b/c.rs"));
    assert!(!selected.is_dir);

    // Verify c.rs is at tree.cursor in visible nodes
    let visible = tree.visible_nodes();
    assert_eq!(visible[tree.cursor].path, Path::new("a/b/c.rs"));
}

#[test]
fn test_mouse_click_selects_tree_item() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    fs::write(root.join("file1.txt"), "1").unwrap();
    fs::write(root.join("file2.txt"), "2").unwrap();
    fs::write(root.join("file3.txt"), "3").unwrap();

    let mut app = App::new(root.to_path_buf());
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|f| ui::draw(f, &mut app))
        .expect("draw failed");

    let tree_rect = app.layout.tree.expect("tree layout");
    assert_eq!(app.tree.cursor, 0);

    // Click on row 2 (which is inner row 1 -> file2.txt)
    let click = mouse_down(tree_rect.x + 2, tree_rect.y + 2, MouseButton::Left);
    app.handle_msg(Msg::Mouse(click));

    assert_eq!(app.focus, Focus::Tree);
    assert_eq!(app.tree.cursor, 1);
    assert_eq!(
        app.tree.selected_node().unwrap().path,
        Path::new("file2.txt")
    );
}

#[test]
fn test_gitignore_dimmed_and_omits_nested_descendants() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    // Create .git directory
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::write(root.join(".git/config"), "git config").unwrap();

    // Create .gitignore
    fs::write(root.join(".gitignore"), "target/\n*.log\n").unwrap();

    // Create non-ignored files
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();

    // Create ignored folder with nested contents
    fs::create_dir_all(root.join("target/debug/build")).unwrap();
    fs::write(root.join("target/debug/build/bin"), "binary").unwrap();

    // Create ignored file
    fs::write(root.join("error.log"), "error log").unwrap();

    let (tree, files) = Tree::walk(root);

    // .git must NEVER appear in tree.nodes
    assert!(
        tree.nodes.iter().all(|n| !n.path.starts_with(".git")),
        ".git must be completely excluded"
    );

    // target directory must appear one level deep with ignored: true
    let target_node = tree
        .nodes
        .iter()
        .find(|n| n.path == Path::new("target"))
        .expect("target should be present");
    assert!(target_node.is_dir);
    assert!(target_node.ignored, "target should be marked ignored");

    // Children of target must NOT appear in tree.nodes
    assert!(
        tree.nodes
            .iter()
            .all(|n| n.path == Path::new("target") || !n.path.starts_with("target")),
        "nested descendants of ignored directories must be omitted"
    );

    // error.log must appear with ignored: true
    let log_node = tree
        .nodes
        .iter()
        .find(|n| n.path == Path::new("error.log"))
        .expect("error.log present");
    assert!(!log_node.is_dir);
    assert!(log_node.ignored, "error.log should be marked ignored");

    // files Vec<PathBuf> must NOT contain ignored entries
    assert!(!files.contains(&PathBuf::from("target")));
    assert!(!files.contains(&PathBuf::from("error.log")));
    assert!(files.contains(&PathBuf::from("src/main.rs")));
    assert!(files.contains(&PathBuf::from(".gitignore")));
}

#[test]
fn test_viewport_scrolling_with_cursor_movement() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    for i in 0..20 {
        fs::write(root.join(format!("file_{i:02}.txt")), "content").unwrap();
    }

    let mut app = App::new(root.to_path_buf());
    let backend = TestBackend::new(60, 10); // 10 rows: 1 status, 9 pane -> 7 inner height
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|f| ui::draw(f, &mut app))
        .expect("draw failed");
    assert_eq!(app.tree.scroll, 0);

    // Move down 12 times
    for _ in 0..12 {
        app.handle_msg(Msg::Key(key_press(KeyCode::Char('j'))));
    }
    assert_eq!(app.tree.cursor, 12);

    // Draw again: adjust_scroll will update scroll so cursor is visible
    terminal
        .draw(|f| ui::draw(f, &mut app))
        .expect("draw failed");
    assert!(
        app.tree.scroll > 0,
        "scroll should have advanced past 0 (scroll={})",
        app.tree.scroll
    );
    let inner_height = 7;
    assert!(
        app.tree.cursor >= app.tree.scroll
            && app.tree.cursor < app.tree.scroll + inner_height,
        "cursor ({}) must be within [scroll, scroll+height) [{}, {})",
        app.tree.cursor,
        app.tree.scroll,
        app.tree.scroll + inner_height
    );
}

#[test]
fn test_arrow_keys_and_l_navigation() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    fs::create_dir_all(root.join("sub")).unwrap();
    fs::write(root.join("sub/item.txt"), "item").unwrap();
    fs::write(root.join("top.txt"), "top").unwrap();

    let mut app = App::new(root.to_path_buf());
    assert_eq!(app.tree.cursor, 0);

    // Down arrow
    app.handle_msg(Msg::Key(key_press(KeyCode::Down)));
    assert_eq!(app.tree.cursor, 1);

    // Up arrow
    app.handle_msg(Msg::Key(key_press(KeyCode::Up)));
    assert_eq!(app.tree.cursor, 0);

    // 'Right' expands directory 'sub'
    app.handle_msg(Msg::Key(key_press(KeyCode::Right)));
    assert!(app.tree.selected_node().unwrap().expanded);
    assert_eq!(app.tree.visible_nodes().len(), 3);

    // 'l' toggles expanded (collapses)
    app.handle_msg(Msg::Key(key_press(KeyCode::Char('l'))));
    assert!(!app.tree.selected_node().unwrap().expanded);
    assert_eq!(app.tree.visible_nodes().len(), 2);

    // 'Right' expands again
    app.handle_msg(Msg::Key(key_press(KeyCode::Right)));
    assert!(app.tree.selected_node().unwrap().expanded);

    // Move to item.txt
    app.handle_msg(Msg::Key(key_press(KeyCode::Down)));
    assert_eq!(app.tree.cursor, 1);
    assert_eq!(app.tree.selected_node().unwrap().path, Path::new("sub/item.txt"));

    // 'Left' jumps to parent 'sub'
    app.handle_msg(Msg::Key(key_press(KeyCode::Left)));
    assert_eq!(app.tree.cursor, 0);
    assert_eq!(app.tree.selected_node().unwrap().path, Path::new("sub"));

    // 'Left' collapses 'sub'
    app.handle_msg(Msg::Key(key_press(KeyCode::Left)));
    assert!(!app.tree.selected_node().unwrap().expanded);
}

#[test]
fn test_reveal_variations() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    fs::create_dir_all(root.join("x/y/z")).unwrap();
    fs::write(root.join("x/y/z/file.rs"), "code").unwrap();
    fs::write(root.join("root.txt"), "root").unwrap();

    let (mut tree, _) = Tree::walk(root);

    // Reveal root level file
    assert!(tree.reveal(Path::new("root.txt")));
    assert_eq!(tree.selected_node().unwrap().path, Path::new("root.txt"));

    // Collapse all
    tree.collapse_all();

    // Reveal with "./" prefix
    assert!(tree.reveal(Path::new("./x/y/z/file.rs")));
    assert_eq!(tree.selected_node().unwrap().path, Path::new("x/y/z/file.rs"));

    // Reveal with absolute path
    tree.collapse_all();
    let abs_path = root.join("x/y/z/file.rs");
    assert!(tree.reveal(&abs_path));
    assert_eq!(tree.selected_node().unwrap().path, Path::new("x/y/z/file.rs"));

    // Reveal non-existent file
    assert!(!tree.reveal(Path::new("does/not/exist.rs")));
}

#[test]
fn test_empty_workspace_directory() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    let (mut tree, files) = Tree::walk(root);
    assert!(tree.nodes.is_empty());
    assert!(files.is_empty());
    assert_eq!(tree.cursor, 0);
    assert_eq!(tree.selected_node(), None);

    // Navigation on empty tree should not panic
    tree.move_up();
    tree.move_down();
    tree.toggle_expanded();
    tree.collapse_or_parent();
    assert_eq!(tree.cursor, 0);

    let mut app = App::new(root.to_path_buf());
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|f| ui::draw(f, &mut app))
        .expect("draw failed");
}
