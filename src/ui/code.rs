use std::collections::HashMap;

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;

use crate::app::edit::MiddleMode;
use crate::app::{App, Focus};
use crate::theme;

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let is_active = app.focus == Focus::Middle;
    let border_color = if is_active {
        theme::BORDER_ACTIVE
    } else {
        theme::BORDER_INACTIVE
    };

    let title_style = if is_active {
        Style::default().fg(theme::BORDER_ACTIVE)
    } else {
        Style::default().fg(theme::SUBTEXT0)
    };

    let title = if let Some(buf) = &app.buf {
        let name = buf
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Untitled");
        let dirty_str = if app.is_dirty() { " [+]" } else { "" };
        let mode_str = match app.middle_mode {
            MiddleMode::Normal => {
                if app.preview_mode {
                    "PREVIEW".to_string()
                } else if buf.is_binary {
                    "VIEW".to_string()
                } else if app.selection.is_some() {
                    "VISUAL".to_string()
                } else {
                    "NORMAL".to_string()
                }
            }
            MiddleMode::Edit => "EDIT".to_string(),
            MiddleMode::Conflict => {
                if let Some(conflict) = &app.conflict {
                    let total = conflict.conflict_count();
                    let active = conflict.active_hunk_idx + 1;
                    format!("CONFLICT {active}/{total}")
                } else {
                    "CONFLICT".to_string()
                }
            }
            MiddleMode::Find => "FIND".to_string(),
            MiddleMode::View => "VIEW".to_string(),
        };
        format!(" Editor: {name} [{mode_str}]{dirty_str} ")
    } else {
        " Editor ".to_string()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(title)
        .title_style(title_style)
        .style(Style::default().bg(theme::BASE).fg(theme::TEXT));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    if app.middle_mode == MiddleMode::Conflict && app.conflict.is_some() {
        render_conflict_view(frame, app, inner);
        return;
    }

    let Some(buf) = &mut app.buf else {
        let hint = Paragraph::new(" No file open — select a file in tree and press Enter ")
            .style(Style::default().fg(theme::SUBTEXT0));
        frame.render_widget(hint, inner);
        return;
    };

    let visible_height = inner.height as usize;

    if buf.is_binary {
        let msg = Paragraph::new(" [Binary file: editing disabled] ")
            .style(Style::default().fg(theme::RED));
        frame.render_widget(msg, inner);
        return;
    }

    if app.preview_mode {
        render_preview_view(frame, app, inner);
        return;
    }

    let total_buffer_lines = buf.lines.len();
    let gutter_digits = format!("{total_buffer_lines}").len().max(3);
    let gutter_width = gutter_digits + 1; // e.g. "  1 "
    let content_width = (inner.width as usize).saturating_sub(gutter_width).max(1);

    let diff = app.diff.as_ref();
    let mut display_rows = Vec::new();
    let mut cursor_display_row = 0;
    let mut cursor_display_col = 0;

    for b_row in 0..buf.lines.len() {
        if buf.is_line_folded(b_row) {
            continue;
        }

        // Interleave phantom deleted ghost lines anchored at this buffer line
        if let Some(diff) = diff {
            for ghost in &diff.ghosts {
                if ghost.anchor_line == b_row {
                    let ghost_chunks =
                        crate::wrap::wrap_line(b_row, &ghost.text, content_width);
                    for chunk in ghost_chunks {
                        display_rows.push(DisplayRow::Ghost {
                            ghost,
                            char_range: chunk.char_range,
                            is_first_of_line: !chunk.is_continuation,
                        });
                    }
                }
            }
        }

        // Buffer line wrapped rows
        let line_chunks =
            crate::wrap::wrap_line(b_row, &buf.lines[b_row], content_width);
        for chunk in line_chunks {
            let is_first = !chunk.is_continuation;

            if b_row == buf.cursor_row {
                let line_len = buf.lines[b_row].chars().count();
                let at_end = buf.cursor_col == line_len;
                if (buf.cursor_col >= chunk.char_range.start
                    && buf.cursor_col < chunk.char_range.end)
                    || (at_end && buf.cursor_col == chunk.char_range.end)
                {
                    cursor_display_row = display_rows.len();
                    cursor_display_col = buf.cursor_col - chunk.char_range.start;
                }
            }

            display_rows.push(DisplayRow::Buffer {
                wrapped: chunk,
                is_first_of_line: is_first,
            });
        }
    }

    // Ghost lines anchored after the last buffer line
    if let Some(diff) = diff {
        for ghost in &diff.ghosts {
            if ghost.anchor_line >= buf.lines.len() {
                let ghost_chunks = crate::wrap::wrap_line(
                    buf.lines.len(),
                    &ghost.text,
                    content_width,
                );
                for chunk in ghost_chunks {
                    display_rows.push(DisplayRow::Ghost {
                        ghost,
                        char_range: chunk.char_range,
                        is_first_of_line: !chunk.is_continuation,
                    });
                }
            }
        }
    }

    let total_display_rows = display_rows.len();

    // Only adjust viewport scroll to follow cursor when cursor position has changed
    let current_cursor = (buf.cursor_row, buf.cursor_col);
    if buf.last_cursor != Some(current_cursor) {
        if cursor_display_row < buf.scroll_row {
            buf.scroll_row = cursor_display_row;
        } else if cursor_display_row >= buf.scroll_row + visible_height {
            buf.scroll_row = cursor_display_row.saturating_sub(visible_height) + 1;
        }
        buf.last_cursor = Some(current_cursor);
    }

    // Clamp scroll_row to valid display row bounds
    let max_scroll = total_display_rows.saturating_sub(visible_height);
    if buf.scroll_row > max_scroll {
        buf.scroll_row = max_scroll;
    }

    // Determine buffer rows needed for visible visual rows
    let start_d_row = buf.scroll_row;
    let end_d_row = (buf.scroll_row + visible_height).min(total_display_rows);

    let mut min_b_row = 0;
    let mut max_b_row = 0;
    let mut found_b = false;

    for row in &display_rows[start_d_row..end_d_row] {
        if let DisplayRow::Buffer { wrapped, .. } = row {
            if !found_b {
                min_b_row = wrapped.buffer_row;
                found_b = true;
            }
            max_b_row = wrapped.buffer_row;
        }
    }

    let hl_map: HashMap<usize, Vec<(Style, String)>> = if found_b {
        buf.highlight_lines(min_b_row, max_b_row).into_iter().collect()
    } else {
        HashMap::new()
    };

    let mut lines_to_render = Vec::with_capacity(visible_height);

    for row in 0..visible_height {
        let d_row_idx = buf.scroll_row + row;
        if d_row_idx < total_display_rows {
            match &display_rows[d_row_idx] {
                DisplayRow::Buffer {
                    wrapped,
                    is_first_of_line,
                } => {
                    let is_cursor_line = wrapped.buffer_row == buf.cursor_row && is_active;

                    let (num_text, num_style) = if *is_first_of_line {
                        let text = format!(
                            "{:>width$}",
                            wrapped.buffer_row + 1,
                            width = gutter_digits
                        );
                        let style = if is_cursor_line {
                            Style::default()
                                .fg(theme::BLUE)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(theme::SUBTEXT0)
                        };
                        (text, style)
                    } else {
                        let text = format!("{:>width$}", "·", width = gutter_digits);
                        let style = Style::default().fg(theme::SURFACE2);
                        (text, style)
                    };

                    let (bar_char, bar_style) = if let Some(diff) = diff {
                        match diff.marks.get(&wrapped.buffer_row) {
                            Some(crate::git::Mark::Added) => (
                                theme::DIFF_GUTTER_BAR,
                                Style::default().fg(theme::DIFF_ADDED),
                            ),
                            Some(crate::git::Mark::Changed) => (
                                theme::DIFF_GUTTER_BAR,
                                Style::default().fg(theme::DIFF_CHANGED),
                            ),
                            Some(crate::git::Mark::DeletedBelow) => (
                                theme::DIFF_GUTTER_BAR,
                                Style::default().fg(theme::DIFF_DELETED),
                            ),
                            None => (" ", Style::default()),
                        }
                    } else {
                        (" ", Style::default())
                    };

                    let mut spans = vec![
                        Span::styled(num_text, num_style),
                        Span::styled(bar_char, bar_style),
                    ];

                    let b_row = wrapped.buffer_row;
                    let line_spans = hl_map.get(&b_row).map(|v| v.as_slice()).unwrap_or(&[]);

                    let selection_range = app.selection.and_then(|sel| {
                        let ((s_row, s_col), (e_row, e_col)) =
                            sel.range(buf.cursor_row, buf.cursor_col);
                        if (s_row, s_col) != (e_row, e_col) && b_row >= s_row && b_row <= e_row {
                            let sel_start = if b_row == s_row { s_col } else { 0 };
                            let sel_end = if b_row == e_row { e_col } else { usize::MAX };
                            Some((sel_start, sel_end))
                        } else {
                            None
                        }
                    });

                    let search_matches = if let Some(find) = &app.find {
                        find.matches_on_line(b_row)
                    } else {
                        Vec::new()
                    };

                    let content_spans = render_highlighted_slice(
                        line_spans,
                        &wrapped.char_range,
                        selection_range,
                        &search_matches,
                        &buf.lines[b_row],
                    );
                    spans.extend(content_spans);

                    let line_chars_count = buf.lines[b_row].chars().count();
                    if wrapped.char_range.end >= line_chars_count {
                        if let Some(fold) = buf.get_fold_at_start(b_row) {
                            let suffix = if buf
                                .lines
                                .get(fold.end_row)
                                .is_some_and(|l| l.trim().ends_with('}'))
                            {
                                " ⋯ }"
                            } else if buf
                                .lines
                                .get(fold.end_row)
                                .is_some_and(|l| l.trim().ends_with(')'))
                            {
                                " ⋯ )"
                            } else if buf
                                .lines
                                .get(fold.end_row)
                                .is_some_and(|l| l.trim().ends_with(']'))
                            {
                                " ⋯ ]"
                            } else {
                                " ⋯"
                            };
                            spans.push(Span::styled(
                                suffix,
                                Style::default()
                                    .fg(theme::MAUVE)
                                    .bg(theme::SURFACE0)
                                    .add_modifier(Modifier::BOLD),
                            ));
                        }
                    }

                    lines_to_render.push(Line::from(spans));
                }
                DisplayRow::Ghost {
                    ghost,
                    char_range,
                    is_first_of_line,
                } => {
                    let num_text = if *is_first_of_line {
                        format!("{:>width$}", "-", width = gutter_digits)
                    } else {
                        format!("{:>width$}", "·", width = gutter_digits)
                    };
                    let num_style = Style::default().fg(theme::DIFF_DELETED);
                    let bar_span = Span::styled(
                        theme::DIFF_GUTTER_BAR,
                        Style::default().fg(theme::DIFF_DELETED),
                    );
                    let mut spans = vec![Span::styled(num_text, num_style), bar_span];

                    let ghost_spans =
                        render_ghost_slice(&ghost.spans, char_range, &ghost.text);
                    spans.extend(ghost_spans);

                    lines_to_render.push(Line::from(spans));
                }
            }
        } else {
            let tilde_span = Span::styled(
                format!("{:>width$} ", "~", width = gutter_digits),
                Style::default().fg(theme::SURFACE2),
            );
            lines_to_render.push(Line::from(vec![tilde_span]));
        }
    }

    let paragraph = Paragraph::new(lines_to_render);
    frame.render_widget(paragraph, inner);

    if is_active && cursor_display_row >= buf.scroll_row {
        let row_offset = cursor_display_row - buf.scroll_row;
        if row_offset < visible_height {
            let cursor_y = inner.y + row_offset as u16;
            let cursor_x = inner.x + gutter_width as u16 + cursor_display_col as u16;
            if cursor_x < inner.x + inner.width {
                frame.set_cursor_position((cursor_x, cursor_y));
            }
        }
    }
}

