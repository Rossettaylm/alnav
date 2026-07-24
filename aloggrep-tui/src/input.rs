use crate::filter_model::Group;
use crate::text_field::TextField;
use aloggrep::expr::{Expr, SameFieldOp};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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
    ChipField::Tag,
    ChipField::Msg,
    ChipField::Pkg,
    ChipField::Pid,
    ChipField::Tid,
    ChipField::Level,
];

#[derive(Debug, Clone)]
pub struct Chip {
    pub field: ChipField,
    pub value: String,
}

#[derive(Default)]
pub struct InputBox {
    pub chips: Vec<Chip>,
    pub draft: TextField,
    pub draft_field: Option<ChipField>,
    /// Highlighted index into [`Self::field_candidates`] (not into `CHIP_FIELDS`).
    pub field_selected: usize,
    /// H9: when true, Enter submits chips as global excludes (not a Filter group).
    /// Only toggled via `!` while chips+draft are empty.
    pub exclude_mode: bool,
}

impl InputBox {
    /// Commit the in-progress token as a chip (default field = msg).
    pub fn commit_draft_as_chip(&mut self) {
        if self.draft.is_empty() && self.draft_field.is_none() {
            return;
        }
        let field = self.draft_field.take().unwrap_or(ChipField::Msg);
        let value = self.draft.take();
        self.field_selected = 0;
        if !value.is_empty() {
            self.chips.push(Chip { field, value });
        }
    }

    pub fn push_char(&mut self, c: char) {
        self.draft.insert(c);
        self.field_selected = 0;
    }

    /// Backspace cascade: draft text -> un-pick the draft's field -> pop the
    /// last committed chip. Matches design doc's "iterative editing" note.
    /// Mid-string: only deletes left of caret; non-empty draft at cursor 0 is a no-op
    /// (does not cascade).
    pub fn backspace(&mut self) {
        if !self.draft.is_empty() {
            if self.draft.backspace() {
                self.field_selected = 0;
            }
        } else if self.draft_field.is_some() {
            self.draft_field = None;
        } else {
            self.chips.pop();
        }
    }

    pub fn set_field(&mut self, field: ChipField) {
        self.draft_field = Some(field);
        self.field_selected = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.chips.is_empty() && self.draft.is_empty() && self.draft_field.is_none()
    }

    /// Toggle exclude mode; only allowed when chips and draft are fully empty.
    pub fn toggle_exclude_mode(&mut self) -> bool {
        if !self.is_empty() {
            return false;
        }
        self.exclude_mode = !self.exclude_mode;
        true
    }

    /// True when Enter should commit a pill rather than submit the group:
    /// there is draft text and/or a selected field awaiting a value.
    pub fn has_pending_draft(&self) -> bool {
        !self.draft.is_empty() || self.draft_field.is_some()
    }

    /// Field-candidate panel is shown whenever draft is non-empty and no field
    /// has been picked yet (value typing after a field pick hides it).
    pub fn field_popup_visible(&self) -> bool {
        !self.draft.is_empty() && self.draft_field.is_none()
    }

    /// Fields whose keyword has `draft` as an ignore-case prefix.
    /// Empty when the panel is not visible.
    pub fn field_candidates(&self) -> Vec<ChipField> {
        if self.draft_field.is_some() {
            return Vec::new();
        }
        let q = self.draft.to_ascii_lowercase();
        CHIP_FIELDS
            .into_iter()
            .filter(|f| f.keyword().starts_with(&q))
            .collect()
    }

    pub fn move_field_selection(&mut self, delta: isize) {
        let len = self.field_candidates().len();
        if len == 0 {
            return;
        }
        let new = self.field_selected as isize + delta;
        self.field_selected = new.clamp(0, len as isize - 1) as usize;
    }

    /// Enter/Tab with field candidates: pick selected field, clear draft.
    /// Returns `true` if a field was confirmed; `false` when no candidates
    /// (caller falls through to pill commit / focus cycle).
    pub fn confirm_field_candidate(&mut self) -> bool {
        let candidates = self.field_candidates();
        if candidates.is_empty() {
            return false;
        }
        let sel = self.field_selected.min(candidates.len() - 1);
        let field = candidates[sel];
        self.draft.clear();
        self.field_selected = 0;
        self.draft_field = Some(field);
        true
    }

