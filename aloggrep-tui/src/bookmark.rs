//! M2 session bookmarks: pin log rows by ingest `row_id`.

/// How many bookmarks appear in the Log top strip.
pub const BOOKMARK_DISPLAY_N: usize = 3;
/// Soft cap on total bookmarks in the session.
pub const BOOKMARK_SOFT_CAP: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookmark {
    pub row_id: u64,
    /// Short label for strip / picker (tag + msg).
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
        self.items
            .iter()
            .rev()
            .take(BOOKMARK_DISPLAY_N)
            .collect()
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

/// Centered-top picker for `mm`: draft filter + selection over bookmarks.
#[derive(Debug, Default)]
pub struct BookmarkPicker {
    pub draft: String,
    pub selected: usize,
}

impl BookmarkPicker {
    pub fn open() -> Self {
        Self::default()
    }

    pub fn push_char(&mut self, c: char) {
        self.draft.push(c);
        self.selected = 0;
    }

    pub fn backspace(&mut self) {
        self.draft.pop();
        self.selected = 0;
    }

    /// Indices into `bookmarks.items` matching draft (ignore-case), newest first.
    pub fn filtered_indices(&self, bookmarks: &BookmarkList) -> Vec<usize> {
        let draft = self.draft.to_ascii_lowercase();
        let mut idxs: Vec<usize> = bookmarks
            .items
            .iter()
            .enumerate()
            .filter(|(_, b)| {
                draft.is_empty() || b.label.to_ascii_lowercase().contains(&draft)
            })
            .map(|(i, _)| i)
            .collect();
        idxs.reverse(); // newest first
        idxs
    }

    pub fn move_selection(&mut self, delta: isize, filtered_len: usize) {
        if filtered_len == 0 {
            self.selected = 0;
            return;
        }
        let cur = self.selected.min(filtered_len - 1) as isize;
        self.selected = (cur + delta).clamp(0, filtered_len as isize - 1) as usize;
    }

    /// Selected bookmark index into `bookmarks.items`, if any.
    pub fn selected_item_index(&self, bookmarks: &BookmarkList) -> Option<usize> {
        let filtered = self.filtered_indices(bookmarks);
        if filtered.is_empty() {
            return None;
        }
        filtered.get(self.selected.min(filtered.len() - 1)).copied()
    }
}

/// Build a compact strip/picker label from row fields.
pub fn bookmark_label(tag: &str, msg: &str) -> String {
    let raw = if tag.is_empty() {
        msg.to_string()
    } else {
        format!("{tag} {msg}")
    };
    let mut out = String::new();
    for (i, ch) in raw.chars().enumerate() {
        if i >= 56 {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_add_dedup_and_cap() {
        let mut list = BookmarkList::default();
        assert!(list
            .try_add(Bookmark {
                row_id: 1,
                label: "a".into(),
            })
            .is_ok());
        assert_eq!(
            list.try_add(Bookmark {
                row_id: 1,
                label: "a".into(),
            }),
            Err(AddError::Duplicate)
        );
        for i in 2..=BOOKMARK_SOFT_CAP as u64 {
            assert!(list
                .try_add(Bookmark {
                    row_id: i,
                    label: format!("r{i}"),
                })
                .is_ok());
        }
        assert_eq!(
            list.try_add(Bookmark {
                row_id: 9999,
                label: "x".into(),
            }),
            Err(AddError::Full)
        );
    }

    #[test]
    fn display_recent_newest_first() {
        let mut list = BookmarkList::default();
        for i in 1..=5u64 {
            list.try_add(Bookmark {
                row_id: i,
                label: format!("{i}"),
            })
            .unwrap();
        }
        let d: Vec<_> = list
            .display_recent()
            .iter()
            .map(|b| b.row_id)
            .collect();
        assert_eq!(d, vec![5, 4, 3]);
    }

    #[test]
    fn picker_filter_ignore_case() {
        let mut list = BookmarkList::default();
        list.try_add(Bookmark {
            row_id: 1,
            label: "Hello World".into(),
        })
        .unwrap();
        list.try_add(Bookmark {
            row_id: 2,
            label: "other".into(),
        })
        .unwrap();
        let mut p = BookmarkPicker::open();
        for c in "hello".chars() {
            p.push_char(c);
        }
        let idxs = p.filtered_indices(&list);
        assert_eq!(idxs, vec![0]);
    }
}
