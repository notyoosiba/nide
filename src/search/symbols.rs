use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use tree_sitter::Parser;

use crate::buffer::Buffer;
use crate::picker::PickItem;
use crate::treesitter::LanguageId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: String,
    pub path: PathBuf,
    pub line: usize, // 0-indexed line number
    pub col: usize,  // 0-indexed column
}

pub fn symbol_to_pick_item(root_dir: &Path, symbol: &Symbol) -> PickItem {
    let rel_path = symbol.path.strip_prefix(root_dir).unwrap_or(&symbol.path);
    let display_rel = rel_path.to_string_lossy().replace('\\', "/");
    let display_text = format!(
        "{} ({})  {}:{}",
        symbol.name,
        symbol.kind,
        display_rel,
        symbol.line + 1
    );
    PickItem {
        path: symbol.path.clone(),
        display_text,
        score: 0,
        match_indices: Vec::new(),
        line: Some(symbol.line),
        col: Some(symbol.col),
    }
}

pub fn extract_project_symbols(
    root_dir: &Path,
    files: &[PathBuf],
    current_buf: Option<&Buffer>,
) -> Vec<PickItem> {
    let mut symbols = Vec::new();
    let current_path = current_buf.map(|b| b.path.as_path());

    // If current buffer is loaded, extract from its in-memory lines first
    if let Some(buf) = current_buf {
        let text = buf.lines.join("\n");
        let buf_symbols = extract_symbols_from_source(&buf.path, &text);
        symbols.extend(buf_symbols);
    }

    // Now scan all repository files
    for file in files {
        let full_path = if file.is_absolute() {
            file.clone()
        } else {
            root_dir.join(file)
        };

        // Skip current buffer since we already extracted its symbols
        if current_path == Some(&full_path) {
            continue;
        }

        // Only scan code files
        if !is_code_file(&full_path) {
            continue;
        }

        // Skip files that don't exist or exceed 1MB
        if let Ok(meta) = full_path.metadata() {
            if meta.len() > 1024 * 1024 {
                continue;
            }
        } else {
            continue;
        }

        if let Ok(text) = std::fs::read_to_string(&full_path) {
            let file_symbols = extract_symbols_from_source(&full_path, &text);
            symbols.extend(file_symbols);
        }
    }

    // Sort symbols by name, then path, then line
    symbols.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.line.cmp(&b.line))
    });

    symbols
        .iter()
        .map(|s| symbol_to_pick_item(root_dir, s))
        .collect()
}

#[must_use]
pub fn is_code_file(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some(
            "rs" | "py" | "pyi" | "js" | "mjs" | "cjs" | "jsx" | "ts" | "mts" | "cts" | "tsx"
            | "go" | "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" | "sh" | "bash"
            | "toml" | "json" | "html" | "css",
        ) => true,
        _ => LanguageId::for_path(path).is_some(),
    }
}

pub fn extract_symbols_from_source(path: &Path, source: &str) -> Vec<Symbol> {
    let mut symbols = Vec::new();

    // 1. Try Tree-sitter if supported
    if let Some(lang_id) = LanguageId::for_path(path) {
        let mut parser = Parser::new();
        if parser.set_language(&lang_id.language()).is_ok() {
            if let Some(tree) = parser.parse(source, None) {
                collect_ts_symbols(tree.root_node(), source.as_bytes(), path, &mut symbols);
            }
        }
    }

    // 2. If Tree-sitter returned no symbols or is not available, fallback to regex
    if symbols.is_empty() {
        extract_symbols_regex(path, source, &mut symbols);
    }

    symbols
}

