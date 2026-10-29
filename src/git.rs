use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Added,
    Changed,
    DeletedBelow,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub deleted_lines: Vec<String>,
    pub added_lines: Vec<String>,
}

impl Hunk {
    /// Returns 0-based buffer line number to jump to for this hunk.
    #[must_use]
    pub fn target_line(&self) -> usize {
        self.new_start.saturating_sub(1)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GhostLine {
    pub anchor_line: usize,
    pub text: String,
    pub spans: Vec<crate::intraline::Span>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Diff {
    pub hunks: Vec<Hunk>,
    pub marks: HashMap<usize, Mark>,
    pub ghosts: Vec<GhostLine>,
    pub changed_spans: HashMap<usize, Vec<crate::intraline::Span>>,
}

impl Diff {
    #[must_use]
    pub fn mark_for(&self, line: usize) -> Option<Mark> {
        self.marks.get(&line).copied()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.hunks.is_empty()
    }
}

fn parse_range(s: &str) -> Option<(usize, usize)> {
    if let Some((start_s, count_s)) = s.split_once(',') {
        let start = start_s.parse::<usize>().ok()?;
        let count = count_s.parse::<usize>().ok()?;
        Some((start, count))
    } else {
        let start = s.parse::<usize>().ok()?;
        Some((start, 1))
    }
}

fn parse_hunk_header(line: &str) -> Option<Hunk> {
    let trimmed = line.strip_prefix("@@ ")?;
    let (ranges, _) = if let Some(pair) = trimmed.split_once(" @@") {
        pair
    } else {
        trimmed.split_once("@@")?
    };
    let mut parts = ranges.split_whitespace();
    let old_part = parts.next()?.strip_prefix('-')?;
    let new_part = parts.next()?.strip_prefix('+')?;

    let (old_start, old_count) = parse_range(old_part)?;
    let (new_start, new_count) = parse_range(new_part)?;

    Some(Hunk {
        old_start,
        old_count,
        new_start,
        new_count,
        deleted_lines: Vec::new(),
        added_lines: Vec::new(),
    })
}

/// Parses git unified diff output into `Diff` containing hunks, marks, ghost lines, and intraline spans.
#[must_use]
pub fn parse_diff(output: &str) -> Diff {
    let mut hunks: Vec<Hunk> = Vec::new();
    let mut current_hunk: Option<Hunk> = None;

    for line in output.lines() {
        if line.starts_with("@@ ") {
            if let Some(h) = current_hunk.take() {
                hunks.push(h);
            }
            if let Some(hunk) = parse_hunk_header(line) {
                current_hunk = Some(hunk);
            }
        } else if let Some(hunk) = &mut current_hunk {
            if let Some(rest) = line.strip_prefix('-') {
                hunk.deleted_lines.push(rest.to_string());
            } else if let Some(rest) = line.strip_prefix('+') {
                hunk.added_lines.push(rest.to_string());
            }
        }
    }
    if let Some(h) = current_hunk {
        hunks.push(h);
    }

    let mut marks = HashMap::new();
    let mut ghosts = Vec::new();
    let mut changed_spans = HashMap::new();

    for hunk in &hunks {
        if hunk.old_count == 0 && hunk.new_count > 0 {
            // Pure addition
            let start = hunk.new_start.saturating_sub(1);
            for i in 0..hunk.new_count {
                marks.insert(start + i, Mark::Added);
            }
        } else if hunk.new_count == 0 {
            // Pure deletion
            let anchor = hunk.new_start;
            if hunk.new_start > 0 {
                marks.insert(hunk.new_start - 1, Mark::DeletedBelow);
            } else {
                marks.insert(0, Mark::DeletedBelow);
            }
            for del in &hunk.deleted_lines {
                let spans = vec![crate::intraline::Span::new(
                    del.clone(),
                    crate::intraline::SegmentKind::Deleted,
                )];
                ghosts.push(GhostLine {
                    anchor_line: anchor,
                    text: del.clone(),
                    spans,
                });
            }
        } else {
            // Replacement / modification
            let start = hunk.new_start.saturating_sub(1);
            for i in 0..hunk.new_count {
                marks.insert(start + i, Mark::Changed);
            }
            let anchor = start;
            for (i, del) in hunk.deleted_lines.iter().enumerate() {
                let del_spans = if i < hunk.added_lines.len() {
                    let (d, a) = crate::intraline::diff(del, &hunk.added_lines[i]);
                    changed_spans.insert(start + i, a);
                    d
                } else {
                    vec![crate::intraline::Span::new(
                        del.clone(),
                        crate::intraline::SegmentKind::Deleted,
                    )]
                };
                ghosts.push(GhostLine {
                    anchor_line: anchor,
                    text: del.clone(),
                    spans: del_spans,
                });
            }
        }
    }

    Diff {
        hunks,
        marks,
        ghosts,
        changed_spans,
    }
}

/// Runs background `git diff -U0 -M --no-color --no-ext-diff --inter-hunk-context=0 HEAD -- <path>`
/// and returns the parsed `Diff`.
#[must_use]
pub fn diff_file(root: &Path, path: &Path) -> Diff {
    let rel_path = path.strip_prefix(root).unwrap_or(path);
    let output = Command::new("git")
        .args([
            "diff",
            "-U0",
            "-M",
            "--no-color",
            "--no-ext-diff",
            "--inter-hunk-context=0",
            "HEAD",
            "--",
        ])
        .arg(rel_path)
        .current_dir(root)
        .output();

    let output = match output {
        Ok(out) if out.status.success() => Ok(out),
        _ => Command::new("git")
            .args([
                "diff",
                "-U0",
                "-M",
                "--no-color",
                "--no-ext-diff",
                "--inter-hunk-context=0",
                "--",
            ])
            .arg(rel_path)
            .current_dir(root)
            .output(),
    };

    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            parse_diff(&text)
        }
        _ => Diff::default(),
    }
}

/// Parses `git status --porcelain -uall` output into a map of relative file paths to status characters.
///
/// Status characters include:
/// - `'M'`: modified (in index or work tree)
/// - `'A'`: added to index
/// - `'D'`: deleted (in index or work tree)
/// - `'?'`: untracked file
#[must_use]
pub fn parse_status(output: &str) -> HashMap<PathBuf, char> {
    let mut marks = HashMap::new();
    for line in output.lines() {
        if let Some((path, status)) = parse_status_line(line) {
            marks.insert(path, status);
        }
    }
    marks
}

/// Parses a single line of `git status --porcelain` output.
#[must_use]
pub fn parse_status_line(line: &str) -> Option<(PathBuf, char)> {
    if line.len() < 3 {
        return None;
    }

    let status_bytes = line.as_bytes();
    let x = status_bytes[0] as char;
    let y = status_bytes[1] as char;
    let rest = &line[2..];
    let path_str = rest.strip_prefix(' ').unwrap_or(rest).trim();
    if path_str.is_empty() {
        return None;
    }

    let status_char = if x == '?' || y == '?' {
        '?'
    } else if x == 'M' || y == 'M' {
        'M'
    } else if x == 'A' || y == 'A' {
        'A'
    } else if x == 'D' || y == 'D' {
        'D'
    } else if x == 'R' || y == 'R' {
        'M'
    } else if x != ' ' {
        x
    } else if y != ' ' {
        y
    } else {
        return None;
    };

    // If rename: "old -> new", take the target path
    let raw_path = if let Some((_, target)) = path_str.split_once(" -> ") {
        target.trim()
    } else {
        path_str
    };

    // Strip surrounding quotes if git quoted the path
    let unquoted = if raw_path.starts_with('"') && raw_path.ends_with('"') && raw_path.len() >= 2 {
        let inner = &raw_path[1..raw_path.len() - 1];
        if inner.contains('\\') {
            unescape_git_path(inner)
        } else {
            inner.to_string()
        }
    } else {
        raw_path.to_string()
    };

    let path = PathBuf::from(unquoted);
    let normalized = path.strip_prefix("./").unwrap_or(&path).to_path_buf();

    Some((normalized, status_char))
}

fn unescape_git_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Runs `git status --porcelain -uall` in the given repository root directory and returns
/// the map of modified, added, deleted, and untracked file marks.
#[must_use]
pub fn list_marks(root: &Path) -> HashMap<PathBuf, char> {
    let output = Command::new("git")
        .args(["status", "--porcelain", "-uall"])
        .current_dir(root)
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            parse_status(&text)
        }
        _ => HashMap::new(),
    }
}

