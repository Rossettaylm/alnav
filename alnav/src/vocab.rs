use lru::LruCache;
use std::collections::HashSet;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::fuzzy;

const TAG_CAP: usize = 5_000;
const PKG_CAP: usize = 2_000;
const MSG_CAP: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CandidateScope {
    #[default]
    All,
    Tag,
    Pkg,
    Msg,
}

pub struct Vocab {
    pub tag_cache: LruCache<String, u32>,
    pub pkg_cache: LruCache<String, u32>,
    pub msg_cache: LruCache<String, u32>,
}

impl Default for Vocab {
    fn default() -> Self {
        Self {
            tag_cache: LruCache::new(NonZeroUsize::new(TAG_CAP).unwrap()),
            pkg_cache: LruCache::new(NonZeroUsize::new(PKG_CAP).unwrap()),
            msg_cache: LruCache::new(NonZeroUsize::new(MSG_CAP).unwrap()),
        }
    }
}

impl Vocab {
    pub fn feed(&mut self, tag: &str, pkg: &str, msg_tokens: &[String]) {
        if !tag.is_empty() {
            increment(&mut self.tag_cache, tag.to_string());
        }
        if !pkg.is_empty() {
            increment(&mut self.pkg_cache, pkg.to_string());
        }
        for token in msg_tokens {
            increment(&mut self.msg_cache, token.clone());
        }
    }

    pub fn tag_candidates(&self, query: &str) -> Vec<String> {
        self.candidates(CandidateScope::Tag, query)
    }

    pub fn pkg_candidates(&self, query: &str) -> Vec<String> {
        self.candidates(CandidateScope::Pkg, query)
    }

    pub fn msg_candidates(&self, query: &str) -> Vec<String> {
        self.candidates(CandidateScope::Msg, query)
    }

    pub fn all_candidates(&self, query: &str) -> Vec<String> {
        self.candidates(CandidateScope::All, query)
    }

    /// Sync filter for a scope (tests + empty-query fast path).
    pub fn candidates(&self, scope: CandidateScope, query: &str) -> Vec<String> {
        let entries = self.snapshot(scope);
        let cancel = AtomicBool::new(false);
        match scope {
            CandidateScope::All => filter_all_entries(&entries, query, &cancel, usize::MAX),
            CandidateScope::Tag | CandidateScope::Pkg | CandidateScope::Msg => {
                filter_sort_entries(&entries, query, &cancel, usize::MAX)
            }
        }
    }

    /// Owned `(key, freq)` snapshot for background matching.
    pub fn snapshot(&self, scope: CandidateScope) -> Vec<(String, u32)> {
        match scope {
            CandidateScope::Tag => cache_entries(&self.tag_cache),
            CandidateScope::Pkg => cache_entries(&self.pkg_cache),
            CandidateScope::Msg => cache_entries(&self.msg_cache),
            CandidateScope::All => {
                let mut out = Vec::with_capacity(
                    self.tag_cache.len() + self.pkg_cache.len() + self.msg_cache.len(),
                );
                out.extend(cache_entries(&self.tag_cache));
                out.extend(cache_entries(&self.pkg_cache));
                out.extend(cache_entries(&self.msg_cache));
                out
            }
        }
    }
}

fn cache_entries(cache: &LruCache<String, u32>) -> Vec<(String, u32)> {
    cache.iter().map(|(k, &f)| (k.clone(), f)).collect()
}

fn increment(cache: &mut LruCache<String, u32>, key: String) {
    if let Some(count) = cache.get_mut(&key) {
        *count += 1;
    } else {
        cache.put(key, 1);
    }
}

/// Empty query: frequency desc. Non-empty: nucleo score desc, then freq, then key.
/// Checks `cancel` every `check_every` entries (use `usize::MAX` to disable).
///
/// Non-empty queries score via [`fuzzy::FuzzyScorer`] (one Pattern/Matcher per call).
pub fn filter_sort_entries(
    entries: &[(String, u32)],
    query: &str,
    cancel: &AtomicBool,
    check_every: usize,
) -> Vec<String> {
    let check_every = check_every.max(1);
    let empty_query = query.is_empty();
    let mut scorer = fuzzy::FuzzyScorer::new(query);
    let mut scored: Vec<(String, u32, u32)> = Vec::new();
    for (i, (k, freq)) in entries.iter().enumerate() {
        if i % check_every == 0 && cancel.load(Ordering::Acquire) {
            return Vec::new();
        }
        let Some(score) = scorer.score(k) else {
            continue;
        };
        scored.push((k.clone(), score, *freq));
    }
    if cancel.load(Ordering::Acquire) {
        return Vec::new();
    }
    sort_scored(&mut scored, empty_query);
    scored.truncate(fuzzy::CANDIDATE_RESULT_CAP);
    scored.into_iter().map(|(k, _, _)| k).collect()
}

/// Like [`filter_sort_entries`] but dedupes by lowercase key (Highlight `all_candidates`).
pub fn filter_all_entries(
    entries: &[(String, u32)],
    query: &str,
    cancel: &AtomicBool,
    check_every: usize,
) -> Vec<String> {
    let check_every = check_every.max(1);
    let empty_query = query.is_empty();
    let mut scorer = fuzzy::FuzzyScorer::new(query);
    let mut seen = HashSet::new();
    let mut scored: Vec<(String, u32, u32)> = Vec::new();
    for (i, (k, freq)) in entries.iter().enumerate() {
        if i % check_every == 0 && cancel.load(Ordering::Acquire) {
            return Vec::new();
        }
        let Some(score) = scorer.score(k) else {
            continue;
        };
        if seen.insert(k.to_lowercase()) {
            scored.push((k.clone(), score, *freq));
        }
    }
    if cancel.load(Ordering::Acquire) {
        return Vec::new();
    }
    sort_scored(&mut scored, empty_query);
    scored.truncate(fuzzy::CANDIDATE_RESULT_CAP);
    scored.into_iter().map(|(k, _, _)| k).collect()
}

