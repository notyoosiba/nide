use std::cmp::Reverse;
use std::path::{Path, PathBuf};

use crate::pty::TerminalPane;

/// Represents a parsed file reference target with optional line and column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkTarget {
    pub raw: String,
    pub path: String,
    pub line: Option<usize>,
    pub col: Option<usize>,
}

/// Checks if a character can be part of a path token or link bounding delimiter.
#[must_use]
pub fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(
            c,
            '_' | '.' | '/' | '~' | ':' | '-' | '#' | '@' | '+' | '\\'
                | '"' | '\'' | '`' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | ',' | ';'
        )
}

/// Strips surrounding quotes, brackets, and trailing sentence punctuation from a token.
#[must_use]
pub fn trim_token(raw: &str) -> &str {
    let mut s = raw.trim();
    loop {
        let prev = s;
        // Trim leading quotes / opening brackets
        s = s.trim_start_matches(['"', '\'', '`', '(', '[', '{', '<']);
        // Trim trailing quotes / closing brackets / punctuation
        s = s.trim_end_matches(['"', '\'', '`', ')', ']', '}', '>', ',', ';', '!', '?']);
        // Trim trailing dot if not '.' or '..'
        if s.ends_with('.') && !s.ends_with("..") && s != "." {
            s = s.trim_end_matches('.');
        }
        // Trim trailing colon if isolated
        if s.ends_with(':') {
            s = s.trim_end_matches(':');
        }
        if s == prev {
            break;
        }
    }
    s
}

/// Parses optional line and column from a `#L12` or `#L12C4` or `#12:4` fragment.
fn parse_line_col_fragment(frag: &str) -> (Option<usize>, Option<usize>) {
    let frag = frag.strip_prefix(['L', 'l']).unwrap_or(frag);
    if let Some((l, c)) = frag.split_once(['C', 'c', ':']) {
        let line = l.parse::<usize>().ok();
        let col = c.parse::<usize>().ok();
        (line, col)
    } else {
        let line = frag.parse::<usize>().ok();
        (line, None)
    }
}

/// Parses a token string into a LinkTarget, stripping quotes/brackets/punctuation,
/// diff prefixes (a/ or b/), and extracting optional line/column numbers.
#[must_use]
pub fn parse_link_target(raw: &str) -> Option<LinkTarget> {
    let trimmed = trim_token(raw);
    if trimmed.is_empty() {
        return None;
    }

    // Strip git diff prefixes a/ and b/
    let without_diff_prefix = if let Some(rest) = trimmed.strip_prefix("a/") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("b/") {
        rest
    } else {
        trimmed
    };

    // 1. Check for #L<line>[C<col>] fragment
    if let Some((path_part, frag)) = without_diff_prefix.split_once('#') {
        let (line, col) = parse_line_col_fragment(frag);
        return Some(LinkTarget {
            raw: raw.to_string(),
            path: path_part.to_string(),
            line,
            col,
        });
    }

    // 2. Check for :line[:col]
    let parts: Vec<&str> = without_diff_prefix.rsplitn(3, ':').collect();
    if parts.len() == 3 {
        if let (Ok(c), Ok(l)) = (parts[0].parse::<usize>(), parts[1].parse::<usize>()) {
            return Some(LinkTarget {
                raw: raw.to_string(),
                path: parts[2].to_string(),
                line: Some(l),
                col: Some(c),
            });
        }
    }
    if parts.len() >= 2 {
        if let Ok(l) = parts[0].parse::<usize>() {
            let path_len = without_diff_prefix.len() - parts[0].len() - 1;
            let path = without_diff_prefix[..path_len].to_string();
            return Some(LinkTarget {
                raw: raw.to_string(),
                path,
                line: Some(l),
                col: None,
            });
        }
    }

    Some(LinkTarget {
        raw: raw.to_string(),
        path: without_diff_prefix.to_string(),
        line: None,
        col: None,
    })
}

/// Helper: does row `r` wrap to `r + 1`?
#[must_use]
pub fn check_row_wraps(screen: &vt100::Screen, r: u16, rows: u16, cols: u16) -> bool {
    if r + 1 >= rows {
        return false;
    }
    if screen.row_wrapped(r) {
        return true;
    }
    let last_col = cols.saturating_sub(1);
    if let (Some(c1), Some(c2)) = (screen.cell(r, last_col), screen.cell(r + 1, 0)) {
        let s1 = c1.contents();
        let s2 = c2.contents();
        if !s1.is_empty() && !s2.is_empty() {
            let ch1 = s1.chars().next().unwrap_or(' ');
            let ch2 = s2.chars().next().unwrap_or(' ');
            if is_token_char(ch1) && is_token_char(ch2) {
                return true;
            }
        }
    }
    false
}

