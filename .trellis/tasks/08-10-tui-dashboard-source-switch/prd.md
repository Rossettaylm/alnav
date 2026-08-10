# TUI No-Arg Dashboard + Source Switch

## Goal

Allow `alnav` to start with no `-f` / `--hdc` / `--adb`. Show a Dashboard to pick a live stream or a recent/typed file. While running, switch sources via `of` / `os` without losing Filter / Exclude / Highlight groups.

## Background

- Today `validate_source` requires one of `-f`, `--hdc`, or `--adb`.
- There is no persisted recent-file list.
- Live sessions are bound at process start; switching requires restart.
- Grill session locked UX: Dashboard is a one-shot launcher; runtime switch uses search panels.

## Requirements

### R1 — No-arg / deferred source startup

- `alnav` with no source opens Dashboard (no bound store yet).
- Startup filter flags (`--tag`, `-e`, etc.) are allowed and applied after a source is chosen.
- Pre-filled `--since` / `--until` become `time_bound`; cleared if the chosen source is stream.

### R2 — Dashboard (unbound source only)

- Flat navigable list: HDC → ADB → Open file… → recent files (newest first).
- `j`/`k` (and arrows) move; `g`/`G` first/last; `Enter` activates current.
- Hotkeys activate immediately: `h`=HDC, `a`=ADB, `o`=Open file Picker, `1`–`9`=Nth recent file.
- Nerd Font glyphs for file/dir/HDC/ADB/recent.
- After a source binds, Dashboard never returns.

### R3 — Recent files persistence

- Store absolute paths under the config directory; configurable limit via `config.toml` (`recent_files_limit`, default 20).
- Record only after a successful open.
- Missing paths may still appear in lists; Enter flashes error (and may drop the entry).

### R4 — Runtime source switch

- Normal + LogList only: `o`+`f` file panel; `o`+`s` centered HDC/ADB panel (no preview).
- Supports stream↔file, file→file, hdc↔adb.
- Opening a panel does not clear; **Enter confirms** then clear+switch; Esc cancels with no change.
- On confirm: keep only Filter / Exclude / Highlight; reset buffer, following, lock, time_bound, bookmarks, search, overlays, pending ops.
- Stream picks use `device=None` (multi-device remains CLI `--device`).

### R5 — File open panel

- Reuse Picker shell: recent fuzzy list + typed path + local path completion (no new UI crate).
- Async Preview: first 10 lines plain text; cancel on selection change; dir/unreadable short status; non-UTF-8 lossy.
- Dashboard “Open file…” uses the same panel.

### R6 — Keys / Help / keymap

- Wire `of`/`os` and Dashboard keys into Help and `keymap.toml` ActionIds where applicable.

## Acceptance Criteria

- [x] AC1: `alnav` with no source opens Dashboard; with `-f`/`--hdc`/`--adb` skips Dashboard.
- [x] AC2: Dashboard `h`/`a`/`o`/`1`–`9`/`j`/`k`/`Enter` behave as specified; source bind dismisses Dashboard permanently.
- [x] AC3: Successful file open updates recent list (capped); persists across restarts.
- [x] AC4: `of`/`os` only in Normal+LogList; Esc cancels; Enter clears non-F/E/H state and switches source.
- [x] AC5: File panel supports recent + path type + Tab/dir completion + async 10-line preview.
- [x] AC6: Stream panel is centered, HDC/ADB only, no preview.
- [x] AC7: Startup filters survive until source bind; stream bind clears `time_bound`.
- [x] AC8: `cargo test -p alnav --bin alnav` green; Help lists new keys.

## Out of Scope

- Multi-device picker / `--device` UI
- Full filesystem browser UI / third-party finder crates
- Returning to Dashboard after bind
- stdin pipe TUI source
- Windows-specific path UX

## Key Decisions

| Decision | Choice |
|----------|--------|
| Dashboard lifecycle | One-shot until source bound |
| Clear timing | On Enter confirm only |
| Preserve on switch | Filter / Exclude / Highlight only |
| Path completion | Local thin `read_dir` helper |
| Multi-device | Deferred (CLI only) |
| File preview | Async first 10 plain lines |
