//! TUI fuzzy matching facade (nucleo-matcher).
//!
//! CLI `alnav grep` keeps its own FilterChain/Expr path; all interactive
//! text matching in the TUI goes through this module.

use alnav::parser::Level;
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::input::{Chip, ChipField};
use crate::model::EntryRow;

/// Separator between tag and msg in Search/Highlight haystacks.
pub const TAG_MSG_SEP: char = '\t';

/// Which log field a paint range belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaintField {
    Tag,
    Msg,
    /// Unparsed / both-empty fallback: paint on the raw (shown as msg).
    Raw,
}

/// One highlight span after mapping nucleo char indices back to a field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSpan {
    pub field: PaintField,
    /// Byte range within that field string.
    pub start: usize,
    pub end: usize,
}

/// Build Search/Highlight haystack: `tag + '\t' + msg`, or `raw` when both empty.
pub fn search_haystack(tag: &str, msg: &str, raw: &str) -> String {
    if tag.is_empty() && msg.is_empty() {
        raw.to_string()
    } else {
        let mut s = String::with_capacity(tag.len() + 1 + msg.len());
        s.push_str(tag);
        s.push(TAG_MSG_SEP);
        s.push_str(msg);
        s
    }
}

pub fn search_haystack_row(row: &EntryRow) -> String {
    search_haystack(&row.tag, &row.msg, &row.raw)
}

fn matcher() -> Matcher {
    Matcher::new(Config::DEFAULT)
}

/// Word-splitting fuzzy pattern (fzf/nucleo style): whitespace separates atoms ANDed
/// together. Use this for all TUI text queries — never a single `Atom` with spaces.
fn fuzzy_pattern(pattern: &str) -> Pattern {
    Pattern::new(
        pattern,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Fuzzy,
    )
}

/// Nucleo score for `pattern` against `haystack`, if it matches.
/// Empty pattern scores `Some(0)` (match-all). Empty haystack with non-empty pattern → `None`.
pub fn fuzzy_score(haystack: &str, pattern: &str) -> Option<u32> {
    if pattern.is_empty() {
        return Some(0);
    }
    if haystack.is_empty() {
        return None;
    }
    let pat = fuzzy_pattern(pattern);
    let mut m = matcher();
    let mut buf = Vec::new();
    pat.score(Utf32Str::new(haystack, &mut buf), &mut m)
}

/// Whether `pattern` fuzzy-matches `haystack` (ignore-case). Empty pattern matches all.
///
/// Multi-word queries (`guild viewmodel`) split on whitespace and AND each atom,
/// so they match CamelCase haystacks without a literal space.
pub fn fuzzy_match(haystack: &str, pattern: &str) -> bool {
    fuzzy_score(haystack, pattern).is_some()
}

/// Char indices (into `haystack`) of a fuzzy match; empty if no match / empty pattern.
/// Merges indices from all whitespace-separated atoms.
pub fn fuzzy_char_indices(haystack: &str, pattern: &str) -> Vec<u32> {
    if pattern.is_empty() || haystack.is_empty() {
        return Vec::new();
    }
    let pat = fuzzy_pattern(pattern);
    let mut m = matcher();
    let mut buf = Vec::new();
    let mut indices = Vec::new();
    let _ = pat.indices(Utf32Str::new(haystack, &mut buf), &mut m, &mut indices);
    indices.sort_unstable();
    indices.dedup();
    indices
}

/// Merge char indices into contiguous byte ranges within `haystack`.
pub fn char_indices_to_byte_ranges(haystack: &str, char_idxs: &[u32]) -> Vec<(usize, usize)> {
    if char_idxs.is_empty() {
        return Vec::new();
    }
    let char_byte: Vec<(usize, usize)> = haystack
        .char_indices()
        .map(|(b, c)| (b, b + c.len_utf8()))
        .collect();
    let mut ranges = Vec::new();
    let mut run_start: Option<(usize, usize)> = None; // (byte_start, last_char_i)
    for &ci in char_idxs {
        let Some(&(bs, be)) = char_byte.get(ci as usize) else {
            continue;
        };
        match run_start {
            None => run_start = Some((bs, ci as usize)),
            Some((start, last)) if ci as usize == last + 1 => {
                run_start = Some((start, ci as usize));
                let _ = be;
            }
            Some((start, last)) => {
                let end = char_byte.get(last).map(|r| r.1).unwrap_or(start);
                ranges.push((start, end));
                run_start = Some((bs, ci as usize));
            }
        }
    }
    if let Some((start, last)) = run_start {
        let end = char_byte.get(last).map(|r| r.1).unwrap_or(start);
        ranges.push((start, end));
    }
    ranges
}

