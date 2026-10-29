use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc};

pub mod collapse;
pub mod conflict;
pub mod definition;
pub mod edit;
pub mod find;
pub mod links;
pub mod picker;
pub mod tree;
pub mod usages;

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
pub use edit::MiddleMode;
pub use find::FindState;
pub use tree::{TreeClipboardOp, TreePrompt, TreePromptKind};
use ratatui::layout::Rect;

use crate::buffer::Buffer;
use crate::pty::{TermId, TerminalPane};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Tree,
    Middle,
    RightTerm,
    QuakeTerm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragBorder {
    TreeMiddle,
    MiddleRight,
    TreeRight,
    Quake,
}

impl Focus {
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Focus::Tree => Focus::Middle,
            Focus::Middle => Focus::RightTerm,
            Focus::RightTerm => Focus::Tree,
            Focus::QuakeTerm => Focus::Tree,
        }
    }
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaneLayout {
    pub tree: Option<Rect>,
    pub middle: Option<Rect>,
    pub right: Option<Rect>,
    pub quake: Option<Rect>,
    pub status: Rect,
}

impl PaneLayout {
    #[must_use]
    pub fn pane_at(&self, col: u16, row: u16) -> Option<Focus> {
        if self.quake.is_some_and(|r| rect_contains(r, col, row)) {
            Some(Focus::QuakeTerm)
        } else if self.tree.is_some_and(|r| rect_contains(r, col, row)) {
            Some(Focus::Tree)
        } else if self.middle.is_some_and(|r| rect_contains(r, col, row)) {
            Some(Focus::Middle)
        } else if self.right.is_some_and(|r| rect_contains(r, col, row)) {
            Some(Focus::RightTerm)
        } else {
            None
        }
    }
}

