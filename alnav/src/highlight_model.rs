use crate::fuzzy;
use crate::model::EntryRow;

/// One committed highlight group: a single pattern highlighted in the log list.
/// Multiple highlights = multiple groups (all enabled patterns paint; `n`/`N`
/// and underline follow `App.active_highlight` only).
#[derive(Clone, Debug)]
pub struct HighlightGroup {
    /// Original pattern string (display + dedup + fuzzy match).
    pub pattern: String,
    pub enabled: bool,
}

impl HighlightGroup {
    /// Store a non-empty pattern for ignore-case nucleo fuzzy matching.
    /// Returns `None` if empty.
    pub fn from_pattern(pattern: &str) -> Option<Self> {
        if pattern.is_empty() {
            return None;
        }
        Some(Self {
            pattern: pattern.to_string(),
            enabled: true,
        })
    }

    pub fn matches_msg(&self, msg: &str) -> bool {
        fuzzy::fuzzy_match(msg, &self.pattern)
    }

    /// Match against tag+msg (or raw) haystack. Used by jump/n/N/stats and highlight.
    pub fn matches_row(&self, tag: &str, msg: &str) -> bool {
        // Prefer joined haystack; when both empty, callers with raw should use
        // [`Self::matches_entry`]. Keep tag/msg-only path for tests / stream
        // without raw: empty+empty never matches a non-empty pattern.
        if tag.is_empty() && msg.is_empty() {
            return false;
        }
        fuzzy::fuzzy_match(&fuzzy::search_haystack(tag, msg, ""), &self.pattern)
    }

    pub fn matches_entry(&self, row: &EntryRow) -> bool {
        fuzzy::matches_search_row(row, &self.pattern)
    }

    /// Case-insensitive equality on the source pattern string.
    pub fn same_pattern_as(&self, other: &str) -> bool {
        self.pattern.eq_ignore_ascii_case(other)
    }
}

#[derive(Default, Clone)]
pub struct HighlightGroupList {
    pub groups: Vec<HighlightGroup>,
}

impl HighlightGroupList {
    /// Flatten enabled groups' patterns with progressive color indices.
    pub fn active_patterns(&self) -> Vec<(&str, usize)> {
        self.paint_patterns(None)
            .into_iter()
            .map(|(p, idx, _)| (p, idx))
            .collect()
    }

    /// Like [`Self::active_patterns`], plus whether each pattern is the
    /// globally active highlight group (for underline / `n`/`N` paint).
    pub fn paint_patterns(&self, active_group: Option<usize>) -> Vec<(&str, usize, bool)> {
        let mut out = Vec::new();
        let mut idx = 0usize;
        for (i, g) in self.groups.iter().enumerate() {
            if !g.enabled {
                continue;
            }
            let is_active = Some(i) == active_group;
            out.push((g.pattern.as_str(), idx, is_active));
            idx += 1;
        }
        out
    }

    pub fn any_match(&self, tag: &str, msg: &str) -> bool {
        self.groups
            .iter()
            .filter(|g| g.enabled)
            .any(|g| g.matches_row(tag, msg))
    }

    pub fn any_match_entry(&self, row: &EntryRow) -> bool {
        self.groups
            .iter()
            .filter(|g| g.enabled)
            .any(|g| g.matches_entry(row))
    }

    /// Index of an existing group with the same pattern (ignore-case), if any.
    pub fn find_equivalent(&self, pattern: &str) -> Option<usize> {
        self.groups.iter().position(|g| g.same_pattern_as(pattern))
    }
}

/// Centered highlight modal draft: free-text + history-chip prefix completion.
#[derive(Default)]
pub struct HighlightBox {
    pub draft: crate::text_field::TextField,
    /// When true, key events are routed here (entered via highlight compose).
    pub editing: bool,
    /// Highlighted index into the current candidate list (not into `groups`).
    pub selected: usize,
}

impl HighlightBox {
    pub fn is_empty(&self) -> bool {
        self.draft.is_empty()
    }

    pub fn push_char(&mut self, c: char) {
        self.draft.insert(c);
        self.selected = 0;
    }

