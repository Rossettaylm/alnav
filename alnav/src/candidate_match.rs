//! Background fuzzy filter for Picker New vocab candidates.
//!
//! Keystroke updates only bump generation + spawn a worker; the UI reads
//! [`CandidateMatchService::display_labels`] (stale-while-revalidate). A newer
//! request cancels the previous worker via [`AtomicBool`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::vocab::{self, CandidateScope, Vocab};

const CANCEL_CHECK_EVERY: usize = 4096;

#[derive(Debug, Clone)]
struct MatchResult {
    gen: u64,
    scope: CandidateScope,
    query: String,
    labels: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub struct CandidateMatchCache {
    pub scope: CandidateScope,
    pub query: String,
    pub labels: Vec<String>,
}

pub struct CandidateMatchService {
    gen: u64,
    cancel: Arc<AtomicBool>,
    tx: Sender<MatchResult>,
    rx: Receiver<MatchResult>,
    /// Generation of the in-flight job (`None` = idle).
    pending_gen: Option<u64>,
    desired_scope: Option<CandidateScope>,
    desired_query: String,
    cache: CandidateMatchCache,
}

impl Default for CandidateMatchService {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            gen: 0,
            cancel: Arc::new(AtomicBool::new(false)),
            tx,
            rx,
            pending_gen: None,
            desired_scope: None,
            desired_query: String::new(),
            cache: CandidateMatchCache::default(),
        }
    }
}

impl CandidateMatchService {
    pub fn pending(&self) -> bool {
        self.pending_gen.is_some()
    }

    pub fn cache(&self) -> &CandidateMatchCache {
        &self.cache
    }

    /// Best-effort labels for UI (exact hit or previous list while matching).
    pub fn display_labels(&self) -> &[String] {
        &self.cache.labels
    }

    /// Whether cache exactly matches the desired `(scope, query)`.
    pub fn cache_matches(&self, scope: CandidateScope, query: &str) -> bool {
        self.cache.scope == scope && self.cache.query == query
    }

    /// Cancel in-flight work and clear desired/cache (picker closed).
    pub fn clear(&mut self) {
        self.cancel.store(true, Ordering::Release);
        self.cancel = Arc::new(AtomicBool::new(false));
        self.pending_gen = None;
        self.desired_scope = None;
        self.desired_query.clear();
        self.cache = CandidateMatchCache::default();
        while self.rx.try_recv().is_ok() {}
    }

    /// Request a match for `scope`/`query`. No-ops if already desired+done/in-flight.
    pub fn request(&mut self, vocab: &Vocab, scope: CandidateScope, query: &str) {
        if self.desired_scope == Some(scope) && self.desired_query == query {
            if self.pending_gen.is_some() || self.cache_matches(scope, query) {
                return;
            }
        }

        self.cancel.store(true, Ordering::Release);
        self.cancel = Arc::new(AtomicBool::new(false));
        while self.rx.try_recv().is_ok() {}

        self.gen = self.gen.wrapping_add(1);
        let gen = self.gen;
        self.desired_scope = Some(scope);
        self.desired_query = query.to_string();
        self.pending_gen = Some(gen);

        // Scope change: drop stale labels so we don't show Tag hits under Msg.
        if self.cache.scope != scope {
            self.cache = CandidateMatchCache {
                scope,
                query: String::new(),
                labels: Vec::new(),
            };
        }

        // Empty query: freq sort only — cheap; apply synchronously to avoid flicker.
        if query.is_empty() {
            let labels = vocab.candidates(scope, "");
            self.cache = CandidateMatchCache {
                scope,
                query: String::new(),
                labels,
            };
            self.pending_gen = None;
            return;
        }

        let entries = vocab.snapshot(scope);
        let cancel = Arc::clone(&self.cancel);
        let tx = self.tx.clone();
        let query = query.to_string();
        thread::spawn(move || {
            let labels = match_entries(&entries, scope, &query, &cancel);
            if cancel.load(Ordering::Acquire) {
                return;
            }
            let _ = tx.send(MatchResult {
                gen,
                scope,
                query,
                labels,
            });
        });
    }

