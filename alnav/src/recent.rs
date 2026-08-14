//! Persisted recent-files list for Dashboard / `C-f` source picker.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const RECENT_FILE_NAME: &str = "recent_files.toml";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
struct RecentToml {
    #[serde(default)]
    files: Vec<String>,
}

/// In-memory recent files (absolute path strings), newest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecentFiles {
    pub paths: Vec<String>,
}

impl RecentFiles {
    pub fn load(config_dir: &Path) -> Self {
        let path = config_dir.join(RECENT_FILE_NAME);
        let Ok(text) = fs::read_to_string(&path) else {
            return Self::default();
        };
        match toml::from_str::<RecentToml>(&text) {
            Ok(t) => Self {
                paths: t
                    .files
                    .into_iter()
                    .filter(|s| !s.trim().is_empty())
                    .collect(),
            },
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, config_dir: &Path) -> Result<(), String> {
        fs::create_dir_all(config_dir).map_err(|e| format!("create config dir: {e}"))?;
        let path = config_dir.join(RECENT_FILE_NAME);
        let body = toml::to_string_pretty(&RecentToml {
            files: self.paths.clone(),
        })
        .map_err(|e| e.to_string())?;
        fs::write(&path, body).map_err(|e| format!("write {}: {e}", path.display()))
    }

    /// Push `path` to front (dedupe), then truncate to `limit` (≥1).
    pub fn record(&mut self, path: impl AsRef<Path>, limit: usize) {
        let limit = limit.max(1);
        let abs = normalize_path(path.as_ref());
        if abs.is_empty() {
            return;
        }
        self.paths.retain(|p| p != &abs);
        self.paths.insert(0, abs);
        if self.paths.len() > limit {
            self.paths.truncate(limit);
        }
    }

    pub fn remove(&mut self, path: &str) {
        self.paths.retain(|p| p != path);
    }
}

fn normalize_path(path: &Path) -> String {
    if let Ok(canon) = path.canonicalize() {
        return canon.display().to_string();
    }
    if path.is_absolute() {
        return path.display().to_string();
    }
    match std::env::current_dir() {
        Ok(cwd) => cwd.join(path).display().to_string(),
        Err(_) => path.display().to_string(),
    }
}

/// Clamp configured recent-files limit into a sane range.
pub fn clamp_limit(n: usize) -> usize {
    n.clamp(1, 200)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_dedupes_and_caps() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.log");
        let b = dir.path().join("b.log");
        fs::write(&a, "a").unwrap();
        fs::write(&b, "b").unwrap();
        let mut recent = RecentFiles::default();
        recent.record(&a, 2);
        recent.record(&b, 2);
        recent.record(&a, 2);
        assert_eq!(recent.paths.len(), 2);
        assert!(recent.paths[0].ends_with("a.log"));
        assert!(recent.paths[1].ends_with("b.log"));
    }

    #[test]
    fn load_save_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut recent = RecentFiles {
            paths: vec!["/tmp/one.log".into(), "/tmp/two.log".into()],
        };
        recent.save(dir.path()).unwrap();
        let loaded = RecentFiles::load(dir.path());
        assert_eq!(loaded.paths, recent.paths);
    }

    #[test]
    fn bad_toml_yields_empty() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(RECENT_FILE_NAME), "files = {{{").unwrap();
        assert!(RecentFiles::load(dir.path()).paths.is_empty());
    }
}
