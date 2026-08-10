//! Runtime / Dashboard source-switch panels (`of` / `os` / Open file…).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use crate::path_complete::{self, PathCandidate};
use crate::recent::RecentFiles;
use crate::text_field::TextField;

pub const FILE_HEAD_LINES: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenFileChoice {
    Recent(String),
    Path(PathCandidate),
}

impl OpenFileChoice {
    pub fn label(&self) -> String {
        match self {
            Self::Recent(p) => p.clone(),
            Self::Path(c) => c.replacement.clone(),
        }
    }

    pub fn path_for_preview(&self) -> Option<PathBuf> {
        match self {
            Self::Recent(p) => Some(PathBuf::from(p)),
            Self::Path(c) if !c.is_dir => Some(path_complete::expand_user(&c.replacement)),
            Self::Path(_) => None,
        }
    }

    pub fn path_to_open(&self) -> Option<PathBuf> {
        match self {
            Self::Recent(p) => Some(PathBuf::from(p)),
            Self::Path(c) if !c.is_dir => Some(path_complete::expand_user(&c.replacement)),
            Self::Path(_) => None,
        }
    }
}

#[derive(Debug)]
pub struct OpenFilePanel {
    /// Bottom draft / query (path typing + recent filter).
    pub draft: TextField,
    pub selected: usize,
    pub choices: Vec<OpenFileChoice>,
    /// Esc returns to Dashboard when true (still unbound).
    pub from_dashboard: bool,
    preview_gen: u64,
    preview_rx: Option<Receiver<FileHeadMsg>>,
    pub preview_lines: Vec<String>,
    pub preview_status: Option<String>,
    pub preview_path: Option<String>,
}

#[derive(Debug)]
struct FileHeadMsg {
    gen: u64,
    path: String,
    result: Result<Vec<String>, String>,
}

impl OpenFilePanel {
    pub fn open(recent: &RecentFiles, from_dashboard: bool) -> Self {
        let mut panel = Self {
            draft: TextField::new(),
            selected: 0,
            choices: Vec::new(),
            from_dashboard,
            preview_gen: 0,
            preview_rx: None,
            preview_lines: Vec::new(),
            preview_status: None,
            preview_path: None,
        };
        panel.refresh_choices(recent);
        panel
    }

    pub fn refresh_choices(&mut self, recent: &RecentFiles) {
        let q = self.draft.as_str();
        let mut choices = Vec::new();
        if q.is_empty() || looks_like_path_query(q) {
            // When typing a path-like query, prefer filesystem completions.
            if !q.is_empty() && looks_like_path_query(q) {
                for c in path_complete::complete(q) {
                    choices.push(OpenFileChoice::Path(c));
                }
            }
        }
        // Always include fuzzy-filtered recent (nucleo) when not purely completing.
        let recent_labels: Vec<String> = recent.paths.clone();
        let indices = crate::fuzzy::fuzzy_label_indices(&recent_labels, q);
        for i in indices {
            let p = recent.paths[i].clone();
            if !choices.iter().any(|c| c.label() == p) {
                choices.push(OpenFileChoice::Recent(p));
            }
        }
        // Empty query: recent first (already), no path cands.
        if q.is_empty() {
            choices = recent
                .paths
                .iter()
                .cloned()
                .map(OpenFileChoice::Recent)
                .collect();
        }
        self.choices = choices;
        if self.selected >= self.choices.len() {
            self.selected = self.choices.len().saturating_sub(1);
        }
        self.request_preview_for_selection();
    }

    pub fn move_sel(&mut self, delta: isize) {
        let n = self.choices.len();
        if n == 0 {
            return;
        }
        let cur = self.selected as isize;
        self.selected = (cur + delta).rem_euclid(n as isize) as usize;
        self.request_preview_for_selection();
    }

    pub fn apply_tab_complete(&mut self, recent: &RecentFiles) {
        let path_cands: Vec<PathCandidate> = self
            .choices
            .iter()
            .filter_map(|c| match c {
                OpenFileChoice::Path(p) => Some(p.clone()),
                _ => None,
            })
            .collect();
        if let Some(common) = path_complete::longest_common_prefix(&path_cands) {
            self.draft = TextField::from_text(common);
            self.refresh_choices(recent);
            return;
        }
        if let Some(OpenFileChoice::Path(c)) = self.choices.get(self.selected).cloned() {
            self.draft = TextField::from_text(c.replacement);
            self.refresh_choices(recent);
        }
    }

    pub fn request_preview_for_selection(&mut self) {
        let Some(choice) = self.choices.get(self.selected) else {
            self.preview_lines.clear();
            self.preview_status = Some("no selection".into());
            self.preview_path = None;
            return;
        };
        let Some(path) = choice.path_for_preview() else {
            self.preview_lines.clear();
            self.preview_status = Some(if matches!(choice, OpenFileChoice::Path(c) if c.is_dir) {
                "directory".into()
            } else {
                "no preview".into()
            });
            self.preview_path = None;
            return;
        };
        let path_str = path.display().to_string();
        if self.preview_path.as_deref() == Some(path_str.as_str()) && !self.preview_lines.is_empty()
        {
            return;
        }
        self.preview_gen = self.preview_gen.wrapping_add(1);
        let gen = self.preview_gen;
        self.preview_path = Some(path_str.clone());
        self.preview_lines.clear();
        self.preview_status = Some("loading…".into());
        let (tx, rx) = mpsc::channel();
        self.preview_rx = Some(rx);
        thread::spawn(move || {
            let result = read_head_lines(&path, FILE_HEAD_LINES);
            let _ = tx.send(FileHeadMsg {
                gen,
                path: path_str,
                result,
            });
        });
    }

    pub fn poll_preview(&mut self) {
        let Some(rx) = &self.preview_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(msg) => {
                if msg.gen != self.preview_gen {
                    return;
                }
                match msg.result {
                    Ok(lines) => {
                        self.preview_lines = lines;
                        self.preview_status = None;
                        self.preview_path = Some(msg.path);
                    }
                    Err(e) => {
                        self.preview_lines.clear();
                        self.preview_status = Some(e);
                    }
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.preview_rx = None;
            }
        }
    }
}

fn looks_like_path_query(q: &str) -> bool {
    q.starts_with('/')
        || q.starts_with('~')
        || q.starts_with('.')
        || q.contains('/')
        || q.contains(std::path::MAIN_SEPARATOR)
}

fn read_head_lines(path: &Path, limit: usize) -> Result<Vec<String>, String> {
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    if path.is_dir() {
        return Err("directory".into());
    }
    if !path.exists() {
        return Err("not found".into());
    }
    let file = File::open(path).map_err(|e| e.to_string())?;
    let reader = BufReader::new(file);
    let mut lines = Vec::new();
    for (i, line) in reader.lines().enumerate() {
        if i >= limit {
            break;
        }
        let raw = line.map_err(|e| e.to_string())?;
        // lossy already via String; replace invalid later if needed
        lines.push(raw);
    }
    if lines.is_empty() {
        Ok(vec!["(empty)".into()])
    } else {
        Ok(lines)
    }
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
    fn open_file_lists_recent_when_empty_draft() {
        let recent = RecentFiles {
            paths: vec!["/tmp/a.log".into(), "/tmp/b.log".into()],
        };
        let panel = OpenFilePanel::open(&recent, false);
        assert_eq!(panel.choices.len(), 2);
        assert!(matches!(&panel.choices[0], OpenFileChoice::Recent(p) if p == "/tmp/a.log"));
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
