use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;

use crate::app::{App, Focus, TreePromptKind};
use crate::theme;

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    let is_active = app.focus == Focus::Tree;

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

    let title = if let Some(prompt) = &app.tree_prompt {
        match prompt.kind {
            TreePromptKind::Create => " Tree [New] ",
            TreePromptKind::Rename { .. } => " Tree [Rename] ",
        }
    } else {
        " Tree "
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

    let (tree_area, prompt_area) = if app.tree_prompt.is_some() && inner.height >= 2 {
        (
            Rect {
                x: inner.x,
                y: inner.y,
                width: inner.width,
                height: inner.height.saturating_sub(1),
            },
            Some(Rect {
                x: inner.x,
                y: inner.y + inner.height.saturating_sub(1),
                width: inner.width,
                height: 1,
            }),
        )
    } else if app.tree_prompt.is_some() {
        (
            Rect {
                x: inner.x,
                y: inner.y,
                width: inner.width,
                height: 0,
            },
            Some(inner),
        )
    } else {
        (inner, None)
    };

    let visible_height = tree_area.height as usize;
    app.tree.adjust_scroll(visible_height);

    let visible_indices = app.tree.visible_indices();
    if !visible_indices.is_empty() && visible_height > 0 {
        let start = app.tree.scroll;
        let end = (start + visible_height).min(visible_indices.len());
        let inner_width = tree_area.width as usize;

        let mut lines = Vec::new();

        for (offset, &node_idx) in visible_indices[start..end].iter().enumerate() {
            let vis_idx = start + offset;
            let node = &app.tree.nodes[node_idx];
            let is_selected = vis_idx == app.tree.cursor;

            let indent = "  ".repeat(node.depth);
            let prefix = if node.is_dir {
                if node.expanded {
                    "▾ "
                } else {
                    "▸ "
                }
            } else {
                "  "
            };
            let name = node.name();

            let fg = if node.ignored {
                theme::OVERLAY0
            } else if let Some(status) = app.git_status_for(&node.path, node.is_dir) {
                theme::git_status_color(status).unwrap_or(if node.is_dir {
                    theme::BLUE
                } else {
                    theme::TEXT
                })
            } else if node.is_dir {
                theme::BLUE
            } else {
                theme::TEXT
            };

            let bg = if is_selected {
                if is_active {
                    theme::SURFACE1
                } else {
                    theme::SURFACE0
                }
            } else {
                theme::BASE
            };

            let mut style = Style::default().fg(fg).bg(bg);
            if node.is_dir && !node.ignored {
                style = style.add_modifier(Modifier::BOLD);
            }
            if node.ignored {
                style = style.add_modifier(Modifier::DIM);
            }

            let row_text = format!("{indent}{prefix}{name}");
            let padded = format!("{row_text:<width$}", width = inner_width);

            lines.push(Line::from(vec![Span::styled(padded, style)]));
        }

        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, tree_area);
    }

    if let (Some(prompt_area), Some(prompt)) = (prompt_area, &app.tree_prompt) {
        let label = match prompt.kind {
            TreePromptKind::Create => "New: ",
            TreePromptKind::Rename { .. } => "Rename: ",
        };
        let text = prompt.input.text();
        let cursor_idx = prompt.input.cursor();
        let before: String = text.chars().take(cursor_idx).collect();
        let at_cursor: String = text.chars().skip(cursor_idx).take(1).collect();
        let at_cursor_str = if at_cursor.is_empty() {
            " ".to_string()
        } else {
            at_cursor
        };
        let after: String = text.chars().skip(cursor_idx + 1).collect();

        let prompt_line = Line::from(vec![
            Span::styled(
                label,
                Style::default()
                    .fg(theme::BLUE)
                    .bg(theme::SURFACE0)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(before, Style::default().fg(theme::TEXT).bg(theme::SURFACE0)),
            Span::styled(
                at_cursor_str,
                Style::default().fg(theme::BASE).bg(theme::TEXT),
            ),
            Span::styled(after, Style::default().fg(theme::TEXT).bg(theme::SURFACE0)),
        ]);
        let p = Paragraph::new(prompt_line).style(Style::default().bg(theme::SURFACE0));
        frame.render_widget(p, prompt_area);
    }
}