fn collect_ts_symbols(
    node: tree_sitter::Node,
    source: &[u8],
    path: &Path,
    symbols: &mut Vec<Symbol>,
) {
    let kind = node.kind();
    match kind {
        "function_item" | "function_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                if let Ok(name) = name_node.utf8_text(source) {
                    let point = name_node.start_position();
                    symbols.push(Symbol {
                        name: name.to_string(),
                        kind: "fn".to_string(),
                        path: path.to_path_buf(),
                        line: point.row,
                        col: point.column,
                    });
                }
            }
        }
        "struct_item" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                if let Ok(name) = name_node.utf8_text(source) {
                    let point = name_node.start_position();
                    symbols.push(Symbol {
                        name: name.to_string(),
                        kind: "struct".to_string(),
                        path: path.to_path_buf(),
                        line: point.row,
                        col: point.column,
                    });
                }
            }
        }
        "enum_item" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                if let Ok(name) = name_node.utf8_text(source) {
                    let point = name_node.start_position();
                    symbols.push(Symbol {
                        name: name.to_string(),
                        kind: "enum".to_string(),
                        path: path.to_path_buf(),
                        line: point.row,
                        col: point.column,
                    });
                }
            }
        }
        "trait_item" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                if let Ok(name) = name_node.utf8_text(source) {
                    let point = name_node.start_position();
                    symbols.push(Symbol {
                        name: name.to_string(),
                        kind: "trait".to_string(),
                        path: path.to_path_buf(),
                        line: point.row,
                        col: point.column,
                    });
                }
            }
        }
        "impl_item" => {
            if let Some(type_node) = node.child_by_field_name("type") {
                if let Ok(name) = type_node.utf8_text(source) {
                    let point = node.start_position();
                    symbols.push(Symbol {
                        name: name.to_string(),
                        kind: "impl".to_string(),
                        path: path.to_path_buf(),
                        line: point.row,
                        col: point.column,
                    });
                }
            }
        }
        "type_item" | "type_alias_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                if let Ok(name) = name_node.utf8_text(source) {
                    let point = name_node.start_position();
                    symbols.push(Symbol {
                        name: name.to_string(),
                        kind: "type".to_string(),
                        path: path.to_path_buf(),
                        line: point.row,
                        col: point.column,
                    });
                }
            }
        }
        "const_item" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                if let Ok(name) = name_node.utf8_text(source) {
                    let point = name_node.start_position();
                    symbols.push(Symbol {
                        name: name.to_string(),
                        kind: "const".to_string(),
                        path: path.to_path_buf(),
                        line: point.row,
                        col: point.column,
                    });
                }
            }
        }
        "function_definition" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                if let Ok(name) = name_node.utf8_text(source) {
                    let point = name_node.start_position();
                    symbols.push(Symbol {
                        name: name.to_string(),
                        kind: "def".to_string(),
                        path: path.to_path_buf(),
                        line: point.row,
                        col: point.column,
                    });
                }
            }
        }
        "class_definition" | "class_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                if let Ok(name) = name_node.utf8_text(source) {
                    let point = name_node.start_position();
                    symbols.push(Symbol {
                        name: name.to_string(),
                        kind: "class".to_string(),
                        path: path.to_path_buf(),
                        line: point.row,
                        col: point.column,
                    });
                }
            }
        }
        "interface_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                if let Ok(name) = name_node.utf8_text(source) {
                    let point = name_node.start_position();
                    symbols.push(Symbol {
                        name: name.to_string(),
                        kind: "interface".to_string(),
                        path: path.to_path_buf(),
                        line: point.row,
                        col: point.column,
                    });
                }
            }
        }
        _ => {}
    }

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            collect_ts_symbols(child, source, path, symbols);
        }
    }
}

static REGEX_FN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|\s)(?:pub(?:\([^)]+\))?\s+)?(?:async\s+)?(?:const\s+)?(?:unsafe\s+)?(?:extern\s+(?:[^\s]+\s+)?)?fn\s+([a-zA-Z_][a-zA-Z0-9_]*)").unwrap()
});
static REGEX_DEF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|\s)(?:async\s+)?def\s+([a-zA-Z_][a-zA-Z0-9_]*)").unwrap()
});
static REGEX_STRUCT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|\s)(?:pub(?:\([^)]+\))?\s+)?(?:export\s+)?struct\s+([a-zA-Z_][a-zA-Z0-9_]*)").unwrap()
});
static REGEX_ENUM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|\s)(?:pub(?:\([^)]+\))?\s+)?(?:export\s+)?enum\s+([a-zA-Z_][a-zA-Z0-9_]*)").unwrap()
});
static REGEX_TRAIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|\s)(?:pub(?:\([^)]+\))?\s+)?trait\s+([a-zA-Z_][a-zA-Z0-9_]*)").unwrap()
});
static REGEX_IMPL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|\s)impl(?:\s*<[^>]*>)?\s+(?:[a-zA-Z0-9_:<>]+\s+for\s+)?([a-zA-Z_][a-zA-Z0-9_]*)").unwrap()
});
static REGEX_CLASS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|\s)(?:export\s+)?(?:default\s+)?class\s+([a-zA-Z_][a-zA-Z0-9_]*)").unwrap()
});
static REGEX_INTERFACE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|\s)(?:export\s+)?interface\s+([a-zA-Z_][a-zA-Z0-9_]*)").unwrap()
});
static REGEX_TYPE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|\s)(?:pub(?:\([^)]+\))?\s+)?(?:export\s+)?type\s+([a-zA-Z_][a-zA-Z0-9_]*)").unwrap()
});
static REGEX_CONST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|\s)(?:pub(?:\([^)]+\))?\s+)?(?:export\s+)?const\s+([a-zA-Z_][a-zA-Z0-9_]*)").unwrap()
});

