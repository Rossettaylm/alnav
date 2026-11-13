//! Filter / Exclude / time-window rule state and its compilation to one [`Expr`].

use crate::fuzzy::SameFieldOp;
use crate::input::{Chip, ChipField};
use alnav::expr::{Expr, IdField, TextField, TimePoint};
use alnav::parser::Level;

/// One AND-combined filter clause. `label` is precomputed display text; `chips`
/// drives pill rendering, dedup, and compilation. Session time window lives on
/// [`crate::app::App`], not on groups.
#[derive(Debug, Clone)]
pub struct Group {
    pub label: String,
    pub chips: Vec<Chip>,
    /// When false, the group is ignored by [`compile_predicate`] (soft disable
    /// via `di`; distinct from deleting with `dd`).
    pub enabled: bool,
    /// How same-field chips combine (interactive And / startup CLI Or).
    pub same_field_op: SameFieldOp,
}

impl Group {
    /// `None` when the group has no chips (matches everything).
    pub fn to_expr(&self) -> Option<Expr> {
        match self.same_field_op {
            SameFieldOp::And => Expr::all(self.chips.iter().map(chip_expr)),
            SameFieldOp::Or => {
                // Same field OR'd, fields AND'd; only the last level chip counts.
                let fields = [
                    ChipField::Tag,
                    ChipField::Msg,
                    ChipField::Pkg,
                    ChipField::Pid,
                    ChipField::Tid,
                ];
                let per_field = fields.into_iter().filter_map(|f| {
                    Expr::any(self.chips.iter().filter(|c| c.field == f).map(chip_expr))
                });
                let level = self
                    .chips
                    .iter()
                    .rev()
                    .find(|c| c.field == ChipField::Level)
                    .map(chip_expr);
                Expr::all(per_field.chain(level))
            }
        }
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

/// Constant-false expression (every parsed entry has level ≥ V).
fn never() -> Expr {
    Expr::LevelGte(Level::V).negate()
}

/// One chip as an ignore-case predicate:
/// - tag / msg / pkg: every whitespace-separated atom is a contiguous substring
///   of that field (empty field never matches; a blank value = field non-empty);
/// - pid / tid: exact equality;
/// - level: minimum level (an unknown level never matches).
pub fn chip_expr(chip: &Chip) -> Expr {
    let text = |field: TextField, kw: &str| {
        Expr::all(
            chip.value
                .split_whitespace()
                .map(|atom| Expr::literal(field, atom, true)),
        )
        .unwrap_or_else(|| Expr::parse(&format!("{kw} ~ \"(?s).\""), true).expect("static pattern"))
    };
    match chip.field {
        ChipField::Tag => text(TextField::Tag, "tag"),
        ChipField::Msg => text(TextField::Msg, "msg"),
        ChipField::Pkg => text(TextField::Pkg, "pkg"),
        ChipField::Pid => Expr::Eq(IdField::Pid, chip.value.clone()),
        ChipField::Tid => Expr::Eq(IdField::Tid, chip.value.clone()),
        ChipField::Level => Level::from_str(&chip.value).map_or_else(never, Expr::LevelGte),
    }
}

/// Global session time window; either side may be open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeBound {
    pub since: Option<String>,
    pub until: Option<String>,
}

impl TimeBound {
    /// True when at least one endpoint is set.
    pub fn is_active(&self) -> bool {
        self.since.is_some() || self.until.is_some()
    }

    /// `time >= since and time <= until` (unparseable sides are dropped).
    fn to_expr(&self) -> Option<Expr> {
        let since = self.since.as_deref().and_then(|s| TimePoint::parse(s).ok());
        let until = self.until.as_deref().and_then(|u| TimePoint::parse(u).ok());
        Expr::all(
            since
                .map(Expr::TimeGe)
                .into_iter()
                .chain(until.map(Expr::TimeLe)),
        )
    }
}

/// One global exclude chip (H9): positive chip match is AND NOT.
#[derive(Debug, Clone)]
pub struct ExcludeEntry {
    pub chip: Chip,
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
#[derive(Default, Clone)]
pub struct GroupList {
    pub groups: Vec<Group>,
    pub excludes: Vec<ExcludeEntry>,
}

impl GroupList {
    /// Whether any include group is enabled (the OR list is non-vacuous).
    pub fn has_any_enabled(&self) -> bool {
        self.groups.iter().any(|g| g.enabled)
    }