    /// Apply any finished results (non-blocking).
    pub fn poll(&mut self) {
        while let Ok(result) = self.rx.try_recv() {
            if result.gen != self.gen {
                continue;
            }
            self.cache = CandidateMatchCache {
                scope: result.scope,
                query: result.query,
                labels: result.labels,
            };
            if self.pending_gen == Some(result.gen) {
                self.pending_gen = None;
            }
        }
    }

    /// Block until the current request finishes or timeout (tests).
    pub fn flush(&mut self, timeout: Duration) {
        let deadline = std::time::Instant::now() + timeout;
        while self.pending_gen.is_some() {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match self.rx.recv_timeout(remaining) {
                Ok(result) => {
                    if result.gen != self.gen {
                        continue;
                    }
                    self.cache = CandidateMatchCache {
                        scope: result.scope,
                        query: result.query,
                        labels: result.labels,
                    };
                    self.pending_gen = None;
                }
                Err(_) => break,
            }
        }
    }
}

fn match_entries(
    entries: &[(String, u32)],
    scope: CandidateScope,
    query: &str,
    cancel: &AtomicBool,
) -> Vec<String> {
    match scope {
        CandidateScope::All => {
            vocab::filter_all_entries(entries, query, cancel, CANCEL_CHECK_EVERY)
        }
        CandidateScope::Tag | CandidateScope::Pkg | CandidateScope::Msg => {
            vocab::filter_sort_entries(entries, query, cancel, CANCEL_CHECK_EVERY)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_vocab() -> Vocab {
        let mut v = Vocab::default();
        v.feed("MyApp", "com.example", &["hello".into(), "world".into()]);
        v.feed("OtherTag", "", &[]);
        v
    }

    #[test]
    fn empty_query_applies_synchronously() {
        let mut svc = CandidateMatchService::default();
        let v = feed_vocab();
        svc.request(&v, CandidateScope::Tag, "");
        assert!(!svc.pending());
        assert!(svc.display_labels().iter().any(|l| l == "MyApp"));
    }

    #[test]
    fn non_empty_async_and_flush() {
        let mut svc = CandidateMatchService::default();
        let v = feed_vocab();
        svc.request(&v, CandidateScope::Tag, "app");
        svc.flush(Duration::from_secs(2));
        assert!(!svc.pending());
        assert!(svc.cache_matches(CandidateScope::Tag, "app"));
        assert_eq!(svc.display_labels(), &["MyApp".to_string()]);
    }

    #[test]
    fn newer_request_discards_stale_gen() {
        let mut svc = CandidateMatchService::default();
        let mut v = Vocab::default();
        for i in 0..20_000 {
            v.feed(&format!("Tag{i:05}"), "", &[]);
        }
        v.feed("TargetApp", "", &[]);

        svc.request(&v, CandidateScope::Tag, "Tag");
        svc.request(&v, CandidateScope::Tag, "TargetApp");
        svc.flush(Duration::from_secs(5));
        assert!(!svc.pending());
        assert!(svc.cache_matches(CandidateScope::Tag, "TargetApp"));
        assert_eq!(svc.display_labels(), &["TargetApp".to_string()]);
    }

    #[test]
    fn parity_with_sync_tag_candidates() {
        let mut svc = CandidateMatchService::default();
        let v = feed_vocab();
        let sync = v.tag_candidates("app");
        svc.request(&v, CandidateScope::Tag, "app");
        svc.flush(Duration::from_secs(2));
        assert_eq!(svc.display_labels(), sync.as_slice());
    }

    #[test]
    fn parity_with_sync_all_candidates() {
        let mut svc = CandidateMatchService::default();
        let v = feed_vocab();
        let sync = v.all_candidates("hello");
        svc.request(&v, CandidateScope::All, "hello");
        svc.flush(Duration::from_secs(2));
        assert_eq!(svc.display_labels(), sync.as_slice());
    }

    #[test]
    fn clear_cancels_and_empties_cache() {
        let mut svc = CandidateMatchService::default();
        let v = feed_vocab();
        svc.request(&v, CandidateScope::Tag, "");
        assert!(!svc.cache().labels.is_empty());
        svc.clear();
        assert!(svc.cache().labels.is_empty());
        assert!(!svc.pending());
    }
}
