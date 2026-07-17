use aloggrep::expr::Expr;
use crate::filter_model::Group;

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

    /// Enter: commit the in-progress token, compile all chips into a `Group`
    /// via `Expr::from_filters` (same-field chips OR'd via its
    /// `compile_joined` helper, cross-field AND'd — matches this project's
    /// documented filter semantics), and clear the buffer. Returns
    /// `Ok(None)` if there's nothing to compile (design doc: empty Enter is
    /// a no-op, not a "clear all" — `dd` on the chip strip is the only way
    /// to remove an already-confirmed group).
    pub fn build_group(&mut self, case_insensitive: bool) -> Result<Option<Group>, String> {
        self.commit_draft_as_chip();
        if self.chips.is_empty() {
            return Ok(None);
        }
        let mut tag = Vec::new();
        let mut msg = Vec::new();
        let mut pkg = Vec::new();
        let mut pid = Vec::new();
        let mut tid = Vec::new();
        let mut level: Option<&str> = None; // last Level chip wins if more than one
        for chip in &self.chips {
            match chip.field {
                ChipField::Tag => tag.push(chip.value.clone()),
                ChipField::Msg => msg.push(chip.value.clone()),
                ChipField::Pkg => pkg.push(chip.value.clone()),
                ChipField::Pid => pid.push(chip.value.clone()),
                ChipField::Tid => tid.push(chip.value.clone()),
                ChipField::Level => level = Some(chip.value.as_str()),
            }
        }
        let expr = Expr::from_filters(&tag, &msg, &pkg, &pid, &tid, level, case_insensitive)?;
        let label = self
            .chips
            .iter()
            .map(|c| format!("{}:{}", c.field.keyword(), c.value))
            .collect::<Vec<_>>()
            .join(" AND ");
        self.chips.clear();
        Ok(Some(Group {
            label,
            expr,
            time: None,
            enabled: true,
        }))
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

#[cfg(test)]
mod build_group_tests {
    use super::*;
    use crate::model::EntryRow;

    fn row(tag: &str, msg: &str, level_line: &str) -> EntryRow {
        EntryRow::from_line(&format!("04-02 10:00:00.000  1  1 {level_line} {tag}   : {msg}")).unwrap()
    }

    #[test]
    fn test_build_group_and_within_chips() {
        let mut input = InputBox::default();
        input.set_field(ChipField::Tag);
        input.push_char('A');
        input.open_popup(); // commits tag:A, opens popup
        input.popup.as_mut().unwrap().push_char('l');
        input.confirm_popup(); // picks Level
        input.push_char('W');

        let group = input.build_group(false).unwrap().unwrap();
        assert_eq!(group.label, "tag:A AND level:W");
        assert!(group.matches(&row("A", "m", "E")));
        assert!(!group.matches(&row("A", "m", "I")));
        assert!(!group.matches(&row("B", "m", "E")));
    }

    #[test]
    fn test_build_group_empty_is_none() {
        let mut input = InputBox::default();
        assert!(input.build_group(false).unwrap().is_none());
    }

    #[test]
    fn test_build_group_clears_chips() {
        let mut input = InputBox::default();
        input.push_char('x');
        input.build_group(false).unwrap();
        assert!(input.chips.is_empty());
    }

    #[test]
    fn test_build_group_same_field_chips_are_ored() {
        let mut input = InputBox::default();
        input.set_field(ChipField::Tag);
        input.push_char('A');
        input.open_popup();
        input.popup.as_mut().unwrap().push_char('t');
        input.confirm_popup(); // picks Tag again
        input.push_char('B');

        let group = input.build_group(false).unwrap().unwrap();
        let row = |tag: &str| EntryRow::from_line(&format!("04-02 10:00:00.000  1  1 I {tag}   : m")).unwrap();
        assert!(group.matches(&row("A")));
        assert!(group.matches(&row("B")));
        assert!(!group.matches(&row("C")));
    }

    #[test]
    fn test_build_group_handles_value_with_both_quote_characters() {
        let mut input = InputBox::default();
        input.push_char('c'); input.push_char('a'); input.push_char('n');
        input.push_char('\''); input.push_char('t'); input.push_char(' ');
        input.push_char('"'); input.push_char('h'); input.push_char('i'); input.push_char('"');
        // draft is now: can't "hi"  (defaults to msg field)
        let group = input.build_group(false).unwrap().unwrap();
        let row = EntryRow::from_line("04-02 10:00:00.000  1  1 I Tag   : can't \"hi\" there").unwrap();
        assert!(group.matches(&row));
    }
}