    pub fn backspace(&mut self) {
        let _ = self.draft.backspace();
        self.selected = 0;
    }

    /// Start editing in the retained HighlightBox model.
    pub fn begin_editing(&mut self) {
        self.editing = true;
        self.selected = 0;
    }

    /// Indices into `groups` whose pattern fuzzy-matches `draft`.
    /// Capped at 6 to match the floating candidate popup height; score-ordered.
    pub fn candidate_indices(&self, groups: &[HighlightGroup]) -> Vec<usize> {
        let labels: Vec<String> = groups.iter().map(|g| g.pattern.clone()).collect();
        fuzzy::fuzzy_label_indices(&labels, self.draft.as_str())
            .into_iter()
            .take(6)
            .collect()
    }

    pub fn move_selection(&mut self, groups: &[HighlightGroup], delta: isize) {
        let len = self.candidate_indices(groups).len();
        if len == 0 {
            return;
        }
        let new = self.selected as isize + delta;
        self.selected = new.clamp(0, len as isize - 1) as usize;
    }

    /// Enter/Tab: empty draft → no-op; non-empty with candidates → selected
    /// pattern; non-empty without candidates → compile draft.
    /// `Ok(None)` = empty (stay editing). `Err` = invalid (empty after trim).
    pub fn confirm_or_submit(
        &mut self,
        groups: &[HighlightGroup],
    ) -> Result<Option<HighlightGroup>, ()> {
        if self.draft.is_empty() {
            return Ok(None);
        }
        let candidates = self.candidate_indices(groups);
        if !candidates.is_empty() {
            let sel = self.selected.min(candidates.len() - 1);
            let pattern = groups[candidates[sel]].pattern.clone();
            self.draft.clear();
            match HighlightGroup::from_pattern(&pattern) {
                Some(g) => Ok(Some(g)),
                None => Err(()),
            }
        } else {
            self.submit_draft()
        }
    }

    /// Enter: compile draft into a single-pattern group and clear draft.
    /// `Ok(None)` = empty (no-op). `Err` = empty pattern.
    pub fn submit_draft(&mut self) -> Result<Option<HighlightGroup>, ()> {
        if self.draft.is_empty() {
            return Ok(None);
        }
        let draft = self.draft.take();
        match HighlightGroup::from_pattern(&draft) {
            Some(g) => Ok(Some(g)),
            None => {
                self.draft = crate::text_field::TextField::from_text(draft);
                Err(())
            }
        }
    }

