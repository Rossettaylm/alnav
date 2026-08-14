//! Runtime / Dashboard source-switch panels (`C-f` / `C-g` / Open file…).

use std::path::{Path, PathBuf};

use crate::log_corpus::{CorpusEntry, LogCorpus};
use crate::recent::RecentFiles;
use crate::text_field::TextField;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenFileChoice {
    Recent(String),
    Corpus { abs: PathBuf, label: String },
}

impl OpenFileChoice {
    pub fn label(&self) -> String {
        match self {
            Self::Recent(p) => p.clone(),
            Self::Corpus { label, .. } => label.clone(),
        }
    }

    /// List row: basename first so left-truncation keeps the filename readable.
    /// Corpus: `crash.log · bugly/nested`; Recent: `crash.log · /abs/parent`.
    pub fn display_label(&self) -> String {
        match self {
            Self::Recent(p) => {
                let path = Path::new(p);
                let name = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(p.as_str());
                match path.parent().and_then(|par| {
                    let s = par.to_string_lossy();
                    if s.is_empty() || s == "." {
                        None
                    } else {
                        Some(s.into_owned())
                    }
                }) {
                    Some(parent) => format!("{name} · {parent}"),
                    None => name.to_string(),
                }
            }
            Self::Corpus { label, .. } => match label.rsplit_once('/') {
                Some((dir, name)) if !name.is_empty() => format!("{name} · {dir}"),
                _ => label.clone(),
            },
        }
    }

    pub fn path_to_open(&self) -> Option<PathBuf> {
        match self {
            Self::Recent(p) => Some(PathBuf::from(p)),
            Self::Corpus { abs, .. } => Some(abs.clone()),
        }
    }

    /// Absolute path string for the right-pane path preview.
    pub fn full_path_display(&self) -> Option<String> {
        self.path_to_open().map(|p| p.display().to_string())
    }

    fn is_recent(&self) -> bool {
        matches!(self, Self::Recent(_))
    }

    fn basename(&self) -> String {
        match self {
            Self::Recent(p) => Path::new(p)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(p.as_str())
                .to_string(),
            Self::Corpus { label, abs, .. } => label
                .rsplit_once('/')
                .map(|(_, name)| name.to_string())
                .unwrap_or_else(|| {
                    abs.file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or(label.as_str())
                        .to_string()
                }),
        }
    }

