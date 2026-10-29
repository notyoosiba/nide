use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use nide::app::{App, Focus, Msg};
use nide::pty::{TermId, TerminalPane};
use nide::{theme, ui};

#[test]
fn test_terminal_mouse_drag_selection_and_smart_ctrl_c() {
    let root = tempfile::tempdir().expect("tempdir failed");
    let mut app = App::new(root.path().to_path_buf());

    // Render once to establish layout geometry
    let backend = TestBackend::new(160, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");

    let right = app.layout.right.expect("right pane rect");
    let inner_x = right.x + 1;
    let inner_y = right.y + 1;

    // Feed text into right terminal
    app.right_term.process_output(b"hello world\r\n");

    // Focus right terminal
    app.focus = Focus::RightTerm;

    // 1. Simulate mouse drag selection over "hello" (cols 0 to 4)
    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: inner_x,
        row: inner_y,
        modifiers: KeyModifiers::NONE,
    }));
    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: inner_x + 4,
        row: inner_y,
        modifiers: KeyModifiers::NONE,
    }));
    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: inner_x + 4,
        row: inner_y,
        modifiers: KeyModifiers::NONE,
    }));

    // Verify selection coordinates
    assert_eq!(app.right_term.selection, Some(((0, 0), (0, 4))));

    // Verify extracted text
    let selected = app.right_term.selected_text();
    assert_eq!(selected.as_deref(), Some("hello"));

    // Redraw and verify selection highlight in Catppuccin Latte
    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");
    let buffer = terminal.backend().buffer();
    for col in 0..=4 {
        let cell = &buffer[(inner_x + col, inner_y)];
        assert_eq!(
            cell.bg,
            theme::SELECTION_BG,
            "Cell at ({}, {}) should be highlighted with SELECTION_BG",
            inner_x + col,
            inner_y
        );
    }
    // Column 5 (" ") should not be selected
    assert_ne!(
        buffer[(inner_x + 5, inner_y)].bg,
        theme::SELECTION_BG,
        "Cell at ({}, {}) should not be highlighted",
        inner_x + 5,
        inner_y
    );

    // 2. Press Ctrl+C while selection is active
    app.right_term.last_written.clear();
    app.handle_msg(Msg::Key(KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    )));

    // Verify selection is cleared
    assert_eq!(app.right_term.selection, None);
    // Verify text copied to clipboard
    assert_eq!(app.clipboard.as_deref(), Some("hello"));
    // Verify \x03 was NOT sent to PTY
    assert!(
        app.right_term.last_written.is_empty(),
        "Ctrl+C with active selection should NOT send \\x03 to PTY"
    );

    // 3. Press Ctrl+C again with NO active selection
    app.handle_msg(Msg::Key(KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    )));
    // Verify \x03 (SIGINT) IS sent
    assert_eq!(
        app.right_term.last_written,
        vec![3],
        "Ctrl+C without selection should send \\x03 (SIGINT) to PTY"
    );
}

#[test]
fn test_terminal_mouse_wheel_scrollback_and_badge_in_border() {
    let root = tempfile::tempdir().expect("tempdir failed");
    let mut app = App::new(root.path().to_path_buf());

    let backend = TestBackend::new(160, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");

    let right = app.layout.right.expect("right pane rect");

    // Feed 60 lines so scrollback history exists
    for i in 1..=60 {
        app.right_term
            .process_output(format!("Log output line {i}\r\n").as_bytes());
    }

    // Scrollback starts at 0
    assert_eq!(app.right_term.screen().scrollback(), 0);

    // Simulate 5 mouse wheel scroll up events inside the terminal
    let mouse_x = right.x + 5;
    let mouse_y = right.y + 5;
    for _ in 0..5 {
        app.handle_msg(Msg::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: mouse_x,
            row: mouse_y,
            modifiers: KeyModifiers::NONE,
        }));
    }

    // Verify parser screen scrollback updated
    assert_eq!(
        app.right_term.screen().scrollback(),
        5,
        "Mouse wheel scroll up should advance scrollback to 5"
    );

    // Render with TestBackend and verify [+5 lines] badge appears in top border
    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");
    let buffer = terminal.backend().buffer();
    let mut border_title = String::new();
    for x in right.x..right.x + right.width {
        border_title.push_str(buffer[(x, right.y)].symbol());
    }
    assert!(
        border_title.contains("[+5 lines]"),
        "Top border must contain '[+5 lines]' badge, found: '{border_title}'"
    );

    // Any key event in RightTerm resets scrollback to 0
    app.focus = Focus::RightTerm;
    app.handle_msg(Msg::Key(KeyEvent::new(
        KeyCode::Char('k'),
        KeyModifiers::NONE,
    )));
    assert_eq!(
        app.right_term.screen().scrollback(),
        0,
        "Any key press must reset scrollback to 0"
    );

    // Redraw and confirm badge is gone
    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");
    let buffer = terminal.backend().buffer();
    let mut border_title_after = String::new();
    for x in right.x..right.x + right.width {
        border_title_after.push_str(buffer[(x, right.y)].symbol());
    }
    assert!(
        !border_title_after.contains("[+5 lines]"),
        "Badge must disappear after scrollback reset to 0"
    );
}

