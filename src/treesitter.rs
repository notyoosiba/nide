use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, LazyLock, RwLock};
use std::time::{Duration, Instant};

use tree_sitter::{InputEdit, Language, ParseOptions, Parser, Point, Query, QueryCursor, Tree};
use tree_sitter::StreamingIterator;

use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LanguageId {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Tsx,
    Go,
    Json,
    Toml,
    Bash,
    C,
    Cpp,
    Html,
    Css,
}

impl LanguageId {
    #[must_use]
    pub fn for_path(path: &Path) -> Option<Self> {
        let ext = path.extension().and_then(|e| e.to_str())?;
        match ext {
            "rs" => Some(Self::Rust),
            "py" | "pyi" => Some(Self::Python),
            "js" | "mjs" | "cjs" | "jsx" => Some(Self::JavaScript),
            "ts" | "mts" | "cts" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            "go" => Some(Self::Go),
            "json" => Some(Self::Json),
            "toml" => Some(Self::Toml),
            "sh" | "bash" => Some(Self::Bash),
            "c" | "h" => Some(Self::C),
            "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => Some(Self::Cpp),
            "html" | "htm" => Some(Self::Html),
            "css" => Some(Self::Css),
            _ => None,
        }
    }

    #[must_use]
    pub fn language(&self) -> Language {
        match self {
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Self::Go => tree_sitter_go::LANGUAGE.into(),
            Self::Json => tree_sitter_json::LANGUAGE.into(),
            Self::Toml => tree_sitter_toml_ng::LANGUAGE.into(),
            Self::Bash => tree_sitter_bash::LANGUAGE.into(),
            Self::C => tree_sitter_c::LANGUAGE.into(),
            Self::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            Self::Html => tree_sitter_html::LANGUAGE.into(),
            Self::Css => tree_sitter_css::LANGUAGE.into(),
        }
    }

    #[must_use]
    pub fn query_source(&self) -> &'static str {
        match self {
            Self::Rust => tree_sitter_rust::HIGHLIGHTS_QUERY,
            Self::Python => tree_sitter_python::HIGHLIGHTS_QUERY,
            Self::JavaScript => tree_sitter_javascript::HIGHLIGHT_QUERY,
            Self::TypeScript => tree_sitter_typescript::HIGHLIGHTS_QUERY,
            Self::Tsx => tree_sitter_typescript::HIGHLIGHTS_QUERY,
            Self::Go => tree_sitter_go::HIGHLIGHTS_QUERY,
            Self::Json => tree_sitter_json::HIGHLIGHTS_QUERY,
            Self::Toml => tree_sitter_toml_ng::HIGHLIGHTS_QUERY,
            Self::Bash => tree_sitter_bash::HIGHLIGHT_QUERY,
            Self::C => tree_sitter_c::HIGHLIGHT_QUERY,
            Self::Cpp => tree_sitter_cpp::HIGHLIGHT_QUERY,
            Self::Html => tree_sitter_html::HIGHLIGHTS_QUERY,
            Self::Css => tree_sitter_css::HIGHLIGHTS_QUERY,
        }
    }
}

static QUERY_CACHE: LazyLock<RwLock<HashMap<LanguageId, Arc<Query>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

pub fn get_or_compile_query(lang_id: LanguageId) -> Result<Arc<Query>, tree_sitter::QueryError> {
    if let Ok(guard) = QUERY_CACHE.read() {
        if let Some(q) = guard.get(&lang_id) {
            return Ok(Arc::clone(q));
        }
    }

    let lang = lang_id.language();
    let query_source = lang_id.query_source();
    let query = Arc::new(Query::new(&lang, query_source)?);

    if let Ok(mut guard) = QUERY_CACHE.write() {
        guard.insert(lang_id, Arc::clone(&query));
    }

    Ok(query)
}

pub type TsSpan = (usize, usize, ratatui::style::Style);
pub type TsSpansByLine = HashMap<usize, Vec<TsSpan>>;

pub struct TsHighlighter {
    pub lang_id: LanguageId,
    pub parser: Parser,
    pub tree: Option<Tree>,
    pub query: Arc<Query>,
    pub cursor: QueryCursor,
    pub needs_parse: bool,
}

impl std::fmt::Debug for TsHighlighter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TsHighlighter")
            .field("lang_id", &self.lang_id)
            .field("has_tree", &self.tree.is_some())
            .field("needs_parse", &self.needs_parse)
            .finish()
    }
}

impl TsHighlighter {
    #[must_use]
    pub fn new(lang_id: LanguageId) -> Option<Self> {
        let mut parser = Parser::new();
        let lang = lang_id.language();
        parser.set_language(&lang).ok()?;
        let query = get_or_compile_query(lang_id).ok()?;
        let cursor = QueryCursor::new();

        Some(Self {
            lang_id,
            parser,
            tree: None,
            query,
            cursor,
            needs_parse: true,
        })
    }

