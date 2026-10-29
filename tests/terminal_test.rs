use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use nide::app::{App, Focus, Msg};
use nide::pty::{encode_key_to_xterm, TermId, TerminalPane};
use nide::{theme, ui};

#[test]
fn test_agent_ready_rendered_in_right_layout_with_green_foreground() {
    let (tx, rx) = mpsc::channel();
    let root = tempfile::tempdir().expect("tempdir failed");
    let mut app = App::with_sender(root.path().to_path_buf(), Some(tx.clone()));

    // Spawns TerminalPane with /bin/sh -c "printf '\033[32magent_ready\033[0m'"
    let term = TerminalPane::new_with_command(
        TermId::RightAgent,
        root.path().to_path_buf(),
        Some(tx),
        "/bin/sh",
        &["-c", "printf '\\033[32magent_ready\\033[0m'"],
    )
    .expect("spawn terminal pane");
    app.right_term = term;

    // Process Msg::PtyOutput
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut got_output = false;
    while Instant::now() < deadline {
        if let Ok(msg) = rx.recv_timeout(Duration::from_millis(50)) {
            let is_output = matches!(msg, Msg::PtyOutput { pane: TermId::RightAgent, .. });
            app.handle_msg(msg);
            if is_output {
                got_output = true;
                break;
            }
        }
    }
    assert!(got_output, "Timed out waiting for Msg::PtyOutput");

    // Render with TestBackend
    let width = 160;
    let height = 30;
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");

    // Assert "agent_ready" is rendered in app.layout.right with green foreground
    let right = app.layout.right.expect("right pane rect");
    let buffer = terminal.backend().buffer();

    let mut found = false;
    for y in right.y + 1..right.y + right.height - 1 {
        let mut row_chars = Vec::new();
        for x in right.x + 1..right.x + right.width - 1 {
            let sym = buffer[(x, y)].symbol();
            row_chars.push((x, sym));
        }
        let line_text: String = row_chars.iter().map(|(_, s)| *s).collect();
        if let Some(idx) = line_text.find("agent_ready") {
            let start_x = row_chars[idx].0;
            for offset in 0.."agent_ready".len() as u16 {
                let cell = &buffer[(start_x + offset, y)];
                assert_eq!(
                    cell.fg,
                    theme::GREEN,
                    "Expected cell at x={}, y={} to have green foreground (theme::GREEN), got {:?}",
                    start_x + offset,
                    y,
                    cell.fg
                );
            }
            found = true;
            break;
        }
    }
    assert!(found, "Could not find 'agent_ready' inside right pane buffer");
}

#[test]
fn test_cat_echoes_keystrokes_to_screen() {
    let (tx, rx) = mpsc::channel();
    let root = tempfile::tempdir().expect("tempdir failed");
    let mut term = TerminalPane::new_with_command(
        TermId::RightAgent,
        root.path().to_path_buf(),
        Some(tx),
        "/bin/sh",
        &["-c", "cat"],
    )
    .expect("spawn cat");

    // Send keystrokes: "cat_echo_test" followed by Enter
    for ch in "cat_echo_test".chars() {
        term.send_key(&KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE))
            .expect("send key");
    }
    term.send_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .expect("send enter");

    // Process output into vt100::Screen
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut echoed = false;
    while Instant::now() < deadline {
        if let Ok(Msg::PtyOutput { bytes, .. }) = rx.recv_timeout(Duration::from_millis(50)) {
            term.process_output(&bytes);
            let screen = term.screen();
            let (rows, cols) = (screen.size().0.get(), screen.size().1.get());
            for r in 0..rows {
                let mut line = String::new();
                for c in 0..cols {
                    if let Some(cell) = screen.cell(r, c) {
                        line.push_str(cell.contents());
                    }
                }
                if line.contains("cat_echo_test") {
                    echoed = true;
                    break;
                }
            }
            if echoed {
                break;
            }
        }
    }
    assert!(
        echoed,
        "Expected echoed output 'cat_echo_test' to appear in vt100::Screen"
    );
}

#[test]
fn test_process_exited_shows_banner_and_enter_respawns() {
    let (tx, rx) = mpsc::channel();
    let root = tempfile::tempdir().expect("tempdir failed");
    let mut app = App::with_sender(root.path().to_path_buf(), Some(tx.clone()));

    // Spawns a process that exits immediately
    let term = TerminalPane::new_with_command(
        TermId::RightAgent,
        root.path().to_path_buf(),
        Some(tx),
        "/bin/sh",
        &["-c", "exit 0"],
    )
    .expect("spawn exit process");
    app.right_term = term;

    // Process until Msg::PtyExited arrives
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut got_exit = false;
    while Instant::now() < deadline {
        if let Ok(msg) = rx.recv_timeout(Duration::from_millis(50)) {
            let is_exit = matches!(msg, Msg::PtyExited { pane: TermId::RightAgent });
            app.handle_msg(msg);
            if is_exit {
                got_exit = true;
                break;
            }
        }
    }
    assert!(got_exit, "Expected Msg::PtyExited");
    assert!(app.right_term.is_exited(), "Terminal should be marked exited");

    // Render with TestBackend
    let width = 160;
    let height = 30;
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");

    // Assert respawn banner is shown in right pane
    let right = app.layout.right.expect("right pane rect");
    let buffer = terminal.backend().buffer();
    let mut found_banner = false;
    for y in right.y + 1..right.y + right.height - 1 {
        let mut line = String::new();
        for x in right.x + 1..right.x + right.width - 1 {
            line.push_str(buffer[(x, y)].symbol());
        }
        if line.contains("[Process exited — press Enter to respawn]") {
            found_banner = true;
            break;
        }
    }
    assert!(
        found_banner,
        "Respawn banner '[Process exited — press Enter to respawn]' must be visible in right pane"
    );

    // Press Enter in Focus::RightTerm to respawn
    app.focus = Focus::RightTerm;
    app.handle_msg(Msg::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));

    // Assert process has respawned
    assert!(
        !app.right_term.is_exited(),
        "Pressing Enter in Focus::RightTerm must respawn the process"
    );
}

#[test]
fn test_terminal_resize_guard() {
    let root = tempfile::tempdir().expect("tempdir failed");
    let mut term = TerminalPane::new(TermId::RightAgent, root.path().to_path_buf(), None);
    assert_eq!(term.size, (24, 80));

    // Resizing with same dimensions does not change
    term.resize(24, 80);
    assert_eq!(term.size, (24, 80));

    // Resizing with different dimensions updates size and parser
    term.resize(40, 120);
    assert_eq!(term.size, (40, 120));
    assert_eq!(
        (term.screen().size().0.get(), term.screen().size().1.get()),
        (40, 120)
    );
}

#[test]
fn test_encode_key_to_xterm_coverage() {
    // Enter
    assert_eq!(
        encode_key_to_xterm(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Some(vec![b'\r'])
    );
    // Backspace
    assert_eq!(
        encode_key_to_xterm(&KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
        Some(vec![0x7f])
    );
    // Ctrl+C
    assert_eq!(
        encode_key_to_xterm(&KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        Some(vec![3])
    );
    // Tab
    assert_eq!(
        encode_key_to_xterm(&KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
        Some(vec![b'\t'])
    );
    // Esc
    assert_eq!(
        encode_key_to_xterm(&KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        Some(vec![0x1b])
    );
    // Arrows
    assert_eq!(
        encode_key_to_xterm(&KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
        Some(b"\x1b[A".to_vec())
    );
    assert_eq!(
        encode_key_to_xterm(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
        Some(b"\x1b[B".to_vec())
    );
}