enum DisplayRow<'a> {
    Buffer {
        wrapped: crate::wrap::WrappedRow,
        is_first_of_line: bool,
    },
    Ghost {
        ghost: &'a crate::git::GhostLine,
        char_range: std::ops::Range<usize>,
        is_first_of_line: bool,
    },
}

pub fn render_ghost_slice(
    spans: &[crate::intraline::Span],
    char_range: &std::ops::Range<usize>,
    fallback_text: &str,
) -> Vec<Span<'static>> {
    let mut result = Vec::new();
    let mut token_char_start = 0;

    for span in spans {
        let char_count = span.text.chars().count();
        let token_char_end = token_char_start + char_count;

        let intersect_start = token_char_start.max(char_range.start);
        let intersect_end = token_char_end.min(char_range.end);

        if intersect_start < intersect_end {
            let offset_start = intersect_start - token_char_start;
            let offset_end = intersect_end - token_char_start;
            let slice_str: String = span
                .text
                .chars()
                .skip(offset_start)
                .take(offset_end - offset_start)
                .collect();

            let style = match span.kind {
                crate::intraline::SegmentKind::Deleted => Style::default()
                    .fg(theme::RED)
                    .add_modifier(Modifier::BOLD | Modifier::CROSSED_OUT),
                crate::intraline::SegmentKind::Unchanged => {
                    Style::default().fg(theme::OVERLAY1)
                }
                crate::intraline::SegmentKind::Added => Style::default().fg(theme::GREEN),
            };

            result.push(Span::styled(slice_str, style));
        }

        token_char_start = token_char_end;
    }

    if result.is_empty() && char_range.start < char_range.end {
        let chars: Vec<char> = fallback_text.chars().collect();
        let end = char_range.end.min(chars.len());
        if char_range.start < end {
            let slice: String = chars[char_range.start..end].iter().collect();
            result.push(Span::styled(
                slice,
                Style::default()
                    .fg(theme::RED)
                    .add_modifier(Modifier::CROSSED_OUT),
            ));
        }
    }

    result
}

