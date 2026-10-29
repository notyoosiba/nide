use std::path::Path;

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewFormat {
    Markdown,
    Csv,
    Tsv,
    Jsonl,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FormattedLine {
    pub line: Line<'static>,
    pub source_line: usize,
}

impl FormattedLine {
    #[must_use]
    pub fn new(line: Line<'static>, source_line: usize) -> Self {
        Self { line, source_line }
    }

    #[must_use]
    pub fn plain(text: String, source_line: usize) -> Self {
        Self {
            line: Line::from(text),
            source_line,
        }
    }

    #[must_use]
    pub fn styled(text: String, style: Style, source_line: usize) -> Self {
        Self {
            line: Line::from(Span::styled(text, style)),
            source_line,
        }
    }

    /// Extract plain text content of this rendered line.
    #[must_use]
    pub fn text(&self) -> String {
        self.line
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<Vec<_>>()
            .join("")
    }
}

/// Check if a path can be previewed in rendered preview mode.
#[must_use]
pub fn is_previewable(path: &Path) -> bool {
    detect_format(path).is_some()
}

/// Detect the preview format from the file extension.
#[must_use]
pub fn detect_format(path: &Path) -> Option<PreviewFormat> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "md" | "markdown" | "mdown" | "mkdn" | "mkd" => Some(PreviewFormat::Markdown),
        "csv" => Some(PreviewFormat::Csv),
        "tsv" => Some(PreviewFormat::Tsv),
        "jsonl" => Some(PreviewFormat::Jsonl),
        _ => None,
    }
}

/// Render buffer lines into preview formatted lines with source-line mapping.
#[must_use]
pub fn render_preview(lines: &[String], format: PreviewFormat, content_width: usize) -> Vec<FormattedLine> {
    match format {
        PreviewFormat::Csv => render_delimited_table(lines, ',', content_width),
        PreviewFormat::Tsv => render_delimited_table(lines, '\t', content_width),
        PreviewFormat::Jsonl => render_jsonl(lines, content_width),
        PreviewFormat::Markdown => render_markdown(lines, content_width),
    }
}

// ---------------------------------------------------------------------------
// CSV / TSV Formatter
// ---------------------------------------------------------------------------

pub fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    current.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                current.push(c);
            }
        } else if c == '"' {
            in_quotes = true;
        } else if c == ',' {
            fields.push(current.trim().to_string());
            current.clear();
        } else {
            current.push(c);
        }
    }
    fields.push(current.trim().to_string());
    fields
}

pub fn parse_tsv_line(line: &str) -> Vec<String> {
    line.split('\t').map(|s| s.trim().to_string()).collect()
}

fn render_delimited_table(
    lines: &[String],
    delimiter: char,
    _content_width: usize,
) -> Vec<FormattedLine> {
    if lines.is_empty() {
        return vec![FormattedLine::styled(
            "(empty file)".to_string(),
            Style::default().fg(theme::SUBTEXT0),
            0,
        )];
    }

    let parsed_rows: Vec<(usize, Vec<String>)> = lines
        .iter()
        .enumerate()
        .map(|(idx, line)| {
            let fields = if delimiter == ',' {
                parse_csv_line(line)
            } else {
                parse_tsv_line(line)
            };
            (idx, fields)
        })
        .collect();

    let num_cols = parsed_rows
        .iter()
        .map(|(_, row)| row.len())
        .max()
        .unwrap_or(0);

    if num_cols == 0 {
        return vec![FormattedLine::styled(
            "(empty table)".to_string(),
            Style::default().fg(theme::SUBTEXT0),
            0,
        )];
    }

    let mut col_widths = vec![3usize; num_cols];
    for (_, row) in &parsed_rows {
        for (col_idx, val) in row.iter().enumerate() {
            col_widths[col_idx] = col_widths[col_idx].max(val.chars().count());
        }
    }

    let mut out = Vec::new();
    let border_style = Style::default().fg(theme::SUBTEXT0);
    let header_style = Style::default()
        .fg(theme::MAUVE)
        .add_modifier(Modifier::BOLD);
    let cell_style = Style::default().fg(theme::TEXT);

    // Top border
    let top_border = make_border_line('┌', '┬', '┐', &col_widths);
    let first_line_idx = parsed_rows.first().map_or(0, |(idx, _)| *idx);
    out.push(FormattedLine::styled(top_border, border_style, first_line_idx));

    for (row_i, (src_line, row)) in parsed_rows.iter().enumerate() {
        let is_header = row_i == 0;
        let mut spans = Vec::new();
        spans.push(Span::styled("│ ", border_style));

        for (col_idx, &width) in col_widths.iter().enumerate().take(num_cols) {
            let val = row.get(col_idx).map(|s| s.as_str()).unwrap_or("");
            let cell_text = format!("{val:<width$}");
            let style = if is_header { header_style } else { cell_style };
            spans.push(Span::styled(cell_text, style));

            if col_idx + 1 < num_cols {
                spans.push(Span::styled(" │ ", border_style));
            } else {
                spans.push(Span::styled(" │", border_style));
            }
        }
        out.push(FormattedLine::new(Line::from(spans), *src_line));

        if is_header {
            let mid_border = make_border_line('├', '┼', '┤', &col_widths);
            out.push(FormattedLine::styled(mid_border, border_style, *src_line));
        }
    }

    // Bottom border
    let last_line_idx = parsed_rows.last().map_or(0, |(idx, _)| *idx);
    let bot_border = make_border_line('└', '┴', '┘', &col_widths);
    out.push(FormattedLine::styled(bot_border, border_style, last_line_idx));

    out
}

