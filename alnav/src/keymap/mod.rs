//! Configurable TUI keymap: DSL, action registry, merge, and KeymapStore.
//!
//! The Rust registry is the sole authority for defaults; `keymap.toml` deep-merges
//! overrides. Help/status format keys via [`KeymapStore::display`].

mod registry;
mod stroke;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent};

// Glob re-exports keep every `crate::keymap::X` path stable after the split.
pub use registry::*;
pub use stroke::*;

/// Effective keymap after merge.
#[derive(Debug, Clone)]
pub struct KeymapStore {
    /// `None` means unbound.
    bindings: HashMap<ActionId, Option<Binding>>,
    pub warnings: Vec<String>,
}

impl Default for KeymapStore {
    fn default() -> Self {
        Self::builtin()
    }
}

impl KeymapStore {
    pub fn builtin() -> Self {
        let mut bindings = HashMap::with_capacity(ActionId::ALL.len());
        for &id in ActionId::ALL {
            let default = id.meta().default;
            bindings.insert(
                id,
                if default.strokes.is_empty() {
                    None
                } else {
                    Some(default)
                },
            );
        }
        Self {
            bindings,
            warnings: Vec::new(),
        }
    }

    pub fn binding(&self, id: ActionId) -> Option<&Binding> {
        self.bindings.get(&id).and_then(|b| b.as_ref())
    }

    pub fn display(&self, id: ActionId) -> Option<String> {
        self.binding(id).map(|b| {
            if b.strokes.len() == 1 {
                b.strokes[0].format_ui()
            } else {
                b.format_compact()
            }
        })
    }

    pub fn matches_stroke(&self, id: ActionId, stroke: &KeyStroke) -> bool {
        match self.binding(id) {
            Some(b) if b.strokes.len() == 1 => b.strokes[0] == *stroke,
            _ => false,
        }
    }

    pub fn matches_code(&self, id: ActionId, code: KeyCode) -> bool {
        let Some(stroke) = stroke_from_keycode(code) else {
            return false;
        };
        self.matches_stroke(id, &stroke)
    }

