use std::io::Cursor;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use image::{ImageFormat, Rgb, RgbImage};
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::Terminal;
use tempfile::TempDir;
use nide::app::{App, Focus, MiddleMode, Msg};
use nide::ui;
use nide::viewer::{
    detect_graphics_protocol, encode_iterm2, encode_sixel_from_png,
    render_half_blocks, GraphicsProtocol,
};

fn create_rgb_test_png(
    width: u32,
    height: u32,
    top_color: [u8; 3],
    bot_color: [u8; 3],
) -> Vec<u8> {
    let mut img = RgbImage::new(width, height);
    let mid_y = height / 2;
    for y in 0..height {
        for x in 0..width {
            let col = if y < mid_y { top_color } else { bot_color };
            img.put_pixel(x, y, Rgb(col));
        }
    }
    let mut bytes = Vec::new();
    img.write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .expect("write PNG");
    bytes
}

#[test]
fn test_iterm2_osc_1337_output_and_unicode_halfblock_rendering() {
    // 1. Verify iTerm2 OSC 1337 escape sequence generation
    let red_top = [255, 0, 0];
    let blue_bot = [0, 0, 255];
    let png_bytes = create_rgb_test_png(4, 2, red_top, blue_bot);

    let iterm2_out = encode_iterm2(&png_bytes);
    assert!(
        iterm2_out.starts_with("\x1b]1337;File=inline=1:"),
        "iTerm2 output must start with OSC 1337 File=inline=1 header, got: {iterm2_out:?}"
    );
    assert!(
        iterm2_out.ends_with('\x07'),
        "iTerm2 output must end with BEL (\\x07)"
    );

    // Verify base64 payload reconstructs the exact PNG data
    let payload = iterm2_out
        .trim_start_matches("\x1b]1337;File=inline=1:")
        .trim_end_matches('\x07');
    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .expect("decode base64");
    assert_eq!(decoded, png_bytes);

    // 2. Verify Unicode half-block (▀) cell rendering in TestBackend
    let backend = TestBackend::new(10, 5);
    let mut terminal = Terminal::new(backend).expect("create terminal");

    let area = Rect::new(1, 1, 4, 1);
    terminal
        .draw(|frame| {
            render_half_blocks(frame, area, &png_bytes).expect("render half blocks");
        })
        .expect("draw");

    let buffer = terminal.backend().buffer();
    for col in 0..4 {
        let cell = buffer.cell((1 + col, 1)).expect("cell exists");
        assert_eq!(
            cell.symbol(),
            "▀",
            "half-block driver must render '▀' symbol"
        );
        assert_eq!(
            cell.fg,
            Color::Rgb(255, 0, 0),
            "top subpixel must be red foreground"
        );
        assert_eq!(
            cell.bg,
            Color::Rgb(0, 0, 255),
            "bottom subpixel must be blue background"
        );
    }
}

