use std::io::Cursor;
use std::sync::mpsc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use image::{ImageFormat, Rgba, RgbaImage};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tempfile::TempDir;
use nide::app::{App, MiddleMode, Msg};
use nide::ui;
use nide::viewer::{
    self, compute_placement_bounds, decode_media, decode_raster, decode_svg,
    encode_kitty_chunks, format_kitty_delete, get_cell_pixel_size, is_supported_media,
    CellPixelSize, MediaData, MediaState, KITTY_MAX_CHUNK_SIZE,
};

fn create_test_png(width: u32, height: u32) -> Vec<u8> {
    let mut img = RgbaImage::new(width, height);
    for pixel in img.pixels_mut() {
        *pixel = Rgba([120, 180, 240, 255]);
    }
    let mut bytes = Vec::new();
    img.write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .expect("write PNG");
    bytes
}

fn create_test_svg(width: u32, height: u32) -> Vec<u8> {
    let r = (width.min(height) / 4).max(1);
    let cx = width / 2;
    let cy = height / 2;
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">
            <rect width="{width}" height="{height}" fill="purple" />
            <circle cx="{cx}" cy="{cy}" r="{r}" fill="blue" />
        </svg>"#
    )
    .into_bytes()
}

#[test]
fn test_decode_in_memory_png_and_svg() {
    // 1. In-memory PNG decoding
    let png_bytes = create_test_png(32, 24);
    let decoded_png = decode_raster(&png_bytes).expect("decode PNG");
    assert_eq!(decoded_png.width, 32);
    assert_eq!(decoded_png.height, 24);
    assert_eq!(decoded_png.format, "PNG");
    assert!(!decoded_png.png_bytes.is_empty());

    // Generic decode_media dispatch for PNG
    let decoded_generic_png = decode_media(&png_bytes, None).expect("generic PNG decode");
    assert_eq!(decoded_generic_png.width, 32);
    assert_eq!(decoded_generic_png.height, 24);

    // 2. In-memory SVG decoding
    let svg_bytes = create_test_svg(64, 48);
    let decoded_svg = decode_svg(&svg_bytes).expect("decode SVG");
    assert_eq!(decoded_svg.width, 64);
    assert_eq!(decoded_svg.height, 48);
    assert_eq!(decoded_svg.format, "SVG");
    assert!(!decoded_svg.png_bytes.is_empty());

    // Generic decode_media dispatch for SVG
    let decoded_generic_svg = decode_media(&svg_bytes, None).expect("generic SVG decode");
    assert_eq!(decoded_generic_svg.width, 64);
    assert_eq!(decoded_generic_svg.height, 48);
    assert_eq!(decoded_generic_svg.format, "SVG");
}

#[test]
fn test_kitty_apc_chunk_formatting_single_and_multi_chunk() {
    let image_id = 42;

    // Single chunk case (payload <= 4096 bytes base64)
    let small_payload = create_test_png(4, 4);
    let single_chunks = encode_kitty_chunks(&small_payload, image_id);
    assert_eq!(single_chunks.len(), 1);

    let chunk0 = &single_chunks[0];
    assert!(
        chunk0.starts_with(&format!("\x1b_Ga=T,f=100,i={image_id},m=0;")),
        "single chunk must start with a=T,f=100,i={image_id},m=0;, got: {chunk0}"
    );
    assert!(chunk0.ends_with("\x1b\\"), "chunk must end with ST (\\x1b\\\\)");

    // Multi chunk case (payload > 4096 bytes base64)
    // 6000 raw bytes produces 8000 base64 chars -> exactly 2 chunks of 4096 + 3904
    let large_raw = vec![170u8; 6000];
    let multi_chunks = encode_kitty_chunks(&large_raw, image_id);
    assert!(
        multi_chunks.len() >= 2,
        "large payload must produce multiple chunks"
    );

    // First chunk: m=1
    let first = &multi_chunks[0];
    assert!(
        first.starts_with(&format!("\x1b_Ga=T,f=100,i={image_id},m=1;")),
        "first multi-chunk must have m=1, got: {first}"
    );
    assert!(first.ends_with("\x1b\\"));
    let first_payload = first
        .strip_prefix(&format!("\x1b_Ga=T,f=100,i={image_id},m=1;"))
        .unwrap()
        .strip_suffix("\x1b\\")
        .unwrap();
    assert_eq!(first_payload.len(), KITTY_MAX_CHUNK_SIZE);

    // Last chunk: m=0
    let last = multi_chunks.last().unwrap();
    assert!(
        last.starts_with("\x1b_Gm=0;"),
        "last chunk must have m=0, got: {last}"
    );
    assert!(last.ends_with("\x1b\\"));
    let last_payload = last
        .strip_prefix("\x1b_Gm=0;")
        .unwrap()
        .strip_suffix("\x1b\\")
        .unwrap();
    assert!(last_payload.len() <= KITTY_MAX_CHUNK_SIZE);

    // Intermediate chunks if any: m=1
    for mid in &multi_chunks[1..multi_chunks.len() - 1] {
        assert!(mid.starts_with("\x1b_Gm=1;"));
        assert!(mid.ends_with("\x1b\\"));
    }

    // Delete sequence format check
    let del = format_kitty_delete(image_id);
    assert_eq!(del, format!("\x1b_Ga=d,d=i,i={image_id},q=2\x1b\\"));
}