    /// Fuzzy haystacks for filter mode (same semantics as other pickers:
    /// non-matching rows are dropped). Omits absolute prefixes (`/Users/...`)
    /// so home-path letters do not keep every recent row.
    fn match_haystacks(&self) -> Vec<String> {
        let name = self.basename();
        let mut out = vec![name.clone()];
        match self {
            Self::Corpus { label, .. } => {
                out.push(label.clone());
                // `crash.log · bugly/nested` — relative only, safe to score.
                out.push(self.display_label());
            }
            Self::Recent(p) => {
                // Parent leaf only (e.g. `Downloads`), never full abs parent.
                if let Some(leaf) = Path::new(p)
                    .parent()
                    .and_then(|par| par.file_name())
                    .and_then(|s| s.to_str())
                {
                    out.push(leaf.to_string());
                    out.push(format!("{name} · {leaf}"));
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }

    fn best_fuzzy_score(&self, scorer: &mut crate::fuzzy::FuzzyScorer) -> Option<u32> {
        self.match_haystacks()
            .iter()
            .filter_map(|h| scorer.score(h))
            .max()
    }
}

#[derive(Debug)]
pub struct OpenFilePanel {
    /// Bottom draft / query (recent + corpus fuzzy).
    pub draft: TextField,
    pub selected: usize,
    pub choices: Vec<OpenFileChoice>,
    /// Esc returns to Dashboard when true (still unbound).
    pub from_dashboard: bool,
    /// Corpus scan progress / count (from [`LogCorpus::status_label`]).
    pub corpus_status: Option<String>,
}

impl OpenFilePanel {
    pub fn open(recent: &RecentFiles, corpus: &LogCorpus, from_dashboard: bool) -> Self {
        let mut panel = Self {
            draft: TextField::new(),
            selected: 0,
            choices: Vec::new(),
            from_dashboard,
            corpus_status: corpus.status_label(),
        };
        panel.refresh_choices(recent, corpus);
        panel
    }

    pub fn refresh_choices(&mut self, recent: &RecentFiles, corpus: &LogCorpus) {
        self.corpus_status = corpus.status_label();
        let q = self.draft.as_str();
        if q.is_empty() {
            // Recent first, then full corpus (dedupe by abs path).
            self.choices = empty_query_choices(recent, corpus.entries());
        } else {
            self.choices = fuzzy_open_choices(recent, corpus.entries(), q);
        }
        if self.selected >= self.choices.len() {
            self.selected = self.choices.len().saturating_sub(1);
        }
    }

    pub fn move_sel(&mut self, delta: isize) {
        let n = self.choices.len();
        if n == 0 {
            return;
        }
        let cur = self.selected as isize;
        self.selected = (cur + delta).rem_euclid(n as isize) as usize;
    }

    pub fn selected_full_path(&self) -> Option<String> {
        self.choices
            .get(self.selected)
            .and_then(|c| c.full_path_display())
    }

    pub fn selected_corpus_label(&self) -> Option<String> {
        match self.choices.get(self.selected)? {
            OpenFileChoice::Corpus { label, .. } => Some(label.clone()),
            OpenFileChoice::Recent(_) => None,
        }
    }
}

fn empty_query_choices(recent: &RecentFiles, corpus: &[CorpusEntry]) -> Vec<OpenFileChoice> {
    let mut choices: Vec<OpenFileChoice> = recent
        .paths
        .iter()
        .cloned()
        .map(OpenFileChoice::Recent)
        .collect();
    for e in corpus {
        let abs = e.abs.display().to_string();
        if recent.paths.iter().any(|r| r == &abs) {
            continue;
        }
        choices.push(OpenFileChoice::Corpus {
            abs: e.abs.clone(),
            label: e.label.clone(),
        });
    }
    choices
}

/// Filter mode (same as Picker `filtered_indices`): drop non-matches, then
/// score-desc; recent wins ties. Cap via [`CANDIDATE_RESULT_CAP`].
fn fuzzy_open_choices(
    recent: &RecentFiles,
    corpus: &[CorpusEntry],
    query: &str,
) -> Vec<OpenFileChoice> {
    let source = empty_query_choices(recent, corpus);
    let mut scorer = crate::fuzzy::FuzzyScorer::new(query);
    let mut scored: Vec<(usize, u32, bool)> = source
        .iter()
        .enumerate()
        .filter_map(|(i, choice)| {
            choice
                .best_fuzzy_score(&mut scorer)
                .map(|score| (i, score, choice.is_recent()))
        })
        .collect();
    scored.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| b.2.cmp(&a.2)) // recent (true) before corpus on tie
            .then_with(|| a.0.cmp(&b.0))
    });
    scored.truncate(crate::fuzzy::CANDIDATE_RESULT_CAP);
    scored
        .into_iter()
        .map(|(i, _, _)| source[i].clone())
        .collect()
}

/// Centered HDC / ADB chooser (no preview).
#[derive(Debug, Clone)]
pub struct StreamSourcePanel {
    /// 0 = HDC, 1 = ADB
    pub selected: usize,
    pub from_dashboard: bool,
}

impl StreamSourcePanel {
    pub fn new(from_dashboard: bool) -> Self {
        Self {
            selected: 0,
            from_dashboard,
        }
    }

    pub fn move_by(&mut self, delta: isize) {
        let next = (self.selected as isize + delta).rem_euclid(2) as usize;
        self.selected = next;
    }

