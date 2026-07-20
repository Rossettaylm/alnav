use crate::input::InputBox;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerKind {
    ActionList,
    Filter,
    Search,
    Bookmark,
    Exclude,
    MsgChip { exclude: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerMode {
    Manage,
    New,
    Edit { index: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmKind {
    DeleteOne { index: usize },
    DeleteAll { count: usize },
}

pub struct PickerSession {
    pub kind: PickerKind,
    pub mode: PickerMode,
    /// Manage 过滤串（不含前缀 `/`）
    pub query: String,
    /// New/Edit draft（不含前缀 `:`）
    pub draft: String,
    pub selected: usize,
    pub confirm: Option<ConfirmKind>,
    /// Filter/Exclude New|Edit 时复用
    pub input: Option<InputBox>,
    /// Picker-local candidates (currently msg-chip tokens).
    pub choices: Vec<String>,
}

impl PickerSession {
    pub fn open(kind: PickerKind) -> Self {
        Self {
            kind,
            mode: PickerMode::Manage,
            query: String::new(),
            draft: String::new(),
            selected: 0,
            confirm: None,
            input: None,
            choices: Vec::new(),
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
        self.input = Self::fresh_input_for_kind(self.kind);
    }

    pub fn enter_edit(&mut self, index: usize, prefill: String) {
        self.mode = PickerMode::Edit { index };
        self.draft = prefill.clone();
        self.selected = 0;
        self.confirm = None;
        self.input = None;
    }

    pub fn enter_edit_input(&mut self, index: usize, input: InputBox) {
        self.mode = PickerMode::Edit { index };
        self.draft.clear();
        self.selected = 0;
        self.confirm = None;
        self.input = Some(input);
    }

    pub fn back_to_manage(&mut self) {
        let selected = match self.mode {
            PickerMode::Edit { index } => index,
            PickerMode::Manage | PickerMode::New => 0,
        };
        self.mode = PickerMode::Manage;
        self.query.clear();
        self.draft.clear();
        self.selected = selected;
        self.confirm = None;
        self.input = None;
    }

    pub fn request_delete_one(&mut self, index: usize) {
        self.confirm = Some(ConfirmKind::DeleteOne { index });
    }

    pub fn request_delete_all(&mut self, count: usize) {
        self.confirm = Some(ConfirmKind::DeleteAll { count });
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

/// ActionList 静态标签，对应 [`PickerKind`].
pub const ACTION_LIST_LABELS: [&str; 4] = ["Filter", "Search", "Bookmark", "Exclude"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_defaults_to_manage_with_slash_prefix() {
        let p = PickerSession::open(PickerKind::Search);
        assert_eq!(p.mode, PickerMode::Manage);
        assert_eq!(p.prompt_prefix(), '/');
    }

    #[test]
    fn colon_and_ctrl_a_enter_new() {
        let mut p = PickerSession::open(PickerKind::Search);
        p.query = "x".into();
        p.enter_new();
        assert_eq!(p.mode, PickerMode::New);
        assert_eq!(p.prompt_prefix(), ':');
        assert!(p.query.is_empty());
        assert!(p.draft.is_empty());
    }

    #[test]
    fn edit_prefills_draft() {
        let mut p = PickerSession::open(PickerKind::Search);
        p.enter_edit(1, "foo".into());
        assert_eq!(p.mode, PickerMode::Edit { index: 1 });
        assert_eq!(p.draft, "foo");
    }

    #[test]
    fn confirm_delete_flow() {
        let mut p = PickerSession::open(PickerKind::Search);
        p.request_delete_one(0);
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
    fn back_to_manage_clears_draft_and_input() {
        let mut p = PickerSession::open(PickerKind::Filter);
        p.enter_edit(0, "chip".into());
        p.back_to_manage();
        assert_eq!(p.mode, PickerMode::Manage);
        assert!(p.draft.is_empty());
        assert!(p.input.is_none());
    }
}