#[test]
fn test_ui_renders_halfblocks_when_viewing_image_in_test_backend() {
    let temp = TempDir::new().unwrap();
    let img_path = temp.path().join("palette.png");
    let red_top = [220, 50, 50];
    let green_bot = [50, 200, 50];
    let png_bytes = create_rgb_test_png(16, 8, red_top, green_bot);
    std::fs::write(&img_path, &png_bytes).unwrap();

    let mut app = App::new(temp.path().to_path_buf());
    app.graphics_protocol = GraphicsProtocol::HalfBlocks;

    app.open_file_at(&img_path, 0, 0).expect("open image");
    assert_eq!(app.middle_mode, MiddleMode::View);

    let backend = TestBackend::new(110, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|frame| {
            ui::draw(frame, &mut app);
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let content = format!("{buffer:?}");

    // Header must show Driver: HalfBlocks
    assert!(
        content.contains("Driver: HalfBlocks") || content.contains("HalfBlocks"),
        "viewer header should show HalfBlocks driver"
    );

    // Buffer must contain '▀' cells from half-block rendering
    let has_halfblock = (0..buffer.area.width).any(|x| {
        (0..buffer.area.height).any(|y| {
            buffer
                .cell((x, y))
                .is_some_and(|c| c.symbol() == "▀")
        })
    });
    assert!(
        has_halfblock,
        "TestBackend buffer must contain '▀' cells when using HalfBlocks graphics protocol"
    );
}

#[test]
fn test_iterm2_and_sixel_emission_in_app() {
    let temp = TempDir::new().unwrap();
    let img_path = temp.path().join("icon.png");
    let png_bytes = create_rgb_test_png(8, 4, [100, 150, 200], [50, 80, 120]);
    std::fs::write(&img_path, &png_bytes).unwrap();

    // Sixel verification
    let sixel_seq = encode_sixel_from_png(&png_bytes).expect("encode sixel");
    assert!(
        sixel_seq.starts_with("\x1bPq\"1;1;8;4"),
        "sixel sequence must have introducer and raster attributes, got: {sixel_seq:?}"
    );
    assert!(
        sixel_seq.ends_with("\x1b\\"),
        "sixel sequence must end with ST (\\x1b\\\\)"
    );

    // Test emission in App with iTerm2 protocol
    let mut app = App::new(temp.path().to_path_buf());
    app.graphics_protocol = GraphicsProtocol::Iterm2;
    app.open_file_at(&img_path, 0, 0).expect("open image");

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|frame| {
            ui::draw(frame, &mut app);
        })
        .unwrap();

    assert!(
        app.last_iterm2_sequence.is_some(),
        "app must emit iTerm2 sequence when graphics_protocol is Iterm2"
    );
    let seq = app.last_iterm2_sequence.as_ref().unwrap();
    assert!(seq.starts_with("\x1b]1337;File=inline=1:"));

    // Test emission in App with Sixel protocol
    app.graphics_protocol = GraphicsProtocol::Sixel;
    app.last_sixel_sequence = None;

    terminal
        .draw(|frame| {
            ui::draw(frame, &mut app);
        })
        .unwrap();

    assert!(
        app.last_sixel_sequence.is_some(),
        "app must emit Sixel sequence when graphics_protocol is Sixel"
    );
    let s_seq = app.last_sixel_sequence.as_ref().unwrap();
    assert!(s_seq.starts_with("\x1bPq\"1;1;8;4"));
}

#[test]
fn test_code_folding_multiline_rust_function_and_navigation() {
    let temp = TempDir::new().unwrap();
    let rust_code = r#"fn compute_sum(a: i32, b: i32) -> i32 {
    let sum = a + b;
    println!("sum: {}", sum);
    sum
}

fn next_function() {
    println!("next");
}
"#;
    let file_path = temp.path().join("main.rs");
    std::fs::write(&file_path, rust_code).unwrap();

    let mut app = App::new(temp.path().to_path_buf());
    app.open_file_at(&file_path, 0, 0).expect("open file");

    // Put editor in MiddleMode::Normal with Middle focus
    app.focus = Focus::Middle;
    app.middle_mode = MiddleMode::Normal;
    if let Some(buf) = &mut app.buf {
        buf.cursor_row = 0;
        buf.cursor_col = 0;
    }

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    // 1. Initial render before fold: body lines are visible
    terminal
        .draw(|frame| {
            ui::draw(frame, &mut app);
        })
        .unwrap();

    let content_initial = format!("{:?}", terminal.backend().buffer());
    assert!(
        content_initial.contains("let sum = a + b;"),
        "body line 1 must be visible initially"
    );
    assert!(
        content_initial.contains("println!(\"sum: {}\", sum);"),
        "body line 2 must be visible initially"
    );
    assert!(
        !content_initial.contains("⋯"),
        "fold indicator must NOT be visible before fold"
    );

    // 2. Press 'f' in MiddleMode::Normal on the function definition line
    app.handle_msg(Msg::Key(KeyEvent::new(
        KeyCode::Char('f'),
        KeyModifiers::empty(),
    )));

    // Verify fold is recorded in buffer
    let buf = app.buf.as_ref().unwrap();
    assert_eq!(buf.folds.len(), 1, "exactly one fold should be created");
    assert_eq!(buf.folds[0].start_row, 0);
    assert_eq!(buf.folds[0].end_row, 4);

    // 3. Render after fold: body lines fold behind '⋯'
    terminal
        .draw(|frame| {
            ui::draw(frame, &mut app);
        })
        .unwrap();

    let content_folded = format!("{:?}", terminal.backend().buffer());
    assert!(
        content_folded.contains("⋯"),
        "folded function must display '⋯' indicator, got: {content_folded}"
    );
    assert!(
        !content_folded.contains("let sum = a + b;"),
        "body line 1 must be folded behind '⋯' and NOT rendered"
    );
    assert!(
        !content_folded.contains("println!(\"sum: {}\", sum);"),
        "body line 2 must be folded behind '⋯' and NOT rendered"
    );
    assert!(
        content_folded.contains("next_function"),
        "subsequent function must remain visible"
    );

    // 4. Cursor movement: pressing 'j' skips the folded body lines
    assert_eq!(app.buf.as_ref().unwrap().cursor_row, 0);
    app.handle_msg(Msg::Key(KeyEvent::new(
        KeyCode::Char('j'),
        KeyModifiers::empty(),
    )));

    // After pressing 'j', cursor must skip past rows 1..=4 and land on row 5 or 6 (next unfolded line)
    let cur_row_after_j = app.buf.as_ref().unwrap().cursor_row;
    assert!(
        cur_row_after_j > 4,
        "cursor row after 'j' must skip folded body lines (1..=4), landed on: {cur_row_after_j}"
    );

    // 5. Cursor movement: pressing 'k' skips backward to the fold start
    app.handle_msg(Msg::Key(KeyEvent::new(
        KeyCode::Char('k'),
        KeyModifiers::empty(),
    )));
    let cur_row_after_k = app.buf.as_ref().unwrap().cursor_row;
    assert_eq!(
        cur_row_after_k, 0,
        "cursor row after 'k' must skip backward over folded lines to start_row 0"
    );

    // 6. Pressing 'f' again unfolds them
    app.handle_msg(Msg::Key(KeyEvent::new(
        KeyCode::Char('f'),
        KeyModifiers::empty(),
    )));

    assert_eq!(
        app.buf.as_ref().unwrap().folds.len(),
        0,
        "pressing 'f' again must unfold the function"
    );

    terminal
        .draw(|frame| {
            ui::draw(frame, &mut app);
        })
        .unwrap();

    let content_unfolded = format!("{:?}", terminal.backend().buffer());
    assert!(
        content_unfolded.contains("let sum = a + b;"),
        "body line 1 must be visible again after unfolding"
    );
    assert!(
        content_unfolded.contains("println!(\"sum: {}\", sum);"),
        "body line 2 must be visible again after unfolding"
    );

    // Now 'j' visits each line sequentially
    app.handle_msg(Msg::Key(KeyEvent::new(
        KeyCode::Char('j'),
        KeyModifiers::empty(),
    )));
    assert_eq!(
        app.buf.as_ref().unwrap().cursor_row,
        1,
        "unfolded navigation must visit line 1"
    );

    app.handle_msg(Msg::Key(KeyEvent::new(
        KeyCode::Char('j'),
        KeyModifiers::empty(),
    )));
    assert_eq!(
        app.buf.as_ref().unwrap().cursor_row,
        2,
        "unfolded navigation must visit line 2"
    );
}