fn extract_symbols_regex(path: &Path, source: &str, symbols: &mut Vec<Symbol>) {
    for (line_idx, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        // Skip comment lines
        if trimmed.starts_with("//")
            || trimmed.starts_with('#')
            || trimmed.starts_with("/*")
            || trimmed.starts_with('*')
        {
            continue;
        }

        if let Some(caps) = REGEX_FN.captures(line) {
            if let Some(m) = caps.get(1) {
                symbols.push(Symbol {
                    name: m.as_str().to_string(),
                    kind: "fn".to_string(),
                    path: path.to_path_buf(),
                    line: line_idx,
                    col: m.start(),
                });
                continue;
            }
        }

        if let Some(caps) = REGEX_DEF.captures(line) {
            if let Some(m) = caps.get(1) {
                symbols.push(Symbol {
                    name: m.as_str().to_string(),
                    kind: "def".to_string(),
                    path: path.to_path_buf(),
                    line: line_idx,
                    col: m.start(),
                });
                continue;
            }
        }

        if let Some(caps) = REGEX_STRUCT.captures(line) {
            if let Some(m) = caps.get(1) {
                symbols.push(Symbol {
                    name: m.as_str().to_string(),
                    kind: "struct".to_string(),
                    path: path.to_path_buf(),
                    line: line_idx,
                    col: m.start(),
                });
                continue;
            }
        }

        if let Some(caps) = REGEX_ENUM.captures(line) {
            if let Some(m) = caps.get(1) {
                symbols.push(Symbol {
                    name: m.as_str().to_string(),
                    kind: "enum".to_string(),
                    path: path.to_path_buf(),
                    line: line_idx,
                    col: m.start(),
                });
                continue;
            }
        }

        if let Some(caps) = REGEX_TRAIT.captures(line) {
            if let Some(m) = caps.get(1) {
                symbols.push(Symbol {
                    name: m.as_str().to_string(),
                    kind: "trait".to_string(),
                    path: path.to_path_buf(),
                    line: line_idx,
                    col: m.start(),
                });
                continue;
            }
        }

        if let Some(caps) = REGEX_IMPL.captures(line) {
            if let Some(m) = caps.get(1) {
                symbols.push(Symbol {
                    name: m.as_str().to_string(),
                    kind: "impl".to_string(),
                    path: path.to_path_buf(),
                    line: line_idx,
                    col: m.start(),
                });
                continue;
            }
        }

        if let Some(caps) = REGEX_CLASS.captures(line) {
            if let Some(m) = caps.get(1) {
                symbols.push(Symbol {
                    name: m.as_str().to_string(),
                    kind: "class".to_string(),
                    path: path.to_path_buf(),
                    line: line_idx,
                    col: m.start(),
                });
                continue;
            }
        }

        if let Some(caps) = REGEX_INTERFACE.captures(line) {
            if let Some(m) = caps.get(1) {
                symbols.push(Symbol {
                    name: m.as_str().to_string(),
                    kind: "interface".to_string(),
                    path: path.to_path_buf(),
                    line: line_idx,
                    col: m.start(),
                });
                continue;
            }
        }

        if let Some(caps) = REGEX_TYPE.captures(line) {
            if let Some(m) = caps.get(1) {
                symbols.push(Symbol {
                    name: m.as_str().to_string(),
                    kind: "type".to_string(),
                    path: path.to_path_buf(),
                    line: line_idx,
                    col: m.start(),
                });
                continue;
            }
        }

        if let Some(caps) = REGEX_CONST.captures(line) {
            if let Some(m) = caps.get(1) {
                symbols.push(Symbol {
                    name: m.as_str().to_string(),
                    kind: "const".to_string(),
                    path: path.to_path_buf(),
                    line: line_idx,
                    col: m.start(),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_rust_symbols() {
        let src = r#"
pub fn calculate_total(a: i32, b: i32) -> i32 {
    a + b
}

pub struct InvoiceItem {
    pub id: u64,
}

pub enum PaymentStatus {
    Pending,
    Paid,
}

pub trait Payable {
    fn amount(&self) -> u64;
}

impl InvoiceItem {
    pub fn new() -> Self { Self { id: 1 } }
}

pub type InvoiceId = u64;
pub const TAX_RATE: f64 = 0.08;
"#;
        let path = PathBuf::from("src/invoice.rs");
        let symbols = extract_symbols_from_source(&path, src);

        assert!(symbols.iter().any(|s| s.name == "calculate_total" && s.kind == "fn"));
        assert!(symbols.iter().any(|s| s.name == "InvoiceItem" && s.kind == "struct"));
        assert!(symbols.iter().any(|s| s.name == "PaymentStatus" && s.kind == "enum"));
        assert!(symbols.iter().any(|s| s.name == "Payable" && s.kind == "trait"));
        assert!(symbols.iter().any(|s| s.name == "InvoiceId" && s.kind == "type"));
        assert!(symbols.iter().any(|s| s.name == "TAX_RATE" && s.kind == "const"));
    }

    #[test]
    fn test_extract_python_symbols() {
        let src = r#"
def process_data(items):
    return [x * 2 for x in items]

class DataProcessor:
    def __init__(self):
        pass
"#;
        let path = PathBuf::from("processor.py");
        let symbols = extract_symbols_from_source(&path, src);

        assert!(symbols.iter().any(|s| s.name == "process_data" && s.kind == "def"));
        assert!(symbols.iter().any(|s| s.name == "DataProcessor" && s.kind == "class"));
    }
}
