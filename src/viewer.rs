use std::path::{Path, PathBuf};

use ratatui::layout::Rect;

use crate::app::{App, MiddleMode};

pub const KITTY_MAX_CHUNK_SIZE: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GraphicsProtocol {
    Kitty,
    Iterm2,
    Sixel,
    #[default]
    HalfBlocks,
}

impl GraphicsProtocol {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Kitty => "Kitty",
            Self::Iterm2 => "iTerm2",
            Self::Sixel => "Sixel",
            Self::HalfBlocks => "HalfBlocks",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedMedia {
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub file_size: usize,
    pub png_bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaData {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub file_size: usize,
    pub png_bytes: Vec<u8>,
    pub kitty_chunks: Vec<String>,
    pub iterm2_sequence: String,
    pub sixel_sequence: Option<String>,
    pub id: u32,
}

impl MediaData {
    #[must_use]
    pub fn new(path: PathBuf, dec: DecodedMedia, image_id: u32) -> Self {
        let kitty_chunks = encode_kitty_chunks(&dec.png_bytes, image_id);
        let iterm2_sequence = encode_iterm2(&dec.png_bytes);
        let sixel_sequence = encode_sixel_from_png(&dec.png_bytes).ok();
        Self {
            path,
            width: dec.width,
            height: dec.height,
            format: dec.format,
            file_size: dec.file_size,
            png_bytes: dec.png_bytes,
            kitty_chunks,
            iterm2_sequence,
            sixel_sequence,
            id: image_id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaState {
    Loading { seq: u64, path: PathBuf },
    Loaded(MediaData),
    Error { seq: u64, path: PathBuf, error: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellPixelSize {
    pub width: u16,
    pub height: u16,
}

#[must_use]
pub fn is_supported_media(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    let ext = ext.to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "tiff" | "tif" | "svg"
    )
}

#[must_use]
pub fn is_svg_bytes(bytes: &[u8]) -> bool {
    let sample_len = bytes.len().min(1024);
    let sample = match std::str::from_utf8(&bytes[..sample_len]) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let trimmed = sample.trim_start();
    trimmed.starts_with("<svg")
        || (trimmed.starts_with("<?xml") && trimmed.contains("<svg"))
}

pub fn decode_svg(bytes: &[u8]) -> Result<DecodedMedia, String> {
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(bytes, &opt)
        .map_err(|e| format!("Failed to parse SVG: {e}"))?;
    let size = tree.size().to_int_size();
    let width = size.width();
    let height = size.height();
    if width == 0 || height == 0 {
        return Err("SVG has zero dimensions".to_string());
    }
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| "Failed to allocate pixmap for SVG".to_string())?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::default(),
        &mut pixmap.as_mut(),
    );
    let png_bytes = pixmap
        .encode_png()
        .map_err(|e| format!("Failed to encode SVG as PNG: {e}"))?;

    Ok(DecodedMedia {
        width,
        height,
        format: "SVG".to_string(),
        file_size: bytes.len(),
        png_bytes,
    })
}

pub fn decode_raster(bytes: &[u8]) -> Result<DecodedMedia, String> {
    let img = image::load_from_memory(bytes)
        .map_err(|e| format!("Failed to decode image: {e}"))?;
    let width = img.width();
    let height = img.height();

    let format = match image::guess_format(bytes).ok() {
        Some(image::ImageFormat::Png) => "PNG",
        Some(image::ImageFormat::Jpeg) => "JPEG",
        Some(image::ImageFormat::Gif) => "GIF",
        Some(image::ImageFormat::WebP) => "WebP",
        Some(image::ImageFormat::Bmp) => "BMP",
        Some(image::ImageFormat::Ico) => "ICO",
        Some(image::ImageFormat::Tiff) => "TIFF",
        _ => "IMAGE",
    }
    .to_string();

    let png_bytes = if format == "PNG" {
        bytes.to_vec()
    } else {
        let mut out = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut out);
        img.write_to(&mut cursor, image::ImageFormat::Png)
            .map_err(|e| format!("Failed to encode to PNG: {e}"))?;
        out
    };

    Ok(DecodedMedia {
        width,
        height,
        format,
        file_size: bytes.len(),
        png_bytes,
    })
}

pub fn decode_media(bytes: &[u8], path: Option<&Path>) -> Result<DecodedMedia, String> {
    let is_svg = path
        .and_then(|p| p.extension())
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
        || is_svg_bytes(bytes);

    if is_svg {
        decode_svg(bytes)
    } else {
        decode_raster(bytes)
    }
}

#[must_use]
pub fn encode_kitty_chunks(png_bytes: &[u8], image_id: u32) -> Vec<String> {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(png_bytes);
    if b64.is_empty() {
        return vec![format!("\x1b_Ga=T,f=100,i={image_id},m=0;\x1b\\")];
    }

    let mut chunks = Vec::new();
    let total_len = b64.len();
    let mut offset = 0;
    let mut chunk_idx = 0;

    while offset < total_len {
        let end = (offset + KITTY_MAX_CHUNK_SIZE).min(total_len);
        let slice = &b64[offset..end];
        let has_more = end < total_len;
        let m = if has_more { 1 } else { 0 };

        if chunk_idx == 0 {
            chunks.push(format!("\x1b_Ga=T,f=100,i={image_id},m={m};{slice}\x1b\\"));
        } else {
            chunks.push(format!("\x1b_Gm={m};{slice}\x1b\\"));
        }

        offset = end;
        chunk_idx += 1;
    }

    chunks
}

#[must_use]
pub fn format_kitty_delete(image_id: u32) -> String {
    format!("\x1b_Ga=d,d=i,i={image_id},q=2\x1b\\")
}

#[must_use]
pub fn get_cell_pixel_size() -> CellPixelSize {
    if let Ok(size) = crossterm::terminal::window_size() {
        if size.columns > 0 && size.rows > 0 && size.width > 0 && size.height > 0 {
            let w = size.width / size.columns;
            let h = size.height / size.rows;
            if w > 0 && h > 0 {
                return CellPixelSize { width: w, height: h };
            }
        }
    }
    // Safe default fallback cell dimensions
    CellPixelSize { width: 10, height: 20 }
}

#[must_use]
pub fn compute_placement_bounds(
    img_w: u32,
    img_h: u32,
    area: Rect,
    cell_size: CellPixelSize,
) -> Rect {
    if area.width == 0 || area.height == 0 || img_w == 0 || img_h == 0 {
        return Rect::default();
    }

    let avail_px_w = u64::from(area.width) * u64::from(cell_size.width);
    let avail_px_h = u64::from(area.height) * u64::from(cell_size.height);

    let scale_w = avail_px_w as f64 / f64::from(img_w);
    let scale_h = avail_px_h as f64 / f64::from(img_h);
    let scale = scale_w.min(scale_h).min(1.0);

    let target_px_w = (f64::from(img_w) * scale).round() as u64;
    let target_px_h = (f64::from(img_h) * scale).round() as u64;

    let cell_w = u64::from(cell_size.width).max(1);
    let cell_h = u64::from(cell_size.height).max(1);

    let target_cols = target_px_w
        .div_ceil(cell_w)
        .max(1)
        .min(u64::from(area.width)) as u16;
    let target_rows = target_px_h
        .div_ceil(cell_h)
        .max(1)
        .min(u64::from(area.height)) as u16;

    let offset_x = area.x + (area.width.saturating_sub(target_cols)) / 2;
    let offset_y = area.y + (area.height.saturating_sub(target_rows)) / 2;

    Rect::new(offset_x, offset_y, target_cols, target_rows)
}

#[must_use]
pub fn rects_intersect(a: Rect, b: Rect) -> bool {
    a.x < b.x.saturating_add(b.width)
        && a.x.saturating_add(a.width) > b.x
        && a.y < b.y.saturating_add(b.height)
        && a.y.saturating_add(a.height) > b.y
}

#[must_use]
pub fn overlay_collides_with_middle(app: &App) -> bool {
    if !app.show_middle {
        return false;
    }

    if app.picker.is_some() || app.show_help {
        return true;
    }

    if app.show_quake {
        if let (Some(quake), Some(middle)) = (app.layout.quake, app.layout.middle) {
            return rects_intersect(quake, middle);
        }
        return true;
    }

    false
}

pub fn handle_overlay_collision(app: &mut App) {
    if app.middle_mode != MiddleMode::View {
        return;
    }
    let Some(id) = app.active_image_id else {
        return;
    };

    if overlay_collides_with_middle(app) {
        if !app.kitty_erased {
            emit_kitty_delete(app, id);
        }
    } else if app.kitty_erased {
        // Overlay closed; clear erased flag so Kitty graphics can be shown
        app.kitty_erased = false;
    }
}

pub fn emit_kitty_delete(app: &mut App, id: u32) {
    let seq = format_kitty_delete(id);
    app.last_kitty_delete = Some(seq.clone());
    app.kitty_erased = true;
    use std::io::Write;
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(seq.as_bytes());
    let _ = stdout.flush();
}

pub fn emit_kitty_chunks(app: &mut App, chunks: &[String]) {
    use std::io::Write;
    let mut stdout = std::io::stdout();
    for chunk in chunks {
        app.last_kitty_sequence = Some(chunk.clone());
        let _ = stdout.write_all(chunk.as_bytes());
    }
    let _ = stdout.flush();
    app.kitty_erased = false;
}

#[must_use]
pub fn detect_graphics_protocol() -> GraphicsProtocol {
    if let Ok(val) =
        std::env::var("NIDE_GRAPHICS").or_else(|_| std::env::var("NIDE_GRAPHICS_PROTOCOL"))
    {
        match val.to_ascii_lowercase().as_str() {
            "kitty" => return GraphicsProtocol::Kitty,
            "iterm2" | "iterm" | "osc1337" => return GraphicsProtocol::Iterm2,
            "sixel" => return GraphicsProtocol::Sixel,
            "halfblocks" | "halfblock" | "half_blocks" | "unicode" | "blocks" => {
                return GraphicsProtocol::HalfBlocks;
            }
            _ => {}
        }
    }

    if std::env::var("KITTY_WINDOW_ID").is_ok()
        || std::env::var("GHOSTTY_RESOURCES_DIR").is_ok()
        || std::env::var("TERM").is_ok_and(|t| t.contains("kitty"))
    {
        return GraphicsProtocol::Kitty;
    }

    if let Ok(prog) = std::env::var("TERM_PROGRAM") {
        if prog == "iTerm.app" || prog == "WezTerm" || prog == "vscode" {
            return GraphicsProtocol::Iterm2;
        }
    }
    if let Ok(term) = std::env::var("LC_TERMINAL") {
        if term == "iTerm2" {
            return GraphicsProtocol::Iterm2;
        }
    }

    if let Ok(term) = std::env::var("TERM") {
        if term.contains("sixel") || term == "mlterm" || term == "foot" {
            return GraphicsProtocol::Sixel;
        }
    }
    if std::env::var("SIXEL_SUPPORT").is_ok() {
        return GraphicsProtocol::Sixel;
    }

    GraphicsProtocol::HalfBlocks
}

#[must_use]
pub fn encode_iterm2(png_bytes: &[u8]) -> String {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(png_bytes);
    format!("\x1b]1337;File=inline=1:{b64}\x07")
}

#[must_use]
pub fn encode_iterm2_with_size(png_bytes: &[u8], width_cols: u16, height_rows: u16) -> String {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(png_bytes);
    format!("\x1b]1337;File=inline=1;width={width_cols};height={height_rows}:{b64}\x07")
}

pub fn emit_iterm2(app: &mut App, seq: &str) {
    app.last_iterm2_sequence = Some(seq.to_string());
    use std::io::Write;
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(seq.as_bytes());
    let _ = stdout.flush();
}

pub fn encode_sixel_from_png(png_bytes: &[u8]) -> Result<String, String> {
    let img = image::load_from_memory(png_bytes)
        .map_err(|e| format!("Failed to decode image for Sixel encoding: {e}"))?;
    Ok(encode_sixel(&img))
}

#[must_use]
pub fn encode_sixel(img: &image::DynamicImage) -> String {
    let rgba = img.to_rgba8();
    let width = rgba.width();
    let height = rgba.height();
    if width == 0 || height == 0 {
        return "\x1bPq\"1;1;0;0\x1b\\".to_string();
    }

    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut color_map: std::collections::HashMap<[u8; 3], usize> = std::collections::HashMap::new();

    for pixel in rgba.pixels() {
        if pixel[3] >= 128 {
            let rgb = [pixel[0], pixel[1], pixel[2]];
            if !color_map.contains_key(&rgb) && palette.len() < 256 {
                let idx = palette.len();
                palette.push(rgb);
                color_map.insert(rgb, idx);
            }
        }
    }

    if palette.is_empty() {
        palette.push([0, 0, 0]);
        color_map.insert([0, 0, 0], 0);
    }

    let mut out = String::new();
    out.push_str(&format!("\x1bPq\"1;1;{width};{height}"));

    for (idx, rgb) in palette.iter().enumerate() {
        let r_pct = (u32::from(rgb[0]) * 100) / 255;
        let g_pct = (u32::from(rgb[1]) * 100) / 255;
        let b_pct = (u32::from(rgb[2]) * 100) / 255;
        out.push_str(&format!("#{idx};2;{r_pct};{g_pct};{b_pct}"));
    }

    let num_bands = height.div_ceil(6);
    for band in 0..num_bands {
        let y_start = band * 6;
        let band_h = (height - y_start).min(6);

        let mut band_colors = Vec::new();
        for dy in 0..band_h {
            let y = y_start + dy;
            for x in 0..width {
                let px = rgba.get_pixel(x, y);
                if px[3] >= 128 {
                    let rgb = [px[0], px[1], px[2]];
                    let c_idx = match color_map.get(&rgb) {
                        Some(&idx) => idx,
                        None => palette
                            .iter()
                            .enumerate()
                            .min_by_key(|(_, p)| {
                                let dr = i32::from(p[0]) - i32::from(rgb[0]);
                                let dg = i32::from(p[1]) - i32::from(rgb[1]);
                                let db = i32::from(p[2]) - i32::from(rgb[2]);
                                dr * dr + dg * dg + db * db
                            })
                            .map(|(idx, _)| idx)
                            .unwrap_or(0),
                    };
                    if !band_colors.contains(&c_idx) {
                        band_colors.push(c_idx);
                    }
                }
            }
        }

        for (i, &c_idx) in band_colors.iter().enumerate() {
            out.push_str(&format!("#{c_idx}"));

            let mut max_x = 0;
            for x in (0..width).rev() {
                let mut found = false;
                for dy in 0..band_h {
                    let y = y_start + dy;
                    let px = rgba.get_pixel(x, y);
                    if px[3] >= 128 {
                        let rgb = [px[0], px[1], px[2]];
                        let actual_idx = color_map.get(&rgb).copied().unwrap_or(0);
                        if actual_idx == c_idx {
                            found = true;
                            break;
                        }
                    }
                }
                if found {
                    max_x = x;
                    break;
                }
            }

            for x in 0..=max_x {
                let mut mask = 0u8;
                for dy in 0..band_h {
                    let y = y_start + dy;
                    let px = rgba.get_pixel(x, y);
                    if px[3] >= 128 {
                        let rgb = [px[0], px[1], px[2]];
                        let actual_idx = color_map.get(&rgb).copied().unwrap_or(0);
                        if actual_idx == c_idx {
                            mask |= 1 << dy;
                        }
                    }
                }
                out.push((63 + mask) as char);
            }

            if i + 1 < band_colors.len() {
                out.push('$');
            }
        }

        out.push('-');
    }

    out.push_str("\x1b\\");
    out
}

pub fn emit_sixel(app: &mut App, seq: &str) {
    app.last_sixel_sequence = Some(seq.to_string());
    use std::io::Write;
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(seq.as_bytes());
    let _ = stdout.flush();
}

pub fn render_half_blocks(
    frame: &mut ratatui::Frame,
    area: Rect,
    png_bytes: &[u8],
) -> Result<(), String> {
    if area.width == 0 || area.height == 0 {
        return Ok(());
    }

    let img = image::load_from_memory(png_bytes)
        .map_err(|e| format!("Failed to decode image for half-block rendering: {e}"))?;

    let target_w = u32::from(area.width);
    let target_h = u32::from(area.height) * 2;

    let resized = img.resize_exact(target_w, target_h, image::imageops::FilterType::Nearest);
    let rgba = resized.to_rgba8();

    let buf = frame.buffer_mut();

    for row in 0..area.height {
        let cell_y = area.y + row;
        let top_py = u32::from(row) * 2;
        let bot_py = top_py + 1;

        for col in 0..area.width {
            let cell_x = area.x + col;
            let top_px = rgba.get_pixel(u32::from(col), top_py);
            let bot_px = rgba.get_pixel(u32::from(col), bot_py);

            let top_alpha = top_px[3];
            let bot_alpha = bot_px[3];

            if let Some(cell) = buf.cell_mut((cell_x, cell_y)) {
                if top_alpha < 128 && bot_alpha < 128 {
                    cell.set_char(' ');
                } else if top_alpha >= 128 && bot_alpha < 128 {
                    cell.set_char('▀');
                    cell.set_fg(ratatui::style::Color::Rgb(top_px[0], top_px[1], top_px[2]));
                    cell.set_bg(crate::theme::BASE);
                } else if top_alpha < 128 && bot_alpha >= 128 {
                    cell.set_char('▄');
                    cell.set_fg(ratatui::style::Color::Rgb(bot_px[0], bot_px[1], bot_px[2]));
                    cell.set_bg(crate::theme::BASE);
                } else {
                    cell.set_char('▀');
                    cell.set_fg(ratatui::style::Color::Rgb(top_px[0], top_px[1], top_px[2]));
                    cell.set_bg(ratatui::style::Color::Rgb(bot_px[0], bot_px[1], bot_px[2]));
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use super::*;

    #[test]
    fn test_is_supported_media_formats() {
        assert!(is_supported_media(Path::new("image.png")));
        assert!(is_supported_media(Path::new("image.PNG")));
        assert!(is_supported_media(Path::new("photo.jpg")));
        assert!(is_supported_media(Path::new("photo.jpeg")));
        assert!(is_supported_media(Path::new("anim.gif")));
        assert!(is_supported_media(Path::new("pic.webp")));
        assert!(is_supported_media(Path::new("icon.bmp")));
        assert!(is_supported_media(Path::new("favicon.ico")));
        assert!(is_supported_media(Path::new("scan.tiff")));
        assert!(is_supported_media(Path::new("scan.tif")));
        assert!(is_supported_media(Path::new("vector.svg")));
        assert!(is_supported_media(Path::new("vector.SVG")));

        assert!(!is_supported_media(Path::new("main.rs")));
        assert!(!is_supported_media(Path::new("README.md")));
        assert!(!is_supported_media(Path::new("data.bin")));
        assert!(!is_supported_media(Path::new("Makefile")));
    }

    #[test]
    fn test_is_svg_bytes_sniffing() {
        assert!(is_svg_bytes(b"<svg xmlns='http://www.w3.org/2000/svg'></svg>"));
        assert!(is_svg_bytes(b"   \n\t<svg viewBox='0 0 10 10'></svg>"));
        assert!(is_svg_bytes(b"<?xml version='1.0'?><svg></svg>"));
        assert!(!is_svg_bytes(b"<?xml version='1.0'?><note><to>User</to></note>"));
        assert!(!is_svg_bytes(b"Hello world"));
        assert!(!is_svg_bytes(&[0, 1, 2, 3]));
    }

    #[test]
    fn test_decode_bmp_and_jpeg() {
        // BMP
        let mut img = image::RgbaImage::new(10, 10);
        for p in img.pixels_mut() {
            *p = image::Rgba([100, 200, 50, 255]);
        }
        let mut bmp_bytes = Vec::new();
        img.write_to(&mut Cursor::new(&mut bmp_bytes), image::ImageFormat::Bmp)
            .unwrap();
        let decoded_bmp = decode_raster(&bmp_bytes).unwrap();
        assert_eq!(decoded_bmp.width, 10);
        assert_eq!(decoded_bmp.height, 10);
        assert_eq!(decoded_bmp.format, "BMP");
        assert!(!decoded_bmp.png_bytes.is_empty());

        // JPEG
        let mut rgb_img = image::RgbImage::new(12, 8);
        for p in rgb_img.pixels_mut() {
            *p = image::Rgb([220, 80, 40]);
        }
        let mut jpeg_bytes = Vec::new();
        rgb_img
            .write_to(&mut Cursor::new(&mut jpeg_bytes), image::ImageFormat::Jpeg)
            .unwrap();
        let decoded_jpeg = decode_raster(&jpeg_bytes).unwrap();
        assert_eq!(decoded_jpeg.width, 12);
        assert_eq!(decoded_jpeg.height, 8);
        assert_eq!(decoded_jpeg.format, "JPEG");
    }

    #[test]
    fn test_decode_svg_invalid() {
        let err = decode_svg(b"not an svg").unwrap_err();
        assert!(err.contains("Failed to parse SVG"));
    }

    #[test]
    fn test_rects_intersect_cases() {
        let r1 = Rect::new(0, 0, 10, 10);
        let r2 = Rect::new(5, 5, 10, 10);
        let r3 = Rect::new(10, 10, 10, 10); // Touching border, no intersection
        let r4 = Rect::new(20, 20, 5, 5);

        assert!(rects_intersect(r1, r2));
        assert!(rects_intersect(r2, r1));
        assert!(!rects_intersect(r1, r3));
        assert!(!rects_intersect(r1, r4));
    }

    #[test]
    fn test_encode_kitty_chunks_empty() {
        let chunks = encode_kitty_chunks(&[], 99);
        assert_eq!(chunks, vec!["\x1b_Ga=T,f=100,i=99,m=0;\x1b\\"]);
    }
}
