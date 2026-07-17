use aloggrep::expr::Expr;

use crate::input::{Chip, ChipField};
use crate::model::EntryRow;

/// One AND-combined filter clause plus an optional read-only time bound
/// (from `--since`/`--until`, which `expr::Expr` can't express — see design
/// doc "过滤条件的整体语义"). `label` is precomputed display text; `chips`
/// drives pill rendering and dedup (time-only groups may have empty chips).
pub struct Group {
    pub label: String,
    pub chips: Vec<Chip>,
    pub expr: Option<Expr>,
    pub time: Option<TimeBound>,
    /// When false, the group is ignored by `GroupList::matches` (soft disable
    /// via `di`; distinct from deleting with `dd`).
    pub enabled: bool,
}

impl Group {
    pub fn matches(&self, row: &EntryRow) -> bool {
        let le = row.as_log_entry();
        let expr_ok = self.expr.as_ref().map_or(true, |e| e.matches(&le));
        let time_ok = self.time.as_ref().map_or(true, |t| t.matches(&le));
        expr_ok && time_ok
    }

    /// Chip multiset equality (field + ignore-case value) plus identical time bound.
    pub fn same_as(&self, other: &Group) -> bool {
        if !time_bound_eq(&self.time, &other.time) {
            return false;
        }
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

fn time_bound_eq(a: &Option<TimeBound>, b: &Option<TimeBound>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.since == b.since && a.until == b.until,
        _ => false,
    }
}

/// Mirrors `filter::TimeFilter`'s auto-detect-format comparison using only
/// `LogEntry`'s public `time_hms`/`time_full` accessors (that private enum
/// isn't exposed from `aloggrep-core`, and this is the only piece needed).
pub struct TimeBound {
    pub since: Option<String>,
    pub until: Option<String>,
}

impl TimeBound {
    fn is_time_only(s: &str) -> bool {
        s.len() == 8 && s.as_bytes().get(2) == Some(&b':') && s.as_bytes().get(5) == Some(&b':')
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

/// OR'd list of groups. Empty list = no filtering (everything visible).
/// All-disabled is treated the same as empty (everything visible).
#[derive(Default)]
pub struct GroupList {
    pub groups: Vec<Group>,
}

impl GroupList {
    pub fn matches(&self, row: &EntryRow) -> bool {
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
            time: None,
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
        let list = GroupList {
            groups: vec![Group {
                label: "since/until".into(),
                chips: Vec::new(),
                expr: None,
                time: Some(bound),
                enabled: true,
            }],
        };
        assert!(list.matches(&row("A", "m", "I"))); // entry ts is exactly 10:00:00.000
    }

    #[test]
    fn test_disabled_group_skipped() {
        let expr = Expr::parse("tag~A", false).unwrap();
        let list = GroupList {
            groups: vec![Group {
                label: "tag:A".into(),
                chips: Vec::new(),
                expr: Some(expr),
                time: None,
                enabled: false,
            }],
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
                    time: None,
                    enabled: false,
                },
                Group {
                    label: "tag:B".into(),
                    chips: Vec::new(),
                    expr: Some(Expr::parse("tag~B", false).unwrap()),
                    time: None,
                    enabled: true,
                },
            ],
        };
        assert!(!list.matches(&row("A", "m", "I")));
        assert!(list.matches(&row("B", "m", "I")));
    }
}
