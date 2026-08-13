//! Config directory resolution and theme.toml / config.toml loading (M4).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::theme;

/// Resolve config directory: `--config-path` > `$ALNAV_HOME` > `~/.config/alnav`.
pub fn resolve_config_dir(cli_override: Option<&Path>) -> PathBuf {
    if let Some(p) = cli_override {
        return p.to_path_buf();
    }
    if let Ok(home) = env::var("ALNAV_HOME") {
        if !home.is_empty() {
            return PathBuf::from(home);
        }
    }
    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".config").join("alnav")
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
            ThemeLoadStatus::Fallback { error, .. } => Some(format!("THEME fallback: {error}")),
            _ => None,
        }
    }
}

/// Default Open-file corpus suffix filter (include the leading dot).
pub fn default_log_extensions() -> Vec<String> {
    vec![".log".into(), ".txt".into()]
}

/// Application settings from `config.toml` (with builtin defaults).
#[derive(Debug, Clone, PartialEq)]
pub struct AppConfig {
    pub picker_left_ratio: f32,
    /// When false, all picker panels render full-width (no right preview pane).
    pub picker_preview_enabled: bool,
    /// Max recent files remembered for Dashboard / `of` (clamped 1..=200).
    pub recent_files_limit: usize,
    /// Directories recursively scanned for Open-file fuzzy corpus.
    pub log_dirs: Vec<String>,
    /// Case-insensitive suffix filter for corpus files (include the dot).
    pub log_extensions: Vec<String>,
    /// Named palette from `config.toml` (folding happens in `palette_by_name`).
    pub theme: String,
}

impl AppConfig {
    pub fn default_config() -> Self {
        Self {
            picker_left_ratio: 0.4,
            picker_preview_enabled: true,
            recent_files_limit: 20,
            log_dirs: Vec::new(),
            log_extensions: default_log_extensions(),
            theme: "default".into(),
        }
    }

    pub fn clamp_ratio(r: f32) -> f32 {
        r.clamp(0.2, 0.8)
    }
}

/// Outcome of loading `config.toml` at startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigLoadStatus {
    /// No config.toml — builtin defaults.
    Builtin,
    /// Successfully loaded from path.
    Loaded(PathBuf),
    /// File present but invalid — builtin + user-visible reason.
    Fallback { path: PathBuf, error: String },
}

