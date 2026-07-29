use lru::LruCache;
use std::num::NonZeroUsize;

use crate::fuzzy;

const TAG_CAP: usize = 5_000;
const PKG_CAP: usize = 2_000;
const MSG_CAP: usize = 100_000;

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
        filter_sort(&self.tag_cache, query)
    }

    pub fn pkg_candidates(&self, query: &str) -> Vec<String> {
        filter_sort(&self.pkg_cache, query)
    }

    pub fn msg_candidates(&self, query: &str) -> Vec<String> {
        filter_sort(&self.msg_cache, query)
    }

    pub fn all_candidates(&self, query: &str) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut entries: Vec<(String, u32, u32)> = self
            .tag_cache
            .iter()
            .chain(self.pkg_cache.iter())
            .chain(self.msg_cache.iter())
            .filter_map(|(k, &freq)| {
                let score = if query.is_empty() {
                    Some(0)
                } else {
                    fuzzy::fuzzy_score(k, query)
                }?;
                if seen.insert(k.to_lowercase()) {
                    Some((k.clone(), score, freq))
                } else {
                    None
                }
            })
            .collect();
        sort_scored(&mut entries, query.is_empty());
        entries.into_iter().map(|(k, _, _)| k).collect()
    }
}

fn increment(cache: &mut LruCache<String, u32>, key: String) {
    if let Some(count) = cache.get_mut(&key) {
        *count += 1;
    } else {
        cache.put(key, 1);
    }
}

/// Empty query: frequency desc. Non-empty: nucleo score desc, then freq, then key.
fn filter_sort(cache: &LruCache<String, u32>, query: &str) -> Vec<String> {
    let mut entries: Vec<(String, u32, u32)> = cache
        .iter()
        .filter_map(|(k, &freq)| {
            let score = if query.is_empty() {
                Some(0)
            } else {
                fuzzy::fuzzy_score(k, query)
            }?;
            Some((k.clone(), score, freq))
        })
        .collect();
    sort_scored(&mut entries, query.is_empty());
    entries.into_iter().map(|(k, _, _)| k).collect()
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
}