#[must_use]
pub fn rect_contains(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

#[must_use]
pub fn compute_terminal_inner_coords(rect: Rect, mouse_col: u16, mouse_row: u16) -> (u16, u16) {
    let inner_x = rect.x + 1;
    let inner_y = rect.y + 1;
    let inner_w = rect.width.saturating_sub(2);
    let inner_h = rect.height.saturating_sub(2);
    let col = if mouse_col >= inner_x {
        (mouse_col - inner_x + 1).min(inner_w.max(1))
    } else {
        1
    };
    let row = if mouse_row >= inner_y {
        (mouse_row - inner_y + 1).min(inner_h.max(1))
    } else {
        1
    };
    (col, row)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Msg {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Paste(String),
    Resize(u16, u16),
    PtyOutput { pane: TermId, bytes: Vec<u8> },
    PtyExited { pane: TermId },
    Fs(notify::Event),
    ProjectWalked(crate::tree::Tree, Vec<PathBuf>),
    Marks(std::collections::HashMap<PathBuf, char>),
    DiffDone {
        seq: u64,
        path: PathBuf,
        diff: crate::git::Diff,
    },
    SearchDone {
        seq: u64,
        rows: Vec<crate::picker::PickItem>,
    },
    MediaRendered {
        seq: u64,
        result: Result<crate::viewer::MediaData, String>,
    },
    Quit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    pub path: PathBuf,
    pub line: usize,
    pub col: usize,
}

pub type JumpLocation = Location;

impl Location {
    #[must_use]
    pub fn new(path: PathBuf, line: usize, col: usize) -> Self {
        Self { path, line, col }
    }
}

impl From<(PathBuf, usize, usize)> for Location {
    fn from((path, line, col): (PathBuf, usize, usize)) -> Self {
        Self { path, line, col }
    }
}

impl From<Location> for (PathBuf, usize, usize) {
    fn from(loc: Location) -> Self {
        (loc.path, loc.line, loc.col)
    }
}

impl PartialEq<Path> for Location {
    fn eq(&self, other: &Path) -> bool {
        self.path == other
    }
}

impl PartialEq<PathBuf> for Location {
    fn eq(&self, other: &PathBuf) -> bool {
        self.path == *other
    }
}

impl PartialEq<str> for Location {
    fn eq(&self, other: &str) -> bool {
        self.path.to_string_lossy() == other || self.path.ends_with(other)
    }
}

impl PartialEq<&str> for Location {
    fn eq(&self, other: &&str) -> bool {
        self.path.to_string_lossy() == *other || self.path.ends_with(*other)
    }
}

impl PartialEq<(PathBuf, usize, usize)> for Location {
    fn eq(&self, other: &(PathBuf, usize, usize)) -> bool {
        self.path == other.0 && self.line == other.1 && self.col == other.2
    }
}

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct JumpHistory {
    pub past: Vec<Location>,
    pub future: Vec<Location>,
}

impl JumpHistory {
    pub fn push(&mut self, loc: impl Into<Location>) {
        let loc = loc.into();
        if self.past.last() == Some(&loc) {
            return;
        }
        self.past.push(loc);
        self.future.clear();
        if self.past.len() > 100 {
            self.past.remove(0);
        }
    }

    pub fn pop(&mut self) -> Option<Location> {
        self.past.pop()
    }

    pub fn clear(&mut self) {
        self.past.clear();
        self.future.clear();
    }
}

impl std::ops::Deref for JumpHistory {
    type Target = [Location];

    fn deref(&self) -> &Self::Target {
        &self.past
    }
}

impl<'a> IntoIterator for &'a JumpHistory {
    type Item = &'a Location;
    type IntoIter = std::slice::Iter<'a, Location>;

    fn into_iter(self) -> Self::IntoIter {
        self.past.iter()
    }
}

impl IntoIterator for JumpHistory {
    type Item = Location;
    type IntoIter = std::vec::IntoIter<Location>;

    fn into_iter(self) -> Self::IntoIter {
        self.past.into_iter()
    }
}

pub struct App {
    pub root_dir: PathBuf,
    pub focus: Focus,
    pub prev_focus: Option<Focus>,
    pub layout: PaneLayout,
    pub should_quit: bool,
    pub tree: crate::tree::Tree,
    pub files: Vec<PathBuf>,
    pub buf: Option<Buffer>,
    pub line: usize,
    pub col: usize,
    pub middle_mode: MiddleMode,
    pub dirty: bool,
    pub right_term: TerminalPane,
    pub tx: Option<mpsc::Sender<Msg>>,
    pub walk_in_flight: Arc<AtomicBool>,
    pub marks_in_flight: Arc<AtomicBool>,
    pub git_marks: std::collections::HashMap<PathBuf, char>,
    pub git_dir_marks: std::collections::HashMap<PathBuf, char>,
    pub watcher: Option<crate::live::FsWatcher>,
    pub show_tree: bool,
    pub show_middle: bool,
    pub show_right: bool,
    pub show_quake: bool,
    pub quake_term: Option<TerminalPane>,
    pub show_help: bool,
    pub widths: [u16; 3],
    pub tree_weight: u32,
    pub middle_weight: u32,
    pub right_weight: u32,
    pub autosave_delay: std::time::Duration,
    pub last_edit: Option<std::time::Instant>,
    pub tree_clipboard: Option<tree::TreeClipboardOp>,
    pub drag_border: Option<DragBorder>,
    pub quake_height: Option<u16>,
    pub selection: Option<edit::Selection>,
    pub clipboard: Option<String>,
    pub last_osc52: Option<String>,
    pub diff: Option<crate::git::Diff>,
    pub diff_seq: u64,
    pub conflict: Option<conflict::ConflictSession>,
    pub status_message: Option<String>,
    pub tree_prompt: Option<tree::TreePrompt>,
    pub find: Option<FindState>,
    pub picker: Option<crate::picker::Picker>,
    pub recent_files: Vec<PathBuf>,
    pub search_seq: u64,
    pub history: JumpHistory,
    pub media_seq: u64,
    pub active_image_id: Option<u32>,
    pub media_state: Option<crate::viewer::MediaState>,
    pub last_kitty_sequence: Option<String>,
    pub last_kitty_delete: Option<String>,
    pub kitty_erased: bool,
    pub graphics_protocol: crate::viewer::GraphicsProtocol,
    pub last_iterm2_sequence: Option<String>,
    pub last_sixel_sequence: Option<String>,
    pub preview_mode: bool,
    pub preview_cursor: usize,
    pub preview_scroll: usize,
    pub preview_lines: Vec<crate::markdown::FormattedLine>,
    pub preview_width: usize,
}

impl App {
    pub fn new(root_dir: PathBuf) -> Self {
        Self::with_sender(root_dir, None)
    }

    pub fn with_sender(root_dir: PathBuf, tx: Option<std::sync::mpsc::Sender<Msg>>) -> Self {
        let (tree, files) = crate::tree::Tree::walk(&root_dir);
        let right_term = TerminalPane::new(TermId::RightAgent, root_dir.clone(), tx.clone());
        let cfg = crate::config::Config::load();
        let initial_widths = cfg.initial_widths();
        let autosave_delay = cfg.autosave_delay();
        let mut app = Self {
            root_dir,
            focus: Focus::Tree,
            prev_focus: None,
            layout: PaneLayout::default(),
            should_quit: false,
            tree,
            files,
            buf: None,
            line: 0,
            col: 0,
            middle_mode: MiddleMode::Normal,
            dirty: false,
            right_term,
            tx,
            walk_in_flight: Arc::new(AtomicBool::new(false)),
            marks_in_flight: Arc::new(AtomicBool::new(false)),
            git_marks: std::collections::HashMap::new(),
            git_dir_marks: std::collections::HashMap::new(),
            watcher: None,
            show_tree: true,
            show_middle: true,
            show_right: true,
            show_quake: false,
            quake_term: None,
            show_help: false,
            widths: initial_widths,
            autosave_delay,
            last_edit: None,
            tree_clipboard: None,
            drag_border: None,
            quake_height: None,
            tree_weight: initial_widths[0] as u32,
            middle_weight: initial_widths[1] as u32,
            right_weight: initial_widths[2] as u32,
            selection: None,
            clipboard: None,
            last_osc52: None,
            diff: None,
            diff_seq: 0,
            conflict: None,
            status_message: None,
            tree_prompt: None,
            find: None,
            picker: None,
            recent_files: Vec::new(),
            search_seq: 0,
            history: JumpHistory::default(),
            media_seq: 0,
            active_image_id: None,
            media_state: None,
            last_kitty_sequence: None,
            last_kitty_delete: None,
            kitty_erased: false,
            graphics_protocol: crate::viewer::detect_graphics_protocol(),
            last_iterm2_sequence: None,
            last_sixel_sequence: None,
            preview_mode: false,
            preview_cursor: 0,
            preview_scroll: 0,
            preview_lines: Vec::new(),
            preview_width: 0,
        };
        app.trigger_git_marks();
        app
    }

    #[must_use]
    pub fn with_config(mut self, cfg: crate::config::Config) -> Self {
        let initial_widths = cfg.initial_widths();
        self.autosave_delay = cfg.autosave_delay();
        self.widths = initial_widths;
        self.tree_weight = initial_widths[0] as u32;
        self.middle_weight = initial_widths[1] as u32;
        self.right_weight = initial_widths[2] as u32;
        self
    }

    pub fn record_recent_file(&mut self, path: PathBuf) {
        let rel_path = path
            .strip_prefix(&self.root_dir)
            .map(|p| p.to_path_buf())
            .unwrap_or(path);
        if let Some(pos) = self.recent_files.iter().position(|p| p == &rel_path) {
            self.recent_files.remove(pos);
        }
        self.recent_files.insert(0, rel_path);
        if self.recent_files.len() > 50 {
            self.recent_files.truncate(50);
        }
    }

    pub fn set_pty_sender(&mut self, tx: std::sync::mpsc::Sender<Msg>) {
        self.tx = Some(tx.clone());
        self.right_term.set_sender(tx.clone());
        if let Some(quake) = &mut self.quake_term {
            quake.set_sender(tx);
        }
        self.trigger_git_marks();
        self.trigger_diff();
    }

    pub fn start_watcher(&mut self) -> Result<(), notify::Error> {
        if let Some(tx) = &self.tx {
            let watcher = crate::live::start_watcher(&self.root_dir, tx.clone())?;
            self.watcher = Some(watcher);
        }
        Ok(())
    }

    pub fn open_file(&mut self, path: impl AsRef<Path>) -> std::io::Result<()> {
        self.open_file_at(path, 0, 0)
    }

    pub fn open_file_at(
        &mut self,
        path: impl AsRef<Path>,
        line: usize,
        col: usize,
    ) -> std::io::Result<()> {
        let path = path.as_ref();
        if self.middle_mode == MiddleMode::Conflict {
            self.status_message =
                Some("Cannot switch files: resolve merge conflicts first (press l/r/b)".into());
            return Ok(());
        }

        let full_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root_dir.join(path)
        };

        if let Some(buf) = &mut self.buf {
            if buf.path == full_path {
                buf.cursor_row = line;
                buf.cursor_col = col;
                buf.last_cursor = None;
                buf.clamp_cursor();
                let visible_height = self
                    .layout
                    .middle
                    .map_or(24, |r| r.height.saturating_sub(2) as usize);
                buf.adjust_scroll(visible_height);

                self.line = buf.cursor_row;
                self.col = buf.cursor_col;
                self.show_middle = true;
                self.selection = None;
                self.find = None;
                self.preview_mode = false;
                self.preview_lines.clear();
                self.preview_cursor = 0;
                self.preview_scroll = 0;
                self.set_focus(Focus::Middle);
                self.tree.reveal(path);
                self.record_recent_file(full_path.clone());
                if crate::viewer::is_supported_media(&full_path) {
                    self.middle_mode = MiddleMode::View;
                    if self.media_state.is_none() {
                        self.trigger_media_decode(&full_path);
                    }
                }
                return Ok(());
            }
        }

        if self.is_dirty() {
            let _ = self.save_active_buffer();
        }

        let is_media = crate::viewer::is_supported_media(&full_path);
        if !is_media {
            if let Some(id) = self.active_image_id {
                crate::viewer::emit_kitty_delete(self, id);
                self.active_image_id = None;
                self.media_state = None;
            }
        }

        let mut buf = Buffer::load(&full_path)?;
        buf.cursor_row = line;
        buf.cursor_col = col;
        buf.clamp_cursor();
        let visible_height = self
            .layout
            .middle
            .map_or(24, |r| r.height.saturating_sub(2) as usize);
        buf.adjust_scroll(visible_height);

        self.line = buf.cursor_row;
        self.col = buf.cursor_col;
        self.dirty = false;
        self.show_middle = true;
        self.selection = None;
        self.find = None;
        self.preview_mode = false;
        self.preview_lines.clear();
        self.preview_cursor = 0;
        self.preview_scroll = 0;
        self.set_focus(Focus::Middle);
        self.middle_mode = if is_media {
            MiddleMode::View
        } else if buf.is_binary {
            MiddleMode::Normal
        } else {
            MiddleMode::Edit
        };
        self.buf = Some(buf);
        self.conflict = None;
        self.status_message = None;
        self.diff = None;
        if is_media {
            self.trigger_media_decode(&full_path);
        } else {
            self.trigger_diff();
        }

        self.tree.reveal(path);
        self.record_recent_file(full_path.clone());

        if let Some(watcher) = &mut self.watcher {
            if let Some(parent) = full_path.parent() {
                if !parent.starts_with(&self.root_dir) {
                    let _ = watcher.watch_path(parent);
                }
            }
        }

        Ok(())
    }

    pub fn jump_back(&mut self) -> bool {
        let Some(prev) = self.history.past.pop() else {
            return false;
        };
        if let Some(buf) = &self.buf {
            self.history.future.push(Location::new(
                buf.path.clone(),
                buf.cursor_row,
                buf.cursor_col,
            ));
        }
        let is_media = crate::viewer::is_supported_media(&prev.path);
        let res = self.open_file_at(&prev.path, prev.line, prev.col);
        if !is_media {
            self.middle_mode = MiddleMode::Normal;
        }
        res.is_ok()
    }

    pub fn jump_forward(&mut self) -> bool {
        let Some(next) = self.history.future.pop() else {
            return false;
        };
        if let Some(buf) = &self.buf {
            self.history.past.push(Location::new(
                buf.path.clone(),
                buf.cursor_row,
                buf.cursor_col,
            ));
        }
        let is_media = crate::viewer::is_supported_media(&next.path);
        let res = self.open_file_at(&next.path, next.line, next.col);
        if !is_media {
            self.middle_mode = MiddleMode::Normal;
        }
        res.is_ok()
    }

    #[must_use]
    pub fn current_path(&self) -> Option<&Path> {
        self.buf.as_ref().map(|b| b.path.as_path())
    }

    pub fn sync_cursor(&mut self) {
        if let Some(buf) = &self.buf {
            self.line = buf.cursor_row;
            self.col = buf.cursor_col;
        }
    }

    pub fn update_preview(&mut self) {
        let width = self
            .layout
            .middle
            .map_or(80, |r| r.width.saturating_sub(6) as usize)
            .max(20);
        self.preview_width = width;
        if let Some(buf) = &self.buf {
            if let Some(fmt) = crate::markdown::detect_format(&buf.path) {
                let lines = crate::markdown::render_preview(&buf.lines, fmt, width);
                self.preview_lines = lines;
                let cur_row = buf.cursor_row;
                if let Some(idx) = self.preview_lines.iter().position(|l| l.source_line >= cur_row) {
                    self.preview_cursor = idx;
                } else {
                    self.preview_cursor = self.preview_lines.len().saturating_sub(1);
                }
            }
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty || self.buf.as_ref().is_some_and(|b| b.dirty)
    }

    pub fn save_active_buffer(&mut self) -> std::io::Result<()> {
        if let Some(buf) = &mut self.buf {
            if buf.dirty {
                buf.save()?;
                self.trigger_git_marks();
                self.trigger_diff();
            }
        }
        self.dirty = false;
        self.last_edit = None;
        Ok(())
    }

    pub fn mark_edited(&mut self) {
        self.dirty = true;
        if let Some(buf) = &mut self.buf {
            buf.dirty = true;
        }
        self.last_edit = Some(std::time::Instant::now());
    }

    pub fn tick(&mut self) {
        if self.is_dirty() && self.middle_mode != MiddleMode::Conflict {
            match self.last_edit {
                Some(last_edit) => {
                    if last_edit.elapsed() >= self.autosave_delay {
                        let _ = self.save_active_buffer();
                    }
                }
                None => {
                    self.last_edit = Some(std::time::Instant::now());
                }
            }
        }
    }

    #[must_use]
    pub fn needs_parse(&self) -> bool {
        self.buf.as_ref().is_some_and(|b| b.needs_parse())
    }

    pub fn parse_more(&mut self, timeout: std::time::Duration) -> bool {
        if let Some(buf) = &mut self.buf {
            buf.parse_more(timeout)
        } else {
            false
        }
    }

    #[must_use]
    pub fn is_visible(&self, pane: Focus) -> bool {
        match pane {
            Focus::Tree => self.show_tree,
            Focus::Middle => self.show_middle,
            Focus::RightTerm => self.show_right,
            Focus::QuakeTerm => self.show_quake,
        }
    }

    #[must_use]
    pub fn visible_panes(&self) -> Vec<Focus> {
        let mut panes = Vec::with_capacity(3);
        if self.show_tree {
            panes.push(Focus::Tree);
        }
        if self.show_middle {
            panes.push(Focus::Middle);
        }
        if self.show_right {
            panes.push(Focus::RightTerm);
        }
        panes
    }

    #[must_use]
    pub fn visible_count(&self) -> usize {
        usize::from(self.show_tree) + usize::from(self.show_middle) + usize::from(self.show_right)
    }

    #[must_use]
    pub fn compute_layout(&self, area: Rect) -> PaneLayout {
        use ratatui::layout::{Constraint, Direction, Layout};

        if area.width == 0 || area.height == 0 {
            return PaneLayout::default();
        }

        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(area);

        let main_area = vertical[0];
        let status_area = vertical[1];

        let mut visible_panes = Vec::new();
        if self.show_tree {
            visible_panes.push(Focus::Tree);
        }
        if self.show_middle {
            visible_panes.push(Focus::Middle);
        }
        if self.show_right {
            visible_panes.push(Focus::RightTerm);
        }

        if visible_panes.is_empty() {
            visible_panes.push(Focus::Middle);
        }

        let total_weight: u32 = visible_panes
            .iter()
            .map(|pane| match pane {
                Focus::Tree => self.widths[0] as u32,
                Focus::Middle => self.widths[1] as u32,
                Focus::RightTerm => self.widths[2] as u32,
                Focus::QuakeTerm => 0,
            })
            .sum();
        let total_weight = total_weight.max(1);

        let constraints: Vec<Constraint> = visible_panes
            .iter()
            .map(|pane| {
                let weight = match pane {
                    Focus::Tree => self.widths[0] as u32,
                    Focus::Middle => self.widths[1] as u32,
                    Focus::RightTerm => self.widths[2] as u32,
                    Focus::QuakeTerm => 0,
                };
                Constraint::Ratio(weight, total_weight)
            })
            .collect();

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(constraints)
            .split(main_area);

        let mut tree_area = None;
        let mut middle_area = None;
        let mut right_area = None;

        for (idx, &pane) in visible_panes.iter().enumerate() {
            match pane {
                Focus::Tree => tree_area = Some(chunks[idx]),
                Focus::Middle => middle_area = Some(chunks[idx]),
                Focus::RightTerm => right_area = Some(chunks[idx]),
                Focus::QuakeTerm => {}
            }
        }

        let quake_area = if self.show_quake {
            let default_h = (main_area.height * 35 / 100).max(1).min(main_area.height);
            let quake_height = self.quake_height.unwrap_or(default_h).max(3).min(main_area.height);
            let quake_y = main_area.y + main_area.height.saturating_sub(quake_height);
            Some(Rect::new(
                main_area.x,
                quake_y,
                main_area.width,
                quake_height,
            ))
        } else {
            None
        };

        PaneLayout {
            tree: tree_area,
            middle: middle_area,
            right: right_area,
            quake: quake_area,
            status: status_area,
        }
    }

    #[must_use]
    pub fn detect_vertical_border(&self, col: u16, row: u16) -> Option<DragBorder> {
        let tree = self.layout.tree;
        let middle = self.layout.middle;
        let right = self.layout.right;

        // Tree / Middle border
        if let (Some(t), Some(_m)) = (tree, middle) {
            let split = t.x + t.width;
            if (col == split || col == split.saturating_sub(1))
                && row >= t.y
                && row < t.y + t.height
            {
                return Some(DragBorder::TreeMiddle);
            }
        }

        // Middle / Right border
        if let (Some(m), Some(_r)) = (middle, right) {
            let split = m.x + m.width;
            if (col == split || col == split.saturating_sub(1))
                && row >= m.y
                && row < m.y + m.height
            {
                return Some(DragBorder::MiddleRight);
            }
        }

        // Tree / Right border (when middle is hidden)
        if middle.is_none() {
            if let (Some(t), Some(_r)) = (tree, right) {
                let split = t.x + t.width;
                if (col == split || col == split.saturating_sub(1))
                    && row >= t.y
                    && row < t.y + t.height
                {
                    return Some(DragBorder::TreeRight);
                }
            }
        }

        None
    }

    pub fn handle_border_drag(&mut self, col: u16, row: u16) {
        let Some(drag) = self.drag_border else {
            return;
        };

        const MIN_WIDTH: u16 = 15;

        match drag {
            DragBorder::TreeMiddle => {
                let (Some(tree), Some(middle)) = (self.layout.tree, self.layout.middle) else {
                    return;
                };
                let combined = tree.width + middle.width;
                if combined < MIN_WIDTH * 2 {
                    return;
                }
                let min_col = tree.x + MIN_WIDTH;
                let max_col = (tree.x + combined).saturating_sub(MIN_WIDTH);
                let clamped_col = col.clamp(min_col, max_col);

                let new_tree_w = clamped_col - tree.x;
                let new_middle_w = combined - new_tree_w;

                self.widths[0] = new_tree_w;
                self.widths[1] = new_middle_w;
                self.tree_weight = new_tree_w as u32;
                self.middle_weight = new_middle_w as u32;

                self.layout.tree = Some(Rect {
                    x: tree.x,
                    y: tree.y,
                    width: new_tree_w,
                    height: tree.height,
                });
                self.layout.middle = Some(Rect {
                    x: clamped_col,
                    y: middle.y,
                    width: new_middle_w,
                    height: middle.height,
                });
            }
            DragBorder::MiddleRight => {
                let (Some(middle), Some(right)) = (self.layout.middle, self.layout.right) else {
                    return;
                };
                let combined = middle.width + right.width;
                if combined < MIN_WIDTH * 2 {
                    return;
                }
                let min_col = middle.x + MIN_WIDTH;
                let max_col = (middle.x + combined).saturating_sub(MIN_WIDTH);
                let clamped_col = col.clamp(min_col, max_col);

                let new_middle_w = clamped_col - middle.x;
                let new_right_w = combined - new_middle_w;

                self.widths[1] = new_middle_w;
                self.widths[2] = new_right_w;
                self.middle_weight = new_middle_w as u32;
                self.right_weight = new_right_w as u32;

                self.layout.middle = Some(Rect {
                    x: middle.x,
                    y: middle.y,
                    width: new_middle_w,
                    height: middle.height,
                });
                self.layout.right = Some(Rect {
                    x: clamped_col,
                    y: right.y,
                    width: new_right_w,
                    height: right.height,
                });

                let inner_w = new_right_w.saturating_sub(2);
                let inner_h = right.height.saturating_sub(2);
                self.right_term.resize(inner_h, inner_w);
            }
            DragBorder::TreeRight => {
                let (Some(tree), Some(right)) = (self.layout.tree, self.layout.right) else {
                    return;
                };
                let combined = tree.width + right.width;
                if combined < MIN_WIDTH * 2 {
                    return;
                }
                let min_col = tree.x + MIN_WIDTH;
                let max_col = (tree.x + combined).saturating_sub(MIN_WIDTH);
                let clamped_col = col.clamp(min_col, max_col);

                let new_tree_w = clamped_col - tree.x;
                let new_right_w = combined - new_tree_w;

                self.widths[0] = new_tree_w;
                self.widths[2] = new_right_w;
                self.tree_weight = new_tree_w as u32;
                self.right_weight = new_right_w as u32;

                self.layout.tree = Some(Rect {
                    x: tree.x,
                    y: tree.y,
                    width: new_tree_w,
                    height: tree.height,
                });
                self.layout.right = Some(Rect {
                    x: clamped_col,
                    y: right.y,
                    width: new_right_w,
                    height: right.height,
                });

                let inner_w = new_right_w.saturating_sub(2);
                let inner_h = right.height.saturating_sub(2);
                self.right_term.resize(inner_h, inner_w);
            }
            DragBorder::Quake => {
                let Some(quake) = self.layout.quake else {
                    return;
                };
                let bottom_y = quake.y + quake.height;
                let min_h: u16 = 3;
                let max_h = bottom_y.saturating_sub(1);
                let desired_h = bottom_y.saturating_sub(row);
                let clamped_h = desired_h.clamp(min_h, max_h);
                let new_y = bottom_y - clamped_h;

                self.quake_height = Some(clamped_h);
                self.layout.quake = Some(Rect {
                    x: quake.x,
                    y: new_y,
                    width: quake.width,
                    height: clamped_h,
                });

                let inner_w = quake.width.saturating_sub(2);
                let inner_h = clamped_h.saturating_sub(2);
                if let Some(term) = &mut self.quake_term {
                    term.resize(inner_h, inner_w);
                }
            }
        }
    }

    pub fn set_focus(&mut self, new_focus: Focus) {
        if self.focus != new_focus {
            self.prev_focus = Some(self.focus);
            self.focus = new_focus;
            if new_focus != Focus::Tree {
                self.tree_prompt = None;
            }
        }
    }

    pub fn cycle_focus(&mut self) {
        let visible = self.visible_panes();
        if visible.is_empty() {
            return;
        }
        if let Some(pos) = visible.iter().position(|&p| p == self.focus) {
            let next = visible[(pos + 1) % visible.len()];
            self.set_focus(next);
        } else {
            self.set_focus(visible[0]);
        }
    }

    pub fn toggle_pane(&mut self, pane: Focus) {
        if pane == Focus::QuakeTerm {
            self.toggle_quake();
            return;
        }
        if self.is_visible(pane) {
            if self.visible_count() <= 1 {
                return;
            }
            match pane {
                Focus::Tree => self.show_tree = false,
                Focus::Middle => self.show_middle = false,
                Focus::RightTerm => self.show_right = false,
                Focus::QuakeTerm => {}
            }
            if self.focus == pane {
                if let Some(prev) = self.prev_focus {
                    if self.is_visible(prev) {
                        self.focus = prev;
                        self.prev_focus = None;
                        return;
                    }
                }
                for candidate in [Focus::Tree, Focus::Middle, Focus::RightTerm] {
                    if self.is_visible(candidate) {
                        self.focus = candidate;
                        self.prev_focus = None;
                        return;
                    }
                }
            }
        } else {
            match pane {
                Focus::Tree => self.show_tree = true,
                Focus::Middle => self.show_middle = true,
                Focus::RightTerm => self.show_right = true,
                Focus::QuakeTerm => {}
            }
        }
    }

    pub fn toggle_tree(&mut self) {
        self.toggle_pane(Focus::Tree);
    }

    pub fn toggle_middle(&mut self) {
        self.toggle_pane(Focus::Middle);
    }

    pub fn toggle_right(&mut self) {
        self.toggle_pane(Focus::RightTerm);
    }

    pub fn toggle_quake(&mut self) {
        if self.show_quake {
            self.show_quake = false;
            if self.focus == Focus::QuakeTerm {
                if let Some(prev) = self.prev_focus {
                    if self.is_visible(prev) && prev != Focus::QuakeTerm {
                        self.set_focus(prev);
                    } else if let Some(&first) = self.visible_panes().first() {
                        self.set_focus(first);
                    }
                } else if let Some(&first) = self.visible_panes().first() {
                    self.set_focus(first);
                }
            }
        } else {
            if self.quake_term.is_none() {
                self.quake_term = Some(TerminalPane::new(
                    TermId::Quake,
                    self.root_dir.clone(),
                    self.tx.clone(),
                ));
            }
            self.show_quake = true;
            self.set_focus(Focus::QuakeTerm);
        }
        crate::viewer::handle_overlay_collision(self);
    }

    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
        crate::viewer::handle_overlay_collision(self);
    }

    pub fn handle_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Quit => {
                if self.middle_mode == MiddleMode::Conflict {
                    self.status_message =
                        Some("Cannot quit: resolve merge conflicts first (press l/r/b)".into());
                    return;
                }
                let _ = self.save_active_buffer();
                self.should_quit = true;
            }
            Msg::Fs(event) => {
                self.handle_fs_event(event);
            }
            Msg::Marks(marks) => {
                self.marks_in_flight.store(false, std::sync::atomic::Ordering::SeqCst);
                self.git_dir_marks = crate::git::rollup_marks(&marks);
                self.git_marks = marks;
            }
            Msg::DiffDone { seq, path, diff } => {
                if seq == self.diff_seq && self.current_path().is_some_and(|p| p == path) {
                    self.diff = Some(diff);
                }
            }
            Msg::MediaRendered { seq, result } => {
                if seq != self.media_seq {
                    return;
                }
                match result {
                    Ok(data) => {
                        self.active_image_id = Some(data.id);
                        self.media_state = Some(crate::viewer::MediaState::Loaded(data));
                    }
                    Err(err) => {
                        let path = self
                            .buf
                            .as_ref()
                            .map(|b| b.path.clone())
                            .unwrap_or_default();
                        self.media_state = Some(crate::viewer::MediaState::Error {
                            seq,
                            path,
                            error: err,
                        });
                    }
                }
            }
            Msg::SearchDone { seq, rows } => {
                if seq == self.search_seq {
                    if let Some(picker) = &mut self.picker {
                        if picker.kind == crate::picker::PickerKind::Grep {
                            picker.items = rows;
                            picker.selected_idx = 0;
                            picker.scroll = 0;
                            picker.update_preview();
                        }
                    }
                }
            }
            Msg::ProjectWalked(tree, files) => {
                self.handle_project_walked(tree, files);
            }
            Msg::Key(key) => {
                if key.kind == KeyEventKind::Release {
                    return;
                }
                self.handle_key(key);
            }
            Msg::Mouse(mouse) => {
                self.handle_mouse(mouse);
            }
            Msg::Paste(text) => {
                if self.focus == Focus::RightTerm {
                    let _ = self.right_term.paste_text(&text);
                } else if self.focus == Focus::QuakeTerm {
                    if let Some(quake) = &mut self.quake_term {
                        let _ = quake.paste_text(&text);
                    }
                } else if self.focus == Focus::Middle {
                    edit::paste_text(self, &text);
                }
            }
            Msg::PtyOutput { pane, bytes } => match pane {
                TermId::RightAgent => self.right_term.process_output(&bytes),
                TermId::Quake => {
                    if let Some(term) = &mut self.quake_term {
                        term.process_output(&bytes);
                    }
                }
            },
            Msg::PtyExited { pane } => match pane {
                TermId::RightAgent => self.right_term.handle_exited(),
                TermId::Quake => {
                    self.show_quake = false;
                    self.quake_term = None;
                    if self.focus == Focus::QuakeTerm {
                        if let Some(prev) = self.prev_focus {
                            if self.is_visible(prev) && prev != Focus::QuakeTerm {
                                self.set_focus(prev);
                            } else if let Some(&first) = self.visible_panes().first() {
                                self.set_focus(first);
                            }
                        } else if let Some(&first) = self.visible_panes().first() {
                            self.set_focus(first);
                        }
                    }
                }
            },
            Msg::Resize(_width, _height) => {
                if let Some(buf) = &mut self.buf {
                    buf.last_cursor = None;
                }
                if self.preview_mode {
                    self.update_preview();
                }
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        let key = crate::key::normalize_key(key);

        if self.show_help
            && (key.code == KeyCode::Char('?')
                || key.code == KeyCode::Esc
                || key.code == KeyCode::Char('q'))
        {
            self.show_help = false;
            crate::viewer::handle_overlay_collision(self);
            return;
        }

        if self.picker.is_some() {
            crate::app::picker::handle_key(self, key);
            return;
        }

        // Global Ctrl+E opens fuzzy file picker modal
        if key.code == KeyCode::Char('e') && key.modifiers.contains(KeyModifiers::CONTROL) {
            crate::app::picker::open_file_picker(self);
            return;
        }

        // Global Ctrl+T opens fuzzy symbol search modal
        if key.code == KeyCode::Char('t') && key.modifiers.contains(KeyModifiers::CONTROL) {
            crate::app::picker::open_symbol_picker(self);
            return;
        }

        // Global Ctrl+C quits outside RightTerm, QuakeTerm and Middle
        if key.code == KeyCode::Char('c')
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && self.focus != Focus::RightTerm
            && self.focus != Focus::QuakeTerm
            && self.focus != Focus::Middle
        {
            if self.tree_prompt.is_some() {
                self.tree_prompt = None;
                return;
            }
            if self.middle_mode == MiddleMode::Conflict {
                self.status_message =
                    Some("Cannot quit: resolve merge conflicts first (press l/r/b)".into());
                return;
            }
            let _ = self.save_active_buffer();
            self.should_quit = true;
            return;
        }

        // Global pane visibility toggles (Alt+1, Alt+2, Alt+3, Alt+`)
        if key.modifiers.contains(KeyModifiers::ALT) {
            match key.code {
                KeyCode::Char('1') => {
                    self.toggle_tree();
                    return;
                }
                KeyCode::Char('2') => {
                    self.toggle_middle();
                    return;
                }
                KeyCode::Char('3') => {
                    self.toggle_right();
                    return;
                }
                KeyCode::Char('`' | '~') => {
                    self.toggle_quake();
                    return;
                }
                _ => {}
            }
        }

        // Global focus cycling escape keys: Alt+Tab, Alt+BackTab, or Ctrl+\
        let is_alt_tab = key.modifiers.contains(KeyModifiers::ALT)
            && (key.code == KeyCode::Tab || key.code == KeyCode::BackTab);
        let is_ctrl_backslash = key.modifiers.contains(KeyModifiers::CONTROL)
            && key.code == KeyCode::Char('\\');
        if is_alt_tab || is_ctrl_backslash {
            self.cycle_focus();
            return;
        }

        // Global ? toggles Help overlay when in Focus::Tree or MiddleMode::Normal or MiddleMode::View
        if key.code == KeyCode::Char('?')
            && !key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT)
            && ((self.focus == Focus::Tree && self.tree_prompt.is_none())
                || (self.focus == Focus::Middle
                    && (self.middle_mode == MiddleMode::Normal
                        || self.middle_mode == MiddleMode::View)))
        {
            self.show_help = true;
            crate::viewer::handle_overlay_collision(self);
            return;
        }

        // Navigation-mode pane toggles:
        // - Shift+1 (or !) -> Left/Tree pane
        // - Shift+2 (or @) -> Middle/Editor pane
        // - Shift+3 (or #) -> Right/Term pane
        // - ~ (Shift+` or ` or §) -> Quake console overlay
        // Active in Focus::Tree (without prompt), MiddleMode::Normal, and MiddleMode::View
        let is_navigation_mode = (self.focus == Focus::Tree && self.tree_prompt.is_none())
            || (self.focus == Focus::Middle
                && (self.middle_mode == MiddleMode::Normal
                    || self.middle_mode == MiddleMode::View));

        if is_navigation_mode
            && !self.show_help
            && !key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT)
        {
            let is_shift_1 = key.code == KeyCode::Char('!')
                || (key.code == KeyCode::Char('1') && key.modifiers.contains(KeyModifiers::SHIFT));
            let is_shift_2 = key.code == KeyCode::Char('@')
                || (key.code == KeyCode::Char('2') && key.modifiers.contains(KeyModifiers::SHIFT));
            let is_shift_3 = key.code == KeyCode::Char('#')
                || (key.code == KeyCode::Char('3') && key.modifiers.contains(KeyModifiers::SHIFT));
            let is_quake_toggle = key.code == KeyCode::Char('~')
                || (key.code == KeyCode::Char('`') && key.modifiers.contains(KeyModifiers::SHIFT))
                || key.code == KeyCode::Char('`')
                || key.code == KeyCode::Char('§');

            if is_shift_1 {
                self.toggle_tree();
                return;
            }
            if is_shift_2 {
                self.toggle_middle();
                return;
            }
            if is_shift_3 {
                self.toggle_right();
                return;
            }
            if is_quake_toggle {
                self.toggle_quake();
                return;
            }
        }

        match key.code {
            KeyCode::Tab => {
                // In MiddleMode::Edit, Tab inserts indentation instead of switching focus
                if self.focus == Focus::Middle && self.middle_mode == MiddleMode::Edit {
                    edit::handle_key(self, key);
                    return;
                }
                // In RightTerm or QuakeTerm, Tab is forwarded to the running terminal process
                if self.focus == Focus::RightTerm {
                    self.right_term.reset_scrollback();
                    let _ = self.right_term.send_key(&key);
                    return;
                }
                if self.focus == Focus::QuakeTerm {
                    if let Some(quake) = &mut self.quake_term {
                        quake.reset_scrollback();
                        let _ = quake.send_key(&key);
                    }
                    return;
                }
                self.cycle_focus();
                return;
            }
            KeyCode::Char('q')
                if self.focus != Focus::RightTerm && self.focus != Focus::QuakeTerm =>
            {
                if self.focus == Focus::Tree && self.tree_prompt.is_some() {
                    tree::handle_key(self, key);
                    return;
                }
                if self.middle_mode == MiddleMode::Conflict {
                    self.status_message =
                        Some("Cannot quit: resolve merge conflicts first (press l/r/b)".into());
                    return;
                }
                // In MiddleMode::Edit or MiddleMode::Find, 'q' is a character key to be inserted into the buffer or search query
                if self.focus == Focus::Middle
                    && (self.middle_mode == MiddleMode::Edit
                        || self.middle_mode == MiddleMode::Find)
                {
                    edit::handle_key(self, key);
                    return;
                }
                // In MiddleMode::Normal or Focus::Tree, 'q' auto-saves and quits
                let _ = self.save_active_buffer();
                self.should_quit = true;
                return;
            }
            _ => {}
        }

        // Focus-specific key handling
        match self.focus {
            Focus::Tree => {
                tree::handle_key(self, key);
            }
            Focus::Middle => {
                edit::handle_key(self, key);
            }
            Focus::RightTerm => {
                if self.right_term.is_exited() {
                    if key.code == KeyCode::Enter {
                        let _ = self.right_term.respawn();
                    }
                    return;
                }

                // Any key event resets scrollback to 0
                self.right_term.reset_scrollback();

                // Smart Ctrl+C / Cmd+C: copy selection or send SIGINT
                let is_ctrl_c = key.modifiers.contains(KeyModifiers::CONTROL)
                    && (key.code == KeyCode::Char('c') || key.code == KeyCode::Char('C'));
                let is_cmd_c = key.modifiers.contains(KeyModifiers::SUPER)
                    && (key.code == KeyCode::Char('c') || key.code == KeyCode::Char('C'));

                if is_ctrl_c || is_cmd_c {
                    if self.right_term.selection.is_some() {
                        if let Some(text) = self.right_term.selected_text() {
                            edit::copy_to_clipboard(self, text);
                        }
                        self.right_term.selection = None;
                        return;
                    }
                    let _ = self.right_term.write_bytes(b"\x03");
                    return;
                }

                // Ctrl+V / Cmd+V / Ctrl+P terminal paste
                let is_ctrl_v = key.modifiers.contains(KeyModifiers::CONTROL)
                    && (key.code == KeyCode::Char('v') || key.code == KeyCode::Char('V'));
                let is_cmd_v = key.modifiers.contains(KeyModifiers::SUPER)
                    && (key.code == KeyCode::Char('v') || key.code == KeyCode::Char('V'));
                let is_ctrl_p = key.modifiers.contains(KeyModifiers::CONTROL)
                    && (key.code == KeyCode::Char('p') || key.code == KeyCode::Char('P'));

                if is_ctrl_v || is_cmd_v || is_ctrl_p {
                    if let Some(text) = self.clipboard.clone() {
                        let _ = self.right_term.paste_text(&text);
                        return;
                    }
                }

                let _ = self.right_term.send_key(&key);
            }
            Focus::QuakeTerm => {
                let Some(quake) = &mut self.quake_term else {
                    return;
                };

                if quake.is_exited() {
                    if key.code == KeyCode::Enter {
                        let _ = quake.respawn();
                    }
                    return;
                }

                // Any key event resets scrollback to 0
                quake.reset_scrollback();

                // Smart Ctrl+C / Cmd+C: copy selection or send SIGINT
                let is_ctrl_c = key.modifiers.contains(KeyModifiers::CONTROL)
                    && (key.code == KeyCode::Char('c') || key.code == KeyCode::Char('C'));
                let is_cmd_c = key.modifiers.contains(KeyModifiers::SUPER)
                    && (key.code == KeyCode::Char('c') || key.code == KeyCode::Char('C'));

                if is_ctrl_c || is_cmd_c {
                    let maybe_text = if quake.selection.is_some() {
                        let text = quake.selected_text();
                        quake.selection = None;
                        text
                    } else {
                        None
                    };

                    if let Some(text) = maybe_text {
                        edit::copy_to_clipboard(self, text);
                        return;
                    }
                    if let Some(quake) = &mut self.quake_term {
                        let _ = quake.write_bytes(b"\x03");
                    }
                    return;
                }

                // Ctrl+V / Cmd+V / Ctrl+P terminal paste
                let is_ctrl_v = key.modifiers.contains(KeyModifiers::CONTROL)
                    && (key.code == KeyCode::Char('v') || key.code == KeyCode::Char('V'));
                let is_cmd_v = key.modifiers.contains(KeyModifiers::SUPER)
                    && (key.code == KeyCode::Char('v') || key.code == KeyCode::Char('V'));
                let is_ctrl_p = key.modifiers.contains(KeyModifiers::CONTROL)
                    && (key.code == KeyCode::Char('p') || key.code == KeyCode::Char('P'));

                if is_ctrl_v || is_cmd_v || is_ctrl_p {
                    if let Some(text) = self.clipboard.clone() {
                        let _ = quake.paste_text(&text);
                        return;
                    }
                }

                let _ = quake.send_key(&key);
            }
        }
        self.sync_cursor();
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        let is_ctrl_or_cmd = mouse.modifiers.contains(KeyModifiers::CONTROL)
            || mouse.modifiers.contains(KeyModifiers::SUPER);

        match mouse.kind {
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.drag_border.is_some() {
                    self.handle_border_drag(mouse.column, mouse.row);
                    self.sync_cursor();
                    return;
                }

                if self.focus == Focus::QuakeTerm {
                    if let Some(quake_rect) = self.layout.quake {
                        if let Some(quake) = &mut self.quake_term {
                            if quake.selection.is_some() {
                                let inner_x = quake_rect.x + 1;
                                let inner_y = quake_rect.y + 1;
                                let inner_w = quake_rect.width.saturating_sub(2);
                                let inner_h = quake_rect.height.saturating_sub(2);
                                if inner_w > 0 && inner_h > 0 {
                                    let clamped_x = mouse.column.clamp(inner_x, inner_x + inner_w - 1);
                                    let clamped_y = mouse.row.clamp(inner_y, inner_y + inner_h - 1);
                                    let term_col = clamped_x - inner_x;
                                    let term_row = clamped_y - inner_y;
                                    if let Some((anchor, _)) = quake.selection {
                                        quake.selection = Some((anchor, (term_row, term_col)));
                                    }
                                }
                            }
                        }
                    }
                } else if self.focus == Focus::RightTerm && self.right_term.selection.is_some() {
                    if let Some(right_rect) = self.layout.right {
                        let inner_x = right_rect.x + 1;
                        let inner_y = right_rect.y + 1;
                        let inner_w = right_rect.width.saturating_sub(2);
                        let inner_h = right_rect.height.saturating_sub(2);
                        if inner_w > 0 && inner_h > 0 {
                            let clamped_x = mouse.column.clamp(inner_x, inner_x + inner_w - 1);
                            let clamped_y = mouse.row.clamp(inner_y, inner_y + inner_h - 1);
                            let term_col = clamped_x - inner_x;
                            let term_row = clamped_y - inner_y;
                            if let Some((anchor, _)) = self.right_term.selection {
                                self.right_term.selection = Some((anchor, (term_row, term_col)));
                            }
                        }
                    }
                } else if self.layout.middle.is_some_and(|r| rect_contains(r, mouse.column, mouse.row)) {
                    edit::handle_mouse(self, mouse);
                }
                self.sync_cursor();
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if self.drag_border.take().is_some() {
                    self.sync_cursor();
                    return;
                }

                if self.focus == Focus::QuakeTerm {
                    if let Some(quake_rect) = self.layout.quake {
                        if let Some(quake) = &mut self.quake_term {
                            if let Some((anchor, current)) = quake.selection {
                                let inner_x = quake_rect.x + 1;
                                let inner_y = quake_rect.y + 1;
                                let inner_w = quake_rect.width.saturating_sub(2);
                                let inner_h = quake_rect.height.saturating_sub(2);
                                let final_pos = if inner_w > 0 && inner_h > 0 {
                                    let clamped_x = mouse.column.clamp(inner_x, inner_x + inner_w - 1);
                                    let clamped_y = mouse.row.clamp(inner_y, inner_y + inner_h - 1);
                                    (clamped_y - inner_y, clamped_x - inner_x)
                                } else {
                                    current
                                };
                                if anchor == final_pos {
                                    quake.selection = None;
                                } else {
                                    quake.selection = Some((anchor, final_pos));
                                }
                            }
                        }
                    }
                } else if self.focus == Focus::RightTerm && self.right_term.selection.is_some() {
                    if let Some(right_rect) = self.layout.right {
                        if let Some((anchor, current)) = self.right_term.selection {
                            let inner_x = right_rect.x + 1;
                            let inner_y = right_rect.y + 1;
                            let inner_w = right_rect.width.saturating_sub(2);
                            let inner_h = right_rect.height.saturating_sub(2);
                            let final_pos = if inner_w > 0 && inner_h > 0 {
                                let clamped_x = mouse.column.clamp(inner_x, inner_x + inner_w - 1);
                                let clamped_y = mouse.row.clamp(inner_y, inner_y + inner_h - 1);
                                (clamped_y - inner_y, clamped_x - inner_x)
                            } else {
                                current
                            };
                            if anchor == final_pos {
                                self.right_term.selection = None;
                            } else {
                                self.right_term.selection = Some((anchor, final_pos));
                            }
                        }
                    }
                } else if self.layout.middle.is_some_and(|r| rect_contains(r, mouse.column, mouse.row)) {
                    edit::handle_mouse(self, mouse);
                }
                self.sync_cursor();
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(quake_rect) = self.layout.quake {
                    if mouse.row == quake_rect.y
                        && mouse.column >= quake_rect.x
                        && mouse.column < quake_rect.x + quake_rect.width
                    {
                        self.drag_border = Some(DragBorder::Quake);
                        return;
                    }
                }

                if let Some(border) = self.detect_vertical_border(mouse.column, mouse.row) {
                    self.drag_border = Some(border);
                    return;
                }

                if let Some(quake_rect) = self.layout.quake {
                    if rect_contains(quake_rect, mouse.column, mouse.row) {
                        if is_ctrl_or_cmd {
                            self.handle_quake_ctrl_click(mouse.column, mouse.row);
                            return;
                        }
                        self.set_focus(Focus::QuakeTerm);
                        let inner_x = quake_rect.x + 1;
                        let inner_y = quake_rect.y + 1;
                        let inner_w = quake_rect.width.saturating_sub(2);
                        let inner_h = quake_rect.height.saturating_sub(2);
                        if let Some(quake) = &mut self.quake_term {
                            if mouse.column >= inner_x
                                && mouse.column < inner_x + inner_w
                                && mouse.row >= inner_y
                                && mouse.row < inner_y + inner_h
                            {
                                let term_col = mouse.column - inner_x;
                                let term_row = mouse.row - inner_y;
                                quake.selection =
                                    Some(((term_row, term_col), (term_row, term_col)));
                            } else {
                                quake.selection = None;
                            }
                        }
                        self.right_term.selection = None;
                        self.sync_cursor();
                        return;
                    }
                }

                if let Some(right_rect) = self.layout.right {
                    if rect_contains(right_rect, mouse.column, mouse.row) {
                        if is_ctrl_or_cmd {
                            self.handle_terminal_ctrl_click(mouse.column, mouse.row);
                            return;
                        }
                        self.set_focus(Focus::RightTerm);
                        let inner_x = right_rect.x + 1;
                        let inner_y = right_rect.y + 1;
                        let inner_w = right_rect.width.saturating_sub(2);
                        let inner_h = right_rect.height.saturating_sub(2);
                        if mouse.column >= inner_x
                            && mouse.column < inner_x + inner_w
                            && mouse.row >= inner_y
                            && mouse.row < inner_y + inner_h
                        {
                            let term_col = mouse.column - inner_x;
                            let term_row = mouse.row - inner_y;
                            self.right_term.selection =
                                Some(((term_row, term_col), (term_row, term_col)));
                        } else {
                            self.right_term.selection = None;
                        }
                        if let Some(quake) = &mut self.quake_term {
                            quake.selection = None;
                        }
                        self.sync_cursor();
                        return;
                    }
                }

                if let Some(target) = self.layout.pane_at(mouse.column, mouse.row) {
                    self.set_focus(target);
                }
                self.right_term.selection = None;
                if let Some(quake) = &mut self.quake_term {
                    quake.selection = None;
                }
                if self.layout.tree.is_some_and(|r| rect_contains(r, mouse.column, mouse.row)) {
                    tree::handle_mouse(self, mouse);
                } else if self.layout.middle.is_some_and(|r| rect_contains(r, mouse.column, mouse.row))
                {
                    edit::handle_mouse(self, mouse);
                }
                self.sync_cursor();
            }
            MouseEventKind::ScrollUp => {
                if let Some(middle_rect) = self.layout.middle {
                    if rect_contains(middle_rect, mouse.column, mouse.row) {
                        if self.preview_mode {
                            self.preview_cursor = self.preview_cursor.saturating_sub(3);
                            if let Some(line) = self.preview_lines.get(self.preview_cursor) {
                                if let Some(buf) = &mut self.buf {
                                    buf.cursor_row = line.source_line;
                                    self.sync_cursor();
                                }
                            }
                        } else if let Some(buf) = &mut self.buf {
                            buf.scroll_row = buf.scroll_row.saturating_sub(3);
                        }
                        return;
                    }
                }
                if let Some(quake_rect) = self.layout.quake {
                    if rect_contains(quake_rect, mouse.column, mouse.row) {
                        if let Some(quake) = &mut self.quake_term {
                            let (col, row) = compute_terminal_inner_coords(
                                quake_rect,
                                mouse.column,
                                mouse.row,
                            );
                            let _ = quake.handle_mouse_scroll(
                                true,
                                col,
                                row,
                                mouse.modifiers,
                            );
                        }
                        return;
                    }
                }
                if let Some(right_rect) = self.layout.right {
                    if rect_contains(right_rect, mouse.column, mouse.row) {
                        let (col, row) = compute_terminal_inner_coords(
                            right_rect,
                            mouse.column,
                            mouse.row,
                        );
                        let _ = self.right_term.handle_mouse_scroll(
                            true,
                            col,
                            row,
                            mouse.modifiers,
                        );
                    }
                }
            }
            MouseEventKind::ScrollDown => {
                if let Some(middle_rect) = self.layout.middle {
                    if rect_contains(middle_rect, mouse.column, mouse.row) {
                        if self.preview_mode {
                            self.preview_cursor = (self.preview_cursor + 3).min(self.preview_lines.len().saturating_sub(1));
                            if let Some(line) = self.preview_lines.get(self.preview_cursor) {
                                if let Some(buf) = &mut self.buf {
                                    buf.cursor_row = line.source_line;
                                    self.sync_cursor();
                                }
                            }
                        } else if let Some(buf) = &mut self.buf {
                            buf.scroll_row = buf.scroll_row.saturating_add(3);
                        }
                        return;
                    }
                }
                if let Some(quake_rect) = self.layout.quake {
                    if rect_contains(quake_rect, mouse.column, mouse.row) {
                        if let Some(quake) = &mut self.quake_term {
                            let (col, row) = compute_terminal_inner_coords(
                                quake_rect,
                                mouse.column,
                                mouse.row,
                            );
                            let _ = quake.handle_mouse_scroll(
                                false,
                                col,
                                row,
                                mouse.modifiers,
                            );
                        }
                        return;
                    }
                }
                if let Some(right_rect) = self.layout.right {
                    if rect_contains(right_rect, mouse.column, mouse.row) {
                        let (col, row) = compute_terminal_inner_coords(
                            right_rect,
                            mouse.column,
                            mouse.row,
                        );
                        let _ = self.right_term.handle_mouse_scroll(
                            false,
                            col,
                            row,
                            mouse.modifiers,
                        );
                    }
                }
            }
            _ => {}
        }
    }

    pub fn handle_quake_ctrl_click(&mut self, col: u16, row: u16) {
        let Some(quake_rect) = self.layout.quake else {
            return;
        };
        let Some(quake_term) = &self.quake_term else {
            return;
        };

        let inner_x = quake_rect.x + 1;
        let inner_y = quake_rect.y + 1;
        let inner_w = quake_rect.width.saturating_sub(2);
        let inner_h = quake_rect.height.saturating_sub(2);

        if col < inner_x || col >= inner_x + inner_w || row < inner_y || row >= inner_y + inner_h {
            self.set_focus(Focus::QuakeTerm);
            return;
        }

        let term_col = col - inner_x;
        let term_row = row - inner_y;

        let current_path = self.buf.as_ref().map(|b| b.path.as_path());
        if let Some((resolved, line, col_idx)) = links::extract_and_resolve(
            quake_term,
            term_row,
            term_col,
            &self.root_dir,
            &self.files,
            current_path,
        ) {
            let line_idx = line.map(|l| l.saturating_sub(1)).unwrap_or(0);
            let col_idx = col_idx.map(|c| c.saturating_sub(1)).unwrap_or(0);
            let _ = self.open_file_at(&resolved, line_idx, col_idx);
        } else {
            self.set_focus(Focus::QuakeTerm);
        }
    }

    pub fn handle_terminal_ctrl_click(&mut self, col: u16, row: u16) {
        let Some(right_rect) = self.layout.right else {
            return;
        };

        let inner_x = right_rect.x + 1;
        let inner_y = right_rect.y + 1;
        let inner_w = right_rect.width.saturating_sub(2);
        let inner_h = right_rect.height.saturating_sub(2);

        if col < inner_x || col >= inner_x + inner_w || row < inner_y || row >= inner_y + inner_h {
            self.set_focus(Focus::RightTerm);
            return;
        }

        let term_col = col - inner_x;
        let term_row = row - inner_y;

        let current_path = self.buf.as_ref().map(|b| b.path.as_path());
        if let Some((resolved, line, col_idx)) = links::extract_and_resolve(
            &self.right_term,
            term_row,
            term_col,
            &self.root_dir,
            &self.files,
            current_path,
        ) {
            let line_idx = line.map(|l| l.saturating_sub(1)).unwrap_or(0);
            let col_idx = col_idx.map(|c| c.saturating_sub(1)).unwrap_or(0);
            let _ = self.open_file_at(&resolved, line_idx, col_idx);
        } else {
            self.set_focus(Focus::RightTerm);
        }
    }

    pub fn handle_fs_event(&mut self, event: notify::Event) {
        if self.is_dirty() {
            self.reconcile_dirty_with_disk(&event);
        } else {
            self.check_clean_buffer_reload(&event);
        }
        self.trigger_background_walk();
        self.trigger_git_marks();
    }

    pub fn check_clean_buffer_reload(&mut self, event: &notify::Event) {
        if self.is_dirty() {
            return;
        }

        let Some(buf) = &mut self.buf else {
            return;
        };

        let matches_path = event.paths.is_empty()
            || event
                .paths
                .iter()
                .any(|p| paths_refer_to_same_file(p, &buf.path, &self.root_dir));

        if !matches_path {
            return;
        }

        if let Ok(bytes) = std::fs::read(&buf.path) {
            let new_hash = crate::buffer::compute_hash(&bytes);
            if new_hash != buf.disk_hash {
                if let Ok(()) = buf.reload() {
                    let visible_height = self
                        .layout
                        .middle
                        .map_or(24, |r| r.height.saturating_sub(2) as usize);
                    buf.adjust_scroll(visible_height);
                    self.sync_cursor();
                    self.trigger_diff();
                    if self.preview_mode {
                        self.update_preview();
                    }
                }
            }
        }
    }

    pub fn reconcile_dirty_with_disk(&mut self, event: &notify::Event) {
        if !self.is_dirty() {
            return;
        }

        let Some(buf) = &mut self.buf else {
            return;
        };

        if self.middle_mode == MiddleMode::Conflict {
            return;
        }

        let matches_path = event.paths.is_empty()
            || event
                .paths
                .iter()
                .any(|p| paths_refer_to_same_file(p, &buf.path, &self.root_dir));

        if !matches_path {
            return;
        }

        let Ok(bytes) = std::fs::read(&buf.path) else {
            return;
        };

        let new_hash = crate::buffer::compute_hash(&bytes);
        if new_hash == buf.disk_hash {
            return;
        }

        let (disk_lines, _, _, _, is_binary) = crate::buffer::parse_buffer_content(&bytes);
        if is_binary {
            return;
        }

        let outcome = conflict::reconcile(
            &buf.base_lines,
            &buf.lines,
            &disk_lines,
            new_hash,
        );

        match outcome {
            conflict::ReconcileOutcome::SilentMerge { merged_lines } => {
                buf.lines = merged_lines;
                buf.base_lines = disk_lines;
                buf.disk_hash = new_hash;
                buf.recompute_total_bytes();
                buf.clamp_cursor();
                buf.reset_highlighter();
                self.dirty = buf.lines != buf.base_lines;
                let visible_height = self
                    .layout
                    .middle
                    .map_or(24, |r| r.height.saturating_sub(2) as usize);
                buf.adjust_scroll(visible_height);
                self.sync_cursor();
                self.trigger_diff();
            }
            conflict::ReconcileOutcome::Conflict { session } => {
                self.middle_mode = MiddleMode::Conflict;
                self.conflict = Some(session);
                self.status_message =
                    Some("Merge conflict detected: resolve hunks with l/r/b/L/R/c/C".into());
            }
        }
    }

    pub fn complete_conflict_resolution(&mut self) -> std::io::Result<()> {
        let Some(session) = &self.conflict else {
            return Ok(());
        };
        if !session.is_all_resolved() {
            return Ok(());
        }

        let merged_lines = session.finish();
        let Some(buf) = &mut self.buf else {
            return Ok(());
        };

        buf.lines = merged_lines;
        buf.save()?;
        self.dirty = false;
        self.conflict = None;
        self.middle_mode = MiddleMode::Edit;
        self.status_message = Some("Conflicts resolved and saved to disk".into());
        buf.clamp_cursor();
        buf.reset_highlighter();
        let visible_height = self
            .layout
            .middle
            .map_or(24, |r| r.height.saturating_sub(2) as usize);
        buf.adjust_scroll(visible_height);
        self.sync_cursor();
        self.trigger_git_marks();
        self.trigger_diff();
        Ok(())
    }

    pub fn trigger_background_walk(&mut self) {
        let Some(tx) = &self.tx else {
            return;
        };

        if self
            .walk_in_flight
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            return;
        }

        let root = self.root_dir.clone();
        let in_flight = Arc::clone(&self.walk_in_flight);
        let tx = tx.clone();
        let expanded_paths: std::collections::HashSet<PathBuf> = self
            .tree
            .nodes
            .iter()
            .filter(|n| n.is_dir && n.expanded)
            .map(|n| n.path.clone())
            .collect();

        std::thread::Builder::new()
            .name("fs-tree-walk".into())
            .spawn(move || {
                let (mut tree, files) = crate::tree::Tree::walk(&root);
                let mut idx = 0;
                while idx < tree.nodes.len() {
                    if tree.nodes[idx].is_dir && expanded_paths.contains(&tree.nodes[idx].path) {
                        if tree.nodes[idx].ignored {
                            tree.lazy_expand_ignored(idx);
                        }
                        tree.nodes[idx].expanded = true;
                    }
                    idx += 1;
                }
                in_flight.store(false, std::sync::atomic::Ordering::SeqCst);
                let _ = tx.send(Msg::ProjectWalked(tree, files));
            })
            .ok();
    }

    pub fn handle_project_walked(&mut self, mut tree: crate::tree::Tree, files: Vec<PathBuf>) {
        self.walk_in_flight
            .store(false, std::sync::atomic::Ordering::SeqCst);

        // Preserve expanded directories from existing tree
        let expanded: std::collections::HashSet<PathBuf> = self
            .tree
            .nodes
            .iter()
            .filter(|n| n.is_dir && n.expanded)
            .map(|n| n.path.clone())
            .collect();

        let mut idx = 0;
        while idx < tree.nodes.len() {
            if tree.nodes[idx].is_dir && expanded.contains(&tree.nodes[idx].path) {
                if tree.nodes[idx].ignored {
                    tree.lazy_expand_ignored(idx);
                }
                tree.nodes[idx].expanded = true;
            }
            idx += 1;
        }

        // Preserve selected node / cursor
        let prev_selected = self.tree.selected_path().map(|p| p.to_path_buf());
        let visible = tree.visible_indices();
        if let Some(selected_path) = prev_selected {
            if let Some(pos) = visible.iter().position(|&idx| tree.nodes[idx].path == selected_path) {
                tree.cursor = pos;
            } else if tree.cursor >= visible.len() {
                tree.cursor = visible.len().saturating_sub(1);
            }
        } else if tree.cursor >= visible.len() {
            tree.cursor = visible.len().saturating_sub(1);
        }

        // Preserve scroll position
        tree.scroll = self.tree.scroll;
        let visible_height = self
            .layout
            .tree
            .map_or(24, |r| r.height.saturating_sub(2) as usize);
        tree.adjust_scroll(visible_height);

        self.tree = tree;
        self.files = files;
    }

    pub fn refresh_tree_sync(&mut self) {
        let (tree, files) = crate::tree::Tree::walk(&self.root_dir);
        self.handle_project_walked(tree, files);
    }

    #[must_use]
    pub fn git_status_for(&self, path: &Path, is_dir: bool) -> Option<char> {
        let rel_path = path.strip_prefix(&self.root_dir).unwrap_or(path);
        if rel_path.as_os_str().is_empty() || rel_path == Path::new(".") {
            return None;
        }
        let map = if is_dir {
            &self.git_dir_marks
        } else {
            &self.git_marks
        };
        map.get(rel_path).copied()
    }

    pub fn trigger_git_marks(&mut self) {
        let Some(tx) = &self.tx else {
            return;
        };

        if self
            .marks_in_flight
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            return;
        }

        let root = self.root_dir.clone();
        let in_flight = Arc::clone(&self.marks_in_flight);
        let tx = tx.clone();

        std::thread::Builder::new()
            .name("git-status".into())
            .spawn(move || {
                let marks = crate::git::list_marks(&root);
                in_flight.store(false, std::sync::atomic::Ordering::SeqCst);
                let _ = tx.send(Msg::Marks(marks));
            })
            .ok();
    }

    pub fn refresh_git_marks_sync(&mut self) {
        let marks = crate::git::list_marks(&self.root_dir);
        self.git_dir_marks = crate::git::rollup_marks(&marks);
        self.git_marks = marks;
    }

    pub fn trigger_diff(&mut self) {
        let Some(buf) = &self.buf else {
            self.diff = None;
            return;
        };
        let Some(tx) = &self.tx else {
            return;
        };

        self.diff_seq = self.diff_seq.wrapping_add(1);
        let seq = self.diff_seq;
        let path = buf.path.clone();
        let root = self.root_dir.clone();
        let tx = tx.clone();

        std::thread::Builder::new()
            .name("git-diff".into())
            .spawn(move || {
                let diff = crate::git::diff_file(&root, &path);
                let _ = tx.send(Msg::DiffDone { seq, path, diff });
            })
            .ok();
    }

    pub fn trigger_media_decode(&mut self, path: &Path) {
        self.media_seq = self.media_seq.wrapping_add(1);
        let seq = self.media_seq;
        let image_id = (seq % 1_000_000 + 1) as u32;
        self.active_image_id = Some(image_id);
        self.kitty_erased = false;
        self.media_state = Some(crate::viewer::MediaState::Loading {
            seq,
            path: path.to_path_buf(),
        });

        let Some(tx) = &self.tx else {
            let bytes = std::fs::read(path).ok();
            let result = bytes
                .ok_or_else(|| format!("Failed to read file: {}", path.display()))
                .and_then(|b| crate::viewer::decode_media(&b, Some(path)))
                .map(|dec| crate::viewer::MediaData::new(path.to_path_buf(), dec, image_id));
            match result {
                Ok(data) => {
                    self.media_state = Some(crate::viewer::MediaState::Loaded(data));
                }
                Err(err) => {
                    self.media_state = Some(crate::viewer::MediaState::Error {
                        seq,
                        path: path.to_path_buf(),
                        error: err,
                    });
                }
            }
            return;
        };

        let tx = tx.clone();
        let path_buf = path.to_path_buf();
        std::thread::Builder::new()
            .name("media-decode".into())
            .spawn(move || {
                let bytes = std::fs::read(&path_buf).ok();
                let result = bytes
                    .ok_or_else(|| format!("Failed to read file: {}", path_buf.display()))
                    .and_then(|b| crate::viewer::decode_media(&b, Some(&path_buf)))
                    .map(|dec| crate::viewer::MediaData::new(path_buf.clone(), dec, image_id));
                let _ = tx.send(Msg::MediaRendered { seq, result });
            })
            .ok();
    }

    pub fn set_git_marks(&mut self, marks: std::collections::HashMap<PathBuf, char>) {
        self.git_dir_marks = crate::git::rollup_marks(&marks);
        self.git_marks = marks;
    }
}

#[must_use]
pub fn paths_refer_to_same_file(p: &Path, buf_path: &Path, root_dir: &Path) -> bool {
    if p == buf_path {
        return true;
    }
    if let (Ok(p_canon), Ok(buf_canon)) = (p.canonicalize(), buf_path.canonicalize()) {
        if p_canon == buf_canon {
            return true;
        }
    }
    if !p.is_absolute() {
        let joined = root_dir.join(p);
        if joined == buf_path {
            return true;
        }
        if let (Ok(j_canon), Ok(buf_canon)) = (joined.canonicalize(), buf_path.canonicalize()) {
            if j_canon == buf_canon {
                return true;
            }
        }
    }
    false
}
