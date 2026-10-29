use std::ops::Range;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::App;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    Yours,
    Agent,
    Both,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictHunk {
    pub base_range: Range<usize>,
    pub yours: Vec<String>, // Left (in-memory dirty buffer)
    pub agent: Vec<String>, // Right (on-disk modified file)
    pub resolution: Option<Resolution>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MergeChunk {
    Clean(Vec<String>),
    Conflict(ConflictHunk),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictSession {
    pub chunks: Vec<MergeChunk>,
    pub active_hunk_idx: usize,
    pub disk_hash: u64,
}

impl ConflictSession {
    #[must_use]
    pub fn new(chunks: Vec<MergeChunk>, disk_hash: u64) -> Self {
        Self {
            chunks,
            active_hunk_idx: 0,
            disk_hash,
        }
    }

    #[must_use]
    pub fn conflict_count(&self) -> usize {
        self.chunks
            .iter()
            .filter(|c| matches!(c, MergeChunk::Conflict(_)))
            .count()
    }

    #[must_use]
    pub fn is_all_resolved(&self) -> bool {
        self.chunks.iter().all(|c| match c {
            MergeChunk::Clean(_) => true,
            MergeChunk::Conflict(hunk) => hunk.resolution.is_some(),
        })
    }

    pub fn active_conflict_hunk(&self) -> Option<&ConflictHunk> {
        self.chunks
            .iter()
            .filter_map(|c| match c {
                MergeChunk::Conflict(h) => Some(h),
                MergeChunk::Clean(_) => None,
            })
            .nth(self.active_hunk_idx)
    }

    pub fn active_conflict_hunk_mut(&mut self) -> Option<&mut ConflictHunk> {
        let active = self.active_hunk_idx;
        self.chunks
            .iter_mut()
            .filter_map(|c| match c {
                MergeChunk::Conflict(h) => Some(h),
                MergeChunk::Clean(_) => None,
            })
            .nth(active)
    }

    pub fn next_hunk(&mut self) {
        let count = self.conflict_count();
        if count > 0 {
            self.active_hunk_idx = (self.active_hunk_idx + 1) % count;
        }
    }

    pub fn prev_hunk(&mut self) {
        let count = self.conflict_count();
        if count > 0 {
            self.active_hunk_idx = (self.active_hunk_idx + count - 1) % count;
        }
    }

    pub fn resolve_active(&mut self, res: Resolution) {
        if let Some(hunk) = self.active_conflict_hunk_mut() {
            hunk.resolution = Some(res);
        }
        let count = self.conflict_count();
        if count > 0 && !self.is_all_resolved() {
            for offset in 1..=count {
                let candidate = (self.active_hunk_idx + offset) % count;
                let is_unresolved = self
                    .chunks
                    .iter()
                    .filter_map(|c| match c {
                        MergeChunk::Conflict(h) => Some(h),
                        MergeChunk::Clean(_) => None,
                    })
                    .nth(candidate)
                    .is_some_and(|h| h.resolution.is_none());
                if is_unresolved {
                    self.active_hunk_idx = candidate;
                    break;
                }
            }
        }
    }

    pub fn resolve_all(&mut self, res: Resolution) {
        for chunk in &mut self.chunks {
            if let MergeChunk::Conflict(hunk) = chunk {
                hunk.resolution = Some(res);
            }
        }
    }

    #[must_use]
    pub fn finish(&self) -> Vec<String> {
        let mut result = Vec::new();
        for chunk in &self.chunks {
            match chunk {
                MergeChunk::Clean(lines) => {
                    result.extend(lines.iter().cloned());
                }
                MergeChunk::Conflict(hunk) => {
                    match hunk.resolution.unwrap_or(Resolution::Yours) {
                        Resolution::Yours => {
                            result.extend(hunk.yours.iter().cloned());
                        }
                        Resolution::Agent => {
                            result.extend(hunk.agent.iter().cloned());
                        }
                        Resolution::Both => {
                            result.extend(hunk.yours.iter().cloned());
                            result.extend(hunk.agent.iter().cloned());
                        }
                    }
                }
            }
        }
        if result.is_empty() {
            result.push(String::new());
        }
        result
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffHunk {
    pub base_range: Range<usize>,
    pub lines: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LineEditOp {
    Equal,
    Delete,
    Insert,
}

#[must_use]
pub fn diff_lines(base: &[String], other: &[String]) -> Vec<DiffHunk> {
    let prefix_len = base
        .iter()
        .zip(other.iter())
        .take_while(|(x, y)| x == y)
        .count();

    let suffix_len = base[prefix_len..]
        .iter()
        .rev()
        .zip(other[prefix_len..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();

    let a_mid = &base[prefix_len..base.len().saturating_sub(suffix_len)];
    let b_mid = &other[prefix_len..other.len().saturating_sub(suffix_len)];

    if a_mid.is_empty() && b_mid.is_empty() {
        return Vec::new();
    }

    if a_mid.is_empty() {
        return vec![DiffHunk {
            base_range: prefix_len..prefix_len,
            lines: b_mid.to_vec(),
        }];
    }

    if b_mid.is_empty() {
        return vec![DiffHunk {
            base_range: prefix_len..(base.len() - suffix_len),
            lines: Vec::new(),
        }];
    }

    let ops = myers_line_diff(a_mid, b_mid);
    let mut hunks = Vec::new();
    let mut cur_base = prefix_len;
    let mut active_start: Option<usize> = None;
    let mut active_lines: Vec<String> = Vec::new();

    for (op, _a_idx, b_idx) in ops {
        match op {
            LineEditOp::Equal => {
                if let Some(start) = active_start.take() {
                    hunks.push(DiffHunk {
                        base_range: start..cur_base,
                        lines: std::mem::take(&mut active_lines),
                    });
                }
                cur_base += 1;
            }
            LineEditOp::Delete => {
                if active_start.is_none() {
                    active_start = Some(cur_base);
                }
                cur_base += 1;
            }
            LineEditOp::Insert => {
                if active_start.is_none() {
                    active_start = Some(cur_base);
                }
                active_lines.push(b_mid[b_idx].clone());
            }
        }
    }

    if let Some(start) = active_start {
        hunks.push(DiffHunk {
            base_range: start..cur_base,
            lines: active_lines,
        });
    }

    hunks
}

fn myers_line_diff(a: &[String], b: &[String]) -> Vec<(LineEditOp, usize, usize)> {
    let n = a.len();
    let m = b.len();

    if n == 0 {
        return (0..m).map(|j| (LineEditOp::Insert, 0, j)).collect();
    }
    if m == 0 {
        return (0..n).map(|i| (LineEditOp::Delete, i, 0)).collect();
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

        // Safety fallback if diff distance is unusually large
        if d > 1000 && n + m > 5000 {
            let mut ops = Vec::with_capacity(n + m);
            for i in 0..n {
                ops.push((LineEditOp::Delete, i, 0));
            }
            for j in 0..m {
                ops.push((LineEditOp::Insert, n, j));
            }
            return ops;
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
            ops.push((LineEditOp::Equal, (x - 1) as usize, (y - 1) as usize));
            x -= 1;
            y -= 1;
        }
        if x == prev_x {
            ops.push((LineEditOp::Insert, x as usize, (y - 1) as usize));
            y -= 1;
        } else {
            ops.push((LineEditOp::Delete, (x - 1) as usize, y as usize));
            x -= 1;
        }
    }

    while x > 0 && y > 0 {
        ops.push((LineEditOp::Equal, (x - 1) as usize, (y - 1) as usize));
        x -= 1;
        y -= 1;
    }

    ops.reverse();
    ops
}

#[derive(Debug)]
pub enum ReconcileOutcome {
    SilentMerge { merged_lines: Vec<String> },
    Conflict { session: ConflictSession },
}

#[must_use]
pub fn hunks_overlap(h1: &Range<usize>, h2: &Range<usize>) -> bool {
    let s1 = h1.start;
    let e1 = h1.end;
    let s2 = h2.start;
    let e2 = h2.end;

    if s1 == e1 && s2 == e2 {
        s1 == s2
    } else if s1 == e1 {
        s1 > s2 && s1 < e2
    } else if s2 == e2 {
        s2 > s1 && s2 < e1
    } else {
        s1 < e2 && s2 < e1
    }
}

fn apply_hunks_to_base_slice(
    base: &[String],
    range: Range<usize>,
    hunks: &[DiffHunk],
) -> Vec<String> {
    let mut result = Vec::new();
    let mut cur = range.start;

    for hunk in hunks {
        if hunk.base_range.start > cur {
            let slice_end = hunk.base_range.start.min(range.end);
            if cur < slice_end && cur < base.len() {
                let actual_end = slice_end.min(base.len());
                result.extend(base[cur..actual_end].iter().cloned());
            }
        }
        result.extend(hunk.lines.iter().cloned());
        cur = hunk.base_range.end;
    }

    if cur < range.end && cur < base.len() {
        let actual_end = range.end.min(base.len());
        result.extend(base[cur..actual_end].iter().cloned());
    }

    result
}

#[must_use]
pub fn reconcile(
    base: &[String],
    yours: &[String],
    agent: &[String],
    disk_hash: u64,
) -> ReconcileOutcome {
    let hunks_y = diff_lines(base, yours);
    let hunks_a = diff_lines(base, agent);

    if hunks_y.is_empty() && hunks_a.is_empty() {
        return ReconcileOutcome::SilentMerge {
            merged_lines: base.to_vec(),
        };
    }
    if hunks_y.is_empty() {
        return ReconcileOutcome::SilentMerge {
            merged_lines: agent.to_vec(),
        };
    }
    if hunks_a.is_empty() {
        return ReconcileOutcome::SilentMerge {
            merged_lines: yours.to_vec(),
        };
    }

    struct Cluster {
        base_range: Range<usize>,
        yours: Vec<DiffHunk>,
        agent: Vec<DiffHunk>,
    }

    let mut clusters: Vec<Cluster> = hunks_y
        .into_iter()
        .map(|y| {
            let r = y.base_range.clone();
            Cluster {
                base_range: r,
                yours: vec![y],
                agent: Vec::new(),
            }
        })
        .collect();

    for a in hunks_a {
        let mut matching_indices = Vec::new();
        for (i, c) in clusters.iter().enumerate() {
            if hunks_overlap(&c.base_range, &a.base_range) {
                matching_indices.push(i);
            }
        }

        if matching_indices.is_empty() {
            clusters.push(Cluster {
                base_range: a.base_range.clone(),
                yours: Vec::new(),
                agent: vec![a],
            });
        } else {
            let first_idx = matching_indices[0];
            let mut merged_start = clusters[first_idx].base_range.start.min(a.base_range.start);
            let mut merged_end = clusters[first_idx].base_range.end.max(a.base_range.end);
            let mut merged_yours = Vec::new();
            let mut merged_agent = vec![a];

            for &idx in matching_indices.iter().rev() {
                let c = clusters.remove(idx);
                merged_start = merged_start.min(c.base_range.start);
                merged_end = merged_end.max(c.base_range.end);
                merged_yours.extend(c.yours);
                merged_agent.extend(c.agent);
            }
            merged_yours.sort_by_key(|h| h.base_range.start);
            merged_agent.sort_by_key(|h| h.base_range.start);

            clusters.push(Cluster {
                base_range: merged_start..merged_end,
                yours: merged_yours,
                agent: merged_agent,
            });
        }
    }

    clusters.sort_by_key(|c| c.base_range.start);

    let mut chunks = Vec::new();
    let mut cur_base = 0;

    for cluster in clusters {
        if cluster.base_range.start > cur_base {
            let actual_end = cluster.base_range.start.min(base.len());
            if cur_base < actual_end {
                chunks.push(MergeChunk::Clean(base[cur_base..actual_end].to_vec()));
            }
        }

        if cluster.agent.is_empty() {
            let lines =
                apply_hunks_to_base_slice(base, cluster.base_range.clone(), &cluster.yours);
            chunks.push(MergeChunk::Clean(lines));
        } else if cluster.yours.is_empty() {
            let lines =
                apply_hunks_to_base_slice(base, cluster.base_range.clone(), &cluster.agent);
            chunks.push(MergeChunk::Clean(lines));
        } else {
            let y_lines =
                apply_hunks_to_base_slice(base, cluster.base_range.clone(), &cluster.yours);
            let a_lines =
                apply_hunks_to_base_slice(base, cluster.base_range.clone(), &cluster.agent);

            if y_lines == a_lines {
                chunks.push(MergeChunk::Clean(y_lines));
            } else {
                chunks.push(MergeChunk::Conflict(ConflictHunk {
                    base_range: cluster.base_range.clone(),
                    yours: y_lines,
                    agent: a_lines,
                    resolution: None,
                }));
            }
        }

        cur_base = cluster.base_range.end;
    }

    if cur_base < base.len() {
        chunks.push(MergeChunk::Clean(base[cur_base..base.len()].to_vec()));
    }

    let has_conflicts = chunks.iter().any(|c| matches!(c, MergeChunk::Conflict(_)));
    if has_conflicts {
        ReconcileOutcome::Conflict {
            session: ConflictSession::new(chunks, disk_hash),
        }
    } else {
        let mut merged_lines = Vec::new();
        for chunk in chunks {
            if let MergeChunk::Clean(lines) = chunk {
                merged_lines.extend(lines);
            }
        }
        if merged_lines.is_empty() {
            merged_lines.push(String::new());
        }
        ReconcileOutcome::SilentMerge { merged_lines }
    }
}

pub fn handle_key(app: &mut App, key: KeyEvent) {
    let has_ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let has_shift = key.modifiers.contains(KeyModifiers::SHIFT);

    let action_taken = {
        let Some(conflict) = &mut app.conflict else {
            return;
        };

        match key.code {
            KeyCode::Char('c') if !has_ctrl => {
                if has_shift {
                    conflict.prev_hunk();
                } else {
                    conflict.next_hunk();
                }
                false
            }
            KeyCode::Char('C') if !has_ctrl => {
                conflict.prev_hunk();
                false
            }
            KeyCode::Char('l') if !has_ctrl && !has_shift => {
                conflict.resolve_active(Resolution::Yours);
                true
            }
            KeyCode::Char('r') if !has_ctrl && !has_shift => {
                conflict.resolve_active(Resolution::Agent);
                true
            }
            KeyCode::Char('b') if !has_ctrl && !has_shift => {
                conflict.resolve_active(Resolution::Both);
                true
            }
            KeyCode::Char('L') if !has_ctrl => {
                conflict.resolve_all(Resolution::Yours);
                true
            }
            KeyCode::Char('R') if !has_ctrl => {
                conflict.resolve_all(Resolution::Agent);
                true
            }
            KeyCode::Esc => {
                app.status_message =
                    Some("Resolve merge conflicts before exiting (press l/r/b/L/R)".into());
                false
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(buf) = &mut app.buf {
                    buf.move_up();
                }
                false
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(buf) = &mut app.buf {
                    buf.move_down();
                }
                false
            }
            _ => false,
        }
    };

    if action_taken {
        let is_resolved = app
            .conflict
            .as_ref()
            .is_some_and(|c| c.is_all_resolved());
        if is_resolved {
            let _ = app.complete_conflict_resolution();
        }
    }
}
