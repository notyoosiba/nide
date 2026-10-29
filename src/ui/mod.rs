pub mod code;
pub mod overlays;
pub mod status;
pub mod term;
pub mod tree;
pub mod viewer;

use ratatui::Frame;

use crate::app::App;

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    if area.width == 0 || area.height == 0 {
        return;
    }

    // 1. Split vertically: main 3-pane area + 1-line status bar at bottom
    let layout = app.compute_layout(area);
    app.layout = layout;

    // Ensure focus is on a visible pane
    if !app.is_visible(app.focus) {
        let visible_panes = app.visible_panes();
        if let Some(prev) = app.prev_focus {
            if app.is_visible(prev) {
                app.focus = prev;
            } else if let Some(&first) = visible_panes.first() {
                app.focus = first;
            }
        } else if let Some(&first) = visible_panes.first() {
            app.focus = first;
        }
    }

    // 3. Render visible panes
    if let Some(area) = layout.tree {
        tree::draw(frame, app, area);
    }
    if let Some(area) = layout.middle {
        if app.middle_mode == crate::app::MiddleMode::View {
            viewer::draw(frame, app, area);
        } else {
            code::draw(frame, app, area);
        }
    }
    if let Some(area) = layout.right {
        term::draw(frame, app, area);
    }

    // 4. Render status bar
    status::draw(frame, app, layout.status);

    // 5. Render Quake overlay
    if app.show_quake {
        if let Some(quake_rect) = layout.quake {
            overlays::draw_quake(frame, app, quake_rect);
        }
    }

    // 6. Render Help overlay
    if app.show_help {
        overlays::draw_help(frame, area);
    }

    // 7. Render overlays (e.g. Fuzzy File Picker)
    if let Some(picker) = &mut app.picker {
        overlays::draw_picker(frame, picker, area);
    }

    // 8. Overlay collision check for MiddleMode::View (Kitty image erase guard)
    crate::viewer::handle_overlay_collision(app);
}