fn sort_scored(entries: &mut [(String, u32, u32)], empty_query: bool) {
    if empty_query {
        entries.sort_unstable_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
    } else {
        entries.sort_unstable_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| b.2.cmp(&a.2))
                .then_with(|| a.0.cmp(&b.0))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feed_increments_tag_freq() {
        let mut v = Vocab::default();
        v.feed("MyTag", "com.example", &[]);
        v.feed("MyTag", "com.example", &[]);
        let cands = v.tag_candidates("");
        assert_eq!(cands.first().map(|s| s.as_str()), Some("MyTag"));
    }

    #[test]
    fn candidates_sorted_by_freq_desc() {
        let mut v = Vocab::default();
        v.feed("Rare", "", &[]);
        v.feed("Common", "", &[]);
        v.feed("Common", "", &[]);
        v.feed("Common", "", &[]);
        let cands = v.tag_candidates("");
        assert_eq!(cands[0], "Common");
        assert_eq!(cands[1], "Rare");
    }

    #[test]
    fn candidates_filtered_by_query() {
        let mut v = Vocab::default();
        v.feed("MyApp", "", &[]);
        v.feed("OtherTag", "", &[]);
        let cands = v.tag_candidates("app");
        assert_eq!(cands, vec!["MyApp".to_string()]);
    }

    #[test]
    fn candidates_multi_word_fuzzy() {
        let mut v = Vocab::default();
        v.feed("GuildFeedListViewModel", "", &[]);
        v.feed("OtherThing", "", &[]);
        let cands = v.tag_candidates("guild viewmodel");
        assert_eq!(cands, vec!["GuildFeedListViewModel".to_string()]);
        let all = v.all_candidates("guild viewmodel");
        assert_eq!(all, vec!["GuildFeedListViewModel".to_string()]);
    }

    #[test]
    fn lru_evicts_when_full() {
        let mut tag_cache: LruCache<String, u32> = LruCache::new(NonZeroUsize::new(2).unwrap());
        increment(&mut tag_cache, "A".into());
        increment(&mut tag_cache, "B".into());
        increment(&mut tag_cache, "C".into());
        assert!(tag_cache.peek("A").is_none());
        assert!(tag_cache.peek("B").is_some());
        assert!(tag_cache.peek("C").is_some());
    }

    #[test]
    fn all_candidates_merges_deduped() {
        let mut v = Vocab::default();
        v.feed("", "", &["hello".into(), "world".into()]);
        v.feed("hello", "", &[]);
        let cands = v.all_candidates("hello");
        let count = cands.iter().filter(|s| s.to_lowercase() == "hello").count();
        assert_eq!(count, 1, "dedup failed: {:?}", cands);
    }

    #[test]
    fn pkg_candidates_filtered() {
        let mut v = Vocab::default();
        v.feed("", "com.tencent.mobileqq", &[]);
        v.feed("", "com.example.other", &[]);
        let cands = v.pkg_candidates("qq");
        assert_eq!(cands, vec!["com.tencent.mobileqq".to_string()]);
    }

    #[test]
    fn filter_sort_entries_respects_cancel() {
        let entries: Vec<(String, u32)> = (0..1000).map(|i| (format!("k{i}"), 1)).collect();
        let cancel = AtomicBool::new(true);
        let out = filter_sort_entries(&entries, "k", &cancel, 1);
        assert!(out.is_empty());
    }

    #[test]
    fn filter_sort_respects_result_cap() {
        use crate::fuzzy::CANDIDATE_RESULT_CAP;
        let entries: Vec<(String, u32)> = (0..5_000).map(|i| (format!("tok_{i:05}"), 1)).collect();
        let cancel = AtomicBool::new(false);
        let out = filter_sort_entries(&entries, "tok", &cancel, 4096);
        assert_eq!(out.len(), CANDIDATE_RESULT_CAP);
        let empty = filter_sort_entries(&entries, "", &cancel, 4096);
        assert_eq!(empty.len(), CANDIDATE_RESULT_CAP);
    }

    #[test]
    fn filter_sort_large_msg_vocab_uses_scorer_path() {
        // Smoke: 100k entries (MSG_CAP) + short query finishes and stays capped.
        // Regression guard for "rebuild Pattern per row" which made this path laggy.
        let entries: Vec<(String, u32)> =
            (0..100_000).map(|i| (format!("tok_{i:05}"), 1)).collect();
        let cancel = AtomicBool::new(false);
        let start = std::time::Instant::now();
        let out = filter_sort_entries(&entries, "tok_99", &cancel, 4096);
        let elapsed = start.elapsed();
        assert!(!out.is_empty());
        assert!(out.len() <= fuzzy::CANDIDATE_RESULT_CAP);
        // Generous CI bound (debug). Release is typically well under this.
        assert!(
            elapsed.as_millis() < 2_000,
            "100k msg filter took {elapsed:?}; FuzzyScorer reuse likely broken"
        );
    }
}
