use std::io::{self, Read, Write};
use std::num::NonZeroU16;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};

use crate::app::Msg;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TermId {
    RightAgent,
    Quake,
}

#[derive(Clone, Debug)]
pub struct CommandConfig {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

impl CommandConfig {
    #[must_use]
    pub fn default_shell(cwd: PathBuf) -> Self {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        Self {
            program: shell,
            args: Vec::new(),
            cwd,
        }
    }

    #[must_use]
    pub fn custom(program: impl Into<String>, args: Vec<String>, cwd: PathBuf) -> Self {
        Self {
            program: program.into(),
            args,
            cwd,
        }
    }
}

pub struct TerminalPane {
    pub id: TermId,
    pub parser: vt100::Parser,
    pub size: (u16, u16),
    pub exited: bool,
    pub selection: Option<((u16, u16), (u16, u16))>,
    pub last_written: Vec<u8>,
    command: CommandConfig,
    tx: Option<mpsc::Sender<Msg>>,
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
}

impl TerminalPane {
    #[must_use]
    pub fn new(id: TermId, cwd: PathBuf, tx: Option<mpsc::Sender<Msg>>) -> Self {
        let command = CommandConfig::default_shell(cwd);
        let mut pane = Self {
            id,
            parser: vt100::Parser::new(
                NonZeroU16::new(24).expect("24 > 0"),
                NonZeroU16::new(80).expect("80 > 0"),
                1000,
            ),
            size: (24, 80),
            exited: false,
            selection: None,
            last_written: Vec::new(),
            command,
            tx,
            master: None,
            writer: None,
            child: None,
        };
        if pane.tx.is_some() {
            let _ = pane.spawn_process();
        }
        pane
    }

    pub fn new_with_command(
        id: TermId,
        cwd: PathBuf,
        tx: Option<mpsc::Sender<Msg>>,
        program: impl Into<String>,
        args: &[&str],
    ) -> anyhow::Result<Self> {
        let command = CommandConfig::custom(
            program,
            args.iter().map(|s| (*s).to_string()).collect(),
            cwd,
        );
        let mut pane = Self {
            id,
            parser: vt100::Parser::new(
                NonZeroU16::new(24).expect("24 > 0"),
                NonZeroU16::new(80).expect("80 > 0"),
                1000,
            ),
            size: (24, 80),
            exited: false,
            selection: None,
            last_written: Vec::new(),
            command,
            tx,
            master: None,
            writer: None,
            child: None,
        };
        pane.spawn_process()?;
        Ok(pane)
    }

    pub fn set_sender(&mut self, tx: mpsc::Sender<Msg>) {
        self.tx = Some(tx);
        if self.child.is_none() {
            let _ = self.spawn_process();
        }
    }