fn make_border_line(left: char, mid: char, right: char, col_widths: &[usize]) -> String {
    let mut s = String::new();
    s.push(left);
    for (i, &w) in col_widths.iter().enumerate() {
        s.push('─');
        for _ in 0..w {
            s.push('─');
        }
        s.push('─');
        if i + 1 < col_widths.len() {
            s.push(mid);
        } else {
            s.push(right);
        }
    }
    s
}

// ---------------------------------------------------------------------------
// JSONL Formatter
// ---------------------------------------------------------------------------

fn render_jsonl(lines: &[String], _content_width: usize) -> Vec<FormattedLine> {
    if lines.is_empty() {
        return vec![FormattedLine::styled(
            "(empty jsonl file)".to_string(),
            Style::default().fg(theme::SUBTEXT0),
            0,
        )];
    }

    // Parse each line as JSON value
    let mut parsed_entries = Vec::new();
    let mut all_keys = Vec::new();
    let mut is_all_objects = true;

    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(serde_json::Value::Object(map)) => {
                for key in map.keys() {
                    if !all_keys.contains(key) {
                        all_keys.push(key.clone());
                    }
                }
                parsed_entries.push((idx, Some(map)));
            }
            Ok(val) => {
                is_all_objects = false;
                parsed_entries.push((idx, None));
                // Just use stringified
                let _ = val;
            }
            Err(_) => {
                is_all_objects = false;
                parsed_entries.push((idx, None));
            }
        }
    }

    if is_all_objects && !all_keys.is_empty() {
        // Render as aligned table
        let num_cols = all_keys.len();
        let mut col_widths: Vec<usize> = all_keys.iter().map(|k| k.chars().count().max(3)).collect();

        for (_, map_opt) in &parsed_entries {
            if let Some(map) = map_opt {
                for (col_idx, key) in all_keys.iter().enumerate() {
                    let val_str = match map.get(key) {
                        Some(serde_json::Value::String(s)) => s.clone(),
                        Some(serde_json::Value::Null) => "null".to_string(),
                        Some(val) => val.to_string(),
                        None => String::new(),
                    };
                    col_widths[col_idx] = col_widths[col_idx].max(val_str.chars().count());
                }
            }
        }

        let mut out = Vec::new();
        let border_style = Style::default().fg(theme::SUBTEXT0);
        let header_style = Style::default()
            .fg(theme::MAUVE)
            .add_modifier(Modifier::BOLD);
        let cell_style = Style::default().fg(theme::TEXT);

        let first_src = parsed_entries.first().map_or(0, |(idx, _)| *idx);
        let last_src = parsed_entries.last().map_or(0, |(idx, _)| *idx);

        // Top border
        let top_border = make_border_line('┌', '┬', '┐', &col_widths);
        out.push(FormattedLine::styled(top_border, border_style, first_src));

        // Header
        let mut header_spans = Vec::new();
        header_spans.push(Span::styled("│ ", border_style));
        for (col_idx, key) in all_keys.iter().enumerate() {
            let width = col_widths[col_idx];
            header_spans.push(Span::styled(format!("{key:<width$}"), header_style));
            if col_idx + 1 < num_cols {
                header_spans.push(Span::styled(" │ ", border_style));
            } else {
                header_spans.push(Span::styled(" │", border_style));
            }
        }
        out.push(FormattedLine::new(Line::from(header_spans), first_src));

        // Separator
        let mid_border = make_border_line('├', '┼', '┤', &col_widths);
        out.push(FormattedLine::styled(mid_border, border_style, first_src));

        // Rows
        for (src_line, map_opt) in parsed_entries {
            if let Some(map) = map_opt {
                let mut spans = Vec::new();
                spans.push(Span::styled("│ ", border_style));
                for (col_idx, key) in all_keys.iter().enumerate() {
                    let val_str = match map.get(key) {
                        Some(serde_json::Value::String(s)) => s.clone(),
                        Some(serde_json::Value::Null) => "null".to_string(),
                        Some(val) => val.to_string(),
                        None => String::new(),
                    };
                    let width = col_widths[col_idx];
                    spans.push(Span::styled(format!("{val_str:<width$}"), cell_style));
                    if col_idx + 1 < num_cols {
                        spans.push(Span::styled(" │ ", border_style));
                    } else {
                        spans.push(Span::styled(" │", border_style));
                    }
                }
                out.push(FormattedLine::new(Line::from(spans), src_line));
            }
        }

        // Bottom border
        let bot_border = make_border_line('└', '┴', '┘', &col_widths);
        out.push(FormattedLine::styled(bot_border, border_style, last_src));

        out
    } else {
        // Fallback: pretty formatted JSON lines
        lines
            .iter()
            .enumerate()
            .map(|(idx, line)| {
                let trimmed = line.trim();
                let display = if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
                    serde_json::to_string_pretty(&v).unwrap_or_else(|_| line.clone())
                } else {
                    line.clone()
                };
                FormattedLine::styled(display, Style::default().fg(theme::TEXT), idx)
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Markdown Formatter
// ---------------------------------------------------------------------------

fn byte_offset_to_line(line_starts: &[usize], offset: usize) -> usize {
    match line_starts.binary_search(&offset) {
        Ok(idx) => idx,
        Err(idx) => idx.saturating_sub(1),
    }
}

fn compute_line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (byte_idx, b) in text.bytes().enumerate() {
        if b == b'\n' {
            starts.push(byte_idx + 1);
        }
    }
    starts
}

fn render_markdown(lines: &[String], content_width: usize) -> Vec<FormattedLine> {
    if lines.is_empty() {
        return vec![FormattedLine::styled(
            "(empty markdown file)".to_string(),
            Style::default().fg(theme::SUBTEXT0),
            0,
        )];
    }

    let full_text = lines.join("\n");
    let line_starts = compute_line_starts(&full_text);

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_STRIKETHROUGH);

    let parser = Parser::new_ext(&full_text, options).into_offset_iter();

    let mut out: Vec<FormattedLine> = Vec::new();

    // State machine for Markdown parsing
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut current_style = Style::default().fg(theme::TEXT);
    let mut style_stack: Vec<Style> = Vec::new();

    // Table state
    let mut in_table = false;
    let mut in_table_head = false;
    let mut table_headers: Vec<String> = Vec::new();
    let mut table_rows: Vec<(usize, Vec<String>)> = Vec::new();
    let mut current_row_cells: Vec<String> = Vec::new();
    let mut current_cell_text = String::new();
    let mut table_start_line = 0;

    // Code block state
    let mut in_code_block = false;
    let mut code_block_lang = String::new();
    let mut code_block_text = String::new();
    let mut code_block_start_line = 0;

    // Heading state
    let mut heading_start_line = 0;

    // List item state
    let mut list_depth: usize = 0;

    // Blockquote state
    let mut blockquote_depth: usize = 0;

    for (event, range) in parser {
        let event_line = byte_offset_to_line(&line_starts, range.start);

        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                heading_start_line = event_line;
                current_spans.clear();

                let (prefix, color) = match level {
                    HeadingLevel::H1 => ("# ", theme::MAUVE),
                    HeadingLevel::H2 => ("## ", theme::BLUE),
                    HeadingLevel::H3 => ("### ", theme::SAPPHIRE),
                    HeadingLevel::H4 => ("#### ", theme::TEAL),
                    HeadingLevel::H5 => ("##### ", theme::LAVENDER),
                    HeadingLevel::H6 => ("###### ", theme::PEACH),
                };
                current_style = Style::default().fg(color).add_modifier(Modifier::BOLD);
                current_spans.push(Span::styled(prefix.to_string(), current_style));
            }
            Event::End(TagEnd::Heading(_)) => {
                out.push(FormattedLine::new(
                    Line::from(std::mem::take(&mut current_spans)),
                    heading_start_line,
                ));
                // Add spacing line after heading
                out.push(FormattedLine::plain(String::new(), heading_start_line));
                current_style = Style::default().fg(theme::TEXT);
            }

            // Tables
            Event::Start(Tag::Table(_)) => {
                in_table = true;
                table_headers.clear();
                table_rows.clear();
                table_start_line = event_line;
            }
            Event::Start(Tag::TableHead) => {
                in_table_head = true;
                current_row_cells.clear();
            }
            Event::Start(Tag::TableRow) => {
                current_row_cells.clear();
            }
            Event::Start(Tag::TableCell) => {
                current_cell_text.clear();
            }
            Event::End(TagEnd::TableCell) => {
                current_row_cells.push(std::mem::take(&mut current_cell_text).trim().to_string());
            }
            Event::End(TagEnd::TableHead) => {
                in_table_head = false;
                table_headers = std::mem::take(&mut current_row_cells);
            }
            Event::End(TagEnd::TableRow) => {
                if !in_table_head {
                    table_rows.push((event_line, std::mem::take(&mut current_row_cells)));
                }
            }
            Event::End(TagEnd::Table) => {
                in_table = false;
                // Render accumulated table
                render_markdown_table(
                    &table_headers,
                    &table_rows,
                    table_start_line,
                    event_line,
                    &mut out,
                );
                out.push(FormattedLine::plain(String::new(), event_line));
            }

            // Code blocks & Mermaid diagrams
            Event::Start(Tag::CodeBlock(kind)) => {
                in_code_block = true;
                code_block_start_line = event_line;
                code_block_text.clear();
                code_block_lang = match kind {
                    CodeBlockKind::Fenced(lang) => lang.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                if code_block_lang.eq_ignore_ascii_case("mermaid") {
                    render_mermaid_block(
                        &code_block_text,
                        code_block_start_line,
                        event_line,
                        content_width,
                        &mut out,
                    );
                } else {
                    render_regular_code_block(
                        &code_block_lang,
                        &code_block_text,
                        code_block_start_line,
                        event_line,
                        &mut out,
                    );
                }
                out.push(FormattedLine::plain(String::new(), event_line));
            }

            // Task list items
            Event::TaskListMarker(checked) => {
                let (icon, color) = if checked {
                    ("☑ ", theme::GREEN)
                } else {
                    ("☐ ", theme::PEACH)
                };
                current_spans.push(Span::styled(
                    icon.to_string(),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ));
            }

            // Lists
            Event::Start(Tag::List(_)) => {
                list_depth += 1;
            }
            Event::End(TagEnd::List(_)) => {
                list_depth = list_depth.saturating_sub(1);
                if list_depth == 0 {
                    out.push(FormattedLine::plain(String::new(), event_line));
                }
            }
            Event::Start(Tag::Item) => {
                current_spans.clear();
                let indent = "  ".repeat(list_depth.saturating_sub(1));
                current_spans.push(Span::styled(
                    format!("{indent}• "),
                    Style::default().fg(theme::BLUE),
                ));
            }
            Event::End(TagEnd::Item) => {
                if !current_spans.is_empty() {
                    out.push(FormattedLine::new(
                        Line::from(std::mem::take(&mut current_spans)),
                        event_line,
                    ));
                }
            }

            // Blockquote
            Event::Start(Tag::BlockQuote(_)) => {
                blockquote_depth += 1;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                blockquote_depth = blockquote_depth.saturating_sub(1);
            }

            // Paragraphs
            Event::Start(Tag::Paragraph) => {
                current_spans.clear();
                if blockquote_depth > 0 {
                    let prefix = "│ ".repeat(blockquote_depth);
                    current_spans.push(Span::styled(
                        prefix,
                        Style::default().fg(theme::OVERLAY0),
                    ));
                }
            }
            Event::End(TagEnd::Paragraph) => {
                if !current_spans.is_empty() {
                    out.push(FormattedLine::new(
                        Line::from(std::mem::take(&mut current_spans)),
                        event_line,
                    ));
                    out.push(FormattedLine::plain(String::new(), event_line));
                }
            }

            // Text & Formatting
            Event::Text(cow_str) => {
                if in_table {
                    current_cell_text.push_str(&cow_str);
                } else if in_code_block {
                    code_block_text.push_str(&cow_str);
                } else {
                    current_spans.push(Span::styled(cow_str.to_string(), current_style));
                }
            }
            Event::Code(code) => {
                if in_table {
                    current_cell_text.push_str(&format!("`{code}`"));
                } else {
                    current_spans.push(Span::styled(
                        format!(" {code} "),
                        Style::default()
                            .fg(theme::PINK)
                            .bg(theme::MANTLE)
                            .add_modifier(Modifier::BOLD),
                    ));
                }
            }
            Event::Start(Tag::Strong) => {
                style_stack.push(current_style);
                current_style = current_style.add_modifier(Modifier::BOLD);
            }
            Event::End(TagEnd::Strong) => {
                current_style = style_stack.pop().unwrap_or_else(|| Style::default().fg(theme::TEXT));
            }
            Event::Start(Tag::Emphasis) => {
                style_stack.push(current_style);
                current_style = current_style.add_modifier(Modifier::ITALIC);
            }
            Event::End(TagEnd::Emphasis) => {
                current_style = style_stack.pop().unwrap_or_else(|| Style::default().fg(theme::TEXT));
            }
            Event::Start(Tag::Strikethrough) => {
                style_stack.push(current_style);
                current_style = current_style.add_modifier(Modifier::CROSSED_OUT);
            }
            Event::End(TagEnd::Strikethrough) => {
                current_style = style_stack.pop().unwrap_or_else(|| Style::default().fg(theme::TEXT));
            }
            Event::Rule => {
                let rule_str = "─".repeat(content_width.clamp(10, 60));
                out.push(FormattedLine::styled(
                    rule_str,
                    Style::default().fg(theme::SURFACE2),
                    event_line,
                ));
                out.push(FormattedLine::plain(String::new(), event_line));
            }
            Event::SoftBreak => {
                if !in_table && !in_code_block {
                    current_spans.push(Span::raw(" "));
                }
            }
            Event::HardBreak if !in_table && !in_code_block => {
                out.push(FormattedLine::new(
                    Line::from(std::mem::take(&mut current_spans)),
                    event_line,
                ));
            }
            _ => {}
        }
    }

    if !current_spans.is_empty() {
        let last_line = lines.len().saturating_sub(1);
        out.push(FormattedLine::new(Line::from(current_spans), last_line));
    }

    // Clean up excessive trailing empty lines
    while out.last().is_some_and(|l| l.text().trim().is_empty()) {
        out.pop();
    }

    if out.is_empty() {
        out.push(FormattedLine::styled(
            "(empty)".to_string(),
            Style::default().fg(theme::SUBTEXT0),
            0,
        ));
    }

    out
}

