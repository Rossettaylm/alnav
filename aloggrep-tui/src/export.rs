//! H10: export current TUI filter state as a one-line `aloggrep` CLI command.

use crate::filter_model::{ExcludeEntry, Group, GroupList, TimeBound};
use crate::input::{Chip, ChipField};

/// How the TUI session was started (mirrored into the exported command).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportSource {
    File(String),
    Hdc { device: Option<String> },
}

impl Default for ExportSource {
    fn default() -> Self {
        Self::File(String::new())
    }
}

/// Build `aloggrep …` from enabled filter groups, excludes, session lock, and
/// the global time window.
///
/// Search chips are never included. Disabled (`di`) groups/excludes are skipped.
/// Values are regex-escaped so CLI `-e` matches TUI literal chip semantics.
/// Always adds `-i` (TUI is ignore-case).
pub fn build_cli_command(
    source: &ExportSource,
    groups: &GroupList,
    lock_pid: Option<&str>,
    lock_tid: Option<&str>,
    time_bound: Option<&TimeBound>,
) -> String {
    let mut parts: Vec<String> = vec!["aloggrep".into()];

    match source {
        ExportSource::File(path) => {
            parts.push("-f".into());
            parts.push(shell_quote(path));
        }
        ExportSource::Hdc { device } => {
            parts.push("--hdc".into());
            if let Some(serial) = device {
                if !serial.is_empty() {
                    parts.push("--device".into());
                    parts.push(shell_quote(serial));
                }
            }
        }
    }

    parts.push("-i".into());

    if let Some(bound) = time_bound.filter(|t| t.is_active()) {
        if let Some(s) = &bound.since {
            parts.push("--since".into());
            parts.push(shell_quote(s));
        }
        if let Some(u) = &bound.until {
            parts.push("--until".into());
            parts.push(shell_quote(u));
        }
    }

    if let Some(expr) = filter_expr(groups) {
        parts.push("-e".into());
        parts.push(shell_quote(&expr));
    }

    if let Some(pid) = lock_pid {
        parts.push("--pid".into());
        parts.push(shell_quote(pid));
    } else if let Some(tid) = lock_tid {
        parts.push("--tid".into());
        parts.push(shell_quote(tid));
    }

    parts.join(" ")
}

fn filter_expr(groups: &GroupList) -> Option<String> {
    let includes: Vec<String> = groups
        .groups
        .iter()
        .filter(|g| g.enabled)
        .filter_map(group_expr)
        .collect();
    let excludes: Vec<String> = groups
        .excludes
        .iter()
        .filter(|e| e.enabled)
        .map(exclude_expr)
        .collect();

    if includes.is_empty() && excludes.is_empty() {
        return None;
    }

    let include_part = match includes.as_slice() {
        [] => None,
        [one] => Some(one.clone()),
        many => Some(
            many.iter()
                .map(|e| format!("({e})"))
                .collect::<Vec<_>>()
                .join(" or "),
        ),
    };

    let exclude_part = excludes
        .iter()
        .map(|e| format!("not ({e})"))
        .collect::<Vec<_>>()
        .join(" and ");

    match (include_part, exclude_part.as_str()) {
        (Some(inc), "") => Some(inc),
        (None, excl) if !excl.is_empty() => Some(excl.to_string()),
        (Some(inc), excl) if !excl.is_empty() => {
            let wrapped = if includes.len() > 1 {
                format!("({inc})")
            } else if needs_parens_for_and(&inc) {
                format!("({inc})")
            } else {
                inc
            };
            Some(format!("{wrapped} and {excl}"))
        }
        _ => None,
    }
}

fn needs_parens_for_and(expr: &str) -> bool {
    expr.contains(" or ")
}

fn group_expr(group: &Group) -> Option<String> {
    if group.chips.is_empty() {
        return None;
    }
    let atoms: Vec<String> = group.chips.iter().map(chip_atom).collect();
    Some(atoms.join(" and "))
}

fn exclude_expr(entry: &ExcludeEntry) -> String {
    chip_atom(&entry.chip)
}

fn chip_atom(chip: &Chip) -> String {
    match chip.field {
        ChipField::Level => format!("level >= {}", chip.value),
        field => {
            let lit = regex::escape(&chip.value);
            format!("{} ~ {}", field.keyword(), expr_quote_value(&lit))
        }
    }
}