    /// Append an exclude chip. Returns false on duplicate.
    pub fn push_exclude(&mut self, chip: Chip) -> Result<bool, String> {
        if chip.value.is_empty() {
            return Err("empty exclude".to_string());
        }
        if self.excludes.iter().any(|e| e.same_chip_as(&chip)) {
            return Ok(false);
        }
        self.excludes.push(ExcludeEntry {
            chip,
            enabled: true,
        });
        Ok(true)
    }
}

/// The whole row predicate (Filter OR-groups ∧ ¬Excludes ∧ lock ∧ time window)
/// as one ignore-case [`Expr`]. Disabled rules are skipped; `None` = no filter.
///
/// `Display` of the result is a valid `alnav grep -i -e` argument that selects
/// the same parsed rows.
pub fn compile_predicate(
    rules: &GroupList,
    lock_pid: Option<&str>,
    lock_tid: Option<&str>,
    time: Option<&TimeBound>,
) -> Option<Expr> {
    let enabled: Vec<Option<Expr>> = rules
        .groups
        .iter()
        .filter(|g| g.enabled)
        .map(Group::to_expr)
        .collect();
    // A chip-less enabled group matches everything, so the OR is vacuous too.
    let include = if enabled.iter().all(Option::is_some) {
        Expr::any(enabled.into_iter().flatten())
    } else {
        None
    };
    let excludes = rules
        .excludes
        .iter()
        .filter(|e| e.enabled)
        .map(|e| chip_expr(&e.chip).negate());
    // pid lock wins over tid (they are mutually exclusive in practice).
    let lock = match (lock_pid, lock_tid) {
        (Some(pid), _) => Some(Expr::Eq(IdField::Pid, pid.to_string())),
        (None, Some(tid)) => Some(Expr::Eq(IdField::Tid, tid.to_string())),
        (None, None) => None,
    };
    let time = time.and_then(TimeBound::to_expr);
    Expr::all(include.into_iter().chain(excludes).chain(lock).chain(time))
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

    impl GroupList {
        fn matches(&self, row: &EntryRow) -> bool {
            compile_predicate(self, None, None, None)
                .map_or(true, |e| e.matches(&row.as_log_entry()))
        }
    }

    impl TimeBound {
        fn matches(&self, entry: &alnav::parser::LogEntry) -> bool {
            self.to_expr().map_or(true, |e| e.matches(entry))
        }
    }

    #[test]
    fn startup_or_group_ors_within_field_ands_across() {
        let g = Group {
            label: "s".into(),
            chips: vec![
                Chip {
                    field: ChipField::Tag,
                    value: "A".into(),
                },
                Chip {
                    field: ChipField::Tag,
                    value: "B".into(),
                },
                Chip {
                    field: ChipField::Msg,
                    value: "x".into(),
                },
            ],
            enabled: true,
            same_field_op: SameFieldOp::Or,
        };
        let list = GroupList {
            groups: vec![g],
            ..Default::default()
        };
        assert!(list.matches(&row("A", "x", "I")));
        assert!(list.matches(&row("B", "x", "I")));
        assert!(!list.matches(&row("A", "y", "I")));
        assert!(!list.matches(&row("C", "x", "I")));
    }

    #[test]
    fn multi_word_chip_requires_every_atom_any_order() {
        let list = GroupList {
            groups: vec![group_chips(
                "msg:bar foo",
                vec![Chip {
                    field: ChipField::Msg,
                    value: "bar foo".into(),
                }],
            )],
            ..Default::default()
        };
        assert!(list.matches(&row("T", "foo x BAR", "I")));
        assert!(!list.matches(&row("T", "foo only", "I")));
    }

    #[test]
    fn lock_and_time_join_the_predicate() {
        let p = compile_predicate(
            &GroupList::default(),
            Some("12"),
            Some("ignored"),
            Some(&TimeBound {
                since: Some("10:00:00".into()),
                until: None,
            }),
        )
        .unwrap();
        let at = |pid: &str, t: &str| {
            EntryRow::from_line(&format!("04-02 {t}.000  {pid}  1 I T   : m")).unwrap()
        };
        assert!(p.matches(&at("12", "10:00:00").as_log_entry()));
        assert!(!p.matches(&at("123", "10:00:00").as_log_entry()));
        assert!(!p.matches(&at("12", "09:59:59").as_log_entry()));
    }

    #[test]
    fn chipless_enabled_group_is_vacuous() {
        let list = GroupList {
            groups: vec![
                group_chips("empty", Vec::new()),
                group_chips(
                    "tag:A",
                    vec![Chip {
                        field: ChipField::Tag,
                        value: "A".into(),
                    }],
                ),
            ],
            ..Default::default()
        };
        assert!(list.matches(&row("Z", "m", "I")));
    }

    fn group_chips(label: &str, chips: Vec<Chip>) -> Group {
        Group {
            label: label.into(),
            chips,
            enabled: true,
            same_field_op: SameFieldOp::And,
        }
    }

    #[test]
    fn test_empty_group_list_matches_everything() {
        let list = GroupList::default();
        assert!(list.matches(&row("A", "m", "I")));
    }

    #[test]
    fn test_single_group_and_within() {
        let list = GroupList {
            groups: vec![group_chips(
                "tag:A AND level:W",
                vec![
                    Chip {
                        field: ChipField::Tag,
                        value: "A".into(),
                    },
                    Chip {
                        field: ChipField::Level,
                        value: "W".into(),
                    },
                ],
            )],
            ..Default::default()
        };
        assert!(list.matches(&row("A", "m", "E")));
        assert!(!list.matches(&row("A", "m", "I")));
        assert!(!list.matches(&row("B", "m", "E")));
    }

    #[test]
    fn test_multiple_groups_or_between() {
        let list = GroupList {
            groups: vec![
                group_chips(
                    "tag:A",
                    vec![Chip {
                        field: ChipField::Tag,
                        value: "A".into(),
                    }],
                ),
                group_chips(
                    "tag:B",
                    vec![Chip {
                        field: ChipField::Tag,
                        value: "B".into(),
                    }],
                ),
            ],
            ..Default::default()
        };
        assert!(list.matches(&row("A", "m", "I")));
        assert!(list.matches(&row("B", "m", "I")));
        assert!(!list.matches(&row("C", "m", "I")));
    }

    #[test]
    fn test_substring_tag_chip() {
        let list = GroupList {
            groups: vec![group_chips(
                "tag:Tag",
                vec![Chip {
                    field: ChipField::Tag,
                    value: "Tag".into(),
                }],
            )],
            ..Default::default()
        };
        assert!(list.matches(&row("MyTag", "m", "I")));
        assert!(!list.matches(&row("aXbYc", "m", "I"))); // gaps ≠ substring
        assert!(!list.matches(&row("zzz", "m", "I")));
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
        let list = GroupList {
            groups: vec![Group {
                label: "tag:A".into(),
                chips: vec![Chip {
                    field: ChipField::Tag,
                    value: "A".into(),
                }],
                enabled: false,
                same_field_op: SameFieldOp::And,
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
                    chips: vec![Chip {
                        field: ChipField::Tag,
                        value: "A".into(),
                    }],
                    enabled: false,
                    same_field_op: SameFieldOp::And,
                },
                Group {
                    label: "tag:B".into(),
                    chips: vec![Chip {
                        field: ChipField::Tag,
                        value: "B".into(),
                    }],
                    enabled: true,
                    same_field_op: SameFieldOp::And,
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
            groups: vec![group_chips(
                "tag:A",
                vec![Chip {
                    field: ChipField::Tag,
                    value: "A".into(),
                }],
            )],
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

    fn chip(field: ChipField, value: &str) -> Chip {
        Chip {
            field,
            value: value.into(),
        }
    }

    fn chip_hits(c: &Chip, row: &EntryRow) -> bool {
        chip_expr(c).matches(&row.as_log_entry())
    }

    #[test]
    fn chip_empty_field_never_matches_and_is_contiguous() {
        let row = EntryRow::from_line("04-02 10:00:00.000  1234  5678 I Tag   : hello").unwrap();
        assert!(!chip_hits(&chip(ChipField::Pkg, "x"), &row));
        assert!(chip_hits(&chip(ChipField::Tag, "Tag"), &row));
        assert!(!chip_hits(&chip(ChipField::Tag, "Tg"), &row));
    }

    #[test]
    fn chip_hex_is_contiguous_not_fuzzy() {
        let line = "2026-07-22 16:43:02.809|1[14107]14321|14107|I|NTKernel|[I] sys_msg_mgr.cc(526)::NotifyRecvSysMsg [SysMsgMgr]->notify recv sys msg: msg_type=0xf01, sub_type=0x6d, is_guild_msg=1";
        let row = EntryRow::from_line(line).unwrap();
        assert!(!chip_hits(&chip(ChipField::Msg, "0x1100"), &row));
        assert!(chip_hits(&chip(ChipField::Msg, "0x6d"), &row));
        assert!(chip_hits(&chip(ChipField::Tag, "ntkernel"), &row));
    }

    #[test]
    fn chip_pid_exact_and_level_minimum() {
        let row = EntryRow::from_line("04-02 10:00:00.000  1234  5678 E Tag   : hello").unwrap();
        assert!(chip_hits(&chip(ChipField::Pid, "1234"), &row));
        assert!(!chip_hits(&chip(ChipField::Pid, "12"), &row));
        assert!(chip_hits(&chip(ChipField::Level, "W"), &row));
        assert!(!chip_hits(&chip(ChipField::Level, "F"), &row));
        assert!(!chip_hits(&chip(ChipField::Level, "bogus"), &row));
    }

    #[test]
    fn compiled_predicate_display_reparses_to_same_expr() {
        let mut list = GroupList::default();
        list.groups.push(group_chips(
            "g",
            vec![
                chip(ChipField::Msg, r#"say "hi" (0)|x \ done"#),
                chip(ChipField::Level, "W"),
            ],
        ));
        list.groups
            .push(group_chips("g2", vec![chip(ChipField::Tag, "Other")]));
        assert!(list.push_exclude(chip(ChipField::Pkg, "noise")).unwrap());
        let time = TimeBound {
            since: Some("04-02 10:00:00".into()),
            until: Some("11:00:00".into()),
        };
        let e = compile_predicate(&list, None, Some("77"), Some(&time)).unwrap();
        assert_eq!(Expr::parse(&e.to_string(), true).unwrap(), e);
    }
}
