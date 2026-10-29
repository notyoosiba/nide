#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentKind {
    Unchanged,
    Added,
    Deleted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub kind: SegmentKind,
}

impl Span {
    #[must_use]
    pub fn new(text: impl Into<String>, kind: SegmentKind) -> Self {
        Self {
            text: text.into(),
            kind,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditOp {
    Equal,
    Delete,
    Insert,
}

/// Tokenizes a line into word tokens, whitespace runs, and punctuation symbols.
#[must_use]
pub fn tokenize(s: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut chars = s.char_indices().peekable();

    while let Some(&(i, c)) = chars.peek() {
        if c.is_alphanumeric() || c == '_' {
            chars.next();
            let mut end = i + c.len_utf8();
            while let Some(&(next_i, next_c)) = chars.peek() {
                if next_c.is_alphanumeric() || next_c == '_' {
                    end = next_i + next_c.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            tokens.push(&s[i..end]);
        } else if c.is_whitespace() {
            chars.next();
            let mut end = i + c.len_utf8();
            while let Some(&(next_i, next_c)) = chars.peek() {
                if next_c.is_whitespace() {
                    end = next_i + next_c.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            tokens.push(&s[i..end]);
        } else {
            chars.next();
            let end = i + c.len_utf8();
            tokens.push(&s[i..end]);
        }
    }

    tokens
}

/// Executes Myers O(ND) diff algorithm between two token sequences.
#[must_use]
pub fn myers_diff<T: PartialEq + Clone>(a: &[T], b: &[T]) -> Vec<(EditOp, T)> {
    let n = a.len();
    let m = b.len();

    if n == 0 && m == 0 {
        return Vec::new();
    }
    if n == 0 {
        return b
            .iter()
            .map(|item| (EditOp::Insert, item.clone()))
            .collect();
    }
    if m == 0 {
        return a
            .iter()
            .map(|item| (EditOp::Delete, item.clone()))
            .collect();
    }

    // Safety fallback for very large sequences
    if n + m > 2000 {
        let mut ops = Vec::with_capacity(n + m);
        ops.extend(a.iter().map(|item| (EditOp::Delete, item.clone())));
        ops.extend(b.iter().map(|item| (EditOp::Insert, item.clone())));
        return ops;
    }

    let max = n + m;
    let offset = max as isize;
    let mut v = vec![0isize; 2 * max + 1];
    let mut trace = Vec::with_capacity(max + 1);

    'outer: for d in 0..=max {
        trace.push(v.clone());
        let d_isize = d as isize;
        let mut k = -d_isize;
        while k <= d_isize {
            let k_idx = (k + offset) as usize;
            let mut x = if k == -d_isize
                || (k != d_isize && v[(k - 1 + offset) as usize] < v[(k + 1 + offset) as usize])
            {
                v[(k + 1 + offset) as usize]
            } else {
                v[(k - 1 + offset) as usize] + 1
            };
            let mut y = x - k;
            while (x as usize) < n && (y as usize) < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[k_idx] = x;
            if (x as usize) >= n && (y as usize) >= m {
                break 'outer;
            }
            k += 2;
        }
    }

    let mut x = n as isize;
    let mut y = m as isize;
    let mut ops = Vec::new();

    for d in (1..trace.len()).rev() {
        let k = x - y;
        let v_prev = &trace[d];
        let d_isize = d as isize;
        let prev_k = if k == -d_isize
            || (k != d_isize
                && v_prev[(k - 1 + offset) as usize] < v_prev[(k + 1 + offset) as usize])
        {
            k + 1
        } else {
            k - 1
        };
        let prev_x = v_prev[(prev_k + offset) as usize];
        let prev_y = prev_x - prev_k;

        while x > prev_x && y > prev_y {
            ops.push((EditOp::Equal, a[(x - 1) as usize].clone()));
            x -= 1;
            y -= 1;
        }
        if x == prev_x {
            ops.push((EditOp::Insert, b[(y - 1) as usize].clone()));
            y -= 1;
        } else {
            ops.push((EditOp::Delete, a[(x - 1) as usize].clone()));
            x -= 1;
        }
    }

    while x > 0 && y > 0 {
        ops.push((EditOp::Equal, a[(x - 1) as usize].clone()));
        x -= 1;
        y -= 1;
    }

    ops.reverse();
    ops
}

fn push_span(spans: &mut Vec<Span>, text: &str, kind: SegmentKind) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = spans.last_mut() {
        if last.kind == kind {
            last.text.push_str(text);
            return;
        }
    }
    spans.push(Span {
        text: text.to_string(),
        kind,
    });
}

/// Word-level Myers intraline diff between `old` and `new` strings.
/// Returns coalesced `(old_spans, new_spans)`.
#[must_use]
pub fn diff_words(old: &str, new: &str) -> (Vec<Span>, Vec<Span>) {
    let old_tokens = tokenize(old);
    let new_tokens = tokenize(new);
    let ops = myers_diff(&old_tokens, &new_tokens);

    let mut old_spans = Vec::new();
    let mut new_spans = Vec::new();

    for (op, token) in ops {
        match op {
            EditOp::Equal => {
                push_span(&mut old_spans, token, SegmentKind::Unchanged);
                push_span(&mut new_spans, token, SegmentKind::Unchanged);
            }
            EditOp::Delete => {
                push_span(&mut old_spans, token, SegmentKind::Deleted);
            }
            EditOp::Insert => {
                push_span(&mut new_spans, token, SegmentKind::Added);
            }
        }
    }

    (old_spans, new_spans)
}

/// Character-level Myers intraline diff between `old` and `new` strings.
/// Returns coalesced `(old_spans, new_spans)`.
#[must_use]
pub fn diff_chars(old: &str, new: &str) -> (Vec<Span>, Vec<Span>) {
    let old_chars: Vec<&str> = old
        .char_indices()
        .map(|(i, c)| &old[i..i + c.len_utf8()])
        .collect();
    let new_chars: Vec<&str> = new
        .char_indices()
        .map(|(i, c)| &new[i..i + c.len_utf8()])
        .collect();
    let ops = myers_diff(&old_chars, &new_chars);

    let mut old_spans = Vec::new();
    let mut new_spans = Vec::new();

    for (op, ch) in ops {
        match op {
            EditOp::Equal => {
                push_span(&mut old_spans, ch, SegmentKind::Unchanged);
                push_span(&mut new_spans, ch, SegmentKind::Unchanged);
            }
            EditOp::Delete => {
                push_span(&mut old_spans, ch, SegmentKind::Deleted);
            }
            EditOp::Insert => {
                push_span(&mut new_spans, ch, SegmentKind::Added);
            }
        }
    }

    (old_spans, new_spans)
}

/// Default intraline diff using word-level Myers algorithm.
#[must_use]
pub fn diff(old: &str, new: &str) -> (Vec<Span>, Vec<Span>) {
    diff_words(old, new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_basic() {
        let tokens = tokenize("let count = 42; // note");
        assert_eq!(
            tokens,
            vec!["let", " ", "count", " ", "=", " ", "42", ";", " ", "/", "/", " ", "note"]
        );
    }

    #[test]
    fn test_myers_diff_identical() {
        let (old_spans, new_spans) = diff("hello world", "hello world");
        assert_eq!(old_spans, vec![Span::new("hello world", SegmentKind::Unchanged)]);
        assert_eq!(new_spans, vec![Span::new("hello world", SegmentKind::Unchanged)]);
    }

    #[test]
    fn test_myers_diff_word_replacement() {
        let (old_spans, new_spans) = diff("let x = 10;", "let y = 10;");
        assert_eq!(
            old_spans,
            vec![
                Span::new("let ", SegmentKind::Unchanged),
                Span::new("x", SegmentKind::Deleted),
                Span::new(" = 10;", SegmentKind::Unchanged),
            ]
        );
        assert_eq!(
            new_spans,
            vec![
                Span::new("let ", SegmentKind::Unchanged),
                Span::new("y", SegmentKind::Added),
                Span::new(" = 10;", SegmentKind::Unchanged),
            ]
        );
    }

    #[test]
    fn test_myers_diff_empty() {
        let (old_spans, new_spans) = diff("", "new line");
        assert_eq!(old_spans, vec![]);
        assert_eq!(new_spans, vec![Span::new("new line", SegmentKind::Added)]);

        let (old_spans, new_spans) = diff("old line", "");
        assert_eq!(old_spans, vec![Span::new("old line", SegmentKind::Deleted)]);
        assert_eq!(new_spans, vec![]);
    }

    #[test]
    fn test_diff_chars() {
        let (old_spans, new_spans) = diff_chars("cat", "cut");
        assert_eq!(
            old_spans,
            vec![
                Span::new("c", SegmentKind::Unchanged),
                Span::new("a", SegmentKind::Deleted),
                Span::new("t", SegmentKind::Unchanged),
            ]
        );
        assert_eq!(
            new_spans,
            vec![
                Span::new("c", SegmentKind::Unchanged),
                Span::new("u", SegmentKind::Added),
                Span::new("t", SegmentKind::Unchanged),
            ]
        );
    }
}