/// Map fuzzy match positions on a Search/Highlight haystack onto tag/msg/raw fields.
pub fn map_search_positions(tag: &str, msg: &str, raw: &str, pattern: &str) -> Vec<FieldSpan> {
    let hay = search_haystack(tag, msg, raw);
    let idxs = fuzzy_char_indices(&hay, pattern);
    if idxs.is_empty() {
        return Vec::new();
    }
    if tag.is_empty() && msg.is_empty() {
        return char_indices_to_byte_ranges(&hay, &idxs)
            .into_iter()
            .map(|(start, end)| FieldSpan {
                field: PaintField::Raw,
                start,
                end,
            })
            .collect();
    }
    let tag_chars = tag.chars().count() as u32;
    let sep_chars = 1u32;
    let msg_start_char = tag_chars + sep_chars;
    let mut out = Vec::new();
    // Split indices by field, then merge to byte ranges within that field.
    let mut tag_idxs = Vec::new();
    let mut msg_idxs = Vec::new();
    for &ci in &idxs {
        if ci < tag_chars {
            tag_idxs.push(ci);
        } else if ci >= msg_start_char {
            msg_idxs.push(ci - msg_start_char);
        }
        // sep char: skip (do not paint)
    }
    for (start, end) in char_indices_to_byte_ranges(tag, &tag_idxs) {
        out.push(FieldSpan {
            field: PaintField::Tag,
            start,
            end,
        });
    }
    for (start, end) in char_indices_to_byte_ranges(msg, &msg_idxs) {
        out.push(FieldSpan {
            field: PaintField::Msg,
            start,
            end,
        });
    }
    out
}

/// Search/Highlight row match: fuzzy on tag+msg (or raw).
pub fn matches_search_row(row: &EntryRow, pattern: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }
    fuzzy_match(&search_haystack_row(row), pattern)
}

/// Filter/Exclude text chip: fuzzy on that field only; empty field → no match.
/// `pid`/`tid` exact; `level` is minimum level (same as CLI LevelGte).
pub fn chip_matches_row(chip: &Chip, row: &EntryRow) -> bool {
    match chip.field {
        ChipField::Tag => {
            if row.tag.is_empty() {
                return false;
            }
            fuzzy_match(&row.tag, &chip.value)
        }
        ChipField::Msg => {
            if row.msg.is_empty() {
                return false;
            }
            fuzzy_match(&row.msg, &chip.value)
        }
        ChipField::Pkg => {
            if row.pkg.is_empty() {
                return false;
            }
            fuzzy_match(&row.pkg, &chip.value)
        }
        ChipField::Pid => row.pid == chip.value,
        ChipField::Tid => row.tid == chip.value,
        ChipField::Level => Level::from_str(&chip.value)
            .map(|min| row.level >= min)
            .unwrap_or(false),
    }
}

/// How same-field chips combine inside a Group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SameFieldOp {
    /// Interactive chips: tag:A AND tag:B.
    #[default]
    And,
    /// Startup CLI multi-values: tag:A OR tag:B.
    Or,
}

/// Evaluate a chip list against a row (AND across fields; same-field per `op`).
pub fn chips_match_row(chips: &[Chip], row: &EntryRow, op: SameFieldOp) -> bool {
    if chips.is_empty() {
        return true;
    }
    match op {
        SameFieldOp::And => chips.iter().all(|c| chip_matches_row(c, row)),
        SameFieldOp::Or => {
            // Group by field; within field OR; across fields AND.
            let mut tag = Vec::new();
            let mut msg = Vec::new();
            let mut pkg = Vec::new();
            let mut pid = Vec::new();
            let mut tid = Vec::new();
            let mut level: Option<&Chip> = None;
            for c in chips {
                match c.field {
                    ChipField::Tag => tag.push(c),
                    ChipField::Msg => msg.push(c),
                    ChipField::Pkg => pkg.push(c),
                    ChipField::Pid => pid.push(c),
                    ChipField::Tid => tid.push(c),
                    ChipField::Level => level = Some(c),
                }
            }
            let field_ok =
                |cs: &[&Chip]| cs.is_empty() || cs.iter().any(|c| chip_matches_row(c, row));
            field_ok(&tag)
                && field_ok(&msg)
                && field_ok(&pkg)
                && field_ok(&pid)
                && field_ok(&tid)
                && level.map(|c| chip_matches_row(c, row)).unwrap_or(true)
        }
    }
}