    pub fn matches_event(&self, id: ActionId, key: KeyEvent) -> bool {
        let Some(stroke) = stroke_from_event(key) else {
            return false;
        };
        self.matches_stroke(id, &stroke)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeymapLoadStatus {
    Builtin,
    Loaded(PathBuf),
    Fallback { path: PathBuf, error: String },
}

impl KeymapLoadStatus {
    pub fn status_hint(&self) -> Option<String> {
        match self {
            Self::Fallback { error, .. } => Some(format!("KEYMAP fallback: {error}")),
            _ => None,
        }
    }

    pub fn warning_hint(warnings: &[String]) -> Option<String> {
        if warnings.is_empty() {
            None
        } else {
            Some(format!("KEYMAP warn: {}", warnings.join("; ")))
        }
    }
}

fn parse_bind_value(v: &toml::Value) -> Result<Option<Binding>, String> {
    match v {
        toml::Value::String(s) => {
            if s.is_empty() {
                Ok(None)
            } else {
                Ok(Some(Binding::parse_str(s)?))
            }
        }
        toml::Value::Boolean(false) => Ok(None),
        toml::Value::Boolean(true) => Err("binding true is invalid; use a key string".into()),
        toml::Value::Array(arr) => {
            if arr.is_empty() {
                return Ok(None);
            }
            let mut strokes = Vec::with_capacity(arr.len());
            for a in arr {
                let s = a
                    .as_str()
                    .ok_or_else(|| "chord elements must be strings".to_string())?;
                strokes.push(KeyStroke::parse(s)?);
            }
            Ok(Some(Binding { strokes }))
        }
        other => Err(format!("invalid binding value: {other}")),
    }
}

fn validate_prefix_tree(store: &KeymapStore) -> Result<(), String> {
    let mut by_ctx: HashMap<KeyContext, Vec<(ActionId, Binding, ActionKind)>> = HashMap::new();
    for &id in ActionId::ALL {
        if let Some(b) = store.binding(id) {
            by_ctx
                .entry(id.context())
                .or_default()
                .push((id, b.clone(), id.meta().kind));
        }
    }
    for (ctx, list) in &by_ctx {
        for i in 0..list.len() {
            for j in 0..list.len() {
                if i == j {
                    continue;
                }
                let (id_a, ba, ka) = &list[i];
                let (id_b, bb, _) = &list[j];
                if ba.strokes == bb.strokes {
                    return Err(format!(
                        "duplicate binding {} for {:?} and {:?} in [{}]",
                        ba.format(),
                        id_a,
                        id_b,
                        ctx.as_toml()
                    ));
                }
                if ba.strokes.len() < bb.strokes.len()
                    && bb.strokes.starts_with(&ba.strokes)
                    && *ka == ActionKind::Leaf
                {
                    return Err(format!(
                        "leaf {:?} in [{}] is a prefix of {:?}",
                        id_a,
                        ctx.as_toml(),
                        id_b
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Merge user TOML onto builtins.
pub fn merge_user_toml(text: &str) -> Result<KeymapStore, String> {
    let table: BTreeMap<String, BTreeMap<String, toml::Value>> =
        toml::from_str(text).map_err(|e| e.to_string())?;
    let mut store = KeymapStore::builtin();
    let mut warnings = Vec::new();

    for (section, entries) in table {
        let Some(ctx) = KeyContext::from_toml(&section) else {
            warnings.push(format!("unknown section [{section}]"));
            continue;
        };
        for (key, val) in entries {
            let Some(id) = action_by_toml(ctx, &key) else {
                warnings.push(format!("unknown action {section}.{key}"));
                continue;
            };
            let parsed = parse_bind_value(&val).map_err(|e| format!("{section}.{key}: {e}"))?;
            store.bindings.insert(id, parsed);
        }
    }

    validate_prefix_tree(&store)?;
    store.warnings = warnings;
    Ok(store)
}

pub fn load_keymap(config_dir: &Path) -> (KeymapStore, KeymapLoadStatus) {
    let path = config_dir.join("keymap.toml");
    if !path.is_file() {
        return (KeymapStore::builtin(), KeymapLoadStatus::Builtin);
    }
    match fs::read_to_string(&path) {
        Ok(text) => match merge_user_toml(&text) {
            Ok(store) => (store, KeymapLoadStatus::Loaded(path)),
            Err(error) => (
                KeymapStore::builtin(),
                KeymapLoadStatus::Fallback { path, error },
            ),
        },
        Err(e) => (
            KeymapStore::builtin(),
            KeymapLoadStatus::Fallback {
                path,
                error: e.to_string(),
            },
        ),
    }
}

/// Serialize builtin defaults to an English-commented keymap.toml body.
pub fn serialize_default_toml() -> String {
    let mut out = String::from(
        "# alnav keymap.toml — override TUI keybindings.\n\
         # Deep-merged onto builtins: only list actions you want to change.\n\
         # Single key: string, e.g. move_down = \"j\"\n\
         # Chord: string array, e.g. some_chord = [\"m\", \"a\"]\n\
         # Modifiers: C- (Ctrl), S- (Shift), M- (Alt). Use S-j not J.\n\
         # Unbind: action = \"\" or action = null\n\n",
    );
    let mut items: Vec<(KeyContext, ActionId)> =
        ActionId::ALL.iter().map(|&id| (id.context(), id)).collect();
    items.sort_by_key(|(c, id)| (c.as_toml().to_string(), id.toml_key()));
    let mut current: Option<KeyContext> = None;
    for (ctx, id) in items {
        if current != Some(ctx) {
            if current.is_some() {
                out.push('\n');
            }
            out.push_str(&format!("[{}]\n", ctx.as_toml()));
            current = Some(ctx);
        }
        let meta = id.meta();
        out.push_str(&format!("# {} — {}\n", meta.label, meta.detail));
        let kind = match meta.kind {
            ActionKind::Prefix => "prefix",
            ActionKind::Leaf => "leaf",
        };
        out.push_str(&format!("# kind: {kind}\n"));
        out.push_str(&format!(
            "{} = \"{}\"\n",
            meta.toml_key,
            meta.default.format()
        ));
    }
    out
}

/// English-commented default `config.toml` body for `--init`.
pub fn serialize_default_config_toml() -> String {
    "# alnav config.toml — application settings.\n\
     #\n\
     # TUI theme (restart to apply). Builtins and signature accent:\n\
     #   default            cyan; no canvas paint; solid wordmark\n\
     #   onedark            blue\n\
     #   dracula            magenta\n\
     #   everforest         green\n\
     #   tokyo-night        blue      (aliases: TokyoNight, tokyo_night)\n\
     #   catppuccin-mocha   magenta   (aliases: catppuccin, mocha)\n\
     #   gruvbox-dark       yellow    (alias: gruvbox)\n\
     #   nord               cyan\n\
     #   kanagawa           blue      (alias: kanagawa-wave)\n\
     # Optional color overlay: theme.toml in this directory.\n\
     theme = \"default\"\n\
     #\n\
     # picker_left_ratio: width fraction of the left (candidate) pane in pickers.\n\
     # Right pane = 1 - this value. Clamped to [0.2, 0.8]. Default 0.4.\n\
     picker_left_ratio = 0.4\n\
     #\n\
     # picker_preview_enabled: show the right Preview column in pickers.\n\
     # When false, pickers are full-width. The unified manage panel (Space Space)\n\
     # is always full-width regardless of this setting.\n\
     picker_preview_enabled = true\n\
     #\n\
     # recent_files_limit: max paths remembered for Dashboard / C-f (1..=200).\n\
     recent_files_limit = 20\n\
     #\n\
     # log_dirs: directories recursively scanned for Open-file (C-f) fuzzy corpus.\n\
     # Empty = recent-only. Supports ~ expansion. No cwd fallback.\n\
     log_dirs = []\n\
     #\n\
     # log_extensions: case-insensitive suffix filter for corpus files.\n\
     # Empty / omitted → default [\".log\", \".txt\"].\n\
     log_extensions = [\".log\", \".txt\"]\n\
     #\n\
     # restore_session: remember the last Filter / Exclude / Highlight / lock /\n\
     # time-window / cursor / bookmark state per file, and bring it back the next\n\
     # time that file is opened. Sessions are skipped entirely when filter flags are\n\
     # passed on the command line. Set false to disable. Default true.\n\
     restore_session = true\n"
        .to_string()
}

/// Write default config/keymap templates. Returns human-readable status lines.
pub fn init_config_dir(config_dir: &Path, force: bool) -> Result<Vec<String>, String> {
    fs::create_dir_all(config_dir).map_err(|e| format!("create config dir: {e}"))?;
    let mut messages = Vec::new();
    for (name, body) in [
        ("config.toml", serialize_default_config_toml()),
        ("keymap.toml", serialize_default_toml()),
    ] {
        let path = config_dir.join(name);
        if path.exists() && !force {
            messages.push(format!("skip {} (exists)", path.display()));
            continue;
        }
        fs::write(&path, body).map_err(|e| format!("write {}: {e}", path.display()))?;
        let verb = if force && path.exists() {
            "wrote"
        } else {
            "created"
        };
        // path.exists was true before write when force; after write always exists.
        let _ = verb;
        messages.push(format!("wrote {}", path.display()));
    }
    Ok(messages)
}
