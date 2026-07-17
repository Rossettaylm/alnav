use regex::Regex;

/// One committed search group: multiple pattern chips highlighted with OR
/// semantics (any match paints). Patterns are always compiled case-insensitively.
pub struct SearchGroup {
    pub patterns: Vec<Regex>,
    pub label: String,
    pub enabled: bool,
}

impl SearchGroup {
    /// Compile pattern strings into a group. Returns `None` if `patterns` is
    /// empty or any pattern fails to compile (caller treats that as a silent
    /// no-op, matching the old search-box typo behavior).
    pub fn from_patterns(patterns: &[String]) -> Option<Self> {
        if patterns.is_empty() {
            return None;
        }
        let mut compiled = Vec::with_capacity(patterns.len());
        for p in patterns {
            let re = Regex::new(&format!("(?i){p}")).ok()?;
            compiled.push(re);
        }
        let label = patterns.join(" AND ");
        Some(Self {
            patterns: compiled,
            label,
            enabled: true,
        })
    }

    pub fn matches_msg(&self, msg: &str) -> bool {
        self.patterns.iter().any(|re| re.is_match(msg))
    }
}

#[derive(Default)]
pub struct SearchGroupList {
    pub groups: Vec<SearchGroup>,
}

impl SearchGroupList {
    /// Flatten enabled groups' patterns with progressive color indices.
    pub fn active_patterns(&self) -> Vec<(&Regex, usize)> {
        let mut out = Vec::new();
        let mut idx = 0usize;
        for g in &self.groups {
            if !g.enabled {
                continue;
            }
            for re in &g.patterns {
                out.push((re, idx));
                idx += 1;
            }
        }
        out
    }

    pub fn any_match(&self, msg: &str) -> bool {
        self.groups
            .iter()
            .filter(|g| g.enabled)
            .any(|g| g.matches_msg(msg))
    }
}

/// Bottom search input: plain pattern chips (no ChipField), Space commits a
/// chip, Enter builds a `SearchGroup`.
#[derive(Default)]
pub struct SearchBox {
    pub chips: Vec<String>,
    pub draft: String,
    /// When true, key events are routed here (entered via `/`).
    pub editing: bool,
}

impl SearchBox {
    pub fn is_empty(&self) -> bool {
        self.chips.is_empty() && self.draft.is_empty()
    }

    pub fn push_char(&mut self, c: char) {
        self.draft.push(c);
    }

    pub fn commit_draft_as_chip(&mut self) {
        if self.draft.is_empty() {
            return;
        }
        let value = std::mem::take(&mut self.draft);
        self.chips.push(value);
    }

    pub fn backspace(&mut self) {
        if !self.draft.is_empty() {
            self.draft.pop();
        } else {
            self.chips.pop();
        }
    }

    /// Enter: commit draft, compile chips into a group, clear buffer.
    /// `Ok(None)` = empty (no-op). `Err` = bad regex (caller ignores silently).
    pub fn build_group(&mut self) -> Result<Option<SearchGroup>, ()> {
        self.commit_draft_as_chip();
        if self.chips.is_empty() {
            return Ok(None);
        }
        let chips = std::mem::take(&mut self.chips);
        match SearchGroup::from_patterns(&chips) {
            Some(g) => Ok(Some(g)),
            None => {
                // Restore chips so the user can edit; treat as silent failure.
                self.chips = chips;
                Err(())
            }
        }
    }

    pub fn clear(&mut self) {
        self.chips.clear();
        self.draft.clear();
        self.editing = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_group_ignore_case() {
        let g = SearchGroup::from_patterns(&["ERROR".into()]).unwrap();
        assert!(g.matches_msg("an error occurred"));
        assert!(g.matches_msg("AN ERROR"));
    }

    #[test]
    fn test_search_group_bad_regex_returns_none() {
        assert!(SearchGroup::from_patterns(&["(unclosed".into()]).is_none());
    }

    #[test]
    fn test_active_patterns_skips_disabled_and_assigns_color_idx() {
        let mut list = SearchGroupList::default();
        list.groups.push(SearchGroup::from_patterns(&["a".into(), "b".into()]).unwrap());
        list.groups.push(SearchGroup::from_patterns(&["c".into()]).unwrap());
        list.groups[0].enabled = false;
        let active = list.active_patterns();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].1, 0);
        assert!(active[0].0.is_match("c"));
    }

    #[test]
    fn test_search_box_space_and_enter() {
        let mut box_ = SearchBox::default();
        box_.push_char('f');
        box_.push_char('o');
        box_.push_char('o');
        box_.commit_draft_as_chip();
        box_.push_char('b');
        box_.push_char('a');
        box_.push_char('r');
        let g = box_.build_group().unwrap().unwrap();
        assert_eq!(g.label, "foo AND bar");
        assert_eq!(g.patterns.len(), 2);
        assert!(box_.is_empty());
    }

    #[test]
    fn test_search_box_empty_enter_is_none() {
        let mut box_ = SearchBox::default();
        assert!(box_.build_group().unwrap().is_none());
    }
}
