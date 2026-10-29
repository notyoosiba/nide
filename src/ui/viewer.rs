use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;

use crate::app::{App, Focus};
use crate::theme;
use crate::viewer::{self, MediaState};

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let is_focused = app.focus == Focus::Middle;
    let border_color = if is_focused {
        theme::BORDER_ACTIVE
    } else {
        theme::BORDER_INACTIVE
    };
    let title_style = if is_focused {
        Style::default()
            .fg(theme::BORDER_ACTIVE)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::SUBTEXT0)
    };

    let title_name = app
        .buf
        .as_ref()
        .and_then(|b| b.path.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("Media Viewer");

    let title = format!(" Editor: {title_name} [VIEW] ");

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

    match &app.media_state {
        Some(MediaState::Loading { path, .. }) => {
            let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let msg = Paragraph::new(vec![
                Line::from(Span::styled(
                    " Loading image... ",
                    Style::default().fg(theme::YELLOW).add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    format!(" File: {filename}"),
                    Style::default().fg(theme::SUBTEXT0),
                )),
            ]);
            frame.render_widget(msg, inner);
        }
        Some(MediaState::Error { error, .. }) => {
            let msg = Paragraph::new(vec![
                Line::from(Span::styled(
                    " [Image Decoder Error] ",
                    Style::default().fg(theme::RED).add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    format!(" {error}"),
                    Style::default().fg(theme::TEXT),
                )),
            ]);
            frame.render_widget(msg, inner);
        }
        Some(MediaState::Loaded(media)) => {
            render_loaded_media(frame, inner, media, app.graphics_protocol);
        }
        None => {
            let msg = Paragraph::new(" No image loaded ")
                .style(Style::default().fg(theme::SUBTEXT0));
            frame.render_widget(msg, inner);
        }
    }

    if let Some(MediaState::Loaded(media)) = &app.media_state {
        if !viewer::overlay_collides_with_middle(app) {
            match app.graphics_protocol {
                viewer::GraphicsProtocol::Kitty => {
                    if app.kitty_erased && !media.kitty_chunks.is_empty() {
                        let chunks = media.kitty_chunks.clone();
                        viewer::emit_kitty_chunks(app, &chunks);
                    }
                }
                viewer::GraphicsProtocol::Iterm2 => {
                    if app.last_iterm2_sequence.as_ref() != Some(&media.iterm2_sequence) {
                        let seq = media.iterm2_sequence.clone();
                        viewer::emit_iterm2(app, &seq);
                    }
                }
                viewer::GraphicsProtocol::Sixel => {
                    if let Some(seq) = &media.sixel_sequence {
                        if app.last_sixel_sequence.as_ref() != Some(seq) {
                            let seq = seq.clone();
                            viewer::emit_sixel(app, &seq);
                        }
                    }
                }
                viewer::GraphicsProtocol::HalfBlocks => {}
            }
        }
    }
}

fn render_loaded_media(
    frame: &mut Frame,
    area: Rect,
    media: &viewer::MediaData,
    protocol: viewer::GraphicsProtocol,
) {
    if area.height < 3 || area.width < 10 {
        return;
    }

    // Split vertically: metadata header (2 lines) + placement area
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(area);

    let header_area = chunks[0];
    let content_area = chunks[1];

    let cell_size = viewer::get_cell_pixel_size();
    let size_kb = (media.file_size as f64) / 1024.0;
    let size_str = if size_kb >= 1024.0 {
        format!("{:.1} MB", size_kb / 1024.0)
    } else {
        format!("{:.1} KB", size_kb)
    };

    let placement_rect = viewer::compute_placement_bounds(
        media.width,
        media.height,
        content_area,
        cell_size,
    );

    let header_lines = vec![
        Line::from(vec![
            Span::styled(" Format: ", Style::default().fg(theme::BLUE).add_modifier(Modifier::BOLD)),
            Span::styled(&media.format, Style::default().fg(theme::TEXT)),
            Span::styled("  |  Dimensions: ", Style::default().fg(theme::BLUE).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{} × {} px", media.width, media.height), Style::default().fg(theme::TEXT)),
            Span::styled("  |  Size: ", Style::default().fg(theme::BLUE).add_modifier(Modifier::BOLD)),
            Span::styled(size_str, Style::default().fg(theme::TEXT)),
        ]),
        Line::from(vec![
            Span::styled(" Placement: ", Style::default().fg(theme::SUBTEXT0)),
            Span::styled(format!("{} × {} cells", placement_rect.width, placement_rect.height), Style::default().fg(theme::TEXT)),
            Span::styled("  |  Driver: ", Style::default().fg(theme::SUBTEXT0)),
            Span::styled(protocol.as_str(), Style::default().fg(theme::TEXT)),
            Span::styled("  |  Cell: ", Style::default().fg(theme::SUBTEXT0)),
            Span::styled(format!("{} × {} px", cell_size.width, cell_size.height), Style::default().fg(theme::TEXT)),
        ]),
    ];
    frame.render_widget(Paragraph::new(header_lines), header_area);

    if placement_rect.width > 0 && placement_rect.height > 0 {
        let placement_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme::SURFACE2))
            .title(format!(" {} ({}×{} px) ", media.format, media.width, media.height))
            .title_style(Style::default().fg(theme::SUBTEXT0))
            .style(Style::default().bg(theme::MANTLE));

        let inner_placement = placement_block.inner(placement_rect);
        frame.render_widget(placement_block, placement_rect);

        if protocol == viewer::GraphicsProtocol::HalfBlocks {
            let draw_area = if inner_placement.width > 0 && inner_placement.height > 0 {
                inner_placement
            } else {
                placement_rect
            };
            let _ = viewer::render_half_blocks(frame, draw_area, &media.png_bytes);
        }
    }
}
