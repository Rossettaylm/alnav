//! TUI text-matching facade (nucleo-matcher + substring helpers).
//!
//! CLI `alnav grep` keeps its own FilterChain/Expr path. Interactive TUI
//! matching goes through this module:
//! - **Substring** (`substr_match`): Filter/Exclude/Search/Highlight vs log rows
//! - **Fuzzy** (`fuzzy_match` / `fuzzy_label_indices`): Picker/vocab/candidate lists

use alnav::parser::Level;
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::input::{Chip, ChipField};
use crate::model::EntryRow;

/// Hard cap on UI candidate-panel results (ResultCap SLO).
/// Empty and non-empty queries both truncate to this many rows after ranking.
pub const CANDIDATE_RESULT_CAP: usize = 256;

/// Separator between tag and msg in Search/Highlight haystacks.
pub const TAG_MSG_SEP: char = '\t';
const TAG_MSG_SEP_LEN: usize = 1; // '\t' is one byte

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

/// Reusable nucleo scorer for **one query × many haystacks**.
///
/// Batch paths (vocab filter, candidate lists) MUST use this instead of calling
/// [`fuzzy_score`] / recreating `Pattern`+`Matcher` per row — that rebuild cost
/// dominates at ~100k Msg vocab size.
///
/// Semantics match the one-shot helpers: empty query → `Some(0)` for every
/// haystack; empty haystack + non-empty query → `None`.
pub struct FuzzyScorer {
    pattern: Pattern,
    matcher: Matcher,
    buf: Vec<char>,
    empty_query: bool,
}

impl FuzzyScorer {
    /// Build a scorer for `query` (ignore-case, Smart normalization, fuzzy atoms).
    pub fn new(query: &str) -> Self {
        Self {
            pattern: fuzzy_pattern(query),
            matcher: matcher(),
            buf: Vec::new(),
            empty_query: query.is_empty(),
        }
    }

    /// Nucleo score for `haystack`, if it matches.
    pub fn score(&mut self, haystack: &str) -> Option<u32> {
        if self.empty_query {
            return Some(0);
        }
        if haystack.is_empty() {
            return None;
        }
        self.pattern
            .score(Utf32Str::new(haystack, &mut self.buf), &mut self.matcher)
    }

    /// Char indices of a fuzzy match; empty if no match / empty query.
    /// Merges indices from all whitespace-separated atoms.
    pub fn char_indices(&mut self, haystack: &str) -> Vec<u32> {
        if self.empty_query || haystack.is_empty() {
            return Vec::new();
        }
        let mut indices = Vec::new();
        let _ = self.pattern.indices(
            Utf32Str::new(haystack, &mut self.buf),
            &mut self.matcher,
            &mut indices,
        );
        indices.sort_unstable();
        indices.dedup();
        indices
    }
}