    /// Enter-only variant of [`Self::confirm_field_candidate`]: requires a
    /// non-empty draft (legacy `field_popup_visible` contract). Enter is
    /// overloaded with pill-commit/group-submit, so it must not hijack a
    /// bare Enter on an empty draft as "pick the first field" — that
    /// selection is reserved for Tab (or typing a prefix, then Enter).
    pub fn confirm_field_candidate_on_enter(&mut self) -> bool {
        if self.draft.is_empty() {
            return false;
        }
        self.confirm_field_candidate()
    }

    /// Compile already-committed chips into a `Group` via `Expr::from_filters`
    /// with [`SameFieldOp::And`]. Does **not** commit the in-progress draft —
    /// caller uses Enter two-step: pending draft → `commit_draft_as_chip`,
    /// empty draft + chips → `build_group`. Returns `Ok(None)` if chips empty.
    pub fn build_group(&mut self, case_insensitive: bool) -> Result<Option<Group>, String> {
        if self.chips.is_empty() {
            return Ok(None);
        }
        let chips = std::mem::take(&mut self.chips);
        build_group_from_chips(chips, case_insensitive)
    }
}

/// Compile a chip list into a `Group` (same rules as [`InputBox::build_group`]).
pub fn build_group_from_chips(
    chips: Vec<Chip>,
    case_insensitive: bool,
) -> Result<Option<Group>, String> {
    if chips.is_empty() {
        return Ok(None);
    }
    let mut tag = Vec::new();
    let mut msg = Vec::new();
    let mut pkg = Vec::new();
    let mut pid = Vec::new();
    let mut tid = Vec::new();
    let mut level: Option<&str> = None; // last Level chip wins if more than one
    for chip in &chips {
        match chip.field {
            ChipField::Tag => tag.push(chip.value.clone()),
            ChipField::Msg => msg.push(chip.value.clone()),
            ChipField::Pkg => pkg.push(chip.value.clone()),
            ChipField::Pid => pid.push(chip.value.clone()),
            ChipField::Tid => tid.push(chip.value.clone()),
            ChipField::Level => level = Some(chip.value.as_str()),
        }
    }
    let expr = Expr::from_filters(
        &tag,
        &msg,
        &pkg,
        &pid,
        &tid,
        level,
        case_insensitive,
        SameFieldOp::And,
    )?;
    let label = chips
        .iter()
        .map(|c| format!("{}:{}", c.field.keyword(), c.value))
        .collect::<Vec<_>>()
        .join(" AND ");
    Ok(Some(Group {
        label,
        chips,
        expr,
        enabled: true,
    }))
}

/// H7 `c`+`m`: split msg into alphanumeric tokens (len ≥ 2, ignore-case dedupe, ≤8).
pub fn tokenize_msg_tokens(msg: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut start: Option<usize> = None;
    for (i, ch) in msg.char_indices() {
        if ch.is_ascii_alphanumeric() {
            if start.is_none() {
                start = Some(i);
            }
        } else if let Some(s) = start.take() {
            push_msg_token(&msg[s..i], &mut out, &mut seen);
        }
    }
    if let Some(s) = start {
        push_msg_token(&msg[s..], &mut out, &mut seen);
    }
    out
}

fn push_msg_token(
    token: &str,
    out: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
) {
    if token.len() < 2 || out.len() >= 8 {
        return;
    }
    let key = token.to_ascii_lowercase();
    if seen.insert(key) {
        out.push(token.to_string());
    }
}

/// Like `tokenize_msg_tokens` but with a higher per-message cap for vocab building.
pub fn tokenize_msg_for_vocab(msg: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut start: Option<usize> = None;
    for (i, ch) in msg.char_indices() {
        if ch.is_ascii_alphanumeric() {
            if start.is_none() {
                start = Some(i);
            }
        } else if let Some(s) = start.take() {
            let token = &msg[s..i];
            if token.len() >= 2 {
                let key = token.to_ascii_lowercase();
                if seen.insert(key) {
                    out.push(token.to_string());
                }
            }
            if out.len() >= 50 {
                return out;
            }
        }
    }
    if let Some(s) = start {
        let token = &msg[s..];
        if token.len() >= 2 {
            let key = token.to_ascii_lowercase();
            if seen.insert(key) {
                out.push(token.to_string());
            }
        }
    }
    out
}