fn render_markdown_table(
    headers: &[String],
    rows: &[(usize, Vec<String>)],
    start_line: usize,
    end_line: usize,
    out: &mut Vec<FormattedLine>,
) {
    let num_cols = headers.len().max(rows.iter().map(|(_, r)| r.len()).max().unwrap_or(0));
    if num_cols == 0 {
        return;
    }

    let mut col_widths = vec![3usize; num_cols];
    for (col_idx, h) in headers.iter().enumerate() {
        col_widths[col_idx] = col_widths[col_idx].max(h.chars().count());
    }
    for (_, row) in rows {
        for (col_idx, val) in row.iter().enumerate() {
            col_widths[col_idx] = col_widths[col_idx].max(val.chars().count());
        }
    }

    let border_style = Style::default().fg(theme::SUBTEXT0);
    let header_style = Style::default()
        .fg(theme::MAUVE)
        .add_modifier(Modifier::BOLD);
    let cell_style = Style::default().fg(theme::TEXT);

    // Top border
    let top_border = make_border_line('┌', '┬', '┐', &col_widths);
    out.push(FormattedLine::styled(top_border, border_style, start_line));

    // Headers
    let mut header_spans = Vec::new();
    header_spans.push(Span::styled("│ ", border_style));
    for (col_idx, &width) in col_widths.iter().enumerate().take(num_cols) {
        let text = headers.get(col_idx).map(|s| s.as_str()).unwrap_or("");
        header_spans.push(Span::styled(format!("{text:<width$}"), header_style));
        if col_idx + 1 < num_cols {
            header_spans.push(Span::styled(" │ ", border_style));
        } else {
            header_spans.push(Span::styled(" │", border_style));
        }
    }
    out.push(FormattedLine::new(Line::from(header_spans), start_line));

    // Separator
    let mid_border = make_border_line('├', '┼', '┤', &col_widths);
    out.push(FormattedLine::styled(mid_border, border_style, start_line));

    // Rows
    for (src_line, row) in rows {
        let mut spans = Vec::new();
        spans.push(Span::styled("│ ", border_style));
        for (col_idx, &width) in col_widths.iter().enumerate().take(num_cols) {
            let text = row.get(col_idx).map(|s| s.as_str()).unwrap_or("");
            spans.push(Span::styled(format!("{text:<width$}"), cell_style));
            if col_idx + 1 < num_cols {
                spans.push(Span::styled(" │ ", border_style));
            } else {
                spans.push(Span::styled(" │", border_style));
            }
        }
        out.push(FormattedLine::new(Line::from(spans), *src_line));
    }

    // Bottom border
    let bot_border = make_border_line('└', '┴', '┘', &col_widths);
    out.push(FormattedLine::styled(bot_border, border_style, end_line));
}