/// Nucleo score for `pattern` against `haystack`, if it matches.
/// Empty pattern scores `Some(0)` (match-all). Empty haystack with non-empty pattern → `None`.
///
/// For many haystacks with the same query, prefer [`FuzzyScorer`].
pub fn fuzzy_score(haystack: &str, pattern: &str) -> Option<u32> {
    FuzzyScorer::new(pattern).score(haystack)
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
///
/// For many haystacks with the same query, prefer [`FuzzyScorer::char_indices`].
pub fn fuzzy_char_indices(haystack: &str, pattern: &str) -> Vec<u32> {
    FuzzyScorer::new(pattern).char_indices(haystack)
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

/// Whitespace-separated atoms for Search/Highlight (contiguous substring AND).
fn substr_atoms(pattern: &str) -> impl Iterator<Item = &str> {
    pattern.split_whitespace().filter(|a| !a.is_empty())
}

/// Ignore-case **contiguous** substring match. Whitespace splits atoms that are ANDed.
/// Used for LogList Filter/Exclude/Search/Highlight — not fuzzy (avoids `0x1100`
/// matching scattered `0`/`x`/`1` digits, or `guild` matching `gu`…`i`…`ld`).
pub fn substr_match(haystack: &str, pattern: &str) -> bool {
    if pattern.trim().is_empty() {
        return true;
    }
    if haystack.is_empty() {
        return false;
    }
    let hay_l = haystack.to_lowercase();
    substr_atoms(pattern).all(|atom| hay_l.contains(&atom.to_lowercase()))
}

/// All ignore-case contiguous occurrences of `needle` in `haystack` as byte ranges.
pub fn find_all_ignore_case_ranges(haystack: &str, needle: &str) -> Vec<(usize, usize)> {
    if needle.is_empty() {
        return Vec::new();
    }
    let hay: Vec<(usize, char)> = haystack.char_indices().collect();
    let needle_l = needle.to_lowercase();
    let needle_len = needle_l.chars().count();
    if needle_len == 0 || hay.len() < needle_len {
        return Vec::new();
    }
    let mut out = Vec::new();
    for start in 0..=hay.len() - needle_len {
        let window: String = hay[start..start + needle_len]
            .iter()
            .map(|(_, c)| *c)
            .collect::<String>()
            .to_lowercase();
        if window == needle_l {
            let byte_start = hay[start].0;
            let byte_end = hay
                .get(start + needle_len)
                .map(|(i, _)| *i)
                .unwrap_or(haystack.len());
            out.push((byte_start, byte_end));
        }
    }
    out
}

/// Contiguous substring ranges for every whitespace atom in `pattern` (for paint).
pub fn substr_byte_ranges(haystack: &str, pattern: &str) -> Vec<(usize, usize)> {
    if !substr_match(haystack, pattern) {
        return Vec::new();
    }
    let mut ranges = Vec::new();
    for atom in substr_atoms(pattern) {
        ranges.extend(find_all_ignore_case_ranges(haystack, atom));
    }
    ranges.sort_unstable_by_key(|(s, _)| *s);
    ranges.dedup();
    ranges
}

fn map_hay_byte_range_to_fields(tag: &str, msg: &str, start: usize, end: usize) -> Vec<FieldSpan> {
    let tag_end = tag.len();
    let msg_start = tag_end + TAG_MSG_SEP_LEN;
    let mut out = Vec::new();
    if start < tag_end {
        let e = end.min(tag_end);
        if e > start {
            out.push(FieldSpan {
                field: PaintField::Tag,
                start,
                end: e,
            });
        }
    }
    if end > msg_start {
        let s = if start >= msg_start {
            start - msg_start
        } else {
            0
        };
        let e = end - msg_start;
        if e > s {
            out.push(FieldSpan {
                field: PaintField::Msg,
                start: s.min(msg.len()),
                end: e.min(msg.len()),
            });
        }
    }
    out
}

/// Map Search/Highlight **substring** hits onto tag/msg/raw fields for LogList paint.
pub fn map_search_positions(tag: &str, msg: &str, raw: &str, pattern: &str) -> Vec<FieldSpan> {
    let hay = search_haystack(tag, msg, raw);
    let ranges = substr_byte_ranges(&hay, pattern);
    if ranges.is_empty() {
        return Vec::new();
    }
    if tag.is_empty() && msg.is_empty() {
        return ranges
            .into_iter()
            .map(|(start, end)| FieldSpan {
                field: PaintField::Raw,
                start,
                end,
            })
            .collect();
    }
    let mut out = Vec::new();
    for (start, end) in ranges {
        out.extend(map_hay_byte_range_to_fields(tag, msg, start, end));
    }
    out
}

/// Search/Highlight row match: contiguous substring on tag+msg (or raw), not fuzzy.
pub fn matches_search_row(row: &EntryRow, pattern: &str) -> bool {
    if pattern.trim().is_empty() {
        return false;
    }
    substr_match(&search_haystack_row(row), pattern)
}

/// Filter/Exclude text chip: **contiguous substring** on that field only;
/// empty field → no match. `pid`/`tid` exact; `level` is minimum level
/// (same as CLI LevelGte). Fuzzy stays for Picker/vocab candidate lists only.
pub fn chip_matches_row(chip: &Chip, row: &EntryRow) -> bool {
    match chip.field {
        ChipField::Tag => {
            if row.tag.is_empty() {
                return false;
            }
            substr_match(&row.tag, &chip.value)
        }
        ChipField::Msg => {
            if row.msg.is_empty() {
                return false;
            }
            substr_match(&row.msg, &chip.value)
        }
        ChipField::Pkg => {
            if row.pkg.is_empty() {
                return false;
            }
            substr_match(&row.pkg, &chip.value)
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

/// Candidate-list fuzzy filter (Picker / MsgChip / Time dates).
/// Empty query → stable source order, truncated to [`CANDIDATE_RESULT_CAP`].
/// Non-empty → nucleo score order (best first), then truncate.
pub fn fuzzy_label_indices(labels: &[String], query: &str) -> Vec<usize> {
    if query.is_empty() {
        return (0..labels.len().min(CANDIDATE_RESULT_CAP)).collect();
    }
    let mut scorer = FuzzyScorer::new(query);
    let mut scored: Vec<(usize, u32)> = labels
        .iter()
        .enumerate()
        .filter_map(|(i, label)| scorer.score(label).map(|score| (i, score)))
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    scored.truncate(CANDIDATE_RESULT_CAP);
    scored.into_iter().map(|(i, _)| i).collect()
}

/// First Search/Highlight substring match byte range (for preview line highlight).
pub fn first_match_byte_range(text: &str, pattern: &str) -> Option<(usize, usize)> {
    substr_byte_ranges(text, pattern).into_iter().next()
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
    fn fuzzy_scorer_matches_one_shot_helpers() {
        let haystacks = ["aXbYc", "Error", "hello", "GuildFeedListViewModel", ""];
        for q in ["", "abc", "err", "xyz", "guild viewmodel"] {
            let mut scorer = FuzzyScorer::new(q);
            for h in haystacks {
                assert_eq!(
                    scorer.score(h),
                    fuzzy_score(h, q),
                    "score mismatch q={q:?} h={h:?}"
                );
                assert_eq!(
                    scorer.char_indices(h),
                    fuzzy_char_indices(h, q),
                    "indices mismatch q={q:?} h={h:?}"
                );
            }
        }
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
    fn fuzzy_label_indices_respects_result_cap() {
        let labels: Vec<String> = (0..CANDIDATE_RESULT_CAP + 50)
            .map(|i| format!("item{i:04}"))
            .collect();
        assert_eq!(fuzzy_label_indices(&labels, "").len(), CANDIDATE_RESULT_CAP);
        assert!(fuzzy_label_indices(&labels, "item").len() <= CANDIDATE_RESULT_CAP);
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
            value: "Tag".into(),
        };
        assert!(chip_matches_row(&tag, &row));
        // Non-contiguous must not match (was fuzzy before).
        let gap = Chip {
            field: ChipField::Tag,
            value: "Tg".into(),
        };
        assert!(!chip_matches_row(&gap, &row));
    }

    #[test]
    fn filter_chip_hex_is_contiguous_not_fuzzy() {
        let line = "2026-07-22 16:43:02.809|1[14107]14321|14107|I|NTKernel|[I] sys_msg_mgr.cc(526)::NotifyRecvSysMsg [SysMsgMgr]->notify recv sys msg: msg_type=0xf01, sub_type=0x6d, is_guild_msg=1, is_online_msg=1 msg_id=0 chat_type=0 peer_uid= seq=0 random=0 time=0";
        let row = EntryRow::from_line(line).unwrap();
        let chips = vec![
            Chip {
                field: ChipField::Tag,
                value: "NTkernel".into(),
            },
            Chip {
                field: ChipField::Msg,
                value: "0x1100".into(),
            },
        ];
        // Fuzzy would stitch 0x + scattered 1/1/0/0; substring must reject.
        assert!(fuzzy_match(&row.msg, "0x1100"));
        assert!(!chips_match_row(&chips, &row, SameFieldOp::And));
        assert!(chips_match_row(
            &[
                Chip {
                    field: ChipField::Tag,
                    value: "NTKernel".into(),
                },
                Chip {
                    field: ChipField::Msg,
                    value: "0x6d".into(),
                },
            ],
            &row,
            SameFieldOp::And
        ));
    }

    #[test]
    fn map_positions_substring_on_fields() {
        let spans = map_search_positions("MyTag", "hello guild world", "", "guild");
        assert!(spans.iter().any(|s| {
            s.field == PaintField::Msg && &"hello guild world"[s.start..s.end] == "guild"
        }));
        // Fuzzy-style gaps must not paint / match for Search/Highlight.
        assert!(map_search_positions("T", "gXuXiXlXd", "", "guild").is_empty());
    }

    #[test]
    fn search_highlight_is_contiguous_not_fuzzy() {
        let row = EntryRow::from_line("04-02 10:00:00.000  1234  5678 I Tag   : gu i ld elsewhere")
            .unwrap();
        assert!(!matches_search_row(&row, "guild"));
        let row2 =
            EntryRow::from_line("04-02 10:00:00.000  1234  5678 I Tag   : hello guild world")
                .unwrap();
        assert!(matches_search_row(&row2, "guild"));
        assert!(matches_search_row(&row2, "hello world")); // AND substrings
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
