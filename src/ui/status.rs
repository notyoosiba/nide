use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::edit::MiddleMode;
use crate::app::{App, Focus, TreePromptKind};
use crate::theme;

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let focus_str = match app.focus {
        Focus::Tree => {
            if let Some(prompt) = &app.tree_prompt {
                match prompt.kind {
                    TreePromptKind::Create => "CREATE",
                    TreePromptKind::Rename { .. } => "RENAME",
                }
            } else {
                "TREE"
            }
        }
        Focus::Middle => match app.middle_mode {
            MiddleMode::Normal => {
                if app.preview_mode {
                    "PREVIEW"
                } else if app.buf.as_ref().is_some_and(|b| b.is_binary) {
                    "VIEW"
                } else if app.selection.is_some() {
                    "VISUAL"
                } else {
                    "NORMAL"
                }
            }
            MiddleMode::Edit => "EDIT",
            MiddleMode::Conflict => "CONFLICT",
            MiddleMode::Find => "FIND",
            MiddleMode::View => "VIEW",
        },
        Focus::RightTerm => "TERMINAL",
        Focus::QuakeTerm => "QUAKE",
    };

    let root_str = app.root_dir.to_string_lossy();

    let (focus_bg, focus_fg) =
        if (app.middle_mode == MiddleMode::Conflict && app.focus == Focus::Middle)
            || (app.tree_prompt.is_some() && app.focus == Focus::Tree)
            || (app.middle_mode == MiddleMode::Find && app.focus == Focus::Middle)
        {
            (theme::PEACH, theme::BASE)
        } else {
            (theme::STATUS_ACCENT, theme::BASE)
        };

    let mut spans = vec![
        Span::styled(
            format!(" [{focus_str}] "),
            Style::default()
                .fg(focus_fg)
                .bg(focus_bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ];

    // Visible pane badges
    if app.show_tree {
        let is_active = app.focus == Focus::Tree;
        spans.push(Span::styled(
            "[1:Tree]",
            Style::default()
                .fg(if is_active {
                    theme::BLUE
                } else {
                    theme::SUBTEXT0
                })
                .add_modifier(if is_active {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        ));
        spans.push(Span::raw(" "));
    }
    if app.show_middle {
        let is_active = app.focus == Focus::Middle;
        spans.push(Span::styled(
            "[2:Editor]",
            Style::default()
                .fg(if is_active {
                    theme::BLUE
                } else {
                    theme::SUBTEXT0
                })
                .add_modifier(if is_active {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        ));
        spans.push(Span::raw(" "));
    }
    if app.show_right {
        let is_active = app.focus == Focus::RightTerm;
        spans.push(Span::styled(
            "[3:Term]",
            Style::default()
                .fg(if is_active {
                    theme::BLUE
                } else {
                    theme::SUBTEXT0
                })
                .add_modifier(if is_active {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        ));
        spans.push(Span::raw(" "));
    }
    if app.show_quake {
        let is_active = app.focus == Focus::QuakeTerm;
        spans.push(Span::styled(
            "[`:Quake]",
            Style::default()
                .fg(if is_active {
                    theme::BLUE
                } else {
                    theme::SUBTEXT0
                })
                .add_modifier(if is_active {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        ));
        spans.push(Span::raw(" "));
    }

    if let Some(buf) = &app.buf {
        let name = buf
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Untitled");
        let dirty_mark = if app.is_dirty() { " [+] " } else { " " };
        spans.push(Span::styled(
            format!("{name}{dirty_mark}"),
            Style::default().fg(if app.is_dirty() {
                theme::PEACH
            } else {
                theme::TEXT
            }),
        ));
        if app.preview_mode {
            let total = app.preview_lines.len();
            let cur = app.preview_cursor + 1;
            let src = buf.cursor_row + 1;
            spans.push(Span::styled(
                format!("Preview {cur}/{total} (Line {src}) | "),
                Style::default().fg(theme::MAUVE),
            ));
        } else {
            spans.push(Span::styled(
                format!("Ln {}, Col {} | ", buf.cursor_row + 1, buf.cursor_col + 1),
                Style::default().fg(theme::SUBTEXT0),
            ));
        }
    }

    if let Some(prompt) = &app.tree_prompt {
        let label = match prompt.kind {
            TreePromptKind::Create => "New",
            TreePromptKind::Rename { .. } => "Rename",
        };
        spans.push(Span::styled(
            format!(" | {label}: {} | Enter: Confirm | Esc: Cancel", prompt.input.text()),
            Style::default()
                .fg(theme::PEACH)
                .add_modifier(Modifier::BOLD),
        ));
    } else if app.middle_mode == MiddleMode::Find {
        let (query, count_info) = if let Some(find) = &app.find {
            let count = find.matches.len();
            let idx_str = if count == 0 {
                "0/0".to_string()
            } else if let Some(idx) = find.current_match_index {
                format!("{}/{}", idx + 1, count)
            } else {
                format!("0/{count}")
            };
            (find.query.as_str(), format!(" [{idx_str}]"))
        } else {
            ("", String::new())
        };
        spans.push(Span::styled(
            format!(" | /{query}{count_info} | Enter: Confirm | Esc: Cancel"),
            Style::default()
                .fg(theme::PEACH)
                .add_modifier(Modifier::BOLD),
        ));
    } else if app.preview_mode {
        spans.push(Span::styled(
            " | Enter: Edit | p: Source | j/k: Move | Esc: Exit",
            Style::default()
                .fg(theme::MAUVE)
                .add_modifier(Modifier::BOLD),
        ));
    } else if app.middle_mode == MiddleMode::Conflict {
        spans.push(Span::styled(
            " | l: Yours | r: Agent | b: Both | L/R: All | c/C: Step",
            Style::default()
                .fg(theme::PEACH)
                .add_modifier(Modifier::BOLD),
        ));
    } else if app.middle_mode == MiddleMode::View {
        if let Some(crate::viewer::MediaState::Loaded(media)) = &app.media_state {
            let size_kb = (media.file_size as f64) / 1024.0;
            let size_str = if size_kb >= 1024.0 {
                format!("{:.1}MB", size_kb / 1024.0)
            } else {
                format!("{:.1}KB", size_kb)
            };
            spans.push(Span::styled(
                format!(" | {} {}×{}px {}", media.format, media.width, media.height, size_str),
                Style::default().fg(theme::BLUE),
            ));
        } else if let Some(crate::viewer::MediaState::Loading { .. }) = &app.media_state {
            spans.push(Span::styled(
                " | Loading...",
                Style::default().fg(theme::YELLOW),
            ));
        }
    } else if let Some(find) = &app.find {
        let count = find.matches.len();
        let idx_str = if count == 0 {
            "0/0".to_string()
        } else if let Some(idx) = find.current_match_index {
            format!("{}/{}", idx + 1, count)
        } else {
            format!("0/{count}")
        };
        spans.push(Span::styled(
            format!(
                " | /{query} [{idx_str}] | n: Next | N: Prev | Esc: Clear",
                query = find.query
            ),
            Style::default().fg(theme::SUBTEXT0),
        ));
    } else {
        spans.push(Span::styled(
            root_str.into_owned(),
            Style::default().fg(theme::TEXT),
        ));
        spans.push(Span::styled(
            " | Tab: Focus | q: Quit",
            Style::default().fg(theme::SUBTEXT0),
        ));
    }

    if let Some(msg) = &app.status_message {
        spans.push(Span::styled(
            format!(" | {msg}"),
            Style::default()
                .fg(theme::RED)
                .add_modifier(Modifier::BOLD),
        ));
    }

    let line = Line::from(spans);
    let bar = Paragraph::new(line).style(
        Style::default()
            .bg(theme::STATUS_BG)
            .fg(theme::STATUS_FG),
    );

    frame.render_widget(bar, area);
}
