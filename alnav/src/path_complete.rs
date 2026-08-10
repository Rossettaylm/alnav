//! Thin filesystem path completion for the Open-File source picker (no new UI).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// One completion candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathCandidate {
    /// Full replacement text for the draft (dirs end with `/` or `MAIN_SEPARATOR`).
    pub replacement: String,
    /// Short label for the candidate list (file/dir name).
    pub display: String,
    pub is_dir: bool,
}

/// Expand `~` at the start of a typed path.
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

/// List completions for a typed path prefix.
///
/// - Empty / relative fragment → complete against cwd
/// - `dir/` → list entries in `dir`
/// - `dir/pre` → entries in `dir` whose name starts with `pre` (case-insensitive)
/// - Hidden entries (leading `.`) omitted unless the fragment itself starts with `.`
pub fn complete(input: &str) -> Vec<PathCandidate> {
    let sep = std::path::MAIN_SEPARATOR;
    let trailing_sep = input.ends_with(sep) || input.ends_with('/');
    let expanded = expand_user(input);
    let (dir, file_prefix) = split_dir_prefix(&expanded, trailing_sep);
    let show_hidden = file_prefix.starts_with('.');
    let Ok(read) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let prefix_lower = file_prefix.to_ascii_lowercase();
    let dir_display = dir_prefix_for_display(input, trailing_sep);
    let mut out = Vec::new();
    for entry in read.flatten() {
        let name = match entry.file_name().into_string() {
            Ok(s) => s,
            Err(_) => continue,
        };
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        if !prefix_lower.is_empty() && !name.to_ascii_lowercase().starts_with(&prefix_lower) {
            continue;
        }
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let mut replacement = format!("{dir_display}{name}");
        if is_dir && !replacement.ends_with(sep) && !replacement.ends_with('/') {
            replacement.push(sep);
        }
        out.push(PathCandidate {
            display: if is_dir { format!("{name}{sep}") } else { name },
            replacement,
            is_dir,
        });
    }
    out.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then_with(|| {
            a.display
                .to_ascii_lowercase()
                .cmp(&b.display.to_ascii_lowercase())
        })
    });
    out
}

fn split_dir_prefix(path: &Path, trailing_sep: bool) -> (PathBuf, String) {
    if trailing_sep {
        return (path.to_path_buf(), String::new());
    }
    match path.file_name().and_then(|s| s.to_str()) {
        Some(name) if path.parent().is_some_and(|p| !p.as_os_str().is_empty()) => {
            let dir = path.parent().unwrap().to_path_buf();
            (dir, name.to_string())
        }
        Some(name) => (PathBuf::from("."), name.to_string()),
        None => (PathBuf::from("."), String::new()),
    }
}

/// Directory portion as the user typed it (keeps `~` and trailing sep).
fn dir_prefix_for_display(input: &str, trailing_sep: bool) -> String {
    let sep = std::path::MAIN_SEPARATOR;
    if input.is_empty() {
        return String::new();
    }
    if trailing_sep {
        return input.to_string();
    }
    if let Some(idx) = input.rfind([sep, '/']) {
        return input[..=idx].to_string();
    }
    String::new()
}

/// Longest common replacement prefix among candidates (for Tab).
pub fn longest_common_prefix(cands: &[PathCandidate]) -> Option<String> {
    let first = cands.first()?.replacement.as_str();
    if cands.len() == 1 {
        return Some(first.to_string());
    }
    let mut end = first.len();
    for c in &cands[1..] {
        let b = c.replacement.as_bytes();
        let a = first.as_bytes();
        let mut i = 0;
        while i < end && i < b.len() && a[i] == b[i] {
            i += 1;
        }
        end = i;
    }
    let mut end = end;
    while end > 0 && !first.is_char_boundary(end) {
        end -= 1;
    }
    if end == 0 {
        None
    } else {
        Some(first[..end].to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completes_files_in_dir() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("alpha.log"), "a").unwrap();
        fs::write(dir.path().join("beta.log"), "b").unwrap();
        fs::create_dir(dir.path().join("subdir")).unwrap();
        let prefix = format!("{}{}", dir.path().display(), std::path::MAIN_SEPARATOR);
        let cands = complete(&prefix);
        assert!(cands.iter().any(|c| c.display.starts_with("alpha")));
        assert!(cands
            .iter()
            .any(|c| c.is_dir && c.display.starts_with("subdir")));
    }

    #[test]
    fn prefix_filters_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("App.log"), "a").unwrap();
        fs::write(dir.path().join("other.log"), "o").unwrap();
        let q = format!("{}{}Ap", dir.path().display(), std::path::MAIN_SEPARATOR);
        let cands = complete(&q);
        assert_eq!(cands.len(), 1);
        assert!(cands[0].display.contains("App"));
    }

    #[test]
    fn longest_common_prefix_works() {
        let cands = vec![
            PathCandidate {
                replacement: "/tmp/foo_a".into(),
                display: "foo_a".into(),
                is_dir: false,
            },
            PathCandidate {
                replacement: "/tmp/foo_b".into(),
                display: "foo_b".into(),
                is_dir: false,
            },
        ];
        assert_eq!(longest_common_prefix(&cands).as_deref(), Some("/tmp/foo_"));
    }
}
