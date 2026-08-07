//! Named Filter/Exclude/Highlight presets under `$config_dir/presets/`.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::filter_model::{ExcludeEntry, Group, GroupList};
use crate::highlight_model::{HighlightGroup, HighlightGroupList};
use crate::input::{build_group_from_chips, Chip, ChipField};

pub const PRESET_VERSION: u32 = 1;
pub const PRESETS_DIR_NAME: &str = "presets";
pub const NAME_MAX_LEN: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetChip {
    pub field: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetFilterGroup {
    pub chips: Vec<PresetChip>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetHighlight {
    pub pattern: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preset {
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub filters: Vec<PresetFilterGroup>,
    #[serde(default)]
    pub excludes: Vec<PresetFilterGroup>,
    #[serde(default)]
    pub highlights: Vec<PresetHighlight>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresetNamePurpose {
    Save,
    Rename { from: String },
}

#[derive(Debug, Clone)]
pub struct PresetNameDialog {
    pub purpose: PresetNamePurpose,
    pub field: crate::text_field::TextField,
    /// When true, show overwrite confirmation for the pending name.
    pub confirm_overwrite: bool,
}

impl PresetNameDialog {
    pub fn save() -> Self {
        Self {
            purpose: PresetNamePurpose::Save,
            field: crate::text_field::TextField::new(),
            confirm_overwrite: false,
        }
    }

    pub fn rename(from: &str) -> Self {
        Self {
            purpose: PresetNamePurpose::Rename {
                from: from.to_string(),
            },
            field: crate::text_field::TextField::from_text(from),
            confirm_overwrite: false,
        }
    }
}

/// Validate display/file name: `[A-Za-z0-9._-]{1,64}`.
pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > NAME_MAX_LEN {
        return Err(format!("name length must be 1..={NAME_MAX_LEN}"));
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err("name must be [A-Za-z0-9._-]".into());
    }
    Ok(())
}

pub fn presets_dir(config_dir: &Path) -> PathBuf {
    config_dir.join(PRESETS_DIR_NAME)
}

pub fn preset_path(config_dir: &Path, name: &str) -> PathBuf {
    presets_dir(config_dir).join(format!("{name}.toml"))
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

fn chip_to_wire(chip: &Chip) -> PresetChip {
    PresetChip {
        field: chip.field.keyword().to_string(),
        value: chip.value.clone(),
    }
}

fn wire_to_chip(c: &PresetChip) -> Result<Chip, String> {
    if c.value.is_empty() {
        return Err(format!("empty {} chip", c.field));
    }
    Ok(Chip {
        field: parse_field(&c.field)?,
        value: c.value.clone(),
    })
}

/// Whether any enabled Filter / Exclude / Highlight exists to persist.
pub fn has_savable_rules(groups: &GroupList, highlights: &HighlightGroupList) -> bool {
    groups
        .groups
        .iter()
        .any(|g| g.enabled && !g.chips.is_empty())
        || groups.excludes.iter().any(|e| e.enabled)
        || highlights
            .groups
            .iter()
            .any(|g| g.enabled && !g.pattern.is_empty())
}

/// Capture enabled Filter / Exclude / Highlight only. `None` if nothing to save.
pub fn capture(
    groups: &GroupList,
    highlights: &HighlightGroupList,
    name: &str,
) -> Result<Option<Preset>, String> {
    validate_name(name)?;
    if !has_savable_rules(groups, highlights) {
        return Ok(None);
    }
    let filters: Vec<PresetFilterGroup> = groups
        .groups
        .iter()
        .filter(|g| g.enabled && !g.chips.is_empty())
        .map(|g| PresetFilterGroup {
            chips: g.chips.iter().map(chip_to_wire).collect(),
        })
        .collect();
    let excludes: Vec<PresetFilterGroup> = groups
        .excludes
        .iter()
        .filter(|e| e.enabled)
        .map(|e| PresetFilterGroup {
            chips: vec![chip_to_wire(&e.chip)],
        })
        .collect();
    let hl: Vec<PresetHighlight> = highlights
        .groups
        .iter()
        .filter(|g| g.enabled && !g.pattern.is_empty())
        .map(|g| PresetHighlight {
            pattern: g.pattern.clone(),
        })
        .collect();
    Ok(Some(Preset {
        version: PRESET_VERSION,
        name: name.to_string(),
        filters,
        excludes,
        highlights: hl,
    }))
}

/// Materialize runtime groups from a preset (all enabled).
pub fn materialize(preset: &Preset) -> Result<(GroupList, HighlightGroupList), String> {
    if preset.version != PRESET_VERSION {
        return Err(format!(
            "unsupported preset version {} (want {PRESET_VERSION})",
            preset.version
        ));
    }
    validate_name(&preset.name)?;
    let mut groups = GroupList::default();
    for fg in &preset.filters {
        let chips: Result<Vec<_>, _> = fg.chips.iter().map(wire_to_chip).collect();
        let chips = chips?;
        match build_group_from_chips(chips, true)? {
            Some(g) => groups.groups.push(g),
            None => {}
        }
    }
    for eg in &preset.excludes {
        for c in &eg.chips {
            let chip = wire_to_chip(c)?;
            groups.push_exclude(chip).map_err(|e| e)?;
        }
    }
    let mut highlights = HighlightGroupList::default();
    for h in &preset.highlights {
        let Some(g) = HighlightGroup::from_pattern(&h.pattern) else {
            return Err("empty highlight pattern".into());
        };
        highlights.groups.push(g);
    }
    Ok((groups, highlights))
}

pub fn to_toml(preset: &Preset) -> Result<String, String> {
    toml::to_string_pretty(preset).map_err(|e| e.to_string())
}

pub fn from_toml(text: &str) -> Result<Preset, String> {
    let preset: Preset = toml::from_str(text).map_err(|e| e.to_string())?;
    // Validate by materializing (rejects bad fields / empty chips).
    let _ = materialize(&preset)?;
    Ok(preset)
}

pub fn ensure_presets_dir(config_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(presets_dir(config_dir)).map_err(|e| e.to_string())
}

pub fn save(config_dir: &Path, preset: &Preset) -> Result<(), String> {
    validate_name(&preset.name)?;
    ensure_presets_dir(config_dir)?;
    let path = preset_path(config_dir, &preset.name);
    let text = to_toml(preset)?;
    fs::write(&path, text).map_err(|e| e.to_string())
}

pub fn exists(config_dir: &Path, name: &str) -> bool {
    preset_path(config_dir, name).is_file()
}

pub fn delete(config_dir: &Path, name: &str) -> Result<(), String> {
    validate_name(name)?;
    let path = preset_path(config_dir, name);
    if path.is_file() {
        fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn rename(config_dir: &Path, from: &str, to: &str) -> Result<(), String> {
    validate_name(from)?;
    validate_name(to)?;
    if from == to {
        return Ok(());
    }
    let src = preset_path(config_dir, from);
    if !src.is_file() {
        return Err(format!("preset '{from}' not found"));
    }
    let mut preset = from_toml(&fs::read_to_string(&src).map_err(|e| e.to_string())?)?;
    preset.name = to.to_string();
    save(config_dir, &preset)?;
    if from != to {
        let _ = fs::remove_file(&src);
    }
    Ok(())
}

/// List valid presets. Returns `(presets sorted by name, skipped_invalid_count)`.
pub fn list(config_dir: &Path) -> (Vec<Preset>, usize) {
    let dir = presets_dir(config_dir);
    let Ok(entries) = fs::read_dir(&dir) else {
        return (Vec::new(), 0);
    };
    let mut presets = Vec::new();
    let mut skipped = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            skipped += 1;
            continue;
        };
        match from_toml(&text) {
            Ok(mut p) => {
                // Prefer filename stem when it is a valid name (name==file contract).
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if validate_name(stem).is_ok() {
                        p.name = stem.to_string();
                    }
                }
                presets.push(p);
            }
            Err(_) => skipped += 1,
        }
    }
    presets.sort_by(|a, b| a.name.cmp(&b.name));
    (presets, skipped)
}

/// Convenience: rebuild Group/Highlight lists for apply (drops disabled semantics).
pub fn apply_lists(
    preset: &Preset,
) -> Result<(Vec<Group>, Vec<ExcludeEntry>, Vec<HighlightGroup>), String> {
    let (groups, highlights) = materialize(preset)?;
    Ok((groups.groups, groups.excludes, highlights.groups))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fuzzy::SameFieldOp;
    use tempfile::TempDir;

    fn sample_groups() -> (GroupList, HighlightGroupList) {
        let mut groups = GroupList::default();
        groups.groups.push(
            build_group_from_chips(
                vec![Chip {
                    field: ChipField::Tag,
                    value: "MyApp".into(),
                }],
                true,
            )
            .unwrap()
            .unwrap(),
        );
        let mut off = build_group_from_chips(
            vec![Chip {
                field: ChipField::Msg,
                value: "noise".into(),
            }],
            true,
        )
        .unwrap()
        .unwrap();
        off.enabled = false;
        groups.groups.push(off);
        assert!(groups
            .push_exclude(Chip {
                field: ChipField::Tag,
                value: "Spam".into(),
            })
            .unwrap());
        let mut highlights = HighlightGroupList::default();
        highlights
            .groups
            .push(HighlightGroup::from_pattern("error").unwrap());
        let mut hl_off = HighlightGroup::from_pattern("skip").unwrap();
        hl_off.enabled = false;
        highlights.groups.push(hl_off);
        (groups, highlights)
    }

    #[test]
    fn validate_name_rules() {
        assert!(validate_name("crash-login").is_ok());
        assert!(validate_name("A_b.1-2").is_ok());
        assert!(validate_name("").is_err());
        assert!(validate_name("has space").is_err());
        assert!(validate_name("中文").is_err());
        assert!(validate_name(&"x".repeat(65)).is_err());
    }

    #[test]
    fn capture_skips_disabled_and_empty() {
        let (groups, highlights) = sample_groups();
        let preset = capture(&groups, &highlights, "crash-login")
            .unwrap()
            .expect("non-empty");
        assert_eq!(preset.filters.len(), 1);
        assert_eq!(preset.filters[0].chips[0].value, "MyApp");
        assert_eq!(preset.excludes.len(), 1);
        assert_eq!(preset.highlights.len(), 1);
        assert_eq!(preset.highlights[0].pattern, "error");
    }

    #[test]
    fn capture_all_disabled_is_none() {
        let mut groups = GroupList::default();
        let mut g = build_group_from_chips(
            vec![Chip {
                field: ChipField::Tag,
                value: "X".into(),
            }],
            true,
        )
        .unwrap()
        .unwrap();
        g.enabled = false;
        groups.groups.push(g);
        assert!(capture(&groups, &HighlightGroupList::default(), "empty")
            .unwrap()
            .is_none());
    }

    #[test]
    fn round_trip_toml_and_disk() {
        let (groups, highlights) = sample_groups();
        let preset = capture(&groups, &highlights, "crash-login")
            .unwrap()
            .unwrap();
        let text = to_toml(&preset).unwrap();
        let loaded = from_toml(&text).unwrap();
        assert_eq!(loaded.name, "crash-login");
        let (g2, h2) = materialize(&loaded).unwrap();
        assert_eq!(g2.groups.len(), 1);
        assert_eq!(g2.groups[0].same_field_op, SameFieldOp::And);
        assert_eq!(g2.excludes.len(), 1);
        assert_eq!(h2.groups.len(), 1);

        let dir = TempDir::new().unwrap();
        save(dir.path(), &preset).unwrap();
        assert!(exists(dir.path(), "crash-login"));
        let (list, skipped) = list(dir.path());
        assert_eq!(skipped, 0);
        assert_eq!(list.len(), 1);
        rename(dir.path(), "crash-login", "login-v2").unwrap();
        assert!(!exists(dir.path(), "crash-login"));
        assert!(exists(dir.path(), "login-v2"));
        delete(dir.path(), "login-v2").unwrap();
        assert!(!exists(dir.path(), "login-v2"));
    }

    #[test]
    fn list_skips_bad_toml() {
        let dir = TempDir::new().unwrap();
        ensure_presets_dir(dir.path()).unwrap();
        fs::write(presets_dir(dir.path()).join("bad.toml"), "not = [valid").unwrap();
        let (groups, highlights) = sample_groups();
        let preset = capture(&groups, &highlights, "ok").unwrap().unwrap();
        save(dir.path(), &preset).unwrap();
        let (list, skipped) = list(dir.path());
        assert_eq!(list.len(), 1);
        assert_eq!(skipped, 1);
    }
}