    pub fn clear(&mut self) {
        self.draft.clear();
        self.editing = false;
        self.selected = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_highlight_group_ignore_case() {
        let g = HighlightGroup::from_pattern("ERROR").unwrap();
        assert!(g.matches_msg("an error occurred"));
        assert!(g.matches_msg("AN ERROR"));
    }

    #[test]
    fn test_highlight_group_matches_tag_or_msg() {
        let g = HighlightGroup::from_pattern("MyTag").unwrap();
        assert!(g.matches_row("MyTag", "hello"));
        assert!(g.matches_row("Other", "see MyTag here"));
        assert!(!g.matches_row("Other", "hello"));
    }

    #[test]
    fn test_highlight_group_fuzzy_and_metacharacters() {
        let g = HighlightGroup::from_pattern("abc").unwrap();
        assert!(g.matches_msg("aXbYc"));

        let g = HighlightGroup::from_pattern("(0)").unwrap();
        assert!(g.matches_msg("code=(0) ok"));
        assert!(!g.matches_msg("code=0 ok"));

        let g = HighlightGroup::from_pattern("(unclosed").unwrap();
        assert!(g.matches_msg("see (unclosed here"));
        assert_eq!(g.pattern, "(unclosed");

        let g = HighlightGroup::from_pattern("foo <bar>").unwrap();
        assert!(g.matches_row("T", "foo <bar> baz"));

        let g = HighlightGroup::from_pattern("a|b").unwrap();
        assert!(g.matches_msg("x a|b y"));
        assert!(!g.matches_msg("x a y"));
    }

    #[test]
    fn test_active_patterns_skips_disabled_and_assigns_color_idx() {
        let mut list = HighlightGroupList::default();
        list.groups.push(HighlightGroup::from_pattern("a").unwrap());
        list.groups.push(HighlightGroup::from_pattern("b").unwrap());
        list.groups.push(HighlightGroup::from_pattern("c").unwrap());
        list.groups[0].enabled = false;
        list.groups[1].enabled = false;
        let active = list.active_patterns();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].1, 0);
        assert_eq!(active[0].0, "c");
    }

    #[test]
    fn test_highlight_box_enter_submits_single_pattern_with_spaces() {
        let mut box_ = HighlightBox::default();
        for c in "foo bar".chars() {
            box_.push_char(c);
        }
        let g = box_.submit_draft().unwrap().unwrap();
        assert_eq!(g.pattern, "foo bar");
        assert!(box_.is_empty());
    }

    #[test]
    fn test_highlight_box_empty_enter_is_none() {
        let mut box_ = HighlightBox::default();
        assert!(box_.submit_draft().unwrap().is_none());
    }

    #[test]
    fn test_find_equivalent_ignore_case() {
        let mut list = HighlightGroupList::default();
        list.groups
            .push(HighlightGroup::from_pattern("Error").unwrap());
        assert_eq!(list.find_equivalent("error"), Some(0));
        assert_eq!(list.find_equivalent("other"), None);
    }

    #[test]
    fn test_candidate_indices_fuzzy_ignore_case() {
        let groups = vec![
            HighlightGroup::from_pattern("Error").unwrap(),
            HighlightGroup::from_pattern("errno").unwrap(),
            HighlightGroup::from_pattern("warn").unwrap(),
        ];
        let mut box_ = HighlightBox::default();
        box_.draft = "er".into();
        let hits = box_.candidate_indices(&groups);
        assert!(hits.contains(&0) && hits.contains(&1));
        assert!(!hits.contains(&2));
        box_.draft = "ER".into();
        let hits = box_.candidate_indices(&groups);
        assert!(hits.contains(&0) && hits.contains(&1));
        box_.draft = "xyz".into();
        assert!(box_.candidate_indices(&groups).is_empty());
        // Multi-word AND atoms
        let camel = vec![HighlightGroup::from_pattern("GuildFeedListViewModel").unwrap()];
        box_.draft = "guild viewmodel".into();
        assert_eq!(box_.candidate_indices(&camel), vec![0]);
    }

    #[test]
    fn test_move_selection_clamps() {
        let groups = vec![
            HighlightGroup::from_pattern("a").unwrap(),
            HighlightGroup::from_pattern("ab").unwrap(),
        ];
        let mut box_ = HighlightBox::default();
        box_.draft = "a".into();
        box_.move_selection(&groups, 1);
        assert_eq!(box_.selected, 1);
        box_.move_selection(&groups, 10);
        assert_eq!(box_.selected, 1);
        box_.move_selection(&groups, -10);
        assert_eq!(box_.selected, 0);
    }

    #[test]
    fn test_confirm_or_submit_picks_candidate_when_present() {
        let groups = vec![
            HighlightGroup::from_pattern("error").unwrap(),
            HighlightGroup::from_pattern("errno").unwrap(),
        ];
        let mut box_ = HighlightBox::default();
        box_.draft = "er".into();
        box_.selected = 1;
        let g = box_.confirm_or_submit(&groups).unwrap().unwrap();
        assert_eq!(g.pattern, "errno");
        assert!(box_.draft.is_empty());
    }

    #[test]
    fn test_confirm_or_submit_creates_when_no_candidates() {
        let groups = vec![HighlightGroup::from_pattern("error").unwrap()];
        let mut box_ = HighlightBox::default();
        box_.draft = "unique".into();
        let g = box_.confirm_or_submit(&groups).unwrap().unwrap();
        assert_eq!(g.pattern, "unique");
    }

    #[test]
    fn test_confirm_or_submit_empty_draft_is_noop_even_with_candidates() {
        let groups = vec![HighlightGroup::from_pattern("error").unwrap()];
        let mut box_ = HighlightBox::default();
        assert!(box_.confirm_or_submit(&groups).unwrap().is_none());
        assert_eq!(box_.candidate_indices(&groups).len(), 1);
    }
}
