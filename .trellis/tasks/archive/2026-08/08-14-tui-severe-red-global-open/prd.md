# Severe row red + Global C-f/C-g source keys

## Goal

Two TUI changes in one session:

1. Severe log rows (E / F / crash signature) must stand out: tag and message use the theme `error` color.
2. Open file / open stream / preset save / preset open must be Global Ctrl chords that work in a real TTY and on the startup Dashboard. Do not use Ctrl+Shift+letter as defaults.

## Background / Confirmed Facts

- `row.severe` already marks E/F and crash-signature lines (`is_severe_row`).
- `C-S-o` / `C-S-l` fail in practice: Cursor/VS Code steal them, and traditional TTYs often drop Shift so the exact modifier match never fires.
- Dashboard previously dispatched only `key.code`, so Global Ctrl chords never ran on the no-arg startup screen.
- Analysis operators (`c` / `C` / `y` / `f` / `t` / `mm` / `dd`) stay two-stage. Dashboard bare `o` still opens the file panel.

## Requirements

### R1 — Severe row paint

- Tag + message foreground uses `theme::severe_entry_style` (`t().error`).
- Line number and timestamp stay muted. Level badges stay `level_badge_style`.
- Fatal and non-E crash rows are Bold; plain Error is red without Bold.
- Keyword / search highlights still overlay. No `Color::Red` in `ui.rs`.

### R2 — Global source / preset keys

| Action | Default |
|--------|---------|
| Open File | `C-f` |
| Open Stream | `C-g` |
| Preset Save | `C-s` |
| Preset Open | `C-o` |

- `KeyContext::Global`. Retired: `of` / `os`, `LogListOpen`, `pending_open`, `KeyContext::Open`.
- One matcher (`dispatch_global_chords`) from LogList Normal **and** Dashboard (after Ctrl+C quit).
- Open File / Open Stream pass `from_dashboard: app.dashboard.is_some()` so Esc returns to Dashboard.
- Do not default-bind `C-S-<letter>`.

## Acceptance Criteria

- [x] E/F/crash tag+msg paint theme error red; F and crash extra Bold; badges unchanged.
- [x] `C-f` opens file panel; `C-g` opens stream panel; `C-S-o` does not open file.
- [x] Dashboard `C-f` / `C-g` open the same panels with `from_dashboard`.
- [x] `C-s` / `C-o` save/open presets (empty library flashes `NO PRESETS`).
- [x] `of` / `os` / `Space w` / `Space o` no longer open source or preset.
- [x] `cargo test -p alnav --bin alnav` green (616).

## Notes

Shipped on `master` as `5e4b89c`. Spec updates: theme-system, directory-structure (Global chords), status-help, command-palette (Dashboard `C-p` still no-op).
