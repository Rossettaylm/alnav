use std::collections::HashSet;

use crate::input::InputBox;
use crate::text_field::TextField;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnifiedKind {
    Filter,
    Highlight,
    Exclude,
}

impl UnifiedKind {
    pub fn tag(self) -> &'static str {
        match self {
            Self::Filter => "Filter",
            Self::Highlight => "Highlight",
            Self::Exclude => "Exclude",
        }
    }

    pub fn as_picker_kind(self) -> PickerKind {
        match self {
            Self::Filter => PickerKind::Filter,
            Self::Highlight => PickerKind::Highlight,
            Self::Exclude => PickerKind::Exclude,
        }
    }
}

/// Stable identity of a row in the unified Manage list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnifiedId {
    pub kind: UnifiedKind,
    pub source_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifiedItem {
    pub id: UnifiedId,
    /// Full list label, e.g. `[Filter]: tag:Foo`.
    pub label: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerKind {
    /// Aggregated Manage panel (Filter + Highlight + Exclude + Bookmark).
    Unified,
    Filter,
    Highlight,
    Bookmark,
    Exclude,
    MsgChip {
        exclude: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerMode {
    Manage,
    New,
    Edit { index: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmKind {
    /// Delete one or more unified items (single or Tab multi-select).
    DeleteMany { items: Vec<UnifiedId> },
    /// Delete a single bookmark by index into `bookmarks.items` (F2/F4).
    DeleteBookmark { index: usize },
}

pub struct PickerSession {
    pub kind: PickerKind,
    pub mode: PickerMode,
    /// Manage 过滤串（不含前缀 `/`）
    pub query: TextField,
    /// New/Edit draft（不含前缀 `:`）
    pub draft: TextField,
    pub selected: usize,
    pub confirm: Option<ConfirmKind>,
    /// Filter/Exclude New|Edit 时复用
    pub input: Option<InputBox>,
    /// Picker-local candidates (currently msg-chip tokens).
    pub choices: Vec<String>,
    /// Tab multi-select set (Unified Manage only); keyed by stable source id.
    pub checked: HashSet<UnifiedId>,
}

impl PickerSession {
    pub fn open(kind: PickerKind) -> Self {
        Self {
            kind,
            mode: PickerMode::Manage,
            query: TextField::new(),
            draft: TextField::new(),
            selected: 0,
            confirm: None,
            input: None,
            choices: Vec::new(),
            checked: HashSet::new(),
        }
    }

    pub fn prompt_prefix(&self) -> char {
        match self.mode {
            PickerMode::Manage => '/',
            PickerMode::New | PickerMode::Edit { .. } => ':',
        }
    }

    pub fn enter_new(&mut self) {
        self.mode = PickerMode::New;
        self.query.clear();
        self.draft.clear();
        self.selected = 0;
        self.confirm = None;
        self.checked.clear();
        self.input = Self::fresh_input_for_kind(self.kind);
    }

    pub fn enter_edit(&mut self, index: usize, prefill: String) {
        self.mode = PickerMode::Edit { index };
        self.draft = TextField::from_text(prefill);
        self.selected = 0;
        self.confirm = None;
        self.checked.clear();
        self.input = None;
    }

    pub fn enter_edit_input(&mut self, index: usize, input: InputBox) {
        self.mode = PickerMode::Edit { index };
        self.draft.clear();
        self.selected = 0;
        self.confirm = None;
        self.checked.clear();
        self.input = Some(input);
    }

    pub fn request_delete_many(&mut self, items: Vec<UnifiedId>) {
        if items.is_empty() {
            return;
        }
        self.confirm = Some(ConfirmKind::DeleteMany { items });
    }

    pub fn cancel_confirm(&mut self) {
        self.confirm = None;
    }

    /// ignore-case 子串过滤；返回源列表下标
    pub fn filtered_indices(labels: &[String], query: &str) -> Vec<usize> {
        if query.is_empty() {
            return (0..labels.len()).collect();
        }
        let q = query.to_lowercase();
        labels
            .iter()
            .enumerate()
            .filter(|(_, label)| label.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect()
    }

    fn fresh_input_for_kind(kind: PickerKind) -> Option<InputBox> {
        match kind {
            PickerKind::Filter => Some(InputBox::default()),
            PickerKind::Exclude => Some(InputBox {
                exclude_mode: true,
                ..InputBox::default()
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_defaults_to_manage_with_slash_prefix() {
        let p = PickerSession::open(PickerKind::Unified);
        assert_eq!(p.mode, PickerMode::Manage);
        assert_eq!(p.prompt_prefix(), '/');
        assert!(p.checked.is_empty());
    }

    #[test]
    fn colon_enter_new() {
        let mut p = PickerSession::open(PickerKind::Highlight);
        p.query = "x".into();
        p.enter_new();
        assert_eq!(p.mode, PickerMode::New);
        assert_eq!(p.prompt_prefix(), ':');
        assert!(p.query.is_empty());
        assert!(p.draft.is_empty());
    }

    #[test]
    fn edit_prefills_draft() {
        let mut p = PickerSession::open(PickerKind::Highlight);
        p.enter_edit(1, "foo".into());
        assert_eq!(p.mode, PickerMode::Edit { index: 1 });
        assert_eq!(p.draft, "foo");
    }

    #[test]
    fn confirm_delete_many_flow() {
        let mut p = PickerSession::open(PickerKind::Unified);
        p.request_delete_many(vec![UnifiedId {
            kind: UnifiedKind::Filter,
            source_index: 0,
        }]);
        assert!(p.confirm.is_some());
        p.cancel_confirm();
        assert!(p.confirm.is_none());
    }

    #[test]
    fn filtered_indices_ignore_case() {
        let labels = vec!["Error".into(), "info".into(), "WARN".into()];
        assert_eq!(PickerSession::filtered_indices(&labels, ""), vec![0, 1, 2]);
        assert_eq!(PickerSession::filtered_indices(&labels, "err"), vec![0]);
        assert_eq!(PickerSession::filtered_indices(&labels, "WARN"), vec![2]);
    }

    #[test]
    fn filter_kind_new_allocates_input_box() {
        let mut p = PickerSession::open(PickerKind::Filter);
        p.enter_new();
        assert!(p.input.is_some());
    }

    #[test]
    fn unified_kind_tags() {
        assert_eq!(UnifiedKind::Filter.tag(), "Filter");
        assert_eq!(UnifiedKind::Exclude.as_picker_kind(), PickerKind::Exclude);
    }
}
