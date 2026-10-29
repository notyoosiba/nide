use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{App, Focus};
use crate::picker::Picker;
use crate::pty::{TermId, TerminalPane};
use crate::theme;
use crate::ui::term;

pub fn draw_picker(frame: &mut Frame, picker: &mut Picker, area: Rect) {
    if area.width < 10 || area.height < 6 {
        return;
    }

    let target_width = (area.width * 85 / 100).clamp(50, 120).min(area.width);
    let target_height = (area.height * 80 / 100).clamp(10, 35).min(area.height);

    let x = area.x + (area.width.saturating_sub(target_width)) / 2;
    let y = area.y + (area.height.saturating_sub(target_height)) / 2;
    let modal_rect = Rect::new(x, y, target_width, target_height);

    // Erase content underneath the modal
    frame.render_widget(Clear, modal_rect);

    let title = match picker.kind {
        crate::picker::PickerKind::Files => format!(" Files ({}) ", picker.items.len()),
        crate::picker::PickerKind::Symbols => format!(" Symbols ({}) ", picker.items.len()),
        crate::picker::PickerKind::Grep => format!(" Grep ({}) ", picker.items.len()),
        crate::picker::PickerKind::Usages => format!(" Usages ({}) ", picker.items.len()),
        crate::picker::PickerKind::Definitions => {
            format!(" Definitions ({}) ", picker.items.len())
        }
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::BORDER_ACTIVE))
        .title(title)
        .title_style(
            Style::default()
                .fg(theme::BORDER_ACTIVE)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(theme::BASE).fg(theme::TEXT));

    let inner = block.inner(modal_rect);
    frame.render_widget(block, modal_rect);

    if inner.width < 10 || inner.height < 4 {
        return;
    }

    let show_preview = inner.width >= 50;

    let (left_col, right_col) = if show_preview {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(inner);
        (cols[0], Some(cols[1]))
    } else {
        (inner, None)
    };

    // Left Column: Query input at top (1 line), Candidate list below
    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(left_col);

    let query_rect = left_chunks[0];
    let list_rect = left_chunks[1];

    // Query line
    let query_spans = vec![
        Span::styled(
            " > ",
            Style::default()
                .fg(theme::BLUE)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(&picker.query, Style::default().fg(theme::TEXT)),
    ];
    frame.render_widget(Paragraph::new(Line::from(query_spans)), query_rect);

    // Set cursor to query input
    let cursor_x = query_rect.x + 3 + picker.query.chars().count() as u16;
    if cursor_x < query_rect.x + query_rect.width {
        frame.set_cursor_position((cursor_x, query_rect.y));
    }

    // Candidate list
    let list_height = list_rect.height as usize;
    let mut scroll = picker.scroll;
    if picker.selected_idx < scroll {
        scroll = picker.selected_idx;
    } else if picker.selected_idx >= scroll + list_height {
        scroll = picker.selected_idx.saturating_sub(list_height) + 1;
    }

    let mut list_lines = Vec::new();
    let end_idx = (scroll + list_height).min(picker.items.len());

    for idx in scroll..end_idx {
        let item = &picker.items[idx];
        let is_selected = idx == picker.selected_idx;
        let bg_color = if is_selected {
            theme::SELECTION_BG
        } else {
            theme::BASE
        };

        let prefix = if is_selected { " > " } else { "   " };
        let mut spans = vec![Span::styled(
            prefix,
            Style::default()
                .fg(if is_selected {
                    theme::BLUE
                } else {
                    theme::SUBTEXT0
                })
                .bg(bg_color)
                .add_modifier(if is_selected {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        )];

        for (ch_idx, ch) in item.display_text.chars().enumerate() {
            let is_match = item.match_indices.contains(&(ch_idx as u32));
            let style = if is_match {
                Style::default()
                    .fg(theme::PEACH)
                    .bg(bg_color)
                    .add_modifier(Modifier::BOLD)
            } else if is_selected {
                Style::default().fg(theme::TEXT).bg(bg_color)
            } else {
                Style::default().fg(theme::SUBTEXT0).bg(bg_color)
            };
            spans.push(Span::styled(ch.to_string(), style));
        }

        list_lines.push(Line::from(spans));
    }

    if list_lines.is_empty() {
        list_lines.push(Line::from(vec![Span::styled(
            "   No matching files",
            Style::default().fg(theme::SUBTEXT0),
        )]));
    }

    frame.render_widget(Paragraph::new(list_lines), list_rect);

    // Right Column: Preview Pane
    if let Some(right_rect) = right_col {
        let preview_title = picker
            .items
            .get(picker.selected_idx)
            .map(|it| format!(" {} ", it.display_text))
            .unwrap_or_else(|| " Preview ".to_string());

        let preview_block = Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(theme::BORDER_INACTIVE))
            .title(preview_title)
            .title_style(
                Style::default()
                    .fg(theme::BLUE)
                    .add_modifier(Modifier::BOLD),
            );

        let preview_inner = preview_block.inner(right_rect);
        frame.render_widget(preview_block, right_rect);

        if preview_inner.width > 0 && preview_inner.height > 0 {
            if let Some(buf) = &mut picker.preview {
                if buf.is_binary {
                    let msg = Paragraph::new(" [Binary file — preview unavailable] ")
                        .style(Style::default().fg(theme::RED));
                    frame.render_widget(msg, preview_inner);
                } else {
                    let visible_lines = preview_inner.height as usize;
                    let target_line = picker
                        .items
                        .get(picker.selected_idx)
                        .and_then(|it| it.line);

                    let start_row = if let Some(target) = target_line {
                        if target >= visible_lines / 2 {
                            target.saturating_sub(visible_lines / 2)
                        } else {
                            0
                        }
                    } else {
                        0
                    };
                    let end_row = (start_row + visible_lines).min(buf.lines.len());
                    let hl_map = buf.highlight_lines(start_row, end_row);

                    let mut preview_lines = Vec::new();
                    let digits = format!("{}", end_row.max(1)).len().max(2);

                    for row in start_row..end_row {
                        let is_target = target_line == Some(row);
                        let gutter_style = if is_target {
                            Style::default()
                                .fg(theme::BLUE)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(theme::SURFACE2)
                        };
                        let mut line_spans = vec![Span::styled(
                            format!("{:>width$} ", row + 1, width = digits),
                            gutter_style,
                        )];

                        let line_bg = if is_target {
                            theme::SURFACE0
                        } else {
                            theme::BASE
                        };

                        if let Some((_, spans)) = hl_map.iter().find(|(r, _)| *r == row) {
                            for (style, text) in spans {
                                let mut s = *style;
                                if is_target {
                                    s = s.bg(line_bg);
                                }
                                line_spans.push(Span::styled(text.clone(), s));
                            }
                        } else if row < buf.lines.len() {
                            let s = if is_target {
                                Style::default().fg(theme::TEXT).bg(line_bg)
                            } else {
                                Style::default().fg(theme::TEXT)
                            };
                            line_spans.push(Span::styled(
                                buf.lines[row].clone(),
                                s,
                            ));
                        }

                        preview_lines.push(Line::from(line_spans));
                    }

                    frame.render_widget(Paragraph::new(preview_lines), preview_inner);
                }
            } else {
                let msg = Paragraph::new(" No preview available ")
                    .style(Style::default().fg(theme::SUBTEXT0));
                frame.render_widget(msg, preview_inner);
            }
        }
    }
}

pub fn draw_quake(frame: &mut Frame, app: &mut App, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    frame.render_widget(Clear, area);

    if app.quake_term.is_none() {
        app.quake_term = Some(TerminalPane::new(
            TermId::Quake,
            app.root_dir.clone(),
            app.tx.clone(),
        ));
    }

    if let Some(quake_term) = &mut app.quake_term {
        term::draw_terminal_pane(
            frame,
            quake_term,
            area,
            "Quake Console",
            app.focus == Focus::QuakeTerm,
        );
    }
}

pub fn draw_help(frame: &mut Frame, area: Rect) {
    if area.width < 10 || area.height < 6 {
        return;
    }

    let target_width = (area.width * 75 / 100).clamp(50, 80).min(area.width);
    let target_height = (area.height * 80 / 100).clamp(16, 26).min(area.height);

    let x = area.x + (area.width.saturating_sub(target_width)) / 2;
    let y = area.y + (area.height.saturating_sub(target_height)) / 2;
    let modal_rect = Rect::new(x, y, target_width, target_height);

    frame.render_widget(Clear, modal_rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::BORDER_ACTIVE))
        .title(" Help — Keybindings ")
        .title_style(
            Style::default()
                .fg(theme::BORDER_ACTIVE)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(theme::BASE).fg(theme::TEXT));

    let inner = block.inner(modal_rect);
    frame.render_widget(block, modal_rect);

    if inner.width < 10 || inner.height < 4 {
        return;
    }

    let rows = [
        ("Shift+1/2/3 (or Alt)", "Toggle Tree / Middle / Right Panes"),
        ("~ / Alt+` / Ctrl+`", "Toggle Quake Console Overlay"),
        ("Alt+Tab / Ctrl+\\", "Cycle Focus"),
        ("Ctrl+E", "Fuzzy File Picker"),
        ("Ctrl+T", "Fuzzy Symbol Search"),
        ("? (Tree / Normal)", "Toggle this Help Overlay"),
        ("Esc / q", "Normal Mode / Quit / Close Modal"),
        ("Tree: a / Del", "Create File (trailing / for dir) / Delete"),
        ("Tree: r / y / x / p", "Rename / Copy / Move / Paste Node"),
        ("Tree: Enter / o", "Open in Editor / External Viewer"),
        ("Editor: i / Esc", "Edit Mode / Normal Mode (Auto-Saves)"),
        ("Editor: / / n / N", "Find in File / Next / Prev Match"),
        ("Editor: d / u", "Go to Definition / Find Usages"),
        ("Editor: [ / ]", "Jump Back / Jump Forward"),
        ("Editor: c / C", "Next / Previous Git Diff Hunk"),
        ("Terminal: Ctrl+Click", "Follow File:Line:Col Link"),
        ("Terminal: Drag / Ctrl+C", "Select Text / Copy (or SIGINT)"),
    ];

    let mut lines = Vec::new();
    for (key, desc) in rows {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:<25}", key),
                Style::default()
                    .fg(theme::BLUE)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(desc, Style::default().fg(theme::TEXT)),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Press ? or Esc to close",
        Style::default()
            .fg(theme::SUBTEXT0)
            .add_modifier(Modifier::ITALIC),
    )));

    frame.render_widget(Paragraph::new(lines), inner);
}