#[test]
fn test_code_folding_python_indentation() {
    let temp = TempDir::new().unwrap();
    let py_code = r#"def calculate(x, y):
    val = x * y
    print(val)
    return val

def after():
    pass
"#;
    let file_path = temp.path().join("calc.py");
    std::fs::write(&file_path, py_code).unwrap();

    let mut app = App::new(temp.path().to_path_buf());
    app.open_file_at(&file_path, 0, 0).expect("open file");
    app.focus = Focus::Middle;
    app.middle_mode = MiddleMode::Normal;

    if let Some(buf) = &mut app.buf {
        buf.cursor_row = 0;
    }

    // Fold Python function
    app.handle_msg(Msg::Key(KeyEvent::new(
        KeyCode::Char('f'),
        KeyModifiers::empty(),
    )));

    let fold_end = {
        let buf = app.buf.as_ref().unwrap();
        assert_eq!(buf.folds.len(), 1, "Python function should fold");
        assert_eq!(buf.folds[0].start_row, 0);
        assert!(buf.folds[0].end_row >= 3);
        buf.folds[0].end_row
    };

    // Move down skips folded body
    app.handle_msg(Msg::Key(KeyEvent::new(
        KeyCode::Char('j'),
        KeyModifiers::empty(),
    )));
    let row = app.buf.as_ref().unwrap().cursor_row;
    assert!(
        row > fold_end || row == 4 || row == 5,
        "cursor must skip to after folded python body"
    );

    // Pressing 'f' unfolds
    if let Some(buf) = &mut app.buf {
        buf.cursor_row = 0;
    }
    app.handle_msg(Msg::Key(KeyEvent::new(
        KeyCode::Char('f'),
        KeyModifiers::empty(),
    )));
    assert_eq!(app.buf.as_ref().unwrap().folds.len(), 0);
}

#[test]
fn test_graphics_protocol_detection_override() {
    // Test env override
    std::env::set_var("NIDE_GRAPHICS", "iterm2");
    assert_eq!(detect_graphics_protocol(), GraphicsProtocol::Iterm2);

    std::env::set_var("NIDE_GRAPHICS", "sixel");
    assert_eq!(detect_graphics_protocol(), GraphicsProtocol::Sixel);

    std::env::set_var("NIDE_GRAPHICS", "halfblocks");
    assert_eq!(detect_graphics_protocol(), GraphicsProtocol::HalfBlocks);

    std::env::set_var("NIDE_GRAPHICS", "kitty");
    assert_eq!(detect_graphics_protocol(), GraphicsProtocol::Kitty);

    std::env::remove_var("NIDE_GRAPHICS");
}
