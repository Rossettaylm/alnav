//! Config directory resolution and theme.toml loading (M4).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::theme::{self, UiTokens};

/// Resolve config directory: `--config-path` > `$ALOGGREP_HOME` > `~/.config/aloggrep`.
pub fn resolve_config_dir(cli_override: Option<&Path>) -> PathBuf {
    if let Some(p) = cli_override {
        return p.to_path_buf();
    }
    if let Ok(home) = env::var("ALOGGREP_HOME") {
        if !home.is_empty() {
            return PathBuf::from(home);
        }
    }
    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".config").join("aloggrep")
}

/// Outcome of loading `theme.toml` at startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeLoadStatus {
    /// No theme.toml — builtin tokens.
    Builtin,
    /// Successfully loaded from path.
    Loaded(PathBuf),
    /// File present but invalid — builtin + user-visible reason.
    Fallback { path: PathBuf, error: String },
}

impl ThemeLoadStatus {
    /// Status-bar hint when the user should notice a fallback.
    pub fn status_hint(&self) -> Option<String> {
        match self {
            ThemeLoadStatus::Fallback { error, .. } => {
                Some(format!("THEME 回退: {error}"))
            }
            _ => None,
        }
    }
}

/// Load `$config_dir/theme.toml` into [`theme`] and return load status.
pub fn load_theme(config_dir: &Path) -> ThemeLoadStatus {
    let path = config_dir.join("theme.toml");
    if !path.is_file() {
        theme::install(UiTokens::builtin());
        return ThemeLoadStatus::Builtin;
    }
    match fs::read_to_string(&path) {
        Ok(text) => match theme::parse_theme_toml(&text) {
            Ok(tokens) => {
                theme::install(tokens);
                ThemeLoadStatus::Loaded(path)
            }
            Err(e) => {
                theme::install(UiTokens::builtin());
                ThemeLoadStatus::Fallback {
                    path,
                    error: e,
                }
            }
        },
        Err(e) => {
            theme::install(UiTokens::builtin());
            ThemeLoadStatus::Fallback {
                path,
                error: e.to_string(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serialize theme install across tests (global OnceLock-like install).
    static THEME_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn resolve_prefers_cli_override() {
        let p = PathBuf::from("/tmp/aloggrep-cfg");
        assert_eq!(resolve_config_dir(Some(&p)), p);
    }

    #[test]
    fn resolve_prefers_aloggrep_home() {
        let _g = THEME_TEST_LOCK.lock().unwrap();
        // SAFETY: test-only env mutation under mutex.
        env::set_var("ALOGGREP_HOME", "/tmp/custom-aloggrep-home");
        let dir = resolve_config_dir(None);
        env::remove_var("ALOGGREP_HOME");
        assert_eq!(dir, PathBuf::from("/tmp/custom-aloggrep-home"));
    }

    #[test]
    fn missing_theme_is_builtin() {
        use ratatui::style::Color;
        let _g = THEME_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let st = load_theme(dir.path());
        assert_eq!(st, ThemeLoadStatus::Builtin);
        assert_eq!(theme::accent(), Color::Cyan);
    }

    #[test]
    fn bad_theme_falls_back_with_status() {
        use ratatui::style::Color;
        let _g = THEME_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("theme.toml"), "accent = !!!\n").unwrap();
        let st = load_theme(dir.path());
        match &st {
            ThemeLoadStatus::Fallback { error, .. } => {
                assert!(!error.is_empty());
                assert!(st.status_hint().unwrap().contains("THEME 回退"));
            }
            other => panic!("expected Fallback, got {other:?}"),
        }
        assert_eq!(theme::accent(), Color::Cyan);
    }

    #[test]
    fn valid_theme_overrides_accent() {
        use ratatui::style::Color;
        let _g = THEME_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("theme.toml"),
            "accent = \"#ff00aa\"\nsuccess = \"blue\"\n",
        )
        .unwrap();
        let st = load_theme(dir.path());
        assert!(matches!(st, ThemeLoadStatus::Loaded(_)));
        assert_eq!(theme::accent(), Color::Rgb(255, 0, 170));
        assert_eq!(theme::success(), Color::Blue);
        // reset builtin for other tests
        theme::install(UiTokens::builtin());
    }
}