    pub fn reparse_full(&mut self) {
        self.tree = None;
        self.needs_parse = true;
    }

    pub fn apply_edit(&mut self, edit: &InputEdit) {
        if let Some(tree) = &mut self.tree {
            tree.edit(edit);
        }
        self.needs_parse = true;
    }

    pub fn parse_slice(&mut self, text: &[u8], timeout: Duration) -> bool {
        let start = Instant::now();
        let mut progress = |_state: &tree_sitter::ParseState| start.elapsed() >= timeout;
        let mut options = ParseOptions::new();
        options = options.progress_callback(&mut progress);

        let mut payload = |offset: usize, _point: Point| {
            if offset < text.len() {
                &text[offset..]
            } else {
                &[]
            }
        };

        let new_tree = self.parser.parse_with_options(
            &mut payload,
            self.tree.as_ref(),
            Some(options),
        );

        if let Some(tree) = new_tree {
            self.tree = Some(tree);
            self.needs_parse = false;
            true
        } else {
            self.needs_parse = true;
            false
        }
    }

    pub fn query_line_spans(
        &mut self,
        lines: &[String],
        text: &[u8],
        start_line: usize,
        end_line: usize,
    ) -> Option<TsSpansByLine> {
        let tree = self.tree.as_ref()?;
        self.cursor.set_point_range(
            Point {
                row: start_line,
                column: 0,
            }..Point {
                row: end_line + 1,
                column: 0,
            },
        );

        let mut captures = self.cursor.captures(&self.query, tree.root_node(), text);
        let mut spans_by_line: HashMap<usize, Vec<(usize, usize, ratatui::style::Style)>> =
            HashMap::new();

        while let Some((m, capture_idx)) = captures.next() {
            let capture = m.captures[*capture_idx];
            let capture_name = &self.query.capture_names()[capture.index as usize];
            if let Some(style) = theme::treesitter_capture_to_style(capture_name) {
                let start_pos = capture.node.start_position();
                let end_pos = capture.node.end_position();

                if start_pos.row == end_pos.row {
                    let r = start_pos.row;
                    if r >= start_line && r <= end_line {
                        spans_by_line
                            .entry(r)
                            .or_default()
                            .push((start_pos.column, end_pos.column, style));
                    }
                } else {
                    for r in start_pos.row..=end_pos.row {
                        if r >= start_line && r <= end_line {
                            let line_len = lines.get(r).map_or(0, |l| l.len());
                            let start_col = if r == start_pos.row {
                                start_pos.column
                            } else {
                                0
                            };
                            let end_col = if r == end_pos.row {
                                end_pos.column
                            } else {
                                line_len
                            };
                            if start_col < end_col {
                                spans_by_line
                                    .entry(r)
                                    .or_default()
                                    .push((start_col, end_col, style));
                            }
                        }
                    }
                }
            }
        }

        Some(spans_by_line)
    }
}

#[must_use]
pub fn overlay_treesitter_spans(
    line_text: &str,
    syntect_spans: Vec<(ratatui::style::Style, String)>,
    ts_spans: &[TsSpan],
) -> Vec<(ratatui::style::Style, String)> {
    if ts_spans.is_empty() || line_text.is_empty() {
        return syntect_spans;
    }

    let byte_len = line_text.len();
    let mut byte_styles = vec![ratatui::style::Style::default().fg(theme::TEXT); byte_len];

    // Seed styles from syntect baseline
    let mut offset = 0;
    for (style, text) in &syntect_spans {
        let next = (offset + text.len()).min(byte_len);
        for slot in &mut byte_styles[offset..next] {
            *slot = *style;
        }
        offset = next;
    }

    // Overlay Tree-sitter captures
    for &(start, end, style) in ts_spans {
        let s = start.min(byte_len);
        let e = end.min(byte_len);
        if s < e {
            for slot in &mut byte_styles[s..e] {
                *slot = style;
            }
        }
    }

    // Reconstruct contiguous spans, stepping along char boundaries for guaranteed valid UTF-8
    let mut result = Vec::new();
    let mut run_start = 0;
    let mut current_style = byte_styles[0];

    for (idx, _) in line_text.char_indices().skip(1) {
        let char_style = byte_styles[idx];
        if char_style != current_style {
            result.push((current_style, line_text[run_start..idx].to_string()));
            run_start = idx;
            current_style = char_style;
        }
    }

    if run_start < byte_len {
        result.push((current_style, line_text[run_start..byte_len].to_string()));
    }

    result
}