    pub fn is_hdc(&self) -> bool {
        self.selected == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_file_lists_recent_and_corpus_when_empty_draft() {
        let recent = RecentFiles {
            paths: vec!["/tmp/a.log".into()],
        };
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("bugly");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("crash.log"), "x").unwrap();
        let mut corpus = LogCorpus::new();
        corpus.configure(vec![root.display().to_string()], vec![".log".into()]);
        corpus.ensure_started();
        let start = std::time::Instant::now();
        while corpus.is_scanning() && start.elapsed() < std::time::Duration::from_secs(2) {
            corpus.poll();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        corpus.poll();

        let panel = OpenFilePanel::open(&recent, &corpus, false);
        assert!(matches!(&panel.choices[0], OpenFileChoice::Recent(p) if p == "/tmp/a.log"));
        assert!(
            panel.choices.iter().any(
                |c| matches!(c, OpenFileChoice::Corpus { label, .. } if label.contains("crash"))
            ),
            "empty draft must list corpus files"
        );
        assert!(panel.selected_full_path().is_some());
    }

    #[test]
    fn display_label_puts_basename_first() {
        let c = OpenFileChoice::Corpus {
            abs: PathBuf::from("/data/bugly/nested/crash.log"),
            label: "bugly/nested/crash.log".into(),
        };
        assert_eq!(c.display_label(), "crash.log · bugly/nested");
        let r = OpenFileChoice::Recent("/tmp/logs/app.log".into());
        assert_eq!(r.display_label(), "app.log · /tmp/logs");
    }

    #[test]
    fn typed_query_filters_out_non_matches() {
        let recent = RecentFiles {
            paths: vec![
                "/Users/lyman/Downloads/alpha.log".into(),
                "/Users/lyman/Downloads/beta.log".into(),
            ],
        };
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("bugly");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("crash.log"), "x").unwrap();
        std::fs::write(root.join("other.txt"), "y").unwrap();
        let mut corpus = LogCorpus::new();
        corpus.configure(
            vec![root.display().to_string()],
            vec![".log".into(), ".txt".into()],
        );
        corpus.ensure_started();
        let start = std::time::Instant::now();
        while corpus.is_scanning() && start.elapsed() < std::time::Duration::from_secs(2) {
            corpus.poll();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        corpus.poll();

        let mut panel = OpenFilePanel::open(&recent, &corpus, false);
        panel.draft = TextField::from_text("crash");
        panel.refresh_choices(&recent, &corpus);
        assert!(!panel.choices.is_empty(), "crash must remain after filter");
        assert!(
            panel.choices.iter().all(|c| {
                c.basename().to_ascii_lowercase().contains("crash")
                    || c.label().to_ascii_lowercase().contains("crash")
            }),
            "non-matching rows must be filtered out: {:?}",
            panel
                .choices
                .iter()
                .map(|c| c.display_label())
                .collect::<Vec<_>>()
        );
        // Home-prefix letters must not keep every recent via abs-path haystack.
        panel.draft = TextField::from_text("Users");
        panel.refresh_choices(&recent, &corpus);
        assert!(
            panel.choices.is_empty(),
            "query Users must not match via /Users abs prefix; got {:?}",
            panel
                .choices
                .iter()
                .map(|c| c.display_label())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn typed_query_fuzzy_includes_corpus_label() {
        let recent = RecentFiles::default();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("bugly");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("crash.log"), "x").unwrap();
        let mut corpus = LogCorpus::new();
        corpus.configure(vec![root.display().to_string()], vec![".log".into()]);
        corpus.ensure_started();
        let start = std::time::Instant::now();
        while corpus.is_scanning() && start.elapsed() < std::time::Duration::from_secs(2) {
            corpus.poll();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        corpus.poll();

        let mut panel = OpenFilePanel::open(&recent, &corpus, false);
        panel.draft = TextField::from_text("crsh");
        panel.refresh_choices(&recent, &corpus);
        assert!(
            panel.choices.iter().any(
                |c| matches!(c, OpenFileChoice::Corpus { label, .. } if label.contains("crash"))
            ),
            "choices={:?}",
            panel.choices.iter().map(|c| c.label()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn stream_panel_toggles() {
        let mut p = StreamSourcePanel::new(false);
        p.move_by(1);
        assert!(!p.is_hdc());
        p.move_by(1);
        assert!(p.is_hdc());
    }
}