#[test]
fn test_bracketed_paste_wrapping() {
    let root = tempfile::tempdir().expect("tempdir failed");
    let mut app = App::new(root.path().to_path_buf());
    app.focus = Focus::RightTerm;

    // Initially bracketed paste is disabled
    assert!(!app.right_term.screen().bracketed_paste());
    app.handle_msg(Msg::Paste("plain text".to_string()));
    assert_eq!(app.right_term.last_written, b"plain text");

    // Enable bracketed paste via standard xterm sequence \x1b[?2004h
    app.right_term.process_output(b"\x1b[?2004h");
    assert!(app.right_term.screen().bracketed_paste());

    // Pasting when bracketed paste is enabled wraps in \x1b[200~ ... \x1b[201~
    app.handle_msg(Msg::Paste("bracketed text".to_string()));
    assert_eq!(
        app.right_term.last_written,
        b"\x1b[200~bracketed text\x1b[201~"
    );

    // Disable bracketed paste via \x1b[?2004l
    app.right_term.process_output(b"\x1b[?2004l");
    assert!(!app.right_term.screen().bracketed_paste());

    app.handle_msg(Msg::Paste("plain again".to_string()));
    assert_eq!(app.right_term.last_written, b"plain again");
}

#[test]
fn test_macos_cmd_c_and_reverse_drag_selection() {
    let root = tempfile::tempdir().expect("tempdir failed");
    let mut app = App::new(root.path().to_path_buf());

    let backend = TestBackend::new(160, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");

    let right = app.layout.right.expect("right pane rect");
    let inner_x = right.x + 1;
    let inner_y = right.y + 1;

    app.right_term.process_output(b"super_copy_test\r\n");
    app.focus = Focus::RightTerm;

    // Reverse drag: click at col 14 ('t'), drag backwards to col 0 ('s')
    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: inner_x + 14,
        row: inner_y,
        modifiers: KeyModifiers::NONE,
    }));
    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: inner_x,
        row: inner_y,
        modifiers: KeyModifiers::NONE,
    }));
    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: inner_x,
        row: inner_y,
        modifiers: KeyModifiers::NONE,
    }));

    // Normalized selection covers the word
    assert_eq!(
        app.right_term.selected_text().as_deref(),
        Some("super_copy_test")
    );

    // Cmd+C (KeyModifiers::SUPER) copies and clears selection
    app.right_term.last_written.clear();
    app.handle_msg(Msg::Key(KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::SUPER,
    )));

    assert_eq!(app.right_term.selection, None);
    assert_eq!(app.clipboard.as_deref(), Some("super_copy_test"));
    assert!(app.right_term.last_written.is_empty());
}

#[test]
fn test_multiline_drag_selection() {
    let root = tempfile::tempdir().expect("tempdir failed");
    let mut term = TerminalPane::new(TermId::RightAgent, root.path().to_path_buf(), None);

    term.process_output(b"alpha\r\nbeta\r\ngamma\r\n");

    // Select from row 0, col 2 ('p') to row 2, col 2 ('m')
    term.selection = Some(((0, 2), (2, 2)));

    let text = term.selected_text().expect("selected text");
    assert_eq!(text, "pha\nbeta\ngam");
}

