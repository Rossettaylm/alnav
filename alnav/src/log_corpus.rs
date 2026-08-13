//! Async filesystem corpus for Open-file fuzzy search (`log_dirs`).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::thread;

const BATCH_SIZE: usize = 256;

/// One indexed log file in the Open-file corpus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusEntry {
    pub abs: PathBuf,
    /// Display / nucleo haystack: `{root_leaf}/{relative}`.
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorpusPhase {
    Idle,
    Scanning,
    Ready,
}

#[derive(Debug)]
enum ScanMsg {
    Batch { gen: u64, entries: Vec<CorpusEntry> },
    Done { gen: u64 },
}

/// Process-local corpus: first open scans async; later opens reuse until refresh.
#[derive(Debug)]
pub struct LogCorpus {
    roots: Vec<String>,
    extensions: Vec<String>,
    entries: Vec<CorpusEntry>,
    phase: CorpusPhase,
    gen: u64,
    cancel_gen: Arc<AtomicU64>,
    rx: Option<Receiver<ScanMsg>>,
    dirty: bool,
}

impl Default for LogCorpus {
    fn default() -> Self {
        Self::new()
    }
}

impl LogCorpus {
    pub fn new() -> Self {
        Self {
            roots: Vec::new(),
            extensions: crate::config::default_log_extensions(),
            entries: Vec::new(),
            phase: CorpusPhase::Idle,
            gen: 0,
            cancel_gen: Arc::new(AtomicU64::new(0)),
            rx: None,
            dirty: false,
        }
    }

    pub fn configure(&mut self, roots: Vec<String>, extensions: Vec<String>) {
        let roots_changed = self.roots != roots;
        let ext_changed = self.extensions != extensions;
        self.roots = roots;
        self.extensions = if extensions.is_empty() {
            crate::config::default_log_extensions()
        } else {
            extensions
        };
        if roots_changed || ext_changed {
            self.invalidate();
        }
    }

    pub fn roots_empty(&self) -> bool {
        self.roots.is_empty()
    }

    pub fn entries(&self) -> &[CorpusEntry] {
        &self.entries
    }

    pub fn phase(&self) -> CorpusPhase {
        self.phase
    }

    pub fn found_count(&self) -> usize {
        self.entries.len()
    }

    pub fn is_scanning(&self) -> bool {
        self.phase == CorpusPhase::Scanning
    }

    /// True if [`poll`] observed new batches / completion since last take.
    pub fn take_dirty(&mut self) -> bool {
        let d = self.dirty;
        self.dirty = false;
        d
    }

    pub fn status_label(&self) -> Option<String> {
        match self.phase {
            CorpusPhase::Scanning => Some(format!("scanning… {}", self.entries.len())),
            CorpusPhase::Ready if !self.entries.is_empty() => {
                Some(format!("{} files", self.entries.len()))
            }
            _ => None,
        }
    }

    /// Start a scan when Idle and roots are configured. No-op if Ready/Scanning.
    pub fn ensure_started(&mut self) {
        if self.roots.is_empty() {
            self.phase = CorpusPhase::Ready;
            return;
        }
        if matches!(self.phase, CorpusPhase::Ready | CorpusPhase::Scanning) {
            return;
        }
        self.start_scan(true);
    }

    /// Drop cache and rescan (Ctrl-r).
    pub fn refresh(&mut self) {
        if self.roots.is_empty() {
            self.entries.clear();
            self.phase = CorpusPhase::Ready;
            self.rx = None;
            self.dirty = true;
            return;
        }
        self.start_scan(true);
    }

    /// Cancel in-flight walk; keep entries already received as Ready.
    pub fn cancel_inflight(&mut self) {
        if self.phase != CorpusPhase::Scanning {
            return;
        }
        self.gen = self.gen.wrapping_add(1);
        self.cancel_gen.store(self.gen, Ordering::Relaxed);
        self.rx = None;
        self.phase = CorpusPhase::Ready;
        self.dirty = true;
    }

    pub fn poll(&mut self) {
        let Some(rx) = &self.rx else {
            return;
        };
        loop {
            match rx.try_recv() {
                Ok(ScanMsg::Batch { gen, entries }) => {
                    if gen != self.gen {
                        continue;
                    }
                    self.entries.extend(entries);
                    self.dirty = true;
                }
                Ok(ScanMsg::Done { gen }) => {
                    if gen != self.gen {
                        continue;
                    }
                    self.phase = CorpusPhase::Ready;
                    self.rx = None;
                    self.dirty = true;
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if self.phase == CorpusPhase::Scanning {
                        self.phase = CorpusPhase::Ready;
                        self.dirty = true;
                    }
                    self.rx = None;
                    break;
                }
            }
        }
    }

    fn invalidate(&mut self) {
        self.gen = self.gen.wrapping_add(1);
        self.cancel_gen.store(self.gen, Ordering::Relaxed);
        self.rx = None;
        self.entries.clear();
        self.phase = CorpusPhase::Idle;
        self.dirty = true;
    }

    fn start_scan(&mut self, clear: bool) {
        self.gen = self.gen.wrapping_add(1);
        let gen = self.gen;
        self.cancel_gen.store(gen, Ordering::Relaxed);
        if clear {
            self.entries.clear();
        }
        self.phase = CorpusPhase::Scanning;
        self.dirty = true;

        let roots = self.roots.clone();
        let extensions = self.extensions.clone();
        let cancel = Arc::clone(&self.cancel_gen);
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);