pub fn render_highlighted_slice(
    line_spans: &[(Style, String)],
    char_range: &std::ops::Range<usize>,
    selection_range: Option<(usize, usize)>,
    search_matches: &[crate::app::find::LineSearchMatch],
    fallback_line: &str,
) -> Vec<Span<'static>> {
    if char_range.start >= char_range.end {
        return Vec::new();
    }

    let line_chars: Vec<char> = fallback_line.chars().collect();
    let total_chars = line_chars.len();
    let start_idx = char_range.start.min(total_chars);
    let end_idx = char_range.end.min(total_chars);

    if start_idx >= end_idx {
        return Vec::new();
    }

    let mut token_idx = 0;
    let mut token_start = 0;

    let mut result: Vec<Span<'static>> = Vec::new();
    let mut current_text = String::new();
    let mut current_style: Option<Style> = None;

    for (i, &ch) in line_chars[start_idx..end_idx].iter().enumerate() {
        let c = start_idx + i;

        // 1. Determine base style from line_spans
        while token_idx < line_spans.len() {
            let token_len = line_spans[token_idx].1.chars().count();
            if c < token_start + token_len {
                break;
            }
            token_start += token_len;
            token_idx += 1;
        }

        let base_style = if token_idx < line_spans.len() && c >= token_start {
            line_spans[token_idx].0
        } else {
            Style::default().fg(theme::TEXT)
        };

        // 2. Check overlays: selection takes highest priority, then search matches
        let final_style = if selection_range.is_some_and(|(s, e)| c >= s && c < e) {
            Style::default()
                .bg(theme::SELECTION_BG)
                .fg(theme::SELECTION_FG)
        } else if let Some(m) = search_matches.iter().find(|m| c >= m.col_start && c < m.col_end) {
            if m.is_current {
                Style::default()
                    .bg(theme::SEARCH_MATCH_CURRENT_BG)
                    .fg(theme::SEARCH_MATCH_CURRENT_FG)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .bg(theme::SEARCH_MATCH_BG)
                    .fg(theme::SEARCH_MATCH_FG)
            }
        } else {
            base_style
        };

        if current_style == Some(final_style) {
            current_text.push(ch);
        } else {
            if let Some(style) = current_style {
                if !current_text.is_empty() {
                    result.push(Span::styled(current_text, style));
                    current_text = String::new();
                }
            }
            current_text.push(ch);
            current_style = Some(final_style);
        }
    }

    if let Some(style) = current_style {
        if !current_text.is_empty() {
            result.push(Span::styled(current_text, style));
        }
    }

    result
}