#[test]
fn test_deep_scrollback_exceeding_screen_height_and_resize_smaller() {
    let root = tempfile::tempdir().expect("tempdir failed");
    let mut app = App::new(root.path().to_path_buf());

    let backend = TestBackend::new(160, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");

    let right = app.layout.right.expect("right pane rect");
    let inner_x = right.x + 1;
    let inner_y = right.y + 1;

    // Feed 120 lines into RightTerm (screen height is 24..28)
    for i in 1..=120 {
        app.right_term
            .process_output(format!("Scrollback line {i:03}\r\n").as_bytes());
    }

    // Scroll up 55 times (exceeding screen height)
    for _ in 0..55 {
        app.handle_msg(Msg::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: inner_x,
            row: inner_y,
            modifiers: KeyModifiers::NONE,
        }));
    }

    assert_eq!(app.right_term.scrollback(), 55);

    // Draw via TestBackend - must NOT panic with integer underflow
    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");

    // Feed 10 more lines while scrolled back
    for i in 121..=130 {
        app.right_term
            .process_output(format!("New line {i:03}\r\n").as_bytes());
    }

    // Redraw - must NOT panic
    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");

    // Resize terminal pane smaller while scrolled back
    let backend_small = TestBackend::new(160, 12);
    let mut terminal_small = Terminal::new(backend_small).expect("create small terminal");
    terminal_small
        .draw(|f| ui::draw(f, &mut app))
        .expect("draw small");

    // Explicitly resize terminal pane to smaller height (e.g. 8 rows) while scrolled back
    app.right_term.resize(8, 40);
    terminal_small
        .draw(|f| ui::draw(f, &mut app))
        .expect("draw after resize");
}

#[test]
fn test_sgr_mouse_wheel_forwarding_and_shift_bypass() {
    let root = tempfile::tempdir().expect("tempdir failed");
    let mut app = App::new(root.path().to_path_buf());

    let backend = TestBackend::new(160, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");

    let right = app.layout.right.expect("right pane rect");
    let inner_x = right.x + 1;
    let inner_y = right.y + 1;

    // Feed lines so scrollback history exists
    for i in 1..=60 {
        app.right_term
            .process_output(format!("Output line {i}\r\n").as_bytes());
    }

    // Enable SGR mouse reporting mode
    app.right_term.process_output(b"\x1b[?1000h\x1b[?1006h");
    assert_ne!(
        app.right_term.screen().mouse_protocol_mode(),
        vt100::MouseProtocolMode::None
    );
    assert_eq!(
        app.right_term.screen().mouse_protocol_encoding(),
        vt100::MouseProtocolEncoding::Sgr
    );

    app.right_term.last_written.clear();

    // ScrollUp at (inner_x + 4, inner_y + 2) -> 1-based (col 5, row 3)
    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: inner_x + 4,
        row: inner_y + 2,
        modifiers: KeyModifiers::NONE,
    }));

    // Verify SGR button 64 sequence sent to PTY and host scrollback remains 0
    assert_eq!(app.right_term.last_written, b"\x1b[<64;5;3M");
    assert_eq!(
        app.right_term.scrollback(),
        0,
        "Host scrollback must remain 0 when mouse protocol is enabled"
    );

    // ScrollDown at the same coordinate
    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: inner_x + 4,
        row: inner_y + 2,
        modifiers: KeyModifiers::NONE,
    }));

    // Verify SGR button 65 sequence sent to PTY and host scrollback remains 0
    assert_eq!(app.right_term.last_written, b"\x1b[<65;5;3M");
    assert_eq!(
        app.right_term.scrollback(),
        0,
        "Host scrollback must remain 0 when mouse protocol is enabled"
    );

    // Shift+ScrollUp bypasses mouse reporting and modifies host scrollback
    app.right_term.last_written.clear();
    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: inner_x + 4,
        row: inner_y + 2,
        modifiers: KeyModifiers::SHIFT,
    }));

    assert!(
        app.right_term.last_written.is_empty(),
        "Shift+ScrollUp must not write mouse sequence to PTY"
    );
    assert_eq!(
        app.right_term.scrollback(),
        1,
        "Shift+ScrollUp must advance host scrollback"
    );

    // Shift+ScrollDown decrements host scrollback
    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: inner_x + 4,
        row: inner_y + 2,
        modifiers: KeyModifiers::SHIFT,
    }));
    assert_eq!(
        app.right_term.scrollback(),
        0,
        "Shift+ScrollDown must decrement host scrollback"
    );
}