fn render_regular_code_block(
    lang: &str,
    code: &str,
    start_line: usize,
    end_line: usize,
    out: &mut Vec<FormattedLine>,
) {
    let border_style = Style::default().fg(theme::SURFACE2);
    let code_style = Style::default().fg(theme::TEAL);

    let title = if lang.is_empty() {
        "┌── Code ──".to_string()
    } else {
        format!("┌── {lang} ──")
    };
    out.push(FormattedLine::styled(title, border_style, start_line));

    let code_lines: Vec<&str> = code.lines().collect();
    let _num_lines = code_lines.len().max(1);

    for (idx, line) in code_lines.iter().enumerate() {
        let mapped_line = start_line + 1 + idx;
        let mapped_line = mapped_line.min(end_line);
        let spans = vec![
            Span::styled("│ ", border_style),
            Span::styled(line.to_string(), code_style),
        ];
        out.push(FormattedLine::new(Line::from(spans), mapped_line));
    }

    out.push(FormattedLine::styled(
        "└─────────".to_string(),
        border_style,
        end_line,
    ));
}

fn render_mermaid_block(
    code: &str,
    start_line: usize,
    end_line: usize,
    content_width: usize,
    out: &mut Vec<FormattedLine>,
) {
    let border_style = Style::default().fg(theme::BLUE);
    out.push(FormattedLine::styled(
        "┌── Mermaid Diagram ──".to_string(),
        border_style,
        start_line,
    ));

    match crate::mermaid::render_mermaid_to_text(code, content_width as u16) {
        Ok(text) => {
            let rendered_lines: Vec<&str> = text.lines().collect();
            let total = rendered_lines.len().max(1);
            let code_line_count = end_line.saturating_sub(start_line).max(1);

            for (idx, r_line) in rendered_lines.iter().enumerate() {
                // Map proportionally across source lines
                let offset = (idx * code_line_count) / total;
                let mapped_line = (start_line + offset).min(end_line);

                let spans = vec![
                    Span::styled("│ ", border_style),
                    Span::styled(
                        r_line.to_string(),
                        Style::default().fg(theme::SAPPHIRE),
                    ),
                ];
                out.push(FormattedLine::new(Line::from(spans), mapped_line));
            }
        }
        Err(err) => {
            out.push(FormattedLine::styled(
                format!("│ [Diagram error: {err}]"),
                Style::default().fg(theme::RED),
                start_line,
            ));
            for (idx, line) in code.lines().enumerate() {
                out.push(FormattedLine::styled(
                    format!("│ {line}"),
                    Style::default().fg(theme::SUBTEXT0),
                    start_line + 1 + idx,
                ));
            }
        }
    }

    out.push(FormattedLine::styled(
        "└─────────────────────".to_string(),
        border_style,
        end_line,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_previewable_extensions() {
        assert!(is_previewable(Path::new("README.md")));
        assert!(is_previewable(Path::new("doc.markdown")));
        assert!(is_previewable(Path::new("data.csv")));
        assert!(is_previewable(Path::new("metrics.tsv")));
        assert!(is_previewable(Path::new("events.jsonl")));
        assert!(!is_previewable(Path::new("main.rs")));
        assert!(!is_previewable(Path::new("Cargo.toml")));
    }

    #[test]
    fn test_csv_table_formatting_and_mapping() {
        let lines = vec![
            "Name,Age,Role".to_string(),
            "Alice,30,Engineer".to_string(),
            "Bob,25,Designer".to_string(),
        ];
        let preview = render_preview(&lines, PreviewFormat::Csv, 80);

        assert!(!preview.is_empty());
        let full_text = preview
            .iter()
            .map(|l| l.text())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(full_text.contains("Name"));
        assert!(full_text.contains("Alice"));
        assert!(full_text.contains("Engineer"));
        assert!(full_text.contains("Bob"));
        assert!(full_text.contains("┌"));
        assert!(full_text.contains("├"));
        assert!(full_text.contains("└"));

        // Check source line mapping:
        // Top border -> line 0
        assert_eq!(preview[0].source_line, 0);
        // Header -> line 0
        assert_eq!(preview[1].source_line, 0);
        assert!(preview[1].text().contains("Name"));
        // Alice row -> line 1
        let alice_line = preview.iter().find(|l| l.text().contains("Alice")).unwrap();
        assert_eq!(alice_line.source_line, 1);
        // Bob row -> line 2
        let bob_line = preview.iter().find(|l| l.text().contains("Bob")).unwrap();
        assert_eq!(bob_line.source_line, 2);
    }

    #[test]
    fn test_tsv_table_formatting() {
        let lines = vec![
            "ID\tCity\tTemp".to_string(),
            "1\tParis\t22".to_string(),
            "2\tTokyo\t26".to_string(),
        ];
        let preview = render_preview(&lines, PreviewFormat::Tsv, 80);

        let paris_line = preview.iter().find(|l| l.text().contains("Paris")).unwrap();
        assert_eq!(paris_line.source_line, 1);
        let tokyo_line = preview.iter().find(|l| l.text().contains("Tokyo")).unwrap();
        assert_eq!(tokyo_line.source_line, 2);
    }

    #[test]
    fn test_jsonl_table_formatting() {
        let lines = vec![
            r#"{"name": "Alice", "status": "active"}"#.to_string(),
            r#"{"name": "Bob", "status": "pending"}"#.to_string(),
        ];
        let preview = render_preview(&lines, PreviewFormat::Jsonl, 80);

        let full_text = preview
            .iter()
            .map(|l| l.text())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(full_text.contains("name"));
        assert!(full_text.contains("Alice"));
        assert!(full_text.contains("Bob"));

        let alice_line = preview.iter().find(|l| l.text().contains("Alice")).unwrap();
        assert_eq!(alice_line.source_line, 0);
        let bob_line = preview.iter().find(|l| l.text().contains("Bob")).unwrap();
        assert_eq!(bob_line.source_line, 1);
    }

    #[test]
    fn test_markdown_formatting_headings_tables_tasks_and_mermaid() {
        let markdown = r#"# Main Title

Some introductory text.

## Features

- [ ] Task 1
- [x] Task 2

| Key | Value |
| --- | ----- |
| Foo | 123   |
| Bar | 456   |

```mermaid
flowchart TD
  A --> B
```
"#;
        let lines: Vec<String> = markdown.lines().map(|s| s.to_string()).collect();
        let preview = render_preview(&lines, PreviewFormat::Markdown, 80);

        let full_text = preview
            .iter()
            .map(|l| l.text())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(full_text.contains("# Main Title"));
        assert!(full_text.contains("## Features"));
        assert!(full_text.contains("☐"));
        assert!(full_text.contains("☑"));
        assert!(full_text.contains("Key"));
        assert!(full_text.contains("Foo"));
        assert!(full_text.contains("Mermaid Diagram"));

        // Verify tasklist source line mapping
        let task1_line = preview.iter().find(|l| l.text().contains("Task 1")).unwrap();
        assert_eq!(lines[task1_line.source_line].trim(), "- [ ] Task 1");

        let task2_line = preview.iter().find(|l| l.text().contains("Task 2")).unwrap();
        assert_eq!(lines[task2_line.source_line].trim(), "- [x] Task 2");

        // Verify table source line mapping
        let foo_line = preview.iter().find(|l| l.text().contains("Foo")).unwrap();
        assert!(lines[foo_line.source_line].contains("Foo"));
    }
}