#[test]
fn test_stale_seq_results_are_dropped() {
    let temp = TempDir::new().unwrap();
    let (tx, _rx) = mpsc::channel();
    let mut app = App::with_sender(temp.path().to_path_buf(), Some(tx));

    app.media_seq = 1;
    let initial_data = MediaData {
        path: temp.path().join("first.png"),
        width: 100,
        height: 100,
        format: "PNG".into(),
        file_size: 500,
        png_bytes: vec![1, 2, 3],
        kitty_chunks: vec!["chunk1".into()],
        iterm2_sequence: String::new(),
        sixel_sequence: None,
        id: 1,
    };

    // Valid seq 1 message arrives
    app.handle_msg(Msg::MediaRendered {
        seq: 1,
        result: Ok(initial_data.clone()),
    });
    assert_eq!(app.media_state, Some(MediaState::Loaded(initial_data)));
    assert_eq!(app.active_image_id, Some(1));

    // Stale sequence test: app advances to seq 2
    app.media_seq = 2;

    // Stale result for seq 1 arrives (should be dropped)
    let stale_data = MediaData {
        path: temp.path().join("stale.png"),
        width: 999,
        height: 999,
        format: "PNG".into(),
        file_size: 9999,
        png_bytes: vec![9, 9, 9],
        kitty_chunks: vec!["stale_chunk".into()],
        iterm2_sequence: String::new(),
        sixel_sequence: None,
        id: 1,
    };
    app.handle_msg(Msg::MediaRendered {
        seq: 1,
        result: Ok(stale_data),
    });

    // Verify state was NOT overwritten by stale sequence
    if let Some(MediaState::Loaded(curr)) = &app.media_state {
        assert_eq!(curr.width, 100);
        assert_eq!(curr.height, 100);
    } else {
        panic!("expected loaded media state");
    }

    // Now valid message for seq 2 arrives
    let fresh_data = MediaData {
        path: temp.path().join("second.png"),
        width: 200,
        height: 150,
        format: "PNG".into(),
        file_size: 800,
        png_bytes: vec![4, 5, 6],
        kitty_chunks: vec!["chunk2".into()],
        iterm2_sequence: String::new(),
        sixel_sequence: None,
        id: 2,
    };
    app.handle_msg(Msg::MediaRendered {
        seq: 2,
        result: Ok(fresh_data),
    });

    if let Some(MediaState::Loaded(curr)) = &app.media_state {
        assert_eq!(curr.width, 200);
        assert_eq!(curr.height, 150);
        assert_eq!(app.active_image_id, Some(2));
    } else {
        panic!("expected fresh loaded media state");
    }
}

