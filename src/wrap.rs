use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WrappedRow {
    pub buffer_row: usize,
    pub char_range: std::ops::Range<usize>,
    pub is_continuation: bool,
}

/// Wraps a single logical line into visual rows of at most `max_width` characters.
/// Word boundaries are respected when possible; oversized words are split.
/// Every character index in `0..line.chars().count()` is guaranteed to be covered
/// consecutively with no gaps or overlaps.
#[must_use]
pub fn wrap_line(buffer_row: usize, line: &str, max_width: usize) -> Vec<WrappedRow> {
    let max_width = max_width.max(1);
    let total_chars = line.chars().count();

    if total_chars == 0 {
        return vec![WrappedRow {
            buffer_row,
            char_range: 0..0,
            is_continuation: false,
        }];
    }

    if total_chars <= max_width {
        return vec![WrappedRow {
            buffer_row,
            char_range: 0..total_chars,
            is_continuation: false,
        }];
    }

    let mut rows = Vec::new();
    let mut current_start = 0;
    let mut current_len = 0;

    for word in line.split_word_bounds() {
        let word_len = word.chars().count();
        if word_len == 0 {
            continue;
        }

        if current_len + word_len <= max_width {
            current_len += word_len;
        } else {
            // Flush current accumulated row if it has content
            if current_len > 0 {
                rows.push(WrappedRow {
                    buffer_row,
                    char_range: current_start..current_start + current_len,
                    is_continuation: !rows.is_empty(),
                });
                current_start += current_len;
                current_len = 0;
            }

            if word_len > max_width {
                // Word is longer than max_width: chunk it
                let mut word_offset = 0;
                while word_offset < word_len {
                    let chunk_len = (word_len - word_offset).min(max_width);
                    if chunk_len == max_width {
                        rows.push(WrappedRow {
                            buffer_row,
                            char_range: (current_start + word_offset)
                                ..(current_start + word_offset + chunk_len),
                            is_continuation: !rows.is_empty(),
                        });
                        word_offset += chunk_len;
                    } else {
                        current_len = chunk_len;
                        current_start += word_offset;
                        break;
                    }
                }
                if current_len == 0 {
                    current_start += word_len;
                }
            } else {
                current_len = word_len;
            }
        }
    }

    if current_len > 0 {
        rows.push(WrappedRow {
            buffer_row,
            char_range: current_start..current_start + current_len,
            is_continuation: !rows.is_empty(),
        });
    }

    if rows.is_empty() {
        rows.push(WrappedRow {
            buffer_row,
            char_range: 0..total_chars,
            is_continuation: false,
        });
    }

    rows
}

/// Wraps all lines in the buffer into a contiguous list of visual rows.
#[must_use]
pub fn wrap_buffer(lines: &[String], max_width: usize) -> Vec<WrappedRow> {
    if lines.is_empty() {
        return vec![WrappedRow {
            buffer_row: 0,
            char_range: 0..0,
            is_continuation: false,
        }];
    }

    let mut rows = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        rows.extend(wrap_line(i, line, max_width));
    }
    rows
}

/// Maps a buffer coordinate `(buffer_row, buffer_col)` to visual coordinate `(visual_row, visual_col)`.
#[must_use]
pub fn buffer_to_visual(
    wrapped: &[WrappedRow],
    buffer_row: usize,
    buffer_col: usize,
) -> Option<(usize, usize)> {
    if wrapped.is_empty() {
        return None;
    }

    let matching: Vec<(usize, &WrappedRow)> = wrapped
        .iter()
        .enumerate()
        .filter(|(_, r)| r.buffer_row == buffer_row)
        .collect();

    if matching.is_empty() {
        let last_idx = wrapped.len().saturating_sub(1);
        let last_row = &wrapped[last_idx];
        let col = buffer_col.saturating_sub(last_row.char_range.start);
        return Some((last_idx, col));
    }

    for &(v_idx, r) in &matching {
        if buffer_col >= r.char_range.start && buffer_col < r.char_range.end {
            return Some((v_idx, buffer_col - r.char_range.start));
        }
    }

    if let Some(&(v_idx, last_row)) = matching.last() {
        let col = buffer_col.saturating_sub(last_row.char_range.start);
        return Some((v_idx, col));
    }

    None
}

/// Maps a visual coordinate `(visual_row, visual_col)` to buffer coordinate `(buffer_row, buffer_col)`.
#[must_use]
pub fn visual_to_buffer(
    wrapped: &[WrappedRow],
    visual_row: usize,
    visual_col: usize,
) -> Option<(usize, usize)> {
    if wrapped.is_empty() {
        return None;
    }
    let v_idx = visual_row.min(wrapped.len().saturating_sub(1));
    let row = &wrapped[v_idx];
    let col = (row.char_range.start + visual_col).min(row.char_range.end);
    Some((row.buffer_row, col))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_empty_and_short_line() {
        let r1 = wrap_line(0, "", 80);
        assert_eq!(r1.len(), 1);
        assert_eq!(r1[0].char_range, 0..0);
        assert!(!r1[0].is_continuation);

        let r2 = wrap_line(1, "short", 80);
        assert_eq!(r2.len(), 1);
        assert_eq!(r2[0].char_range, 0..5);
        assert!(!r2[0].is_continuation);
    }

    #[test]
    fn test_wrap_long_line_coverage_and_continuity() {
        let text = "The quick brown fox jumps over the lazy dog. A second sentence follows.";
        let width = 15;
        let rows = wrap_line(0, text, width);

        assert!(rows.len() > 1);
        assert!(!rows[0].is_continuation);
        for row in &rows[1..] {
            assert!(row.is_continuation);
        }

        // Assert no gaps and full coverage
        assert_eq!(rows[0].char_range.start, 0);
        for i in 0..rows.len() - 1 {
            assert_eq!(rows[i].char_range.end, rows[i + 1].char_range.start);
        }
        assert_eq!(rows.last().unwrap().char_range.end, text.chars().count());
    }

    #[test]
    fn test_wrap_long_word_without_spaces() {
        let word = "abcdefghijklmnopqrstuvwxyz";
        let rows = wrap_line(0, word, 10);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].char_range, 0..10);
        assert_eq!(rows[1].char_range, 10..20);
        assert_eq!(rows[2].char_range, 20..26);
    }

    #[test]
    fn test_bidirectional_coordinate_mapping() {
        let lines = vec![
            "short line".to_string(),
            "first part second part third part".to_string(),
        ];
        let wrapped = wrap_buffer(&lines, 12);

        // Map buffer pos in first line
        let (v_row, v_col) = buffer_to_visual(&wrapped, 0, 5).unwrap();
        assert_eq!(v_row, 0);
        assert_eq!(v_col, 5);

        let (b_row, b_col) = visual_to_buffer(&wrapped, v_row, v_col).unwrap();
        assert_eq!(b_row, 0);
        assert_eq!(b_col, 5);

        // Map buffer pos in wrapped second line
        let (v_row2, v_col2) = buffer_to_visual(&wrapped, 1, 15).unwrap();
        assert!(v_row2 >= 1);
        let (b_row2, b_col2) = visual_to_buffer(&wrapped, v_row2, v_col2).unwrap();
        assert_eq!(b_row2, 1);
        assert_eq!(b_col2, 15);
    }
}
