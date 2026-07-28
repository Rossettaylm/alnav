//! M2 session bookmarks: pin log rows by ingest `row_id`.

/// How many bookmarks appear in the Log top strip.
pub const BOOKMARK_DISPLAY_N: usize = 3;
/// Soft cap on total bookmarks in the session.
pub const BOOKMARK_SOFT_CAP: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookmark {
    pub row_id: u64,
    /// Full strip/picker label (tag + first msg line); display truncates by width.
    pub label: String,
}

#[derive(Debug, Default)]
pub struct BookmarkList {
    /// Oldest → newest.
    pub items: Vec<Bookmark>,
}

impl BookmarkList {
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn contains_id(&self, row_id: u64) -> bool {
        self.items.iter().any(|b| b.row_id == row_id)
    }

    /// Newest-first slice for the Log top strip (≤ [`BOOKMARK_DISPLAY_N`]).
    pub fn display_recent(&self) -> Vec<&Bookmark> {
        self.items.iter().rev().take(BOOKMARK_DISPLAY_N).collect()
    }

    /// Push if under cap and not duplicate. Returns `Ok(true)` on insert.
    pub fn try_add(&mut self, bm: Bookmark) -> Result<(), AddError> {
        if self.contains_id(bm.row_id) {
            return Err(AddError::Duplicate);
        }
        if self.items.len() >= BOOKMARK_SOFT_CAP {
            return Err(AddError::Full);
        }
        self.items.push(bm);
        Ok(())
    }

    pub fn remove_id(&mut self, row_id: u64) -> bool {
        let before = self.items.len();
        self.items.retain(|b| b.row_id != row_id);
        self.items.len() < before
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn update_label(&mut self, row_id: u64, label: String) -> bool {
        let Some(bm) = self.items.iter_mut().find(|b| b.row_id == row_id) else {
            return false;
        };
        if bm.label == label {
            return false;
        }
        bm.label = label;
        true
    }

    pub fn delete_at(&mut self, index: usize) -> bool {
        if index >= self.items.len() {
            return false;
        }
        self.items.remove(index);
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddError {
    Duplicate,
    Full,
}

/// Result of jumping to a bookmarked `row_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpResult {
    Ok,
    /// Row left the ring buffer.
    Evicted,
    /// Row still buffered but not in current `visible`.
    Filtered,
}

/// Build strip/picker label from row fields (full first line; no eager truncate).
pub fn bookmark_label(tag: &str, msg: &str) -> String {
    let msg = msg.lines().next().unwrap_or("").trim_end();
    if tag.is_empty() {
        msg.to_string()
    } else if msg.is_empty() {
        tag.to_string()
    } else {
        format!("{tag} {msg}")
    }
}

/// Fit `label` into `max_cols` display cells, appending `…` when truncated.
pub fn fit_label(label: &str, max_cols: usize) -> String {
    if max_cols == 0 {
        return String::new();
    }
    let count = label.chars().count();
    if count <= max_cols {
        return label.to_string();
    }
    if max_cols == 1 {
        return "…".to_string();
    }
    let mut out: String = label.chars().take(max_cols - 1).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bm(row_id: u64, label: &str) -> Bookmark {
        Bookmark {
            row_id,
            label: label.into(),
        }
    }

    #[test]
    fn try_add_dedup_and_cap() {
        let mut list = BookmarkList::default();
        assert!(list.try_add(bm(1, "a")).is_ok());
        assert_eq!(list.try_add(bm(1, "a")), Err(AddError::Duplicate));
        for i in 2..=BOOKMARK_SOFT_CAP as u64 {
            assert!(list.try_add(bm(i, &format!("r{i}"))).is_ok());
        }
        assert_eq!(list.try_add(bm(9999, "x")), Err(AddError::Full));
    }

    #[test]
    fn display_recent_newest_first() {
        let mut list = BookmarkList::default();
        for i in 1..=5u64 {
            list.try_add(bm(i, &format!("{i}"))).unwrap();
        }
        let d: Vec<_> = list.display_recent().iter().map(|b| b.row_id).collect();
        assert_eq!(d, vec![5, 4, 3]);
    }

    #[test]
    fn bookmark_label_keeps_long_text() {
        let msg = "x".repeat(80);
        let label = bookmark_label("Tag", &msg);
        assert!(label.len() > 56, "must not eagerly truncate at 56");
        assert!(label.starts_with("Tag "));
        assert_eq!(label.chars().count(), 4 + 80); // "Tag " + 80
    }

    #[test]
    fn bookmark_label_first_line_only() {
        let label = bookmark_label("T", "line1\nline2");
        assert_eq!(label, "T line1");
    }

    #[test]
    fn fit_label_truncates_with_ellipsis() {
        assert_eq!(fit_label("abcdef", 4), "abc…");
        assert_eq!(fit_label("ab", 4), "ab");
        assert_eq!(fit_label("ab", 0), "");
    }
}
