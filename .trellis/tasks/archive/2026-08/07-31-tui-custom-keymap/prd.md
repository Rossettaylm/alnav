# TUI customizable keymap via keymap.toml

## Goal

Replace hard-coded TUI keybindings with a registry-backed keymap loaded from
`$config_dir/keymap.toml` at startup. Custom bindings must drive key dispatch,
status-bar L1/L2 hints, and the Help panel from one source of truth. Provide
`alnav --init` to write English-commented default `config.toml` and
`keymap.toml` templates.

## Requirements

### Registry & store

- Every keyboard action is registered in a Rust `KeymapStore` / action registry
  (context, ActionId, default binding, prefix vs leaf, Help label/detail,
  handler association, capability flags such as file-only).
- Runtime lookup: current context (+ pending prefix state) → binding → action.
- Rust registry is the **sole authority** for defaults; `--init` serializes
  `keymap.toml` from it (no hand-maintained duplicate default file).

### keymap.toml

- Loaded from config dir (`--config-path` > `$ALNAV_HOME` > `~/.config/alnav`).
- Context sections: `[log_list]`, `[picker]`, `[leader]`, `[global]`, etc.
  (aligned with existing Help `ContextKind` family).
- Binding syntax:
  - Single key: string — `"j"`, `"C-s"`, `"S-j"`, `"Space"`, `"Esc"`, …
  - Chord: string array — `["m", "a"]`, `["Space", "Space"]`
  - Modifiers: Emacs-style `C-` / `S-` / `M-`; Shift+letter only as `S-j`
    (bare `"J"` is invalid).
- One binding per action (no alias list in one TOML value). Multiple ActionIds
  may share one handler; Help/status may aggregate display (e.g. `j/k`).
- Unbind: `null` or `""` → action has no trigger; omitted from Help/L1.
- Merge: deep-merge user file onto builtin defaults **per action**.
- Chord model: prefix-suspend (existing `pending_*`); a leaf must not occupy a
  true prefix of another binding unless the shorter one is a prefix action.
- Error policy:
  - TOML / type / key-DSL parse errors → whole-table fallback to defaults + status.
  - Unknown section or unknown action → warn + skip that entry.
  - Key steal / illegal prefix overlap after merge → whole-table fallback.

### Hard reserves & non-keymap input

- Hard-reserved: Normal-mode `C-c` quit path.
- Insert / Search / Picker draft: printable characters are text input, not
  keymap actions.
- `Esc` / `Enter` / draft editing keys (`C-a`, `C-e`, …) remain configurable
  actions with current defaults.
- Mouse wheel: **out of keymap v1** (keep fixed scroll behavior).
- Load once at TUI startup; no hot-reload.

### Capability gating

- File-only actions (e.g. time window `t` / `ts` / `tu`) stay in the registry
  but are filtered out of the active set on `--hdc` / `--adb`.
- Live: no dispatch, no L1/Active, no Help full-catalog entries for gated-out
  actions. Full default documentation lives in generated `keymap.toml`.

### Help & status bar

- L1 / L2 / Help Active / Help catalog key strings come from `KeymapStore`
  formatting of the effective binding.
- Labels/details come from registry metadata (English, as today).
- Unbound actions hidden. Same-handler aggregation allowed for compact L1.

### `--init`

- Flag: `--init` on the TUI binary entry (with `--force` to overwrite).
- Writes **missing** `config.toml` and `keymap.toml` only (not `theme.toml`).
- `--force` overwrites existing targets.
- Comments in generated files: **English**.
- Config dir resolution same as runtime; create directory if needed.
- Exit after write; do not enter TUI.
- `keymap.toml` content serialized from the action registry.

### Out of scope (v1)

- Hot-reload, mouse/scroll in keymap, CLI `grep` keymaps, Windows-specific
  key quirks beyond existing support level, timeout-based chord disambiguation,
  multi-alias syntax on one TOML key.

## Acceptance Criteria

- [x] Builtin defaults reproduce current key behavior (no intentional regressions).
- [x] User `keymap.toml` deep-merges; partial override works.
- [x] Unbind via `null`/`""` removes dispatch and Help/L1 entry.
- [x] Prefix/leaf validation and key conflicts fall back as specified; unknown
      actions warn+skip.
- [x] Status bar and Help show customized key strings for rebound actions.
- [x] Live session hides/disables file-only time actions regardless of keymap.
- [x] `alnav --init` creates missing `config.toml` + `keymap.toml` with English
      comments; `--force` overwrites; skips `theme.toml`.
- [x] Unit tests cover parse DSL, merge, conflict/prefix rules, unbind, serialize
      for `--init`; `cargo test -p alnav` green.

## Notes

- Grilling consensus locked 2026-07-31 (full remap, array chords + string
  singles, context sections, deep merge, minimal hard-reserve, one-to-one,
  `--init`+`--force` without theme, prefix-suspend, registry authority,
  layered errors, capability gate + Help filter, startup-only, no mouse).