#[test]
fn test_alternate_screen_cursor_scroll_and_application_cursor() {
    let root = tempfile::tempdir().expect("tempdir failed");
    let mut app = App::new(root.path().to_path_buf());

    let backend = TestBackend::new(160, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");

    let right = app.layout.right.expect("right pane rect");
    let inner_x = right.x + 1;
    let inner_y = right.y + 1;

    // Feed lines so scrollback history exists
    for i in 1..=60 {
        app.right_term
            .process_output(format!("Output line {i}\r\n").as_bytes());
    }

    // Enter alternate screen without mouse reporting
    app.right_term.process_output(b"\x1b[?1049h");
    assert!(app.right_term.screen().alternate_screen());
    assert_eq!(
        app.right_term.screen().mouse_protocol_mode(),
        vt100::MouseProtocolMode::None
    );
    assert!(!app.right_term.screen().application_cursor());

    app.right_term.last_written.clear();

    // Normal cursor mode: ScrollUp sends \x1b[A, ScrollDown sends \x1b[B
    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: inner_x,
        row: inner_y,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(app.right_term.last_written, b"\x1b[A");
    assert_eq!(app.right_term.scrollback(), 0);

    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: inner_x,
        row: inner_y,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(app.right_term.last_written, b"\x1b[B");
    assert_eq!(app.right_term.scrollback(), 0);

    // Set application cursor mode \x1b[?1h
    app.right_term.process_output(b"\x1b[?1h");
    assert!(app.right_term.screen().application_cursor());

    // Application cursor mode: ScrollUp sends \x1bOA, ScrollDown sends \x1bOB
    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: inner_x,
        row: inner_y,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(app.right_term.last_written, b"\x1bOA");
    assert_eq!(app.right_term.scrollback(), 0);

    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: inner_x,
        row: inner_y,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(app.right_term.last_written, b"\x1bOB");
    assert_eq!(app.right_term.scrollback(), 0);

    // Shift bypass does not write cursor escapes to PTY
    app.right_term.last_written.clear();
    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: inner_x,
        row: inner_y,
        modifiers: KeyModifiers::SHIFT,
    }));
    assert!(
        app.right_term.last_written.is_empty(),
        "Shift+ScrollUp on alternate screen must not send cursor escape to PTY"
    );
}

#[test]
fn test_default_and_utf8_mouse_protocol_encoding() {
    let root = tempfile::tempdir().expect("tempdir failed");
    let mut term = TerminalPane::new(TermId::RightAgent, root.path().to_path_buf(), None);

    // Enable Default mouse protocol mode (VT200 / mode 1000 without 1006)
    term.process_output(b"\x1b[?1000h");
    assert_ne!(
        term.screen().mouse_protocol_mode(),
        vt100::MouseProtocolMode::None
    );
    assert_eq!(
        term.screen().mouse_protocol_encoding(),
        vt100::MouseProtocolEncoding::Default
    );

    // ScrollUp at (col 5, row 3): btn 64 + 32 = 96 (0x60 '`'), col 5 + 32 = 37 ('%'), row 3 + 32 = 35 ('#')
    term.handle_mouse_scroll(true, 5, 3, KeyModifiers::NONE)
        .expect("handle scroll");
    assert_eq!(term.last_written, vec![0x1b, b'[', b'M', 96, 37, 35]);

    // ScrollDown at (col 5, row 3): btn 65 + 32 = 97 (0x61 'a'), col 5 + 32 = 37 ('%'), row 3 + 32 = 35 ('#')
    term.handle_mouse_scroll(false, 5, 3, KeyModifiers::NONE)
        .expect("handle scroll");
    assert_eq!(term.last_written, vec![0x1b, b'[', b'M', 97, 37, 35]);

    // Enable UTF-8 mouse protocol encoding \x1b[?1005h
    term.process_output(b"\x1b[?1005h");
    assert_eq!(
        term.screen().mouse_protocol_encoding(),
        vt100::MouseProtocolEncoding::Utf8
    );

    term.handle_mouse_scroll(true, 10, 4, KeyModifiers::NONE)
        .expect("handle scroll");
    assert_eq!(term.last_written, vec![0x1b, b'[', b'M', 96, 42, 36]);
}

#[test]
fn test_quake_term_mouse_wheel_forwarding() {
    let root = tempfile::tempdir().expect("tempdir failed");
    let mut app = App::new(root.path().to_path_buf());

    let backend = TestBackend::new(160, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");

    // Open Quake console
    app.handle_msg(Msg::Key(KeyEvent::new(
        KeyCode::Char('`'),
        KeyModifiers::ALT,
    )));
    terminal.draw(|f| ui::draw(f, &mut app)).expect("draw");

    let quake_rect = app.layout.quake.expect("quake rect");
    let inner_x = quake_rect.x + 1;
    let inner_y = quake_rect.y + 1;

    let quake = app.quake_term.as_mut().expect("quake term");
    quake.process_output(b"\x1b[?1000h\x1b[?1006h");
    quake.last_written.clear();

    // ScrollUp inside Quake terminal
    app.handle_msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: inner_x + 3,
        row: inner_y + 1,
        modifiers: KeyModifiers::NONE,
    }));

    let quake = app.quake_term.as_ref().expect("quake term");
    assert_eq!(quake.last_written, b"\x1b[<64;4;2M");
    assert_eq!(quake.scrollback(), 0);
}