/// Extracts a contiguous token at the given (row, col) coordinates on the vt100 screen,
/// reconstructing soft-wrapped terminal rows.
#[must_use]
pub fn extract_token_from_screen(
    screen: &vt100::Screen,
    row: u16,
    col: u16,
) -> Option<LinkTarget> {
    let (rows, cols) = screen.size();
    let rows = rows.get();
    let cols = cols.get();
    if row >= rows || col >= cols {
        return None;
    }

    // Find start of wrapped row sequence
    let mut start_row = row;
    while start_row > 0 && check_row_wraps(screen, start_row - 1, rows, cols) {
        start_row -= 1;
    }

    // Find end of wrapped row sequence
    let mut end_row = row;
    while end_row + 1 < rows && check_row_wraps(screen, end_row, rows, cols) {
        end_row += 1;
    }

    // Reconstruct combined text and compute click_char_idx
    let mut combined = String::new();
    let mut click_char_idx = 0;

    for r in start_row..=end_row {
        if r == row {
            click_char_idx = combined.chars().count() + (col as usize);
        }

        let is_last = r == end_row;
        let mut row_str = String::new();
        for c in 0..cols {
            if let Some(cell) = screen.cell(r, c) {
                let cnt = cell.contents();
                if cnt.is_empty() {
                    row_str.push(' ');
                } else {
                    row_str.push_str(cnt);
                }
            } else {
                row_str.push(' ');
            }
        }

        if is_last {
            combined.push_str(row_str.trim_end());
        } else {
            combined.push_str(&row_str);
        }
    }

    let chars: Vec<char> = combined.chars().collect();
    if click_char_idx >= chars.len() {
        return None;
    }
    if !is_token_char(chars[click_char_idx]) {
        return None;
    }

    // Expand left
    let mut start = click_char_idx;
    while start > 0 && is_token_char(chars[start - 1]) {
        start -= 1;
    }

    // Expand right
    let mut end = click_char_idx;
    while end < chars.len() && is_token_char(chars[end]) {
        end += 1;
    }

    let raw_token: String = chars[start..end].iter().collect();
    parse_link_target(&raw_token)
}

/// Convenience function extracting token at (row, col) from a TerminalPane.
#[must_use]
pub fn extract_token_at(pane: &TerminalPane, row: u16, col: u16) -> Option<LinkTarget> {
    extract_token_from_screen(pane.screen(), row, col)
}

/// Ranks matching candidates by longest shared directory prefix with `current_file`.
#[must_use]
pub fn pick_best_candidate<'a>(
    candidates: &[&'a PathBuf],
    current_file: Option<&Path>,
    root: &Path,
) -> Option<&'a PathBuf> {
    if candidates.is_empty() {
        return None;
    }
    if candidates.len() == 1 {
        return Some(candidates[0]);
    }

    let curr_rel = current_file.map(|p| p.strip_prefix(root).unwrap_or(p));
    let curr_parent = curr_rel.and_then(|p| p.parent()).unwrap_or(Path::new(""));
    let curr_components: Vec<_> = curr_parent.components().collect();

    candidates
        .iter()
        .copied()
        .max_by_key(|cand| {
            let cand_rel = cand.strip_prefix(root).unwrap_or(cand);
            let cand_parent = cand_rel.parent().unwrap_or(Path::new(""));
            let cand_components: Vec<_> = cand_parent.components().collect();

            let shared_len = curr_components
                .iter()
                .zip(cand_components.iter())
                .take_while(|(a, b)| a == b)
                .count();

            (
                shared_len,
                Reverse(cand_components.len()),
                Reverse(cand.to_path_buf()),
            )
        })
}

/// Resolves a target path string against direct path under root, suffix match in files,
/// or basename match in files, with directory tie-breaking.
#[must_use]
pub fn resolve_file(
    target_path_str: &str,
    root: &Path,
    files: &[PathBuf],
    current_file: Option<&Path>,
) -> Option<PathBuf> {
    if target_path_str.is_empty() {
        return None;
    }

    // 1. Direct path check (under root or absolute)
    let direct = if let Some(rel) = target_path_str.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(rel)
        } else {
            root.join(target_path_str)
        }
    } else if Path::new(target_path_str).is_absolute() {
        PathBuf::from(target_path_str)
    } else {
        root.join(target_path_str)
    };

    if direct.is_file() {
        return Some(direct);
    }

    // 2. Suffix match in files
    let target_path = Path::new(target_path_str);
    let mut candidates: Vec<&PathBuf> = files
        .iter()
        .filter(|f| f.as_path() == target_path || f.ends_with(target_path))
        .collect();

    // 3. Basename match in files
    if candidates.is_empty() {
        if let Some(name) = target_path.file_name() {
            candidates = files
                .iter()
                .filter(|f| f.file_name() == Some(name))
                .collect();
        }
    }

    // 4. Fallback: if not found and starts with a/ or b/, try unstripped or stripped
    if candidates.is_empty() {
        if let Some(rest) = target_path_str
            .strip_prefix("a/")
            .or_else(|| target_path_str.strip_prefix("b/"))
        {
            let rest_path = Path::new(rest);
            candidates = files
                .iter()
                .filter(|f| f.as_path() == rest_path || f.ends_with(rest_path))
                .collect();
        }
    }

    if let Some(best) = pick_best_candidate(&candidates, current_file, root) {
        if best.is_absolute() {
            Some(best.clone())
        } else {
            Some(root.join(best))
        }
    } else {
        None
    }
}

