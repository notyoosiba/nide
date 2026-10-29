use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, BorderType, Borders};
use ratatui::Frame;

use crate::app::{App, Focus};
use crate::pty::TerminalPane;
use crate::theme;

#[must_use]
pub fn ansi_to_theme(idx: u8) -> Color {
    match idx {
        0 => theme::SURFACE1,
        1 => theme::RED,
        2 => theme::GREEN,
        3 => theme::YELLOW,
        4 => theme::BLUE,
        5 => theme::MAUVE,
        6 => theme::TEAL,
        7 => theme::SUBTEXT0,
        8 => theme::SURFACE2,
        9 => theme::MAROON,
        10 => theme::GREEN,
        11 => theme::PEACH,
        12 => theme::SAPPHIRE,
        13 => theme::PINK,
        14 => theme::SKY,
        15 => theme::TEXT,
        16..=255 => Color::Indexed(idx),
    }
}

#[must_use]
pub fn vt100_color_to_ratatui_fg(color: vt100::Color) -> Color {
    match color {
        vt100::Color::Default => theme::TEXT,
        vt100::Color::Idx(idx) => ansi_to_theme(idx),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

#[must_use]
pub fn vt100_color_to_ratatui_bg(color: vt100::Color) -> Color {
    match color {
        vt100::Color::Default => theme::BASE,
        vt100::Color::Idx(idx) => ansi_to_theme(idx),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    draw_terminal_pane(
        frame,
        &mut app.right_term,
        area,
        "Terminal",
        app.focus == Focus::RightTerm,
    );
}

pub fn draw_terminal_pane(
    frame: &mut Frame,
    term: &mut TerminalPane,
    area: Rect,
    title_prefix: &str,
    is_active: bool,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

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

    let scrollback = term.screen().scrollback();
    let title = if scrollback > 0 {
        ratatui::text::Line::from(vec![
            ratatui::text::Span::styled(format!(" {title_prefix} "), title_style),
            ratatui::text::Span::styled(
                format!("[+{scrollback} lines] "),
                Style::default().fg(theme::PEACH).add_modifier(Modifier::BOLD),
            ),
        ])
    } else {
        ratatui::text::Line::from(ratatui::text::Span::styled(
            format!(" {title_prefix} "),
            title_style,
        ))
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(title)
        .style(Style::default().bg(theme::BASE).fg(theme::TEXT));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    // Resize PTY and parser with resize guard
    term.resize(inner.height, inner.width);

    // Render vt100 Screen cells into ratatui buffer
    let screen = term.screen();
    let buf = frame.buffer_mut();

    for row in 0..inner.height {
        for col in 0..inner.width {
            let target_x = inner.x + col;
            let target_y = inner.y + row;
            if target_x >= buf.area.width || target_y >= buf.area.height {
                continue;
            }

            let is_selected = term.is_cell_selected(row, col);

            let (sym, mut style) = if let Some(cell) = screen.cell(row, col) {
                let contents = cell.contents();
                let sym = if contents.is_empty() {
                    " "
                } else {
                    contents
                };

                let mut fg = vt100_color_to_ratatui_fg(cell.fgcolor());
                let mut bg = vt100_color_to_ratatui_bg(cell.bgcolor());

                if cell.inverse() {
                    std::mem::swap(&mut fg, &mut bg);
                }

                let mut style = Style::default().fg(fg).bg(bg);
                if cell.bold() {
                    style = style.add_modifier(Modifier::BOLD);
                }
                if cell.italic() {
                    style = style.add_modifier(Modifier::ITALIC);
                }
                if cell.underline() {
                    style = style.add_modifier(Modifier::UNDERLINED);
                }
                (sym, style)
            } else {
                (
                    " ",
                    Style::default().bg(theme::BASE).fg(theme::TEXT),
                )
            };

            if is_selected {
                style = style.bg(theme::SELECTION_BG).fg(theme::SELECTION_FG);
            }

            let buf_cell = &mut buf[(target_x, target_y)];
            buf_cell.set_symbol(sym);
            buf_cell.set_style(style);
        }
    }

    // If exited, render the respawn banner
    if term.is_exited() {
        let banner = "[Process exited — press Enter to respawn]";
        let (cursor_row, cursor_col) = screen.cursor_position();
        let banner_row = if cursor_col > 0 && cursor_row + 1 < inner.height {
            cursor_row + 1
        } else {
            cursor_row.min(inner.height.saturating_sub(1))
        };
        let start_y = inner.y + banner_row;
        let banner_style = Style::default()
            .fg(theme::MAROON)
            .bg(theme::BASE)
            .add_modifier(Modifier::BOLD);

        let mut row_offset = 0;
        let mut col_offset = 0;
        for ch in banner.chars() {
            if col_offset >= inner.width {
                col_offset = 0;
                row_offset += 1;
            }
            let target_y = start_y + row_offset;
            let target_x = inner.x + col_offset;
            if target_y < inner.y + inner.height
                && target_y < buf.area.height
                && target_x < buf.area.width
            {
                let buf_cell = &mut buf[(target_x, target_y)];
                let mut s = [0u8; 4];
                buf_cell.set_symbol(ch.encode_utf8(&mut s));
                buf_cell.set_style(banner_style);
            }
            col_offset += 1;
        }
    } else if is_active && !screen.hide_cursor() {
        let (cursor_row, cursor_col) = screen.cursor_position();
        let cursor_x = inner.x + cursor_col;
        let cursor_y = inner.y + cursor_row;
        if cursor_x < inner.x + inner.width && cursor_y < inner.y + inner.height {
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }
}
