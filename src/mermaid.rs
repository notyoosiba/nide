use merman::ascii::{AsciiLayoutProfile, AsciiRenderOptions, AsciiViewportPolicy, OverflowPolicy};
use merman::svg::export::{RasterFitBox, RasterOptions};
use merman::{
    AsciiRequest, OperationControl, PngRequest, RenderOutput, RenderRequest, Renderer, SvgRequest,
};

/// Rasterize a Mermaid diagram source into PNG image bytes.
pub fn render_mermaid_to_png(
    source: &str,
    max_width: u32,
    max_height: u32,
) -> Result<Vec<u8>, String> {
    let raster = RasterOptions::default()
        .with_fit_to(RasterFitBox::contain(max_width, max_height))
        .with_scale(2.0)
        .with_background("white");
    let renderer = Renderer::new().with_parse_options(merman::ParseOptions::strict());
    let output = renderer
        .render(RenderRequest::png(
            source,
            OperationControl::new(),
            PngRequest {
                svg: SvgRequest {
                    options: merman::svg::SvgRenderOptions {
                        diagram_id: Some("diagram".to_string()),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                options: raster,
            },
        ))
        .map_err(|e| format!("{e}"))?;

    match output {
        RenderOutput::Png(Some(output)) => Ok(output.bytes),
        _ => Err("No Mermaid diagram produced".to_string()),
    }
}

/// Render a Mermaid diagram source into terminal-friendly Unicode/ASCII text.
pub fn render_mermaid_to_text(source: &str, max_width: u16) -> Result<String, String> {
    let options = AsciiRenderOptions::unicode().with_layout_profile(AsciiLayoutProfile::Auto);
    let renderer = Renderer::new().with_parse_options(merman::ParseOptions::strict());
    let output = renderer
        .render(RenderRequest::ascii(
            source,
            OperationControl::new(),
            AsciiRequest {
                options,
                viewport: AsciiViewportPolicy::with_max_width(max_width.max(20) as usize)
                    .overflow(OverflowPolicy::Fallback),
                ..Default::default()
            },
        ))
        .map_err(|e| format!("{e}"))?;

    match output {
        RenderOutput::Ascii(Some(report)) => Ok(report.text),
        _ => Err("No Mermaid diagram produced".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_FLOWCHART: &str = r#"flowchart TD
    A[Start] --> B[Process]
    B --> C[Done]
"#;

    #[test]
    fn test_render_mermaid_to_png() {
        let png_bytes = render_mermaid_to_png(SAMPLE_FLOWCHART, 800, 600).expect("png rendering should succeed");
        assert!(!png_bytes.is_empty());
        // PNG magic bytes
        assert_eq!(&png_bytes[0..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn test_render_mermaid_to_text() {
        let text = render_mermaid_to_text(SAMPLE_FLOWCHART, 80).expect("text rendering should succeed");
        assert!(!text.is_empty());
        assert!(text.contains("Start") || text.contains("Process") || text.contains("Done"));
    }
}