/// Filter `&str` labels with the same semantics as [`fuzzy_label_indices`].
pub fn fuzzy_str_labels(labels: &[&str], query: &str) -> Vec<String> {
    let owned: Vec<String> = labels.iter().map(|s| (*s).to_string()).collect();
    fuzzy_label_indices(&owned, query)
        .into_iter()
        .map(|i| owned[i].clone())
        .collect()
}

/// Small-list fuzzy filter (Picker / MsgChip / Time dates). Empty query → all.
/// Returns source indices sorted by nucleo score (best first); stable for ties.
pub fn fuzzy_label_indices(labels: &[String], query: &str) -> Vec<usize> {
    if query.is_empty() {
        return (0..labels.len()).collect();
    }
    let mut matcher = matcher();
    let pattern = fuzzy_pattern(query);
    let mut scored: Vec<(usize, u32)> = labels
        .iter()
        .enumerate()
        .filter_map(|(i, label)| {
            let score =
                pattern.score(Utf32Str::new(label.as_str(), &mut Vec::new()), &mut matcher)?;
            Some((i, score as u32))
        })
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    scored.into_iter().map(|(i, _)| i).collect()
}

/// First fuzzy match byte range inside `text` (for preview line highlight).
pub fn first_match_byte_range(text: &str, pattern: &str) -> Option<(usize, usize)> {
    let idxs = fuzzy_char_indices(text, pattern);
    char_indices_to_byte_ranges(text, &idxs).into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_haystack_joins_or_raw() {
        assert_eq!(search_haystack("T", "m", "raw"), "T\tm");
        assert_eq!(search_haystack("", "", "raw line"), "raw line");
    }

    #[test]
    fn fuzzy_non_contiguous() {
        assert!(fuzzy_match("aXbYc", "abc"));
        assert!(fuzzy_match("Error", "err"));
        assert!(!fuzzy_match("hello", "xyz"));
    }

    #[test]
    fn fuzzy_multi_word_and_atoms() {
        // Whitespace splits atoms (AND); must not require a literal space in haystack.
        assert!(fuzzy_match("GuildFeedListViewModel", "guild viewmodel"));
        assert!(fuzzy_match("GuildFeedListViewModel", "guild feed view"));
        assert!(!fuzzy_match("GuildFeedListViewModel", "guild xyz"));
        // Picker path shares the same Pattern semantics.
        let labels = vec!["GuildFeedListViewModel".into(), "OtherThing".into()];
        assert_eq!(fuzzy_label_indices(&labels, "guild viewmodel"), vec![0]);
    }

    #[test]
    fn fuzzy_label_indices_empty_and_score() {
        let labels = vec!["Error".into(), "info".into(), "WARN".into()];
        assert_eq!(fuzzy_label_indices(&labels, ""), vec![0, 1, 2]);
        assert_eq!(fuzzy_label_indices(&labels, "err"), vec![0]);
        assert_eq!(fuzzy_label_indices(&labels, "WARN"), vec![2]);
        // non-contiguous
        let labels2 = vec!["aXbYc".into(), "zzz".into()];
        assert_eq!(fuzzy_label_indices(&labels2, "abc"), vec![0]);
    }

    #[test]
    fn chip_empty_field_no_match() {
        let row = EntryRow::from_line("04-02 10:00:00.000  1234  5678 I Tag   : hello").unwrap();
        let chip = Chip {
            field: ChipField::Pkg,
            value: "x".into(),
        };
        assert!(!chip_matches_row(&chip, &row));
        let tag = Chip {
            field: ChipField::Tag,
            value: "Tg".into(), // fuzzy T…g
        };
        assert!(chip_matches_row(&tag, &row));
    }

    #[test]
    fn map_positions_split_tag_msg() {
        let spans = map_search_positions("ab", "cd", "ab\tcd", "ac");
        assert!(spans.iter().any(|s| s.field == PaintField::Tag));
        assert!(spans.iter().any(|s| s.field == PaintField::Msg));
    }

    #[test]
    fn pid_exact_level_gte() {
        let row = EntryRow::from_line("04-02 10:00:00.000  1234  5678 E Tag   : hello").unwrap();
        assert!(chip_matches_row(
            &Chip {
                field: ChipField::Pid,
                value: "1234".into(),
            },
            &row
        ));
        assert!(!chip_matches_row(
            &Chip {
                field: ChipField::Pid,
                value: "12".into(),
            },
            &row
        ));
        assert!(chip_matches_row(
            &Chip {
                field: ChipField::Level,
                value: "W".into(),
            },
            &row
        ));
        assert!(!chip_matches_row(
            &Chip {
                field: ChipField::Level,
                value: "F".into(),
            },
            &row
        ));
    }
}
