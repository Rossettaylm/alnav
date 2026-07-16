#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipField {
    Tag,
    Msg,
    Pkg,
    Pid,
    Tid,
    Level,
}

impl ChipField {
    pub fn keyword(self) -> &'static str {
        match self {
            ChipField::Tag => "tag",
            ChipField::Msg => "msg",
            ChipField::Pkg => "pkg",
            ChipField::Pid => "pid",
            ChipField::Tid => "tid",
            ChipField::Level => "level",
        }
    }
}

pub const CHIP_FIELDS: [ChipField; 6] = [
    ChipField::Tag, ChipField::Msg, ChipField::Pkg, ChipField::Pid, ChipField::Tid, ChipField::Level,
];

#[derive(Debug, Clone)]
pub struct Chip {
    pub field: ChipField,
    pub value: String,
}

#[derive(Default)]
pub struct InputBox {
    pub chips: Vec<Chip>,
    pub draft: String,
    pub draft_field: Option<ChipField>,
    pub popup: Option<Popup>,
}

impl InputBox {
    /// `/` always means "finish whatever token is in progress (if any), then
    /// start choosing the next field" (see design doc "Insert 模式").
    /// Committing the in-progress token is the caller's job (Task 11 opens
    /// the popup right after calling this); this method only does the commit.
    pub fn commit_draft_as_chip(&mut self) {
        if self.draft.is_empty() && self.draft_field.is_none() {
            return;
        }
        let field = self.draft_field.take().unwrap_or(ChipField::Msg);
        let value = std::mem::take(&mut self.draft);
        if !value.is_empty() {
            self.chips.push(Chip { field, value });
        }
    }

    pub fn push_char(&mut self, c: char) {
        self.draft.push(c);
    }

    /// Backspace cascade: draft text -> un-pick the draft's field -> pop the
    /// last committed chip. Matches design doc's "iterative editing" note.
    pub fn backspace(&mut self) {
        if !self.draft.is_empty() {
            self.draft.pop();
        } else if self.draft_field.is_some() {
            self.draft_field = None;
        } else {
            self.chips.pop();
        }
    }

    pub fn set_field(&mut self, field: ChipField) {
        self.draft_field = Some(field);
    }

    pub fn is_empty(&self) -> bool {
        self.chips.is_empty() && self.draft.is_empty() && self.draft_field.is_none()
    }

    /// `/`: finish the in-progress token, then open the field popup.
    pub fn open_popup(&mut self) {
        self.commit_draft_as_chip();
        self.popup = Some(Popup::default());
    }

    /// Enter/Tab inside the popup: pick the highlighted field and close it.
    pub fn confirm_popup(&mut self) {
        if let Some(popup) = &self.popup {
            if let Some(field) = popup.selected_field() {
                self.set_field(field);
            }
        }
        self.popup = None;
    }

    pub fn cancel_popup(&mut self) {
        self.popup = None;
    }
}

#[derive(Default)]
pub struct Popup {
    pub query: String,
    pub selected: usize,
}

impl Popup {
    pub fn matches(&self) -> Vec<ChipField> {
        CHIP_FIELDS
            .into_iter()
            .filter(|f| f.keyword().starts_with(self.query.to_ascii_lowercase().as_str()))
            .collect()
    }

    pub fn push_char(&mut self, c: char) {
        self.query.push(c);
        self.selected = 0;
    }

    pub fn backspace(&mut self) {
        self.query.pop();
        self.selected = 0;
    }

    pub fn move_selection(&mut self, delta: isize) {
        let len = self.matches().len();
        if len == 0 {
            return;
        }
        let new = self.selected as isize + delta;
        self.selected = new.clamp(0, len as isize - 1) as usize;
    }

    pub fn selected_field(&self) -> Option<ChipField> {
        self.matches().get(self.selected).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_char_appends_to_draft() {
        let mut input = InputBox::default();
        input.push_char('a');
        input.push_char('b');
        assert_eq!(input.draft, "ab");
    }

    #[test]
    fn test_commit_draft_with_field_pushes_chip() {
        let mut input = InputBox::default();
        input.set_field(ChipField::Tag);
        input.push_char('M');
        input.push_char('y');
        input.commit_draft_as_chip();
        assert_eq!(input.chips.len(), 1);
        assert_eq!(input.chips[0].field, ChipField::Tag);
        assert_eq!(input.chips[0].value, "My");
        assert!(input.draft.is_empty());
        assert!(input.draft_field.is_none());
    }

    #[test]
    fn test_commit_draft_without_field_defaults_to_msg() {
        let mut input = InputBox::default();
        input.push_char('x');
        input.commit_draft_as_chip();
        assert_eq!(input.chips[0].field, ChipField::Msg);
    }

    #[test]
    fn test_commit_empty_draft_is_noop() {
        let mut input = InputBox::default();
        input.commit_draft_as_chip();
        assert!(input.chips.is_empty());
    }

    #[test]
    fn test_backspace_cascade() {
        let mut input = InputBox::default();
        input.set_field(ChipField::Tag);
        input.push_char('a');
        input.backspace(); // pops draft char
        assert_eq!(input.draft, "");
        input.backspace(); // un-picks field
        assert!(input.draft_field.is_none());
        input.push_char('x');
        input.commit_draft_as_chip();
        input.backspace(); // pops the committed chip
        assert!(input.chips.is_empty());
    }
}

#[cfg(test)]
mod popup_tests {
    use super::*;

    #[test]
    fn test_popup_filters_by_prefix() {
        let mut popup = Popup::default();
        popup.push_char('t');
        let matches = popup.matches();
        assert!(matches.contains(&ChipField::Tag));
        assert!(matches.contains(&ChipField::Tid));
        assert!(!matches.contains(&ChipField::Msg));
    }

    #[test]
    fn test_popup_move_selection_clamps() {
        let mut popup = Popup::default();
        popup.move_selection(-5);
        assert_eq!(popup.selected, 0);
    }

    #[test]
    fn test_open_confirm_popup_sets_draft_field() {
        let mut input = InputBox::default();
        input.push_char('x'); // in-progress msg draft
        input.open_popup();
        assert!(input.chips[0].value == "x"); // draft was committed as msg:x
        assert!(input.popup.is_some());

        input.popup.as_mut().unwrap().push_char('t');
        input.popup.as_mut().unwrap().push_char('a');
        input.popup.as_mut().unwrap().push_char('g');
        input.confirm_popup();

        assert_eq!(input.draft_field, Some(ChipField::Tag));
        assert!(input.popup.is_none());
    }

    #[test]
    fn test_cancel_popup_leaves_draft_field_unset() {
        let mut input = InputBox::default();
        input.open_popup();
        input.cancel_popup();
        assert!(input.popup.is_none());
        assert!(input.draft_field.is_none());
    }

    #[test]
    fn test_popup_move_selection_noop_when_no_matches() {
        let mut popup = Popup::default();
        popup.push_char('z'); // no field keyword starts with "z"
        assert!(popup.matches().is_empty());
        popup.move_selection(1); // must not panic
        assert_eq!(popup.selected, 0);
    }
}