/// Rolls up file status marks to all ancestor directories.
/// Priority: `'M'` (4) > `'A'` (3) > `'?'` (2) > `'D'` (1).
#[must_use]
pub fn rollup_marks(marks: &HashMap<PathBuf, char>) -> HashMap<PathBuf, char> {
    let mut dir_marks: HashMap<PathBuf, char> = HashMap::new();

    fn priority(c: char) -> u8 {
        match c {
            'M' => 4,
            'A' => 3,
            '?' => 2,
            'D' => 1,
            _ => 0,
        }
    }

    for (path, &status) in marks {
        let mut curr = path.as_path();
        while let Some(parent) = curr.parent() {
            if parent.as_os_str().is_empty() {
                break;
            }
            let entry = dir_marks.entry(parent.to_path_buf()).or_insert(status);
            if priority(status) > priority(*entry) {
                *entry = status;
            }
            curr = parent;
        }
    }

    dir_marks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_status_variations() {
        let input = "\
?? untracked.txt
 M modified_worktree.rs
M  modified_index.rs
MM modified_both.rs
A  added_index.rs
AM added_and_modified.rs
 D deleted_worktree.txt
D  deleted_index.txt
R  old_name.rs -> new_name.rs
?? \"quoted name with spaces.txt\"
?? ./relative_path.txt
";
        let marks = parse_status(input);

        assert_eq!(marks.get(Path::new("untracked.txt")), Some(&'?'));
        assert_eq!(marks.get(Path::new("modified_worktree.rs")), Some(&'M'));
        assert_eq!(marks.get(Path::new("modified_index.rs")), Some(&'M'));
        assert_eq!(marks.get(Path::new("modified_both.rs")), Some(&'M'));
        assert_eq!(marks.get(Path::new("added_index.rs")), Some(&'A'));
        assert_eq!(marks.get(Path::new("added_and_modified.rs")), Some(&'M'));
        assert_eq!(marks.get(Path::new("deleted_worktree.txt")), Some(&'D'));
        assert_eq!(marks.get(Path::new("deleted_index.txt")), Some(&'D'));
        assert_eq!(marks.get(Path::new("new_name.rs")), Some(&'M'));
        assert_eq!(
            marks.get(Path::new("quoted name with spaces.txt")),
            Some(&'?')
        );
        assert_eq!(marks.get(Path::new("relative_path.txt")), Some(&'?'));
    }

    #[test]
    fn test_rollup_marks_priority_and_ancestors() {
        let mut marks = HashMap::new();
        marks.insert(PathBuf::from("a/b/c/file.rs"), 'M');
        marks.insert(PathBuf::from("a/b/new.rs"), '?');
        marks.insert(PathBuf::from("deleted_dir/gone.txt"), 'D');

        let rolled = rollup_marks(&marks);

        // a/b/c ancestor gets 'M'
        assert_eq!(rolled.get(Path::new("a/b/c")), Some(&'M'));
        // a/b has both 'M' child (a/b/c/file.rs) and '?' child (a/b/new.rs); 'M' wins
        assert_eq!(rolled.get(Path::new("a/b")), Some(&'M'));
        // a has 'M' descendant
        assert_eq!(rolled.get(Path::new("a")), Some(&'M'));
        // deleted_dir has 'D' child
        assert_eq!(rolled.get(Path::new("deleted_dir")), Some(&'D'));
    }

    #[test]
    fn test_parse_diff_pure_addition() {
        let diff_text = "\
diff --git a/foo.rs b/foo.rs
index 1234567..89abcde 100644
--- a/foo.rs
+++ b/foo.rs
@@ -2,0 +3,2 @@ fn bar()
+let a = 1;
+let b = 2;
";
        let diff = parse_diff(diff_text);
        assert_eq!(diff.hunks.len(), 1);
        let h = &diff.hunks[0];
        assert_eq!(h.old_start, 2);
        assert_eq!(h.old_count, 0);
        assert_eq!(h.new_start, 3);
        assert_eq!(h.new_count, 2);
        assert_eq!(h.target_line(), 2);
        assert_eq!(diff.marks.get(&2), Some(&Mark::Added));
        assert_eq!(diff.marks.get(&3), Some(&Mark::Added));
        assert!(diff.ghosts.is_empty());
    }

    #[test]
    fn test_parse_diff_pure_deletion() {
        let diff_text = "\
diff --git a/foo.rs b/foo.rs
--- a/foo.rs
+++ b/foo.rs
@@ -3,2 +2,0 @@ fn bar()
-del1
-del2
";
        let diff = parse_diff(diff_text);
        assert_eq!(diff.hunks.len(), 1);
        let h = &diff.hunks[0];
        assert_eq!(h.old_start, 3);
        assert_eq!(h.old_count, 2);
        assert_eq!(h.new_start, 2);
        assert_eq!(h.new_count, 0);
        assert_eq!(h.target_line(), 1);
        // Anchor line for ghost is new_start = 2
        // Line before deletion is new_start - 1 = 1 (DeletedBelow)
        assert_eq!(diff.marks.get(&1), Some(&Mark::DeletedBelow));
        assert_eq!(diff.ghosts.len(), 2);
        assert_eq!(diff.ghosts[0].anchor_line, 2);
        assert_eq!(diff.ghosts[0].text, "del1");
        assert_eq!(diff.ghosts[1].anchor_line, 2);
        assert_eq!(diff.ghosts[1].text, "del2");
    }

    #[test]
    fn test_parse_diff_modification_with_intraline() {
        let diff_text = "\
diff --git a/foo.rs b/foo.rs
--- a/foo.rs
+++ b/foo.rs
@@ -5 +5 @@ fn bar()
-let x = 10;
+let y = 10;
";
        let diff = parse_diff(diff_text);
        assert_eq!(diff.hunks.len(), 1);
        let h = &diff.hunks[0];
        assert_eq!(h.old_start, 5);
        assert_eq!(h.new_start, 5);
        assert_eq!(h.target_line(), 4);
        assert_eq!(diff.marks.get(&4), Some(&Mark::Changed));
        assert_eq!(diff.ghosts.len(), 1);
        assert_eq!(diff.ghosts[0].anchor_line, 4);
        assert_eq!(diff.ghosts[0].text, "let x = 10;");
        // Verify intraline spans on ghost line
        assert_eq!(
            diff.ghosts[0].spans,
            vec![
                crate::intraline::Span::new("let ", crate::intraline::SegmentKind::Unchanged),
                crate::intraline::Span::new("x", crate::intraline::SegmentKind::Deleted),
                crate::intraline::Span::new(" = 10;", crate::intraline::SegmentKind::Unchanged),
            ]
        );
        // Verify changed_spans for line 4
        assert_eq!(
            diff.changed_spans.get(&4),
            Some(&vec![
                crate::intraline::Span::new("let ", crate::intraline::SegmentKind::Unchanged),
                crate::intraline::Span::new("y", crate::intraline::SegmentKind::Added),
                crate::intraline::Span::new(" = 10;", crate::intraline::SegmentKind::Unchanged),
            ])
        );
    }
}
