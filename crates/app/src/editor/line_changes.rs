//! Saved-text comparison for the source gutter. No repository or disk I/O.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LineChange {
    #[default]
    Unchanged,
    Added,
    Modified,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LineMarker {
    pub change: LineChange,
    pub deleted_before: usize,
    pub deleted_after: usize,
}

type CachedMarkers = Option<((u64, u64), Arc<[LineMarker]>)>;

#[derive(Debug, Default)]
pub(super) struct Cache(Mutex<CachedMarkers>);

impl Cache {
    pub fn get_or_compute(
        &self,
        key: (u64, u64),
        compute: impl FnOnce() -> Arc<[LineMarker]>,
    ) -> Arc<[LineMarker]> {
        let mut value = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((cached_key, markers)) = &*value
            && *cached_key == key
        {
            return Arc::clone(markers);
        }
        let markers = compute();
        *value = Some((key, Arc::clone(&markers)));
        markers
    }
}

impl Clone for Cache {
    fn clone(&self) -> Self {
        Self(Mutex::new(
            self.0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone(),
        ))
    }
}

impl PartialEq for Cache {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}
impl Eq for Cache {}

pub(super) fn compare(old: &[Cow<'_, str>], new: &[Cow<'_, str>]) -> Arc<[LineMarker]> {
    let mut matches = Vec::new();
    let mut prefix = 0;
    while prefix < old.len().min(new.len()) && old[prefix] == new[prefix] {
        matches.push((prefix, prefix));
        prefix += 1;
    }
    let mut old_end = old.len();
    let mut new_end = new.len();
    while old_end > prefix && new_end > prefix && old[old_end - 1] == new[new_end - 1] {
        old_end -= 1;
        new_end -= 1;
    }

    // Unique common lines anchor distant edits without a quadratic whole-file table.
    let mut occurrences = HashMap::<&str, (usize, usize, usize, usize)>::new();
    for (index, line) in old.iter().enumerate().take(old_end).skip(prefix) {
        let entry = occurrences.entry(line.as_ref()).or_default();
        entry.0 = index;
        entry.1 += 1;
    }
    for (index, line) in new.iter().enumerate().take(new_end).skip(prefix) {
        if let Some(entry) = occurrences.get_mut(line.as_ref()) {
            entry.2 = index;
            entry.3 += 1;
        }
    }
    let mut candidates = occurrences
        .values()
        .filter(|entry| entry.1 == 1 && entry.3 == 1)
        .map(|entry| (entry.0, entry.2))
        .collect::<Vec<_>>();
    candidates.sort_unstable();
    let mut tails: Vec<usize> = Vec::new();
    let mut previous = vec![None; candidates.len()];
    for (index, &(_, new_index)) in candidates.iter().enumerate() {
        let position = tails.partition_point(|&tail| candidates[tail].1 < new_index);
        if position > 0 {
            previous[index] = Some(tails[position - 1]);
        }
        if position == tails.len() {
            tails.push(index);
        } else {
            tails[position] = index;
        }
    }
    let mut anchors = Vec::new();
    let mut next = tails.last().copied();
    while let Some(index) = next {
        anchors.push(candidates[index]);
        next = previous[index];
    }
    anchors.reverse();
    anchors.push((old_end, new_end));
    let (mut a, mut b) = (prefix, prefix);
    // Limit work for completely rewritten or highly repetitive large files.
    // Such unmatched blocks are conservatively marked as replacements.
    let mut budget = 1_000_000usize;
    for (x, y) in anchors {
        match_gap(old, new, a..x, b..y, &mut budget, &mut matches);
        if x < old_end {
            matches.push((x, y));
        }
        a = x + 1;
        b = y + 1;
    }
    matches.extend((old_end..old.len()).zip(new_end..new.len()));
    matches.push((old.len(), new.len()));

    let mut markers = vec![LineMarker::default(); new.len().max(1)];
    let (mut a, mut b) = (0, 0);
    for (x, y) in matches {
        let paired = (x - a).min(y - b);
        for marker in &mut markers[b..b + paired] {
            marker.change = LineChange::Modified;
        }
        for marker in &mut markers[b + paired..y] {
            marker.change = LineChange::Added;
        }
        let deleted = (x - a).saturating_sub(paired);
        if deleted > 0 {
            if y < markers.len() {
                markers[y].deleted_before += deleted;
            } else {
                markers.last_mut().unwrap().deleted_after += deleted;
            }
        }
        a = x + 1;
        b = y + 1;
    }
    markers.into()
}

fn match_gap(
    old: &[Cow<'_, str>],
    new: &[Cow<'_, str>],
    mut a: std::ops::Range<usize>,
    mut b: std::ops::Range<usize>,
    budget: &mut usize,
    matches: &mut Vec<(usize, usize)>,
) {
    while !a.is_empty() && !b.is_empty() && old[a.start] == new[b.start] {
        matches.push((a.start, b.start));
        a.start += 1;
        b.start += 1;
    }
    let mut suffix = Vec::new();
    while !a.is_empty() && !b.is_empty() && old[a.end - 1] == new[b.end - 1] {
        a.end -= 1;
        b.end -= 1;
        suffix.push((a.end, b.end));
    }
    let cells = (a.len() + 1).saturating_mul(b.len() + 1);
    if !a.is_empty() && !b.is_empty() && cells <= *budget {
        *budget -= cells;
        let width = b.len() + 1;
        let mut lengths = vec![0usize; cells];
        for i in (0..a.len()).rev() {
            for j in (0..b.len()).rev() {
                lengths[i * width + j] = if old[a.start + i] == new[b.start + j] {
                    1 + lengths[(i + 1) * width + j + 1]
                } else {
                    lengths[(i + 1) * width + j].max(lengths[i * width + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < a.len() && j < b.len() {
            if old[a.start + i] == new[b.start + j] {
                matches.push((a.start + i, b.start + j));
                i += 1;
                j += 1;
            } else if lengths[(i + 1) * width + j] >= lengths[i * width + j + 1] {
                i += 1;
            } else {
                j += 1;
            }
        }
    }
    matches.extend(suffix.into_iter().rev());
}