/// Quote a value for embedding inside an `-e` expression.
/// Prefer double quotes so the whole `-e` arg can be shell single-quoted.
fn expr_quote_value(s: &str) -> String {
    if !s.contains('"') {
        format!("\"{s}\"")
    } else if !s.contains('\'') {
        format!("'{s}'")
    } else {
        // Expr lexer has no escapes; drop `"` so the value still parses.
        format!("\"{}\"", s.replace('"', ""))
    }
}

/// POSIX-ish single-quote shell escaping.
pub fn shell_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".into();
    }
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter_model::{Group, GroupList, TimeBound};
    use crate::input::{build_group_from_chips, Chip, ChipField};

    fn chip(field: ChipField, value: &str) -> Chip {
        Chip {
            field,
            value: value.into(),
        }
    }

    fn group_from(chips: Vec<Chip>) -> Group {
        build_group_from_chips(chips, true).unwrap().unwrap()
    }

    #[test]
    fn empty_filter_exports_skeleton_file() {
        let cmd = build_cli_command(
            &ExportSource::File("app.log".into()),
            &GroupList::default(),
            None,
            None,
            None,
        );
        assert_eq!(cmd, "aloggrep -f 'app.log' -i");
    }

    #[test]
    fn empty_filter_exports_hdc() {
        let cmd = build_cli_command(
            &ExportSource::Hdc {
                device: Some("XYZ".into()),
            },
            &GroupList::default(),
            None,
            None,
            None,
        );
        assert_eq!(cmd, "aloggrep --hdc --device 'XYZ' -i");
    }

    #[test]
    fn exports_group_and_exclude_and_lock() {
        let mut list = GroupList::default();
        list.groups.push(group_from(vec![
            chip(ChipField::Tag, "OkHttp"),
            chip(ChipField::Msg, "timeout"),
        ]));
        assert!(list.push_exclude(chip(ChipField::Tag, "Noise")).unwrap());
        let cmd = build_cli_command(
            &ExportSource::File("a.log".into()),
            &list,
            Some("1234"),
            None,
            None,
        );
        assert!(cmd.starts_with("aloggrep -f 'a.log' -i -e "));
        assert!(
            cmd.contains(r#"tag ~ "OkHttp" and msg ~ "timeout""#),
            "{cmd}"
        );
        assert!(cmd.contains(r#"not (tag ~ "Noise")"#), "{cmd}");
        assert!(cmd.ends_with("--pid '1234'"));
    }

    #[test]
    fn or_between_groups() {
        let mut list = GroupList::default();
        list.groups
            .push(group_from(vec![chip(ChipField::Tag, "A")]));
        list.groups
            .push(group_from(vec![chip(ChipField::Tag, "B")]));
        let cmd = build_cli_command(&ExportSource::File("f".into()), &list, None, None, None);
        assert!(cmd.contains(r#"(tag ~ "A") or (tag ~ "B")"#), "{cmd}");
    }

    #[test]
    fn skips_disabled_groups_and_excludes() {
        let mut list = GroupList::default();
        let mut g = group_from(vec![chip(ChipField::Tag, "Keep")]);
        g.enabled = true;
        let mut off = group_from(vec![chip(ChipField::Tag, "Off")]);
        off.enabled = false;
        list.groups.push(g);
        list.groups.push(off);
        assert!(list.push_exclude(chip(ChipField::Msg, "x")).unwrap());
        list.excludes[0].enabled = false;
        let cmd = build_cli_command(&ExportSource::File("f".into()), &list, None, None, None);
        assert!(cmd.contains(r#"tag ~ "Keep""#), "{cmd}");
        assert!(!cmd.contains("Off"));
        assert!(!cmd.contains("not "));
    }

    #[test]
    fn escapes_regex_metacharacters() {
        let mut list = GroupList::default();
        list.groups
            .push(group_from(vec![chip(ChipField::Msg, "(0)")]));
        let cmd = build_cli_command(&ExportSource::File("f".into()), &list, None, None, None);
        assert!(cmd.contains(r#"msg ~ "\(0\)""#), "{cmd}");
    }

    #[test]
    fn exports_global_since_until() {
        let bound = TimeBound {
            since: Some("10:00:00".into()),
            until: Some("11:00:00".into()),
        };
        let cmd = build_cli_command(
            &ExportSource::File("f".into()),
            &GroupList::default(),
            None,
            None,
            Some(&bound),
        );
        assert!(cmd.contains("--since '10:00:00'"));
        assert!(cmd.contains("--until '11:00:00'"));
    }

    #[test]
    fn shell_quote_handles_embedded_single_quote() {
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }
}
