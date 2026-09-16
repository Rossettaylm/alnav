//! Per-file analysis session under `$config_dir/sessions/`.
//!
//! Sole owner of the on-disk session shape: nothing else in the crate knows the
//! wire format. Keyed by FNV-1a of the normalized absolute log path (R1), so one
//! archive exists per log file. `load` / `save` / `clear` are the only entry
//! points; `save` writes through a temp file + rename (R3) and trims the
//! directory to [`MAX_SESSION_FILES`] by mtime.
//!
//! The [`crate::preset`] wire format is deliberately **not** reused: `Preset`
//! drops disabled groups, while a session must round-trip `enabled` (R4).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::filter_model::{ExcludeEntry, GroupList, TimeBound};
use crate::highlight_model::{HighlightGroup, HighlightGroupList};
use crate::input::{build_group_from_chips, Chip, ChipField};

pub const SESSION_VERSION: u32 = 1;
pub const SESSIONS_DIR_NAME: &str = "sessions";
/// Directory capacity; the oldest archives are evicted by mtime past this (R1).
pub const MAX_SESSION_FILES: usize = 50;

/// FNV-1a 64-bit. Hand-written to avoid a dependency for a 10-line hash; only
/// used for file naming, never for security.
fn fnv1a64(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in s.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

pub fn sessions_dir(config_dir: &Path) -> PathBuf {
    config_dir.join(SESSIONS_DIR_NAME)
}

/// Normalized absolute path string, mirroring `recent::normalize_path`.
/// `None` when the path yields nothing usable as a key.
pub fn normalize_source(path: &Path) -> Option<String> {
    let s = if let Ok(canon) = path.canonicalize() {
        canon.display().to_string()
    } else if path.is_absolute() {
        path.display().to_string()
    } else {
        match std::env::current_dir() {
            Ok(cwd) => cwd.join(path).display().to_string(),
            Err(_) => path.display().to_string(),
        }
    };
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

pub fn session_path(config_dir: &Path, source: &Path) -> PathBuf {
    let key = normalize_source(source).unwrap_or_default();
    sessions_dir(config_dir).join(format!("{:016x}.toml", fnv1a64(&key)))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionChip {
    pub field: String,
    pub value: String,
}

/// A Filter group, or (single-chip) an Exclude entry. `enabled` is the reason
/// this cannot share [`crate::preset::PresetFilterGroup`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionFilterGroup {
    pub chips: Vec<SessionChip>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_same_field_op")]
    pub same_field_op: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionHighlight {
    pub pattern: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// A pin as `raw` line plus its stable file `row_id` (`line_index + 1`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionBookmark {
    pub row_id: u64,
    pub raw: String,
}

fn default_true() -> bool {
    true
}

fn default_same_field_op() -> String {
    "and".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionFile {
    pub version: u32,
    /// Normalized absolute path this archive belongs to; mismatches are rejected.
    pub source: String,
    #[serde(default)]
    pub filters: Vec<SessionFilterGroup>,
    #[serde(default)]
    pub excludes: Vec<SessionFilterGroup>,
    #[serde(default)]
    pub highlights: Vec<SessionHighlight>,
    #[serde(default)]
    pub active_highlight: Option<usize>,
    #[serde(default)]
    pub lock_pid: Option<String>,
    #[serde(default)]
    pub lock_tid: Option<String>,
    #[serde(default)]
    pub time_since: Option<String>,
    #[serde(default)]
    pub time_until: Option<String>,
    #[serde(default)]
    pub view_focus_highlight: bool,
    #[serde(default)]
    pub view_focus_severe: bool,
    #[serde(default)]
    pub cursor_row_id: u64,
    #[serde(default)]
    pub list_offset: usize,
    #[serde(default)]
    pub bookmarks: Vec<SessionBookmark>,
}

impl SessionFile {
    pub fn new(source: String) -> Self {
        Self {
            version: SESSION_VERSION,
            source,
            filters: Vec::new(),
            excludes: Vec::new(),
            highlights: Vec::new(),
            active_highlight: None,
            lock_pid: None,
            lock_tid: None,
            time_since: None,
            time_until: None,
            view_focus_highlight: false,
            view_focus_severe: false,
            cursor_row_id: 0,
            list_offset: 0,
            bookmarks: Vec::new(),
        }
    }

    /// R6: nothing configured, no pins, and the cursor still at (or before) the
    /// first row. Such a session is not worth overwriting an existing archive
    /// with. Lock, time window and view focus count as configuration — a run
    /// that only set those is a session the user built deliberately.
    pub fn is_degenerate(&self) -> bool {
        self.filters.is_empty()
            && self.excludes.is_empty()
            && self.highlights.is_empty()
            && self.bookmarks.is_empty()
            && self.lock_pid.is_none()
            && self.lock_tid.is_none()
            && self.time_since.is_none()
            && self.time_until.is_none()
            && !self.view_focus_highlight
            && !self.view_focus_severe
            && self.cursor_row_id <= 1
    }

    pub fn is_active(&self) -> bool {
        !self.is_degenerate()
    }

    pub fn time_bound(&self) -> Option<TimeBound> {
        if self.time_since.is_none() && self.time_until.is_none() {
            return None;
        }
        Some(TimeBound {
            since: self.time_since.clone(),
            until: self.time_until.clone(),
        })
    }
}

/// Outcome of reading an archive. Callers treat `Missing` and `Invalid` alike
/// (no session); only `Invalid` surfaces a flash (R9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionLoad {
    Missing,
    Loaded(SessionFile),
    Invalid(String),
}

fn parse_field(s: &str) -> Result<ChipField, String> {
    match s {
        "tag" => Ok(ChipField::Tag),
        "msg" => Ok(ChipField::Msg),
        "pkg" => Ok(ChipField::Pkg),
        "pid" => Ok(ChipField::Pid),
        "tid" => Ok(ChipField::Tid),
        "level" => Ok(ChipField::Level),
        other => Err(format!("unknown field '{other}'")),
    }
}

pub fn chip_to_wire(chip: &Chip) -> SessionChip {
    SessionChip {
        field: chip.field.keyword().to_string(),
        value: chip.value.clone(),
    }
}

pub fn wire_to_chip(c: &SessionChip) -> Result<Chip, String> {
    if c.value.is_empty() {
        return Err(format!("empty {} chip", c.field));
    }
    Ok(Chip {
        field: parse_field(&c.field)?,
        value: c.value.clone(),
    })
}

pub fn same_field_op_to_wire(op: crate::fuzzy::SameFieldOp) -> String {
    match op {
        crate::fuzzy::SameFieldOp::And => "and".into(),
        crate::fuzzy::SameFieldOp::Or => "or".into(),
    }
}

pub fn wire_to_same_field_op(s: &str) -> Result<crate::fuzzy::SameFieldOp, String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "and" => Ok(crate::fuzzy::SameFieldOp::And),
        "or" => Ok(crate::fuzzy::SameFieldOp::Or),
        other => Err(format!("unknown same_field_op '{other}'")),
    }
}

/// Materialize runtime rule lists, preserving each entry's `enabled` flag.
pub fn materialize_rules(
    file: &SessionFile,
) -> Result<(GroupList, HighlightGroupList, Option<usize>), String> {
    let mut groups = GroupList::default();
    for fg in &file.filters {
        let chips: Vec<Chip> = fg
            .chips
            .iter()
            .map(wire_to_chip)
            .collect::<Result<_, _>>()?;
        let Some(mut group) = build_group_from_chips(chips, true)? else {
            continue;
        };
        group.enabled = fg.enabled;
        group.same_field_op = wire_to_same_field_op(&fg.same_field_op)?;
        groups.groups.push(group);
    }
    for eg in &file.excludes {
        for c in &eg.chips {
            let chip = wire_to_chip(c)?;
            if groups.excludes.iter().any(|e| e.same_chip_as(&chip)) {
                continue;
            }
            groups.excludes.push(ExcludeEntry {
                chip,
                enabled: eg.enabled,
            });
        }
    }
    let mut highlights = HighlightGroupList::default();
    for h in &file.highlights {
        let Some(mut group) = HighlightGroup::from_pattern(&h.pattern) else {
            return Err("empty highlight pattern".into());
        };
        group.enabled = h.enabled;
        highlights.groups.push(group);
    }
    let active = file
        .active_highlight
        .filter(|i| *i < highlights.groups.len());
    Ok((groups, highlights, active))
}

pub fn to_toml(file: &SessionFile) -> Result<String, String> {
    toml::to_string_pretty(file).map_err(|e| e.to_string())
}

/// Parse + validate. Rejects bad versions, unknown chip fields, empty values.
pub fn from_toml(text: &str) -> Result<SessionFile, String> {
    let file: SessionFile = toml::from_str(text).map_err(|e| e.to_string())?;
    if file.version != SESSION_VERSION {
        return Err(format!(
            "unsupported version {} (want {SESSION_VERSION})",
            file.version
        ));
    }
    if file.source.is_empty() {
        return Err("empty source path".into());
    }
    let _ = materialize_rules(&file)?;
    Ok(file)
}

pub fn load(config_dir: &Path, source: &Path) -> SessionLoad {
    let Some(key) = normalize_source(source) else {
        return SessionLoad::Missing;
    };
    let path = session_path(config_dir, source);
    if !path.is_file() {
        return SessionLoad::Missing;
    }
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => return SessionLoad::Invalid(format!("read: {e}")),
    };
    match from_toml(&text) {
        Ok(file) if file.source != key => SessionLoad::Invalid("source path mismatch".into()),
        Ok(file) => SessionLoad::Loaded(file),
        Err(e) => SessionLoad::Invalid(e),
    }
}

