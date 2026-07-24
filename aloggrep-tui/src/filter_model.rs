use aloggrep::expr::Expr;

use crate::input::{Chip, ChipField};
use crate::model::EntryRow;

/// One AND-combined filter clause. `label` is precomputed display text; `chips`
/// drives pill rendering and dedup. Session time window lives on [`crate::app::App`],
/// not on groups.
pub struct Group {
    pub label: String,
    pub chips: Vec<Chip>,
    pub expr: Option<Expr>,
    /// When false, the group is ignored by `GroupList::matches` (soft disable
    /// via `di`; distinct from deleting with `dd`).
    pub enabled: bool,
}

impl Group {
    pub fn matches(&self, row: &EntryRow) -> bool {
        let le = row.as_log_entry();
        self.expr.as_ref().map_or(true, |e| e.matches(&le))
    }

    /// Chip multiset equality (field + ignore-case value).
    pub fn same_as(&self, other: &Group) -> bool {
        if self.chips.len() != other.chips.len() {
            return false;
        }
        let mut a = normalize_chips(&self.chips);
        let mut b = normalize_chips(&other.chips);
        a.sort();
        b.sort();
        a == b
    }
}

fn normalize_chips(chips: &[Chip]) -> Vec<(ChipField, String)> {
    chips
        .iter()
        .map(|c| (c.field, c.value.to_ascii_lowercase()))
        .collect()
}

/// Mirrors `filter::TimeFilter`'s auto-detect-format comparison using only
/// `LogEntry`'s public `time_hms`/`time_full` accessors (that private enum
/// isn't exposed from `aloggrep-core`, and this is the only piece needed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeBound {
    pub since: Option<String>,
    pub until: Option<String>,
}

impl TimeBound {
    pub(crate) fn is_time_only(s: &str) -> bool {
        s.len() == 8 && s.as_bytes().get(2) == Some(&b':') && s.as_bytes().get(5) == Some(&b':')
    }

    /// True when at least one endpoint is set.
    pub fn is_active(&self) -> bool {
        self.since.is_some() || self.until.is_some()
    }

    pub fn matches(&self, entry: &aloggrep::parser::LogEntry) -> bool {
        if let Some(s) = &self.since {
            let ok = if Self::is_time_only(s) {
                entry.time_hms().map_or(true, |hms| hms >= s.as_str())
            } else {
                entry.time_full().map_or(true, |full| full >= s.as_str())
            };
            if !ok {
                return false;
            }
        }
        if let Some(u) = &self.until {
            let ok = if Self::is_time_only(u) {
                entry.time_hms().map_or(true, |hms| hms <= u.as_str())
            } else {
                entry.time_full().map_or(true, |full| full <= u.as_str())
            };
            if !ok {
                return false;
            }
        }
        true
    }
}

/// One global exclude chip (H9): positive `expr` is matched as AND NOT.
#[derive(Debug)]
pub struct ExcludeEntry {
    pub chip: Chip,
    pub expr: Expr,
    pub enabled: bool,
}

impl ExcludeEntry {
    /// Ignore-case field+value equality (for dedup).
    pub fn same_chip_as(&self, chip: &Chip) -> bool {
        self.chip.field == chip.field && self.chip.value.eq_ignore_ascii_case(&chip.value)
    }
}

/// OR'd list of groups, plus global AND-NOT excludes (H9).
/// Empty / all-disabled includes = no include filtering (everything eligible).
/// Each enabled exclude independently ANDs a NOT.
#[derive(Default)]
pub struct GroupList {
    pub groups: Vec<Group>,
    pub excludes: Vec<ExcludeEntry>,
}

impl GroupList {
    pub fn matches(&self, row: &EntryRow) -> bool {
        self.include_matches(row) && self.excludes_allow(row)
    }

    /// Whether any include group is enabled (the OR list is non-vacuous).
    pub fn has_any_enabled(&self) -> bool {
        self.groups.iter().any(|g| g.enabled)
    }

    fn include_matches(&self, row: &EntryRow) -> bool {
        let mut any_enabled = false;
        for g in &self.groups {
            if !g.enabled {
                continue;
            }
            any_enabled = true;
            if g.matches(row) {
                return true;
            }
        }
        !any_enabled
    }

    /// False when any enabled exclude's positive expr matches the row.
    fn excludes_allow(&self, row: &EntryRow) -> bool {
        let le = row.as_log_entry();
        for e in &self.excludes {
            if e.enabled && e.expr.matches(&le) {
                return false;
            }
        }
        true
    }