impl ConfigLoadStatus {
    /// Status-bar hint when the user should notice a fallback.
    pub fn status_hint(&self) -> Option<String> {
        match self {
            ConfigLoadStatus::Fallback { error, .. } => Some(format!("CONFIG fallback: {error}")),
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ConfigToml {
    picker_left_ratio: Option<f32>,
    picker_preview_enabled: Option<bool>,
    recent_files_limit: Option<usize>,
    log_dirs: Option<Vec<String>>,
    log_extensions: Option<Vec<String>>,
    theme: Option<String>,
}

/// Normalize configured extensions: trim, ensure leading `.`, lowercase.
/// Empty list falls back to [`default_log_extensions`].
pub fn normalize_log_extensions(raw: Option<Vec<String>>) -> Vec<String> {
    let Some(list) = raw else {
        return default_log_extensions();
    };
    let mut out: Vec<String> = list
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(|s| {
            let lower = s.to_ascii_lowercase();
            if lower.starts_with('.') {
                lower
            } else {
                format!(".{lower}")
            }
        })
        .collect();
    out.sort();
    out.dedup();
    if out.is_empty() {
        default_log_extensions()
    } else {
        out
    }
}

fn normalize_log_dirs(raw: Option<Vec<String>>) -> Vec<String> {
    raw.unwrap_or_default()
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Load `$config_dir/config.toml` and return config + load status.
pub fn load_config(config_dir: &Path) -> (AppConfig, ConfigLoadStatus) {
    let path = config_dir.join("config.toml");
    if !path.is_file() {
        return (AppConfig::default_config(), ConfigLoadStatus::Builtin);
    }
    match fs::read_to_string(&path) {
        Ok(text) => match toml::from_str::<ConfigToml>(&text) {
            Ok(t) => {
                let mut cfg = AppConfig::default_config();
                if let Some(r) = t.picker_left_ratio {
                    cfg.picker_left_ratio = AppConfig::clamp_ratio(r);
                }
                if let Some(v) = t.picker_preview_enabled {
                    cfg.picker_preview_enabled = v;
                }
                if let Some(n) = t.recent_files_limit {
                    cfg.recent_files_limit = crate::recent::clamp_limit(n);
                }
                cfg.log_dirs = normalize_log_dirs(t.log_dirs);
                cfg.log_extensions = normalize_log_extensions(t.log_extensions);
                if let Some(name) = t.theme {
                    let name = name.trim().to_string();
                    if !name.is_empty() {
                        cfg.theme = name;
                    }
                }
                (cfg, ConfigLoadStatus::Loaded(path))
            }
            Err(e) => (
                AppConfig::default_config(),
                ConfigLoadStatus::Fallback {
                    path,
                    error: e.to_string(),
                },
            ),
        },
        Err(e) => (
            AppConfig::default_config(),
            ConfigLoadStatus::Fallback {
                path,
                error: e.to_string(),
            },
        ),
    }
}

/// Load named palette plus optional `$config_dir/theme.toml` overlay.
pub fn load_theme(config_dir: &Path, theme_name: &str) -> ThemeLoadStatus {
    let mut unknown: Option<String> = None;
    let canonical = crate::palette::resolve_theme_name(theme_name).unwrap_or("default");
    let base = match crate::theme_builtins::palette_by_name(theme_name) {
        Some(p) => p,
        None => {
            unknown = Some(format!("unknown theme '{theme_name}'"));
            crate::palette::Palette::default_ansi()
        }
    };
    let path = config_dir.join("theme.toml");
    if !path.is_file() {
        theme::install(theme::map_to_tokens_for(&base, canonical));
        if let Some(error) = unknown {
            return ThemeLoadStatus::Fallback {
                path: config_dir.to_path_buf(),
                error,
            };
        }
        if crate::palette::resolve_theme_name(theme_name) == Some("default") {
            ThemeLoadStatus::Builtin
        } else {
            ThemeLoadStatus::Loaded(config_dir.to_path_buf())
        }
    } else {
        match fs::read_to_string(&path) {
            Ok(text) => match theme::apply_overlay_for(base, canonical, &text) {
                Ok(tokens) => {
                    theme::install(tokens);
                    if let Some(error) = unknown {
                        ThemeLoadStatus::Fallback { path, error }
                    } else {
                        ThemeLoadStatus::Loaded(path)
                    }
                }
                Err(e) => {
                    theme::install(theme::map_to_tokens_for(&base, canonical));
                    ThemeLoadStatus::Fallback { path, error: e }
                }
            },
            Err(e) => {
                theme::install(theme::map_to_tokens_for(&base, canonical));
                ThemeLoadStatus::Fallback {
                    path,
                    error: e.to_string(),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;
    use std::sync::Mutex;

    /// Serialize theme install across tests (global OnceLock-like install).
    static THEME_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn resolve_prefers_cli_override() {
        let p = PathBuf::from("/tmp/alnav-cfg");
        assert_eq!(resolve_config_dir(Some(&p)), p);
    }

    #[test]
    fn resolve_prefers_alnav_home() {
        let _g = THEME_TEST_LOCK.lock().unwrap();
        // SAFETY: test-only env mutation under mutex.
        env::set_var("ALNAV_HOME", "/tmp/custom-alnav-home");
        let dir = resolve_config_dir(None);
        env::remove_var("ALNAV_HOME");
        assert_eq!(dir, PathBuf::from("/tmp/custom-alnav-home"));
    }

    #[test]
    fn resolve_ignores_legacy_aloggrep_home() {
        let _g = THEME_TEST_LOCK.lock().unwrap();
        // SAFETY: test-only env mutation under mutex.
        env::remove_var("ALNAV_HOME");
        env::set_var("ALOGGREP_HOME", "/tmp/legacy-aloggrep-home");
        let dir = resolve_config_dir(None);
        env::remove_var("ALOGGREP_HOME");
        assert_ne!(dir, PathBuf::from("/tmp/legacy-aloggrep-home"));
        assert!(
            dir.ends_with(Path::new(".config").join("alnav")),
            "expected ~/.config/alnav default, got {dir:?}"
        );
    }

    #[test]
    fn missing_theme_is_builtin() {
        use ratatui::style::Color;
        let _g = THEME_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let st = load_theme(dir.path(), "default");
        assert_eq!(st, ThemeLoadStatus::Builtin);
        assert_eq!(theme::accent(), Color::Cyan);
    }

    #[test]
    fn bad_theme_falls_back_with_status() {
        use ratatui::style::Color;
        let _g = THEME_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("theme.toml"), "accent = !!!\n").unwrap();
        let st = load_theme(dir.path(), "default");
        match &st {
            ThemeLoadStatus::Fallback { error, .. } => {
                assert!(!error.is_empty());
                assert!(st.status_hint().unwrap().contains("THEME fallback"));
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
        let st = load_theme(dir.path(), "default");
        assert!(matches!(st, ThemeLoadStatus::Loaded(_)));
        assert_eq!(theme::accent(), Color::Rgb(255, 0, 170));
        assert_eq!(theme::success(), Color::Blue);
        // reset builtin for other tests
        theme::install(theme::UiTokens::builtin());
    }

    #[test]
    fn unknown_theme_name_falls_back_default() {
        let _g = THEME_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let st = load_theme(dir.path(), "not-a-theme");
        match &st {
            ThemeLoadStatus::Fallback { error, .. } => {
                assert!(error.contains("unknown theme"));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(theme::accent(), Color::Cyan);
    }

    #[test]
    fn palette_overlay_changes_error_keeps_accent() {
        let _g = THEME_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("theme.toml"),
            "[palette]\nred = \"#ff0000\"\n",
        )
        .unwrap();
        let st = load_theme(dir.path(), "kanagawa");
        assert!(matches!(st, ThemeLoadStatus::Loaded(_)));
        assert_eq!(
            theme::minimap_severe_style().fg,
            Some(Color::Rgb(255, 0, 0))
        );
        let p = crate::theme_builtins::palette_by_name("kanagawa").unwrap();
        assert_eq!(theme::accent(), p.blue);
        theme::install(theme::UiTokens::builtin());
    }

    #[test]
    fn highlight_len_7_discards_entire_overlay() {
        let _g = THEME_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("theme.toml"),
            "accent = \"#ffffff\"\nhighlight = [\"#000000\",\"#000000\",\"#000000\",\"#000000\",\"#000000\",\"#000000\",\"#000000\"]\n",
        )
        .unwrap();
        let st = load_theme(dir.path(), "default");
        assert!(matches!(st, ThemeLoadStatus::Fallback { .. }));
        assert_eq!(theme::accent(), Color::Cyan);
        theme::install(theme::UiTokens::builtin());
    }

    #[test]
    fn load_config_reads_theme_name() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "theme = \"Nord\"\n").unwrap();
        let (cfg, st) = load_config(dir.path());
        assert!(matches!(st, ConfigLoadStatus::Loaded(_)));
        assert_eq!(cfg.theme, "Nord");
    }

    #[test]
    fn load_config_defaults_theme_default() {
        let dir = tempfile::tempdir().unwrap();
        let (cfg, _) = load_config(dir.path());
        assert_eq!(cfg.theme, "default");
    }

    #[test]
    fn load_config_missing_uses_default_ratio() {
        let dir = tempfile::tempdir().unwrap();
        let (cfg, st) = load_config(dir.path());
        assert!((cfg.picker_left_ratio - 0.4).abs() < f32::EPSILON);
        assert!(matches!(st, ConfigLoadStatus::Builtin));
    }

    #[test]
    fn load_config_clamps_out_of_range() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "picker_left_ratio = 0.05\n").unwrap();
        let (cfg, _) = load_config(dir.path());
        assert!((cfg.picker_left_ratio - 0.2).abs() < f32::EPSILON);
    }

    #[test]
    fn load_config_bad_toml_falls_back() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "picker_left_ratio = {{{").unwrap();
        let (cfg, st) = load_config(dir.path());
        assert!((cfg.picker_left_ratio - 0.4).abs() < f32::EPSILON);
        assert!(matches!(st, ConfigLoadStatus::Fallback { .. }));
        assert!(st.status_hint().unwrap().contains("CONFIG fallback"));
    }

    #[test]
    fn load_config_picks_up_preview_disabled() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            "picker_preview_enabled = false\n",
        )
        .unwrap();
        let (cfg, st) = load_config(dir.path());
        assert!(!cfg.picker_preview_enabled);
        assert!(matches!(st, ConfigLoadStatus::Loaded(_)));
    }

    #[test]
    fn load_config_defaults_preview_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let (cfg, _) = load_config(dir.path());
        assert!(cfg.picker_preview_enabled);
    }

    #[test]
    fn load_config_defaults_log_dirs_empty_and_extensions() {
        let dir = tempfile::tempdir().unwrap();
        let (cfg, _) = load_config(dir.path());
        assert!(cfg.log_dirs.is_empty());
        assert_eq!(cfg.log_extensions, default_log_extensions());
    }

    #[test]
    fn load_config_picks_up_log_dirs_and_extensions() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            "log_dirs = [\"~/logs\", \" /data/bugly \"]\n\
             log_extensions = [\"LOG\", \"xlog\"]\n",
        )
        .unwrap();
        let (cfg, st) = load_config(dir.path());
        assert!(matches!(st, ConfigLoadStatus::Loaded(_)));
        assert_eq!(cfg.log_dirs, vec!["~/logs", "/data/bugly"]);
        assert_eq!(
            cfg.log_extensions,
            vec![".log".to_string(), ".xlog".to_string()]
        );
    }

    #[test]
    fn empty_log_extensions_fall_back_to_default() {
        assert_eq!(
            normalize_log_extensions(Some(vec![])),
            default_log_extensions()
        );
    }
}
