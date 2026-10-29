use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::edit::MiddleMode;
use crate::app::App;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindMatch {
    pub line: usize,
    pub col_start: usize,
    pub col_end: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineSearchMatch {
    pub col_start: usize,
    pub col_end: usize,
    pub is_current: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FindState {
    pub query: String,
    pub anchor: (usize, usize),
    pub matches: Vec<FindMatch>,
    pub current_match_index: Option<usize>,
}

impl FindState {
    #[must_use]
    pub fn new(anchor: (usize, usize)) -> Self {
        Self {
            query: String::new(),
            anchor,
            matches: Vec::new(),
            current_match_index: None,
        }
    }

    #[must_use]
    pub fn matches_on_line(&self, line: usize) -> Vec<LineSearchMatch> {
        let start = self.matches.partition_point(|m| m.line < line);
        let mut result = Vec::new();
        for idx in start..self.matches.len() {
            let m = &self.matches[idx];
            if m.line != line {
                break;
            }
            let is_current = self.current_match_index == Some(idx);
            result.push(LineSearchMatch {
                col_start: m.col_start,
                col_end: m.col_end,
                is_current,
            });
        }
        result
    }
}

/// Compiles a smart-case regex, falling back to escaping literal regex if the input is invalid syntax.
#[must_use]
pub fn compile_regex(query: &str) -> Option<regex::Regex> {
    if query.is_empty() {
        return None;
    }
    let case_insensitive = !query.chars().any(char::is_uppercase);

    if let Ok(re) = regex::RegexBuilder::new(query)
        .case_insensitive(case_insensitive)
        .build()
    {
        Some(re)
    } else {
        let escaped = regex::escape(query);
        regex::RegexBuilder::new(&escaped)
            .case_insensitive(case_insensitive)
            .build()
            .ok()
    }
}

#[must_use]
pub fn byte_to_char_idx(s: &str, byte_idx: usize) -> usize {
    s[..byte_idx.min(s.len())].chars().count()
}

pub fn find_matches_in_buffer(lines: &[String], re: &regex::Regex) -> Vec<FindMatch> {
    let mut matches = Vec::new();
    for (row, line) in lines.iter().enumerate() {
        for mat in re.find_iter(line) {
            if mat.start() == mat.end() {
                continue;
            }
            let col_start = byte_to_char_idx(line, mat.start());
            let col_end = byte_to_char_idx(line, mat.end());
            if col_start < col_end {
                matches.push(FindMatch {
                    line: row,
                    col_start,
                    col_end,
                });
            }
        }
    }
    matches
}

pub fn start_find(app: &mut App) {
    let Some(buf) = &app.buf else { return };
    if buf.is_binary {
        return;
    }

    let anchor = (buf.cursor_row, buf.cursor_col);
    app.selection = None;
    app.find = Some(FindState::new(anchor));
    app.middle_mode = MiddleMode::Find;
}

pub fn confirm_find(app: &mut App) {
    app.middle_mode = MiddleMode::Normal;
}

pub fn cancel_find(app: &mut App) {
    if let Some(find) = app.find.take() {
        if let Some(buf) = &mut app.buf {
            buf.cursor_row = find.anchor.0;
            buf.cursor_col = find.anchor.1;
            let visible_height = app
                .layout
                .middle
                .map_or(24, |r| r.height.saturating_sub(2) as usize);
            buf.adjust_scroll(visible_height);
        }
    }
    app.middle_mode = MiddleMode::Normal;
    app.sync_cursor();
}

pub fn recompute(app: &mut App) {
    let Some(find) = &mut app.find else { return };
    let Some(buf) = &mut app.buf else { return };

    let re = compile_regex(&find.query);
    find.matches = re.as_ref().map_or_else(Vec::new, |r| {
        find_matches_in_buffer(&buf.lines, r)
    });

    if find.matches.is_empty() {
        find.current_match_index = None;
        buf.cursor_row = find.anchor.0;
        buf.cursor_col = find.anchor.1;
    } else {
        let anchor = find.anchor;
        let match_idx = find
            .matches
            .iter()
            .position(|m| (m.line, m.col_start) >= anchor)
            .unwrap_or(0);

        find.current_match_index = Some(match_idx);
        let m = &find.matches[match_idx];
        buf.cursor_row = m.line;
        buf.cursor_col = m.col_start;
    }

    let visible_height = app
        .layout
        .middle
        .map_or(24, |r| r.height.saturating_sub(2) as usize);
    buf.adjust_scroll(visible_height);
    app.sync_cursor();
}

pub fn step_next(app: &mut App) {
    let Some(find) = &mut app.find else { return };
    if find.matches.is_empty() {
        return;
    }
    let Some(buf) = &mut app.buf else { return };

    let cur_pos = (buf.cursor_row, buf.cursor_col);
    let next_idx = find
        .matches
        .iter()
        .position(|m| (m.line, m.col_start) > cur_pos)
        .unwrap_or(0);

    find.current_match_index = Some(next_idx);
    let m = &find.matches[next_idx];
    buf.cursor_row = m.line;
    buf.cursor_col = m.col_start;

    let visible_height = app
        .layout
        .middle
        .map_or(24, |r| r.height.saturating_sub(2) as usize);
    buf.adjust_scroll(visible_height);
    app.sync_cursor();
}

pub fn step_prev(app: &mut App) {
    let Some(find) = &mut app.find else { return };
    if find.matches.is_empty() {
        return;
    }
    let Some(buf) = &mut app.buf else { return };

    let cur_pos = (buf.cursor_row, buf.cursor_col);
    let prev_idx = find
        .matches
        .iter()
        .rposition(|m| (m.line, m.col_start) < cur_pos)
        .unwrap_or(find.matches.len() - 1);

    find.current_match_index = Some(prev_idx);
    let m = &find.matches[prev_idx];
    buf.cursor_row = m.line;
    buf.cursor_col = m.col_start;

    let visible_height = app
        .layout
        .middle
        .map_or(24, |r| r.height.saturating_sub(2) as usize);
    buf.adjust_scroll(visible_height);
    app.sync_cursor();
}

pub fn handle_key(app: &mut App, key: KeyEvent) {
    let has_ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let has_alt = key.modifiers.contains(KeyModifiers::ALT);
    let has_super = key.modifiers.contains(KeyModifiers::SUPER);

    if has_alt {
        return;
    }

    if has_ctrl || has_super {
        match key.code {
            KeyCode::Char('c') => {
                cancel_find(app);
                return;
            }
            KeyCode::Char('n') => {
                step_next(app);
                return;
            }
            KeyCode::Char('p') => {
                step_prev(app);
                return;
            }
            KeyCode::Char('u') => {
                if let Some(find) = &mut app.find {
                    find.query.clear();
                }
                recompute(app);
                return;
            }
            KeyCode::Char('w') => {
                if let Some(find) = &mut app.find {
                    while let Some(c) = find.query.pop() {
                        if c.is_whitespace() {
                            break;
                        }
                    }
                }
                recompute(app);
                return;
            }
            _ => return,
        }
    }

    match key.code {
        KeyCode::Esc => {
            cancel_find(app);
        }
        KeyCode::Enter => {
            confirm_find(app);
        }
        KeyCode::Backspace => {
            if let Some(find) = &mut app.find {
                find.query.pop();
            }
            recompute(app);
        }
        KeyCode::Up => {
            step_prev(app);
        }
        KeyCode::Down => {
            step_next(app);
        }
        KeyCode::Char(c) => {
            if let Some(find) = &mut app.find {
                find.query.push(c);
            }
            recompute(app);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smart_case_regex_compilation() {
        // Lowercase query -> case-insensitive
        let re_lower = compile_regex("foo").expect("valid regex");
        assert!(re_lower.is_match("foo"));
        assert!(re_lower.is_match("FOO"));
        assert!(re_lower.is_match("Foo"));

        // Uppercase query -> case-sensitive
        let re_upper = compile_regex("Foo").expect("valid regex");
        assert!(re_upper.is_match("Foo"));
        assert!(!re_upper.is_match("foo"));
        assert!(!re_upper.is_match("FOO"));

        // Invalid regex syntax -> fallback to literal escaping
        let re_invalid = compile_regex("foo(").expect("fallback literal");
        assert!(re_invalid.is_match("foo("));
        assert!(re_invalid.is_match("FOO("));
        assert!(!re_invalid.is_match("foo"));

        // Regex patterns
        let re_pat = compile_regex(r"\d+").expect("valid regex pattern");
        assert!(re_pat.is_match("item 42"));
        assert!(!re_pat.is_match("no digits"));

        // Empty query
        assert!(compile_regex("").is_none());
    }

    #[test]
    fn test_find_matches_in_buffer_positions() {
        let lines = vec![
            "alpha beta alpha".to_string(),
            "no match here".to_string(),
            "ALPHA at end".to_string(),
        ];
        let re = compile_regex("alpha").unwrap();
        let matches = find_matches_in_buffer(&lines, &re);

        assert_eq!(matches.len(), 3);
        assert_eq!(
            matches[0],
            FindMatch {
                line: 0,
                col_start: 0,
                col_end: 5
            }
        );
        assert_eq!(
            matches[1],
            FindMatch {
                line: 0,
                col_start: 11,
                col_end: 16
            }
        );
        assert_eq!(
            matches[2],
            FindMatch {
                line: 2,
                col_start: 0,
                col_end: 5
            }
        );
    }

    #[test]
    fn test_find_state_matches_on_line() {
        let mut state = FindState::new((0, 0));
        state.matches = vec![
            FindMatch {
                line: 1,
                col_start: 2,
                col_end: 6,
            },
            FindMatch {
                line: 1,
                col_start: 10,
                col_end: 14,
            },
            FindMatch {
                line: 3,
                col_start: 0,
                col_end: 4,
            },
        ];
        state.current_match_index = Some(1);

        let line0 = state.matches_on_line(0);
        assert!(line0.is_empty());

        let line1 = state.matches_on_line(1);
        assert_eq!(line1.len(), 2);
        assert_eq!(line1[0].col_start, 2);
        assert_eq!(line1[0].col_end, 6);
        assert!(!line1[0].is_current);
        assert_eq!(line1[1].col_start, 10);
        assert_eq!(line1[1].col_end, 14);
        assert!(line1[1].is_current);

        let line3 = state.matches_on_line(3);
        assert_eq!(line3.len(), 1);
        assert!(!line3[0].is_current);
    }
}