    /// Compile and append an exclude chip. Returns false on duplicate.
    pub fn push_exclude(&mut self, chip: Chip) -> Result<bool, String> {
        if self.excludes.iter().any(|e| e.same_chip_as(&chip)) {
            return Ok(false);
        }
        let group = crate::input::build_group_from_chips(vec![chip.clone()], true)?
            .ok_or_else(|| "empty exclude".to_string())?;
        let expr = group
            .expr
            .ok_or_else(|| "exclude needs an expression".to_string())?;
        self.excludes.push(ExcludeEntry {
            chip,
            expr,
            enabled: true,
        });
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EntryRow;

    fn row(tag: &str, msg: &str, level_line: &str) -> EntryRow {
        EntryRow::from_line(&format!(
            "04-02 10:00:00.000  1234  5678 {level_line} {tag}   : {msg}"
        ))
        .unwrap()
    }

    fn group(label: &str, expr: Option<Expr>) -> Group {
        Group {
            label: label.into(),
            chips: Vec::new(),
            expr,
            enabled: true,
        }
    }

    #[test]
    fn test_empty_group_list_matches_everything() {
        let list = GroupList::default();
        assert!(list.matches(&row("A", "m", "I")));
    }

    #[test]
    fn test_single_group_and_within() {
        let expr = Expr::parse("tag~A and level>=W", false).unwrap();
        let list = GroupList {
            groups: vec![group("tag:A AND level:W", Some(expr))],
            ..Default::default()
        };
        assert!(list.matches(&row("A", "m", "E")));
        assert!(!list.matches(&row("A", "m", "I")));
        assert!(!list.matches(&row("B", "m", "E")));
    }

    #[test]
    fn test_multiple_groups_or_between() {
        let g1 = Expr::parse("tag~A", false).unwrap();
        let g2 = Expr::parse("tag~B", false).unwrap();
        let list = GroupList {
            groups: vec![group("tag:A", Some(g1)), group("tag:B", Some(g2))],
            ..Default::default()
        };
        assert!(list.matches(&row("A", "m", "I")));
        assert!(list.matches(&row("B", "m", "I")));
        assert!(!list.matches(&row("C", "m", "I")));
    }

    #[test]
    fn test_time_bound_since_until() {
        let bound = TimeBound {
            since: Some("10:00:00".into()),
            until: Some("10:00:00".into()),
        };
        let r = row("A", "m", "I");
        let entry = r.as_log_entry();
        assert!(bound.matches(&entry)); // entry ts is exactly 10:00:00.000
        let early = TimeBound {
            since: Some("10:00:01".into()),
            until: None,
        };
        assert!(!early.matches(&entry));
    }

    #[test]
    fn test_disabled_group_skipped() {
        let expr = Expr::parse("tag~A", false).unwrap();
        let list = GroupList {
            groups: vec![Group {
                label: "tag:A".into(),
                chips: Vec::new(),
                expr: Some(expr),
                enabled: false,
            }],
            ..Default::default()
        };
        // all-disabled ≡ empty → everything visible
        assert!(list.matches(&row("B", "m", "I")));
    }

    #[test]
    fn test_disabled_among_enabled_only_active_match() {
        let list = GroupList {
            groups: vec![
                Group {
                    label: "tag:A".into(),
                    chips: Vec::new(),
                    expr: Some(Expr::parse("tag~A", false).unwrap()),
                    enabled: false,
                },
                Group {
                    label: "tag:B".into(),
                    chips: Vec::new(),
                    expr: Some(Expr::parse("tag~B", false).unwrap()),
                    enabled: true,
                },
            ],
            excludes: Vec::new(),
        };
        assert!(!list.matches(&row("A", "m", "I")));
        assert!(list.matches(&row("B", "m", "I")));
    }

    #[test]
    fn test_exclude_and_not_after_include() {
        let mut list = GroupList {
            groups: vec![group("tag:A", Some(Expr::parse("tag~A", true).unwrap()))],
            excludes: Vec::new(),
        };
        assert!(list
            .push_exclude(Chip {
                field: ChipField::Msg,
                value: "spam".into(),
            })
            .unwrap());
        assert!(list.matches(&row("A", "ok", "I")));
        assert!(!list.matches(&row("A", "has spam here", "I")));
        assert!(!list.matches(&row("B", "ok", "I")));
    }

    #[test]
    fn test_exclude_only_subtracts_from_all() {
        let mut list = GroupList::default();
        assert!(list
            .push_exclude(Chip {
                field: ChipField::Tag,
                value: "Noise".into(),
            })
            .unwrap());
        assert!(list.matches(&row("Keep", "m", "I")));
        assert!(!list.matches(&row("Noise", "m", "I")));
    }

    #[test]
    fn test_exclude_di_disables() {
        let mut list = GroupList::default();
        list.push_exclude(Chip {
            field: ChipField::Tag,
            value: "Spam".into(),
        })
        .unwrap();
        assert!(!list.matches(&row("Spam", "m", "I")));
        list.excludes[0].enabled = false;
        assert!(list.matches(&row("Spam", "m", "I")));
    }

    #[test]
    fn test_exclude_dedup_ignore_case() {
        let mut list = GroupList::default();
        assert!(list
            .push_exclude(Chip {
                field: ChipField::Tag,
                value: "Foo".into(),
            })
            .unwrap());
        assert!(!list
            .push_exclude(Chip {
                field: ChipField::Tag,
                value: "foo".into(),
            })
            .unwrap());
        assert_eq!(list.excludes.len(), 1);
    }
}