fn render_conflict_view(frame: &mut Frame, app: &mut App, inner: Rect) {
    let Some(conflict) = &app.conflict else {
        return;
    };
    let Some(buf) = &mut app.buf else {
        return;
    };

    let total_conflicts = conflict.conflict_count();
    let visible_height = inner.height as usize;
    let content_width = (inner.width as usize).saturating_sub(6).max(1);

    let mut lines_to_render = Vec::new();
    let mut conflict_idx = 0;
    let mut active_hunk_start_row = 0;

    for chunk in &conflict.chunks {
        match chunk {
            crate::app::conflict::MergeChunk::Clean(clean_lines) => {
                for line in clean_lines {
                    let wrapped = crate::wrap::wrap_line(0, line, content_width);
                    for (i, w) in wrapped.into_iter().enumerate() {
                        let gutter = if i == 0 { "      " } else { "    · " };
                        let gutter_span =
                            Span::styled(gutter, Style::default().fg(theme::SURFACE2));
                        let text_span = Span::styled(
                            line[w.char_range].to_string(),
                            Style::default().fg(theme::TEXT),
                        );
                        lines_to_render.push(Line::from(vec![gutter_span, text_span]));
                    }
                }
            }
            crate::app::conflict::MergeChunk::Conflict(hunk) => {
                let is_active = conflict_idx == conflict.active_hunk_idx;
                if is_active {
                    active_hunk_start_row = lines_to_render.len();
                }

                let status_tag = match hunk.resolution {
                    Some(crate::app::conflict::Resolution::Yours) => " [RESOLVED: Yours]",
                    Some(crate::app::conflict::Resolution::Agent) => " [RESOLVED: Agent]",
                    Some(crate::app::conflict::Resolution::Both) => " [RESOLVED: Both]",
                    None => {
                        if is_active {
                            " [ACTIVE — l: Yours, r: Agent, b: Both]"
                        } else {
                            ""
                        }
                    }
                };

                let header_style = if is_active {
                    Style::default()
                        .fg(theme::BLUE)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(theme::SUBTEXT0)
                        .add_modifier(Modifier::BOLD)
                };
                let header_text = format!("<<<<<<< Yours (in-memory){status_tag}");
                lines_to_render.push(Line::from(vec![
                    Span::styled("◄◄◄◄  ", header_style),
                    Span::styled(header_text, header_style),
                ]));

                let yours_style = Style::default().fg(theme::BLUE);
                for line in &hunk.yours {
                    lines_to_render.push(Line::from(vec![
                        Span::styled("  │   ", yours_style),
                        Span::styled(line.clone(), yours_style),
                    ]));
                }

                let sep_style = if is_active {
                    Style::default()
                        .fg(theme::PEACH)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme::SUBTEXT0)
                };
                lines_to_render.push(Line::from(vec![
                    Span::styled("====  ", sep_style),
                    Span::styled("======= Agent (on-disk)", sep_style),
                ]));

                let agent_style = Style::default().fg(theme::PEACH);
                for line in &hunk.agent {
                    lines_to_render.push(Line::from(vec![
                        Span::styled("  │   ", agent_style),
                        Span::styled(line.clone(), agent_style),
                    ]));
                }

                let footer_style = Style::default().fg(theme::SUBTEXT1);
                let footer_text =
                    format!(">>>>>>> End Hunk {}/{}", conflict_idx + 1, total_conflicts);
                lines_to_render.push(Line::from(vec![
                    Span::styled("►►►►  ", footer_style),
                    Span::styled(footer_text, footer_style),
                ]));

                conflict_idx += 1;
            }
        }
    }

    if active_hunk_start_row < buf.scroll_row {
        buf.scroll_row = active_hunk_start_row;
    } else if active_hunk_start_row >= buf.scroll_row + visible_height {
        buf.scroll_row = active_hunk_start_row.saturating_sub(visible_height / 2);
    }

    let total_rows = lines_to_render.len();
    let start_row = buf.scroll_row.min(total_rows.saturating_sub(1));
    let end_row = (start_row + visible_height).min(total_rows);

    let visible_lines: Vec<Line> = if start_row < total_rows {
        lines_to_render[start_row..end_row].to_vec()
    } else {
        Vec::new()
    };

    let paragraph = Paragraph::new(visible_lines);
    frame.render_widget(paragraph, inner);
}

