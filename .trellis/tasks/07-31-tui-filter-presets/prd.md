# TUI named filter presets save/load

## Goal

Add a reusable named **filter preset** library for the alnav TUI: save the
current enabled Filter / Exclude / Highlight rules under the config directory,
then search and one-key apply (replace those three rule sets).

This is **not** a full session snapshot (no bookmarks, time window, lock,
search, or log source).

## Requirements

### Persist

- Save only **enabled** Filter groups, Exclude groups, and Highlight groups.
- Do not persist disabled (`di`) state, `time_bound`, lock pid/tid, bookmarks,
  Search, or source (`-f` / `--hdc` / `--adb`).
- If no enabled rules remain after skipping disabled groups, refuse save and
  flash.
- Storage: `$config_dir/presets/<name>.toml` (config dir =
  `--config-path` / `$ALNAV_HOME` / `~/.config/alnav`).
- Format: TOML `version = 1` with `name`, `[[filters]]` / `[[excludes]]` chip
  lists, `[[highlights]]` with `pattern`.
- Display name = filename; strict slug `[A-Za-z0-9._-]{1,64}`.
- Name collision: Confirm then overwrite.

### Keys / UX

- `Space w`: save — simple name dialog, **no** candidates, **no** Preview.
- `Space o`: open/apply — Picker Manage with fuzzy candidates + Preview.
- Preview renders Filter → Exclude → Highlight like existing chip strips
  (stacked top to bottom).
- Open picker is **pure Manage** (no Manage→New auto-switch). Create only via
  `Space w`.
- Empty library: `Space o` does not open; flash only.
- Manage: `Ctrl-X` rename (reuse save name dialog; confirm logic differs),
  `Delete` / `Ctrl-Backspace` delete with Confirm.
- Bad/unreadable TOML: skip from list; if any skipped, flash summary count on
  open.

### Apply

- Clear and replace Filter / Exclude / Highlight only.
- Leave time_bound, lock, bookmarks, Search unchanged.
- `following = false`; keep current row when still visible, else clamp to
  nearest visible.

### Out of scope

- Bookmarks / time / lock / search / source in presets.
- Empty preset as “clear all” shortcut.
- Changing `yc` CLI export behavior (remains independent).

## Acceptance Criteria

- [x] `Space w` saves enabled F/E/H to `presets/<name>.toml` after name entry.
- [x] Empty enabled rules → save refused with flash.
- [x] Duplicate name → Confirm → overwrite.
- [x] `Space o` lists valid presets, fuzzy search, Preview chip-strip layout.
- [x] Enter applies: replaces F/E/H only; other session state intact;
      following off; cursor retention as specified.
- [x] Empty presets dir → `Space o` flashes, no panel.
- [x] Rename / delete work from Manage; rename reuses save name UI.
- [x] Invalid files skipped + flash count on open.
- [x] Help / status hints document `Space w` / `Space o`.
- [x] Unit tests cover serialize/deserialize, name validation, apply replace
      semantics; `cargo test -p alnav` green.

## Notes

- Coexists with H10 `yc` export; does not replace it.
- Grilling consensus captured 2026-07-31.