/// Write via temp file + rename so a crashed write never truncates the archive.
pub fn save(config_dir: &Path, source: &Path, file: &SessionFile) -> Result<(), String> {
    let dir = sessions_dir(config_dir);
    fs::create_dir_all(&dir).map_err(|e| format!("create sessions dir: {e}"))?;
    let target = session_path(config_dir, source);
    let text = to_toml(file)?;
    let stem = target
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("session");
    let tmp = dir.join(format!(".tmp-{stem}-{}", std::process::id()));
    if let Err(e) = fs::write(&tmp, &text) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("write temp: {e}"));
    }
    if let Err(e) = fs::rename(&tmp, &target) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("rename {}: {e}", target.display()));
    }
    evict_oldest(config_dir);
    Ok(())
}

pub fn clear(config_dir: &Path, source: &Path) -> Result<(), String> {
    let path = session_path(config_dir, source);
    if !path.is_file() {
        return Ok(());
    }
    fs::remove_file(&path).map_err(|e| format!("remove {}: {e}", path.display()))
}

pub fn exists(config_dir: &Path, source: &Path) -> bool {
    session_path(config_dir, source).is_file()
}

/// Trim the directory to [`MAX_SESSION_FILES`], oldest mtime first.
/// Best-effort: a failed eviction must never fail the save that triggered it.
fn evict_oldest(config_dir: &Path) {
    let dir = sessions_dir(config_dir);
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
        files.push((mtime, path));
    }
    if files.len() <= MAX_SESSION_FILES {
        return;
    }
    files.sort_by_key(|(mtime, _)| *mtime);
    let excess = files.len() - MAX_SESSION_FILES;
    for (_, path) in files.into_iter().take(excess) {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fuzzy::SameFieldOp;

    fn sample(source: &str) -> SessionFile {
        let mut file = SessionFile::new(source.to_string());
        file.filters = vec![
            SessionFilterGroup {
                chips: vec![SessionChip {
                    field: "tag".into(),
                    value: "MyApp".into(),
                }],
                enabled: true,
                same_field_op: "and".into(),
            },
            SessionFilterGroup {
                chips: vec![SessionChip {
                    field: "msg".into(),
                    value: "boom".into(),
                }],
                enabled: false,
                same_field_op: "or".into(),
            },
        ];
        file.excludes = vec![SessionFilterGroup {
            chips: vec![SessionChip {
                field: "tag".into(),
                value: "Spam".into(),
            }],
            enabled: false,
            same_field_op: "and".into(),
        }];
        file.highlights = vec![SessionHighlight {
            pattern: "error".into(),
            enabled: false,
        }];
        file.active_highlight = Some(0);
        file.lock_pid = Some("1234".into());
        file.time_since = Some("10:00:00".into());
        file.view_focus_severe = true;
        file.cursor_row_id = 8421;
        file.list_offset = 17;
        file.bookmarks = vec![SessionBookmark {
            row_id: 8123,
            raw: "09-15 10:00:00.000  1234  5678 E MyApp   : boom".into(),
        }];
        file
    }

    #[test]
    fn fnv1a64_known_vectors() {
        assert_eq!(fnv1a64(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64("a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn session_path_is_stable_per_source() {
        let dir = Path::new("/tmp/cfg");
        let a = session_path(dir, Path::new("/tmp/a.log"));
        let b = session_path(dir, Path::new("/tmp/b.log"));
        assert_ne!(a, b);
        assert_eq!(a, session_path(dir, Path::new("/tmp/a.log")));
        assert!(a.starts_with(sessions_dir(dir)));
    }

    #[test]
    fn toml_roundtrip_preserves_all_fields() {
        let file = sample("/tmp/a.log");
        let text = to_toml(&file).unwrap();
        let back = from_toml(&text).unwrap();
        assert_eq!(back, file);
    }

    #[test]
    fn materialize_preserves_disabled_flags() {
        let file = sample("/tmp/a.log");
        let (groups, highlights, active) = materialize_rules(&file).unwrap();
        assert_eq!(groups.groups.len(), 2);
        assert!(groups.groups[0].enabled);
        assert!(!groups.groups[1].enabled);
        assert_eq!(groups.groups[0].same_field_op, SameFieldOp::And);
        assert_eq!(groups.groups[1].same_field_op, SameFieldOp::Or);
        assert_eq!(groups.excludes.len(), 1);
        assert!(!groups.excludes[0].enabled);
        assert_eq!(highlights.groups.len(), 1);
        assert!(!highlights.groups[0].enabled);
        assert_eq!(active, Some(0));
    }

    #[test]
    fn materialize_drops_out_of_range_active_highlight() {
        let mut file = sample("/tmp/a.log");
        file.active_highlight = Some(9);
        let (_, _, active) = materialize_rules(&file).unwrap();
        assert_eq!(active, None);
    }

    #[test]
    fn version_mismatch_is_invalid() {
        let mut file = sample("/tmp/a.log");
        file.version = 2;
        let text = toml::to_string_pretty(&file).unwrap();
        let err = from_toml(&text).unwrap_err();
        assert!(err.contains("unsupported version"), "{err}");
    }

    #[test]
    fn unknown_chip_field_is_invalid() {
        let mut file = SessionFile::new("/tmp/a.log".into());
        file.filters = vec![SessionFilterGroup {
            chips: vec![SessionChip {
                field: "nope".into(),
                value: "x".into(),
            }],
            enabled: true,
            same_field_op: "and".into(),
        }];
        let text = toml::to_string_pretty(&file).unwrap();
        assert!(from_toml(&text).unwrap_err().contains("unknown field"));
    }

    #[test]
    fn bad_same_field_op_is_invalid() {
        let mut file = sample("/tmp/a.log");
        file.filters[0].same_field_op = "xor".into();
        let text = toml::to_string_pretty(&file).unwrap();
        assert!(from_toml(&text)
            .unwrap_err()
            .contains("unknown same_field_op"));
    }

    #[test]
    fn save_load_roundtrip_and_no_temp_left() {
        let cfg = tempfile::tempdir().unwrap();
        let log = cfg.path().join("a.log");
        fs::write(&log, "x").unwrap();
        let key = normalize_source(&log).unwrap();
        let file = sample(&key);
        save(cfg.path(), &log, &file).unwrap();
        match load(cfg.path(), &log) {
            SessionLoad::Loaded(back) => assert_eq!(back, file),
            other => panic!("expected Loaded, got {other:?}"),
        }
        let leftovers: Vec<_> = fs::read_dir(sessions_dir(cfg.path()))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(".tmp-"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn missing_file_is_missing() {
        let cfg = tempfile::tempdir().unwrap();
        let log = cfg.path().join("a.log");
        fs::write(&log, "x").unwrap();
        assert_eq!(load(cfg.path(), &log), SessionLoad::Missing);
    }

    #[test]
    fn source_mismatch_is_invalid() {
        let cfg = tempfile::tempdir().unwrap();
        let log = cfg.path().join("a.log");
        fs::write(&log, "x").unwrap();
        let file = sample("/somewhere/else.log");
        save(cfg.path(), &log, &file).unwrap();
        match load(cfg.path(), &log) {
            SessionLoad::Invalid(e) => assert!(e.contains("mismatch"), "{e}"),
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn corrupt_toml_is_invalid_and_file_kept() {
        let cfg = tempfile::tempdir().unwrap();
        let log = cfg.path().join("a.log");
        fs::write(&log, "x").unwrap();
        let path = session_path(cfg.path(), &log);
        fs::create_dir_all(sessions_dir(cfg.path())).unwrap();
        fs::write(&path, "version = {{{").unwrap();
        assert!(matches!(load(cfg.path(), &log), SessionLoad::Invalid(_)));
        assert!(path.is_file());
    }

    #[test]
    fn clear_removes_archive_and_is_idempotent() {
        let cfg = tempfile::tempdir().unwrap();
        let log = cfg.path().join("a.log");
        fs::write(&log, "x").unwrap();
        let key = normalize_source(&log).unwrap();
        save(cfg.path(), &log, &sample(&key)).unwrap();
        assert!(exists(cfg.path(), &log));
        clear(cfg.path(), &log).unwrap();
        assert!(!exists(cfg.path(), &log));
        clear(cfg.path(), &log).unwrap();
    }

    #[test]
    fn degenerate_detection() {
        let empty = SessionFile::new("/tmp/a.log".into());
        assert!(empty.is_degenerate());
        assert!(!empty.is_active());

        let mut cursor_only = SessionFile::new("/tmp/a.log".into());
        cursor_only.cursor_row_id = 1;
        assert!(cursor_only.is_degenerate());
        cursor_only.cursor_row_id = 2;
        assert!(cursor_only.is_active());

        let mut pinned = SessionFile::new("/tmp/a.log".into());
        pinned.bookmarks = vec![SessionBookmark {
            row_id: 3,
            raw: "x".into(),
        }];
        assert!(pinned.is_active());

        // Lock / time window / view focus are configuration on their own: a run
        // that set only one of them is a session worth keeping.
        let mut locked = SessionFile::new("/tmp/a.log".into());
        locked.lock_pid = Some("1234".into());
        assert!(locked.is_active());

        let mut timed = SessionFile::new("/tmp/a.log".into());
        timed.time_since = Some("09-15 10:00:00".into());
        assert!(timed.is_active());

        let mut focused = SessionFile::new("/tmp/a.log".into());
        focused.view_focus_severe = true;
        assert!(focused.is_active());
    }

    fn toml_names(dir: &Path) -> Vec<String> {
        fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".toml"))
            .collect()
    }

    #[test]
    fn eviction_trims_to_max_and_keeps_newest() {
        let cfg = tempfile::tempdir().unwrap();
        let dir = sessions_dir(cfg.path());
        fs::create_dir_all(&dir).unwrap();
        let total = MAX_SESSION_FILES + 5;
        for i in 0..total {
            fs::write(dir.join(format!("seed{i:04}.toml")), "version = 1").unwrap();
        }
        // Coarse-mtime filesystems could tie the loop writes; this sleep makes
        // `newest.toml` strictly the most recent so the survivor assert is sound.
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(dir.join("newest.toml"), "version = 1").unwrap();

        evict_oldest(cfg.path());

        let remaining = toml_names(&dir);
        assert_eq!(remaining.len(), MAX_SESSION_FILES);
        assert!(remaining.contains(&"newest.toml".to_string()));
    }

    #[test]
    fn eviction_is_noop_under_cap() {
        let cfg = tempfile::tempdir().unwrap();
        let dir = sessions_dir(cfg.path());
        fs::create_dir_all(&dir).unwrap();
        for i in 0..3 {
            fs::write(dir.join(format!("seed{i}.toml")), "version = 1").unwrap();
        }
        evict_oldest(cfg.path());
        assert_eq!(toml_names(&dir).len(), 3);
    }

    #[test]
    fn time_bound_none_when_both_sides_absent() {
        let file = SessionFile::new("/tmp/a.log".into());
        assert!(file.time_bound().is_none());
        let mut one = SessionFile::new("/tmp/a.log".into());
        one.time_until = Some("11:00:00".into());
        let bound = one.time_bound().unwrap();
        assert!(bound.is_active());
        assert_eq!(bound.since, None);
    }
}
