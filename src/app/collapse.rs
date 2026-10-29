use crate::app::{App, MiddleMode};
use crate::buffer::Buffer;

pub fn toggle_fold(app: &mut App) {
    if app.middle_mode != MiddleMode::Normal {
        return;
    }
    let Some(buf) = &mut app.buf else {
        return;
    };
    if buf.lines.is_empty() {
        return;
    }

    let cursor_row = buf.cursor_row;

    // Check if cursor is on an existing fold
    if buf.remove_fold_at(cursor_row) {
        return;
    }

    // Try detecting foldable range at cursor_row
    if let Some((start_row, end_row)) = find_foldable_range(buf, cursor_row) {
        if end_row > start_row {
            buf.add_fold(start_row, end_row);
            buf.cursor_row = start_row;
            buf.clamp_cursor();
        }
    }
}

pub fn unfold_all(app: &mut App) {
    if let Some(buf) = &mut app.buf {
        buf.folds.clear();
    }
}

#[must_use]
pub fn find_foldable_range(buf: &mut Buffer, cursor_row: usize) -> Option<(usize, usize)> {
    if buf.lines.is_empty() || cursor_row >= buf.lines.len() {
        return None;
    }

    // 1. Try Tree-sitter if available
    if let Some(range) = find_ts_foldable_range(buf, cursor_row) {
        return Some(range);
    }

    // 2. Try brace matching
    if let Some(range) = find_brace_foldable_range(buf, cursor_row) {
        return Some(range);
    }

    // 3. Fallback to indentation
    find_indent_foldable_range(buf, cursor_row)
}

fn find_ts_foldable_range(buf: &mut Buffer, cursor_row: usize) -> Option<(usize, usize)> {
    let needs_parse = buf
        .ts_highlighter
        .as_ref()
        .is_some_and(|ts| ts.tree.is_none() || ts.needs_parse);
    if needs_parse {
        let text = buf.content_bytes();
        if let Some(ts) = &mut buf.ts_highlighter {
            ts.parse_slice(&text, std::time::Duration::from_millis(50));
        }
    }

    let ts = buf.ts_highlighter.as_ref()?;
    let tree = ts.tree.as_ref()?;
    let root = tree.root_node();

    let col = buf.cursor_col.min(buf.lines[cursor_row].len());
    let pt = tree_sitter::Point::new(cursor_row, col);
    let mut curr = root.named_descendant_for_point_range(pt, pt)?;

    let mut best_candidate: Option<(usize, usize)> = None;

    loop {
        let kind = curr.kind();
        let start = curr.start_position().row;
        let mut end = curr.end_position().row;
        if curr.end_position().column == 0 && end > start {
            end = end.saturating_sub(1);
        }

        if end > start && start <= cursor_row && cursor_row <= end && is_foldable_kind(kind) {
            let clamped_end = end.min(buf.lines.len().saturating_sub(1));
            if clamped_end > start {
                if start == cursor_row {
                    return Some((start, clamped_end));
                }
                if best_candidate.is_none() {
                    best_candidate = Some((start, clamped_end));
                }
            }
        }

        let Some(parent) = curr.parent() else {
            break;
        };
        curr = parent;
    }

    best_candidate
}

#[must_use]
pub fn is_foldable_kind(kind: &str) -> bool {
    matches!(
        kind,
        // Functions / methods
        "function_item"
            | "function_definition"
            | "function_declaration"
            | "method_definition"
            | "method_declaration"
            | "arrow_function"
            | "closure_expression"
            // Types / structs / classes
            | "struct_item"
            | "struct_specifier"
            | "enum_item"
            | "enum_specifier"
            | "trait_item"
            | "impl_item"
            | "class_definition"
            | "class_declaration"
            | "interface_declaration"
            | "type_declaration"
            // Blocks
            | "block"
            | "compound_statement"
            | "statement_block"
            | "match_expression"
            | "match_arm"
            // Control flow
            | "if_statement"
            | "for_statement"
            | "while_statement"
            | "loop_expression"
            | "try_statement"
            | "switch_statement"
            // Data / markup
            | "table"
            | "object"
            | "array"
            | "element"
            | "rule_set"
    )
}

fn find_brace_foldable_range(buf: &Buffer, cursor_row: usize) -> Option<(usize, usize)> {
    let line = &buf.lines[cursor_row];
    if line.contains('{') {
        let mut depth = 0isize;
        for (r, l) in buf.lines.iter().enumerate().skip(cursor_row) {
            for ch in l.chars() {
                if ch == '{' {
                    depth += 1;
                } else if ch == '}' {
                    depth -= 1;
                    if depth == 0 && r > cursor_row {
                        return Some((cursor_row, r));
                    }
                }
            }
        }
    }

    // If cursor_row didn't have '{', scan upwards to see if cursor_row is inside an enclosing block
    let mut depth = 0isize;
    for r in (0..=cursor_row).rev() {
        let l = &buf.lines[r];
        for ch in l.chars().rev() {
            if ch == '}' {
                depth += 1;
            } else if ch == '{' {
                if depth == 0 {
                    return find_brace_foldable_range(buf, r);
                }
                depth -= 1;
            }
        }
    }

    None
}

fn find_indent_foldable_range(buf: &Buffer, cursor_row: usize) -> Option<(usize, usize)> {
    let curr_line = &buf.lines[cursor_row];
    let curr_trimmed = curr_line.trim_start();
    if curr_trimmed.is_empty() {
        return None;
    }
    let base_indent = curr_line.len() - curr_trimmed.len();

    let mut end_row = cursor_row;
    for r in (cursor_row + 1)..buf.lines.len() {
        let l = &buf.lines[r];
        let trimmed = l.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        let indent = l.len() - trimmed.len();
        if indent > base_indent {
            end_row = r;
        } else {
            break;
        }
    }

    if end_row > cursor_row {
        Some((cursor_row, end_row))
    } else {
        None
    }
}
