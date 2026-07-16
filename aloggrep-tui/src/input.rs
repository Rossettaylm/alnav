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