/// H7/H9 candidates consumed by the unified fzf picker.
pub fn msg_token_candidates(msg: &str) -> Vec<String> {
    tokenize_msg_tokens(msg)
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
    fn test_backspace_mid_cursor_does_not_cascade() {
        let mut input = InputBox::default();
        input.push_char('a');
        input.push_char('b');
        input.draft.home();
        input.backspace(); // cursor at start, non-empty → no-op
        assert_eq!(input.draft.as_str(), "ab");
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
mod field_popup_tests {
    use super::*;

    #[test]
    fn test_field_popup_hidden_when_draft_empty() {
        let input = InputBox::default();
        assert!(!input.field_popup_visible());
        assert_eq!(input.field_candidates().len(), CHIP_FIELDS.len());
    }

    #[test]
    fn test_field_popup_visible_and_filters_by_draft_prefix() {
        let mut input = InputBox::default();
        input.push_char('t');
        assert!(input.field_popup_visible());
        let matches = input.field_candidates();
        assert!(matches.contains(&ChipField::Tag));
        assert!(matches.contains(&ChipField::Tid));
        assert!(!matches.contains(&ChipField::Msg));
    }

    #[test]
    fn test_field_popup_hidden_after_field_picked() {
        let mut input = InputBox::default();
        input.push_char('t');
        assert!(input.confirm_field_candidate());
        assert_eq!(input.draft_field, Some(ChipField::Tag));
        assert!(input.draft.is_empty());
        assert!(!input.field_popup_visible());
        input.push_char('x');
        assert!(
            !input.field_popup_visible(),
            "value typing must not show field popup"
        );
    }

    #[test]
    fn test_confirm_field_candidate_clears_draft() {
        let mut input = InputBox::default();
        for c in "tag".chars() {
            input.push_char(c);
        }
        assert!(input.confirm_field_candidate());
        assert_eq!(input.draft_field, Some(ChipField::Tag));
        assert!(input.draft.is_empty());
        assert_eq!(input.field_selected, 0);
    }

    #[test]
    fn test_confirm_field_candidate_noop_when_no_matches() {
        let mut input = InputBox::default();
        input.push_char('z');
        assert!(input.field_popup_visible());
        assert!(input.field_candidates().is_empty());
        assert!(!input.confirm_field_candidate());
        assert_eq!(input.draft, "z");
        assert!(input.draft_field.is_none());
    }

    #[test]
    fn test_commit_pill_hides_popup() {
        let mut input = InputBox::default();
        for c in "error".chars() {
            input.push_char(c);
        }
        assert!(input.field_popup_visible());
        input.commit_draft_as_chip();
        assert!(!input.field_popup_visible());
        assert!(input.draft.is_empty());
    }

    #[test]
    fn test_move_field_selection_clamps() {
        let mut input = InputBox::default();
        input.push_char('t'); // tag, tid
        input.move_field_selection(-5);
        assert_eq!(input.field_selected, 0);
        input.move_field_selection(10);
        assert_eq!(input.field_selected, 1);
    }

    #[test]
    fn test_move_field_selection_noop_when_no_matches() {
        let mut input = InputBox::default();
        input.push_char('z');
        input.move_field_selection(1);
        assert_eq!(input.field_selected, 0);
    }
}

#[cfg(test)]
mod build_group_tests {
    use super::*;
    use crate::model::EntryRow;

    fn row(tag: &str, msg: &str, level_line: &str) -> EntryRow {
        EntryRow::from_line(&format!(
            "04-02 10:00:00.000  1  1 {level_line} {tag}   : {msg}"
        ))
        .unwrap()
    }

    #[test]
    fn test_build_group_and_within_chips() {
        let mut input = InputBox::default();
        input.set_field(ChipField::Tag);
        input.push_char('A');
        input.commit_draft_as_chip();
        input.push_char('l');
        assert!(input.confirm_field_candidate()); // Level
        input.push_char('W');
        input.commit_draft_as_chip(); // level:W
        let group = input.build_group(false).unwrap().unwrap();
        assert_eq!(group.label, "tag:A AND level:W");
        assert_eq!(group.chips.len(), 2);
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
        input.commit_draft_as_chip();
        input.build_group(false).unwrap();
        assert!(input.chips.is_empty());
    }

    #[test]
    fn test_build_group_same_field_chips_are_anded() {
        let mut input = InputBox::default();
        input.set_field(ChipField::Msg);
        for c in "trace=".chars() {
            input.push_char(c);
        }
        input.commit_draft_as_chip();
        input.push_char('m');
        assert!(input.confirm_field_candidate()); // Msg
        for c in "0x1100".chars() {
            input.push_char(c);
        }
        input.commit_draft_as_chip();

        let group = input.build_group(true).unwrap().unwrap();
        assert_eq!(group.label, "msg:trace= AND msg:0x1100");
        let row = |msg: &str| {
            EntryRow::from_line(&format!("04-02 10:00:00.000  1  1 I Tag   : {msg}")).unwrap()
        };
        assert!(group.matches(&row("foo trace=0x1100 bar")));
        assert!(!group.matches(&row("foo trace=999 bar")));
        assert!(!group.matches(&row("foo code=0x1100 bar")));
    }

    #[test]
    fn test_build_group_handles_value_with_both_quote_characters() {
        let mut input = InputBox::default();
        input.push_char('c');
        input.push_char('a');
        input.push_char('n');
        input.push_char('\'');
        input.push_char('t');
        input.push_char(' ');
        input.push_char('"');
        input.push_char('h');
        input.push_char('i');
        input.push_char('"');
        // draft is now: can't "hi"  (defaults to msg field)
        input.commit_draft_as_chip();
        let group = input.build_group(false).unwrap().unwrap();
        let row =
            EntryRow::from_line("04-02 10:00:00.000  1  1 I Tag   : can't \"hi\" there").unwrap();
        assert!(group.matches(&row));
    }

    #[test]
    fn test_build_group_literal_metacharacters() {
        let row = |msg: &str| {
            EntryRow::from_line(&format!("04-02 10:00:00.000  1  1 I Tag   : {msg}")).unwrap()
        };
        let mut input = InputBox::default();
        for c in "(0)".chars() {
            input.push_char(c);
        }
        input.commit_draft_as_chip();
        let group = input.build_group(true).unwrap().unwrap();
        assert!(group.matches(&row("code=(0) ok")));
        assert!(!group.matches(&row("code=0 ok")));

        let mut input = InputBox::default();
        for c in "foo <bar>".chars() {
            input.push_char(c);
        }
        input.commit_draft_as_chip();
        let group = input.build_group(false).unwrap().unwrap();
        assert!(group.matches(&row("see foo <bar> here")));
    }

    #[test]
    fn test_has_pending_draft() {
        let mut input = InputBox::default();
        assert!(!input.has_pending_draft());
        input.set_field(ChipField::Tag);
        assert!(input.has_pending_draft());
        input.push_char('a');
        assert!(input.has_pending_draft());
        input.commit_draft_as_chip();
        assert!(!input.has_pending_draft());
        assert!(!input.chips.is_empty());
    }
}

#[cfg(test)]
mod msg_tokenize_tests {
    use super::*;

    #[test]
    fn tokenize_splits_on_non_alnum_drops_short_dedupes_caps() {
        // `_` and `!` are separators; "AB" dedupes against earlier "ab"; "z" dropped (len<2).
        let tokens = tokenize_msg_tokens("ab cd!ef_gh AB xy z more1 more2 more3 more4");
        assert_eq!(
            tokens,
            vec!["ab", "cd", "ef", "gh", "xy", "more1", "more2", "more3"]
        );
        assert!(!tokens.iter().any(|t| t == "z"));
        assert!(!tokens.iter().any(|t| t == "more4")); // capped at 8
    }

    #[test]
    fn tokenize_empty_or_punctuation_only() {
        assert!(tokenize_msg_tokens("").is_empty());
        assert!(tokenize_msg_tokens("!!! ---").is_empty());
        assert!(tokenize_msg_tokens("a b").is_empty()); // both len < 2
    }

    #[test]
    fn msg_candidates_reuse_tokenizer_output() {
        assert_eq!(
            msg_token_candidates("hello world timeout"),
            vec!["hello", "world", "timeout"]
        );
    }

    #[test]
    fn msg_candidates_empty_without_tokens() {
        assert!(msg_token_candidates("a!").is_empty());
    }

    #[test]
    fn tokenize_for_vocab_no_8_cap() {
        let msg = "aa bb cc dd ee ff gg hh ii jj kk";
        let result = tokenize_msg_for_vocab(msg);
        assert!(result.len() > 8, "expected >8 tokens, got {}", result.len());
        assert!(result.contains(&"aa".to_string()));
        assert!(result.contains(&"kk".to_string()));
    }
}