fn render_preview_view(frame: &mut Frame, app: &mut App, inner: Rect) {
    if app.preview_lines.is_empty() {
        app.update_preview();
    }

    let visible_height = inner.height as usize;
    let total_preview_lines = app.preview_lines.len();
    if total_preview_lines == 0 {
        return;
    }

    // Scroll adjustment based on preview_cursor
    if app.preview_cursor < app.preview_scroll {
        app.preview_scroll = app.preview_cursor;
    } else if app.preview_cursor >= app.preview_scroll + visible_height {
        app.preview_scroll = app.preview_cursor.saturating_sub(visible_height) + 1;
    }

    let start_idx = app.preview_scroll;
    let end_idx = (start_idx + visible_height).min(total_preview_lines);

    let max_src_line = app.buf.as_ref().map_or(1, |b| b.lines.len());
    let gutter_digits = format!("{max_src_line}").len().max(3);

    let mut rendered_rows = Vec::new();
    let is_focused = app.focus == Focus::Middle;

    for idx in start_idx..end_idx {
        let p_line = &app.preview_lines[idx];
        let is_cursor = idx == app.preview_cursor && is_focused;

        let src_num = p_line.source_line + 1;
        let cursor_marker = if is_cursor { "▶" } else { " " };
        let gutter_text = format!("{cursor_marker}{src_num:>gutter_digits$} │ ");
        let gutter_style = if is_cursor {
            Style::default().fg(theme::MAUVE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::SURFACE2)
        };

        let mut spans = vec![Span::styled(gutter_text, gutter_style)];
        for span in &p_line.line.spans {
            let mut style = span.style;
            if is_cursor {
                style = style.bg(theme::MANTLE);
            }
            spans.push(Span::styled(span.content.clone(), style));
        }

        rendered_rows.push(Line::from(spans));
    }

    let p = Paragraph::new(rendered_rows);
    frame.render_widget(p, inner);
}