/// Convenience entry point: extracts token at (row, col) from the terminal and resolves it.
#[must_use]
pub fn extract_and_resolve(
    pane: &TerminalPane,
    row: u16,
    col: u16,
    root: &Path,
    files: &[PathBuf],
    current_file: Option<&Path>,
) -> Option<(PathBuf, Option<usize>, Option<usize>)> {
    let target = extract_token_from_screen(pane.screen(), row, col)?;
    let resolved = resolve_file(&target.path, root, files, current_file)?;
    Some((resolved, target.line, target.col))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_link_target_main_rs_with_line_and_col() {
        let res = parse_link_target("src/main.rs:25:5").expect("parsed target");
        assert_eq!(res.path, "src/main.rs");
        assert_eq!(res.line, Some(25));
        assert_eq!(res.col, Some(5));
    }

    #[test]
    fn test_parse_link_target_diff_prefix() {
        let res = parse_link_target("a/src/foo.rs:12").expect("parsed target");
        assert_eq!(res.path, "src/foo.rs");
        assert_eq!(res.line, Some(12));
        assert_eq!(res.col, None);

        let res_b = parse_link_target("b/src/foo.rs:12").expect("parsed target");
        assert_eq!(res_b.path, "src/foo.rs");
        assert_eq!(res_b.line, Some(12));
        assert_eq!(res_b.col, None);
    }

    #[test]
    fn test_parse_link_target_hash_line() {
        let res = parse_link_target("foo.rs#L12").expect("parsed target");
        assert_eq!(res.path, "foo.rs");
        assert_eq!(res.line, Some(12));
        assert_eq!(res.col, None);

        let res_col = parse_link_target("foo.rs#L12C4").expect("parsed target");
        assert_eq!(res_col.path, "foo.rs");
        assert_eq!(res_col.line, Some(12));
        assert_eq!(res_col.col, Some(4));
    }

    #[test]
    fn test_parse_link_target_quoted() {
        let res = parse_link_target("\"buffer.rs\"").expect("parsed target");
        assert_eq!(res.path, "buffer.rs");
        assert_eq!(res.line, None);
        assert_eq!(res.col, None);

        let res_single = parse_link_target("'buffer.rs'").expect("parsed target");
        assert_eq!(res_single.path, "buffer.rs");

        let res_bracket = parse_link_target("[buffer.rs]").expect("parsed target");
        assert_eq!(res_bracket.path, "buffer.rs");

        let res_parens_comma = parse_link_target("(\"buffer.rs\"),").expect("parsed target");
        assert_eq!(res_parens_comma.path, "buffer.rs");
    }

    #[test]
    fn test_same_basename_tie_breaking() {
        let root = Path::new("/workspace");
        let files = vec![
            PathBuf::from("src/app/mod.rs"),
            PathBuf::from("src/ui/mod.rs"),
            PathBuf::from("src/tree/mod.rs"),
        ];

        // Currently open: src/app/edit.rs -> should prefer src/app/mod.rs
        let current_app = Path::new("/workspace/src/app/edit.rs");
        let resolved = resolve_file("mod.rs", root, &files, Some(current_app)).expect("resolved");
        assert_eq!(resolved, PathBuf::from("/workspace/src/app/mod.rs"));

        // Currently open: src/ui/term.rs -> should prefer src/ui/mod.rs
        let current_ui = Path::new("/workspace/src/ui/term.rs");
        let resolved = resolve_file("mod.rs", root, &files, Some(current_ui)).expect("resolved");
        assert_eq!(resolved, PathBuf::from("/workspace/src/ui/mod.rs"));
    }

    #[test]
    fn test_soft_wrapped_path_extraction_on_25_col_screen() {
        let r = std::num::NonZeroU16::new(5).unwrap();
        let c = std::num::NonZeroU16::new(25).unwrap();
        let mut parser = vt100::Parser::new(r, c, 0);
        // "echo long/path/to/very/nested/file.rs:10:5" wraps at column 25
        parser.process(b"echo long/path/to/very/nested/file.rs:10:5");
        let screen = parser.screen();

        // Row 0 wrapped to Row 1
        assert!(screen.row_wrapped(0));

        // Click on Row 0 at column 7 ('g' in long)
        let target_r0 = extract_token_from_screen(screen, 0, 7).expect("extracted from row 0");
        assert_eq!(target_r0.path, "long/path/to/very/nested/file.rs");
        assert_eq!(target_r0.line, Some(10));
        assert_eq!(target_r0.col, Some(5));

        // Click on Row 1 at column 2 ('e' in sted)
        let target_r1 = extract_token_from_screen(screen, 1, 2).expect("extracted from row 1");
        assert_eq!(target_r1.path, "long/path/to/very/nested/file.rs");
        assert_eq!(target_r1.line, Some(10));
        assert_eq!(target_r1.col, Some(5));
    }
}