    pub fn spawn_process(&mut self) -> anyhow::Result<()> {
        self.kill();

        let pty_system = native_pty_system();
        let rows = self.size.0.max(1);
        let cols = self.size.1.max(1);
        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = CommandBuilder::new(&self.command.program);
        for arg in &self.command.args {
            cmd.arg(arg);
        }
        let cwd = if self.command.cwd.is_dir() {
            self.command.cwd.clone()
        } else {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        };
        cmd.cwd(cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");

        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);

        let writer = pair.master.take_writer()?;
        let mut reader = pair.master.try_clone_reader()?;

        if let Some(tx) = self.tx.clone() {
            let term_id = self.id;
            let builder = thread::Builder::new().name(format!("pty-reader-{term_id:?}"));
            let _ = builder.spawn(move || {
                let mut buf = [0u8; 4096];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            if tx
                                .send(Msg::PtyOutput {
                                    pane: term_id,
                                    bytes: buf[..n].to_vec(),
                                })
                                .is_err()
                            {
                                return;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let _ = tx.send(Msg::PtyExited { pane: term_id });
            });
        }

        self.child = Some(child);
        self.writer = Some(writer);
        self.master = Some(pair.master);
        self.exited = false;
        Ok(())
    }

    pub fn process_output(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    pub fn handle_exited(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.try_wait();
        }
        self.exited = true;
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        let rows = rows.max(1);
        let cols = cols.max(1);
        if self.size == (rows, cols) {
            return;
        }
        self.size = (rows, cols);
        let saved_scrollback = self.scrollback();
        if let (Some(r), Some(c)) = (NonZeroU16::new(rows), NonZeroU16::new(cols)) {
            self.parser.screen_mut().set_size(r, c);
        }
        if saved_scrollback > 0 {
            self.set_scrollback(saved_scrollback);
        }
        if let Some(master) = &self.master {
            let _ = master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.last_written = bytes.to_vec();
        if let Some(writer) = &mut self.writer {
            writer.write_all(bytes)?;
            writer.flush()?;
        }
        Ok(())
    }

    pub fn paste_text(&mut self, text: &str) -> io::Result<()> {
        if self.screen().bracketed_paste() {
            let wrapped = format!("\x1b[200~{text}\x1b[201~");
            self.write_bytes(wrapped.as_bytes())
        } else {
            self.write_bytes(text.as_bytes())
        }
    }

    pub fn send_key(&mut self, key: &KeyEvent) -> io::Result<()> {
        self.reset_scrollback();

        let is_ctrl_c = key.modifiers.contains(KeyModifiers::CONTROL)
            && (key.code == KeyCode::Char('c') || key.code == KeyCode::Char('C'));
        let is_cmd_c = key.modifiers.contains(KeyModifiers::SUPER)
            && (key.code == KeyCode::Char('c') || key.code == KeyCode::Char('C'));

        if is_ctrl_c || is_cmd_c {
            if self.selection.is_some() {
                if let Some(text) = self.selected_text() {
                    crate::app::edit::osc52_copy(&text);
                }
                self.selection = None;
                return Ok(());
            }
            return self.write_bytes(b"\x03");
        }

        if let Some(bytes) = encode_key_to_xterm(key) {
            self.write_bytes(&bytes)?;
        }
        Ok(())
    }

    pub fn scroll_up(&mut self, lines: usize) {
        let current = self.parser.screen().scrollback();
        self.parser
            .screen_mut()
            .set_scrollback(current.saturating_add(lines));
    }

    pub fn scroll_down(&mut self, lines: usize) {
        let current = self.parser.screen().scrollback();
        self.parser
            .screen_mut()
            .set_scrollback(current.saturating_sub(lines));
    }

    pub fn reset_scrollback(&mut self) {
        if self.parser.screen().scrollback() != 0 {
            self.parser.screen_mut().set_scrollback(0);
        }
    }

    #[must_use]
    pub fn scrollback(&self) -> usize {
        self.parser.screen().scrollback()
    }

    pub fn set_scrollback(&mut self, offset: usize) {
        self.parser.screen_mut().set_scrollback(offset);
    }

    #[must_use]
    pub fn normalized_selection(&self) -> Option<((u16, u16), (u16, u16))> {
        let (p1, p2) = self.selection?;
        if (p1.0, p1.1) <= (p2.0, p2.1) {
            Some((p1, p2))
        } else {
            Some((p2, p1))
        }
    }

    #[must_use]
    pub fn is_cell_selected(&self, row: u16, col: u16) -> bool {
        let Some(((s_row, s_col), (e_row, e_col))) = self.normalized_selection() else {
            return false;
        };
        if (s_row, s_col) == (e_row, e_col) {
            return false;
        }
        if row < s_row || row > e_row {
            return false;
        }
        if s_row == e_row {
            col >= s_col && col <= e_col
        } else if row == s_row {
            col >= s_col
        } else if row == e_row {
            col <= e_col
        } else {
            true
        }
    }

    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    #[must_use]
    pub fn selected_text(&self) -> Option<String> {
        let ((s_row, s_col), (e_row, e_col)) = self.normalized_selection()?;
        if (s_row, s_col) == (e_row, e_col) {
            return None;
        }

        let (rows, cols) = self.screen().size();
        let rows = rows.get();
        let cols = cols.get();
        if s_row >= rows {
            return None;
        }
        let start_row = s_row;
        let end_row = e_row.min(rows.saturating_sub(1));
        let start_col = s_col.min(cols);
        let end_col = e_col.saturating_add(1).min(cols);

        let contents = self
            .screen()
            .contents_between(start_row, start_col, end_row, end_col);
        if contents.is_empty() {
            None
        } else {
            Some(contents)
        }
    }

    pub fn respawn(&mut self) -> anyhow::Result<()> {
        self.parser = vt100::Parser::new(
            NonZeroU16::new(self.size.0.max(1)).expect("max(1) > 0"),
            NonZeroU16::new(self.size.1.max(1)).expect("max(1) > 0"),
            1000,
        );
        self.selection = None;
        self.last_written.clear();
        self.spawn_process()
    }

    pub fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.writer = None;
        self.master = None;
    }

    #[must_use]
    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    #[must_use]
    pub fn is_exited(&self) -> bool {
        self.exited
    }

    /// Returns the text content of the specified row on the terminal screen.
    #[must_use]
    pub fn row_text(&self, row: u16) -> String {
        let (rows, cols) = self.screen().size();
        if row >= rows.get() {
            return String::new();
        }
        self.screen()
            .rows(0, cols.get())
            .nth(row as usize)
            .unwrap_or_default()
    }

    /// Returns whether vt100 marked this row as wrapped to the next line.
    #[must_use]
    pub fn is_row_wrapped(&self, row: u16) -> bool {
        self.screen().row_wrapped(row)
    }

    /// Returns whether row continues onto the next line (via vt100 wrap or right-margin continuation).
    #[must_use]
    pub fn row_wraps_to_next(&self, row: u16) -> bool {
        let (rows, cols) = self.screen().size();
        crate::app::links::check_row_wraps(self.screen(), row, rows.get(), cols.get())
    }

    /// Returns the cell string at (row, col) on the screen.
    #[must_use]
    pub fn cell_contents(&self, row: u16, col: u16) -> String {
        self.screen()
            .cell(row, col)
            .map_or_else(String::new, |c| c.contents().to_string())
    }

    /// Handles mouse wheel scroll events inside this terminal pane.
    ///
    /// If Shift is not held:
    /// - If mouse protocol mode is active (`!= None`), encodes and sends SGR/Default/UTF8 mouse sequences.
    /// - Else if alternate screen is active, sends cursor Up/Down escape sequences.
    /// - Otherwise (or if Shift is held), scrolls the host scrollback buffer.
    pub fn handle_mouse_scroll(
        &mut self,
        up: bool,
        col_1based: u16,
        row_1based: u16,
        modifiers: KeyModifiers,
    ) -> io::Result<()> {
        let (rows, cols) = (self.size.0.max(1), self.size.1.max(1));
        let col = col_1based.clamp(1, cols);
        let row = row_1based.clamp(1, rows);

        if !modifiers.contains(KeyModifiers::SHIFT) {
            if self.screen().mouse_protocol_mode() != vt100::MouseProtocolMode::None {
                let mut btn: u16 = if up { 64 } else { 65 };
                if modifiers.contains(KeyModifiers::SHIFT) {
                    btn += 4;
                }
                if modifiers.contains(KeyModifiers::ALT) {
                    btn += 8;
                }
                if modifiers.contains(KeyModifiers::CONTROL) {
                    btn += 16;
                }

                match self.screen().mouse_protocol_encoding() {
                    vt100::MouseProtocolEncoding::Sgr => {
                        let seq = format!("\x1b[<{btn};{col};{row}M");
                        return self.write_bytes(seq.as_bytes());
                    }
                    vt100::MouseProtocolEncoding::Default => {
                        let b = (btn.saturating_add(32)).min(255) as u8;
                        let c = (col.saturating_add(32)).min(255) as u8;
                        let r = (row.saturating_add(32)).min(255) as u8;
                        return self.write_bytes(&[0x1b, b'[', b'M', b, c, r]);
                    }
                    vt100::MouseProtocolEncoding::Utf8 => {
                        let mut bytes = vec![0x1b, b'[', b'M'];
                        if let Some(ch) = char::from_u32(btn as u32 + 32) {
                            let mut buf = [0u8; 4];
                            bytes.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        if let Some(ch) = char::from_u32(col as u32 + 32) {
                            let mut buf = [0u8; 4];
                            bytes.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        if let Some(ch) = char::from_u32(row as u32 + 32) {
                            let mut buf = [0u8; 4];
                            bytes.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        return self.write_bytes(&bytes);
                    }
                }
            } else if self.screen().alternate_screen() {
                let seq = if up {
                    if self.screen().application_cursor() {
                        b"\x1bOA"
                    } else {
                        b"\x1b[A"
                    }
                } else if self.screen().application_cursor() {
                    b"\x1bOB"
                } else {
                    b"\x1b[B"
                };
                return self.write_bytes(seq);
            }
        }

        if up {
            self.scroll_up(1);
        } else {
            self.scroll_down(1);
        }
        Ok(())
    }
}

impl Drop for TerminalPane {
    fn drop(&mut self) {
        self.kill();
    }
}

#[must_use]
pub fn encode_key_to_xterm(key: &KeyEvent) -> Option<Vec<u8>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    match key.code {
        KeyCode::Char(c) => {
            if ctrl {
                match c {
                    'a'..='z' => Some(vec![(c as u8 - b'a') + 1]),
                    'A'..='Z' => Some(vec![(c as u8 - b'A') + 1]),
                    '@' | ' ' => Some(vec![0]),
                    '[' => Some(vec![27]),
                    '\\' => Some(vec![28]),
                    ']' => Some(vec![29]),
                    '^' => Some(vec![30]),
                    '_' => Some(vec![31]),
                    '?' => Some(vec![127]),
                    _ => {
                        let mut bytes = Vec::new();
                        let mut buf = [0u8; 4];
                        bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                        Some(bytes)
                    }
                }
            } else if alt {
                let mut bytes = vec![0x1b];
                let mut buf = [0u8; 4];
                bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                Some(bytes)
            } else {
                let mut bytes = Vec::new();
                let mut buf = [0u8; 4];
                bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                Some(bytes)
            }
        }
        KeyCode::Enter => Some(vec![b'\r']),
        KeyCode::Backspace => Some(vec![0x7f]),
        KeyCode::Tab => {
            if shift {
                Some(b"\x1b[Z".to_vec())
            } else {
                Some(vec![b'\t'])
            }
        }
        KeyCode::BackTab => Some(b"\x1b[Z".to_vec()),
        KeyCode::Esc => Some(vec![0x1b]),
        KeyCode::Up => Some(arrow_sequence('A', shift, alt, ctrl)),
        KeyCode::Down => Some(arrow_sequence('B', shift, alt, ctrl)),
        KeyCode::Right => Some(arrow_sequence('C', shift, alt, ctrl)),
        KeyCode::Left => Some(arrow_sequence('D', shift, alt, ctrl)),
        KeyCode::Home => Some(b"\x1b[H".to_vec()),
        KeyCode::End => Some(b"\x1b[F".to_vec()),
        KeyCode::PageUp => Some(b"\x1b[5~".to_vec()),
        KeyCode::PageDown => Some(b"\x1b[6~".to_vec()),
        KeyCode::Delete => Some(b"\x1b[3~".to_vec()),
        KeyCode::Insert => Some(b"\x1b[2~".to_vec()),
        KeyCode::F(1) => Some(b"\x1bOP".to_vec()),
        KeyCode::F(2) => Some(b"\x1bOQ".to_vec()),
        KeyCode::F(3) => Some(b"\x1bOR".to_vec()),
        KeyCode::F(4) => Some(b"\x1bOS".to_vec()),
        KeyCode::F(5) => Some(b"\x1b[15~".to_vec()),
        KeyCode::F(6) => Some(b"\x1b[17~".to_vec()),
        KeyCode::F(7) => Some(b"\x1b[18~".to_vec()),
        KeyCode::F(8) => Some(b"\x1b[19~".to_vec()),
        KeyCode::F(9) => Some(b"\x1b[20~".to_vec()),
        KeyCode::F(10) => Some(b"\x1b[21~".to_vec()),
        KeyCode::F(11) => Some(b"\x1b[23~".to_vec()),
        KeyCode::F(12) => Some(b"\x1b[24~".to_vec()),
        _ => None,
    }
}

fn arrow_sequence(dir: char, shift: bool, alt: bool, ctrl: bool) -> Vec<u8> {
    let modifier_code = match (shift, alt, ctrl) {
        (false, false, false) => 0,
        (true, false, false) => 2,
        (false, true, false) => 3,
        (true, true, false) => 4,
        (false, false, true) => 5,
        (true, false, true) => 6,
        (false, true, true) => 7,
        (true, true, true) => 8,
    };
    if modifier_code == 0 {
        format!("\x1b[{dir}").into_bytes()
    } else {
        format!("\x1b[1;{modifier_code}{dir}").into_bytes()
    }
}