#[test]
fn test_collision_guard_emits_kitty_delete_on_quake_and_help() {
    let temp = TempDir::new().unwrap();
    let img_path = temp.path().join("photo.png");
    let png_bytes = create_test_png(40, 30);
    std::fs::write(&img_path, &png_bytes).unwrap();

    let mut app = App::new(temp.path().to_path_buf());

    // 1. Open image in MiddleMode::View
    app.open_file_at(&img_path, 0, 0).expect("open image");
    assert_eq!(app.middle_mode, MiddleMode::View);
    assert!(is_supported_media(&img_path));
    assert!(app.active_image_id.is_some());
    let img_id = app.active_image_id.unwrap();
    let expected_delete = format!("\x1b_Ga=d,d=i,i={img_id},q=2\x1b\\");

    // Clear any prior delete trace
    app.last_kitty_delete = None;
    app.kitty_erased = false;

    // 2. Toggle Quake overlay -> collision guard must emit delete sequence
    app.toggle_quake();
    assert!(app.show_quake);
    assert_eq!(
        app.last_kitty_delete.as_deref(),
        Some(expected_delete.as_str()),
        "collision guard must emit Kitty delete sequence when Quake overlay is active"
    );
    assert!(app.kitty_erased);

    // Close Quake overlay
    app.toggle_quake();
    assert!(!app.show_quake);

    // Reset delete sequence recorder
    app.last_kitty_delete = None;
    app.kitty_erased = false;

    // 3. Toggle Help overlay -> collision guard must emit delete sequence
    app.toggle_help();
    assert!(app.show_help);
    assert_eq!(
        app.last_kitty_delete.as_deref(),
        Some(expected_delete.as_str()),
        "collision guard must emit Kitty delete sequence when Help overlay is active"
    );
    assert!(app.kitty_erased);

    // Close Help overlay with Esc key
    app.handle_msg(Msg::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty())));
    assert!(!app.show_help);

    // 4. Opening fuzzy picker modal -> collision guard must emit delete sequence
    app.last_kitty_delete = None;
    app.kitty_erased = false;
    app.picker = Some(nide::picker::Picker::new(
        temp.path().to_path_buf(),
        vec![img_path.clone()],
        &[],
    ));
    viewer::handle_overlay_collision(&mut app);
    assert_eq!(
        app.last_kitty_delete.as_deref(),
        Some(expected_delete.as_str()),
        "collision guard must emit Kitty delete sequence when Picker modal is active"
    );
}

#[test]
fn test_safe_cell_sizing_and_placement_bounds() {
    let cell_size = get_cell_pixel_size();
    assert!(cell_size.width > 0);
    assert!(cell_size.height > 0);

    let area = ratatui::layout::Rect::new(10, 5, 40, 20);
    let mock_cell = CellPixelSize {
        width: 10,
        height: 20,
    };

    // Square 100x100 image into 40x20 cell area (available pixels: 400w x 400h)
    let bounds = compute_placement_bounds(100, 100, area, mock_cell);
    assert!(bounds.width <= area.width);
    assert!(bounds.height <= area.height);
    assert!(bounds.x >= area.x);
    assert!(bounds.y >= area.y);

    // Extreme aspect ratio (panoramic: 800w x 100h)
    let pano_bounds = compute_placement_bounds(800, 100, area, mock_cell);
    assert!(pano_bounds.width <= area.width);
    assert!(pano_bounds.height <= area.height);

    // Zero dimension guard
    let empty_bounds = compute_placement_bounds(0, 0, area, mock_cell);
    assert_eq!(empty_bounds.width, 0);
    assert_eq!(empty_bounds.height, 0);
}

#[test]
fn test_ui_rendering_middle_mode_view_in_test_backend() {
    let temp = TempDir::new().unwrap();
    let img_path = temp.path().join("drawing.svg");
    let svg_bytes = create_test_svg(50, 50);
    std::fs::write(&img_path, &svg_bytes).unwrap();

    let mut app = App::new(temp.path().to_path_buf());
    app.open_file_at(&img_path, 0, 0).expect("open svg");
    assert_eq!(app.middle_mode, MiddleMode::View);

    let backend = TestBackend::new(90, 30);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|frame| {
            ui::draw(frame, &mut app);
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let content = format!("{buffer:?}");

    // Middle pane title should indicate VIEW mode
    assert!(
        content.contains("[VIEW]"),
        "middle pane title should contain [VIEW]"
    );
    // Metadata header should display format and dimensions
    assert!(
        content.contains("Format: SVG") || content.contains("SVG"),
        "metadata header should indicate SVG format"
    );
    assert!(
        content.contains("50 × 50 px"),
        "metadata header should show dimensions 50 × 50 px"
    );

    // Now toggle Quake overlay and draw again
    app.toggle_quake();
    terminal
        .draw(|frame| {
            ui::draw(frame, &mut app);
        })
        .unwrap();

    assert!(app.kitty_erased, "kitty image must be marked erased when Quake is active");
    assert!(app.last_kitty_delete.is_some(), "delete sequence must have been emitted");
}