        thread::spawn(move || {
            for root_raw in &roots {
                if cancel.load(Ordering::Relaxed) != gen {
                    return;
                }
                let root = expand_user(root_raw);
                let Ok(meta) = fs::symlink_metadata(&root) else {
                    continue;
                };
                if meta.file_type().is_symlink() || !meta.is_dir() {
                    continue;
                }
                let leaf = root
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("root")
                    .to_string();
                walk_root(&root, &leaf, &extensions, gen, &cancel, &tx);
            }
            let _ = tx.send(ScanMsg::Done { gen });
        });
    }
}

/// Expand leading `~` / `~/` like a shell home prefix.
pub fn expand_user(input: &str) -> PathBuf {
    if input == "~" {
        return home_dir().unwrap_or_else(|| PathBuf::from("~"));
    }
    if let Some(rest) = input.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(input)
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn suffix_matches(name: &str, extensions: &[String]) -> bool {
    let lower = name.to_ascii_lowercase();
    extensions.iter().any(|ext| lower.ends_with(ext.as_str()))
}

fn walk_root(
    root: &Path,
    leaf: &str,
    extensions: &[String],
    gen: u64,
    cancel: &AtomicU64,
    tx: &mpsc::Sender<ScanMsg>,
) {
    let mut stack: Vec<(PathBuf, String)> = vec![(root.to_path_buf(), String::new())];
    let mut batch = Vec::with_capacity(BATCH_SIZE);

    while let Some((dir, rel)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) != gen {
            return;
        }
        let Ok(read) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            if cancel.load(Ordering::Relaxed) != gen {
                return;
            }
            let name_os = entry.file_name();
            let Some(name) = name_os.to_str() else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            let ft = meta.file_type();
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                let child_rel = if rel.is_empty() {
                    name.to_string()
                } else {
                    format!("{rel}/{name}")
                };
                stack.push((path, child_rel));
                continue;
            }
            if !ft.is_file() || !suffix_matches(name, extensions) {
                continue;
            }
            let label = if rel.is_empty() {
                format!("{leaf}/{name}")
            } else {
                format!("{leaf}/{rel}/{name}")
            };
            batch.push(CorpusEntry { abs: path, label });
            if batch.len() >= BATCH_SIZE {
                let entries = std::mem::take(&mut batch);
                if tx.send(ScanMsg::Batch { gen, entries }).is_err() {
                    return;
                }
            }
        }
    }

    if !batch.is_empty()
        && tx
            .send(ScanMsg::Batch {
                gen,
                entries: batch,
            })
            .is_err()
    {
        return;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::time::{Duration, Instant};

    fn wait_ready(corpus: &mut LogCorpus, timeout: Duration) {
        let start = Instant::now();
        while corpus.is_scanning() && start.elapsed() < timeout {
            corpus.poll();
            thread::sleep(Duration::from_millis(5));
        }
        corpus.poll();
    }

    #[test]
    fn expand_user_home_prefix() {
        if env::var_os("HOME").is_none() {
            return;
        }
        let p = expand_user("~/logs/a.log");
        assert!(p.is_absolute());
        assert!(p.ends_with(Path::new("logs").join("a.log")));
    }

    #[test]
    fn scan_filters_suffix_skips_dot_and_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("bugly");
        fs::create_dir_all(root.join("nested")).unwrap();
        fs::write(root.join("keep.log"), "a").unwrap();
        fs::write(root.join("skip.bin"), "b").unwrap();
        fs::write(root.join("nested").join("deep.txt"), "c").unwrap();
        fs::write(root.join(".hidden.log"), "h").unwrap();
        fs::create_dir(root.join(".git")).unwrap();
        fs::write(root.join(".git").join("x.log"), "g").unwrap();
        let link = root.join("link.log");
        let _ = symlink(root.join("keep.log"), &link);

        let mut corpus = LogCorpus::new();
        corpus.configure(
            vec![root.display().to_string()],
            vec![".log".into(), ".txt".into()],
        );
        corpus.ensure_started();
        wait_ready(&mut corpus, Duration::from_secs(2));
        assert_eq!(corpus.phase(), CorpusPhase::Ready);

        let labels: Vec<_> = corpus.entries().iter().map(|e| e.label.as_str()).collect();
        assert!(labels.contains(&"bugly/keep.log"));
        assert!(labels.contains(&"bugly/nested/deep.txt"));
        assert!(!labels.iter().any(|l| l.contains("skip.bin")));
        assert!(!labels.iter().any(|l| l.contains(".hidden")));
        assert!(!labels.iter().any(|l| l.contains(".git")));
        // symlink file skipped
        assert_eq!(labels.iter().filter(|l| l.ends_with("link.log")).count(), 0);
    }

    #[test]
    fn refresh_clears_and_rescans() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("logs");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.log"), "a").unwrap();

        let mut corpus = LogCorpus::new();
        corpus.configure(vec![root.display().to_string()], vec![".log".into()]);
        corpus.ensure_started();
        wait_ready(&mut corpus, Duration::from_secs(2));
        assert_eq!(corpus.found_count(), 1);

        fs::write(root.join("b.log"), "b").unwrap();
        corpus.refresh();
        wait_ready(&mut corpus, Duration::from_secs(2));
        assert_eq!(corpus.found_count(), 2);
    }
}
