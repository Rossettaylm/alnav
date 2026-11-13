# Changelog

## Unreleased

- **Breaking:** the `aloggrep`, `alg`, and `aloggrep-tui` compat binaries are gone; use `alnav grep` and `alnav`.
- Removed the unreachable standalone Input box (Insert mode) and its `[input]` keymap section; Filter / Exclude are created only through the picker (`;`, `` ` ``, `Space Space`, `5`).
- **Breaking:** `alnav grep --pid` / `--tid` match exactly (`--pid 12` no longer matches `123`). Use `-e 'pid ~ REGEX'` for pattern matching.
- **Breaking:** `--package` and `-e 'pkg ~ …'` no longer fall back to tag / msg on lines without a package (threadtime / brief); such lines never match.
- `-e` gains `pid == ID`, `tid == ID`, `time >= T`, `time <= T`, and `\"` / `\\` escapes in quoted values. Fixed a hang on bare words containing `>` (e.g. `msg ~ a>b`).
- `yc` exports one exact `alnav grep -i -e '…'`: lock becomes `pid ==` / `tid ==`, the time window `time >= / <=`, multi-word chips and startup OR-groups keep their TUI meaning. The command selects the same parsed rows the TUI shows.
- TUI Filter / Exclude / lock / time window now compile to one `Expr` shared by stream intake, the file scan, and Preview. Filter Preview now honours the time window.

## 0.3.0 — 2026-11-12

- `/` is a highlight find-or-create picker: search existing highlights, Enter activates and jumps; a trailing create row appears when the query is not an exact hit (including prefixes of existing patterns).
- Bookmarks become a snapshot compare tray: `ma` pins an owned copy, `mm` opens a large panel sorted by log time with Δt; live pins stay visible on the LogList through Filter / Exclude / lock / time window.
- File-mode time histogram on `th`: bucketed overlay over visible rows; Enter jumps, `s` binds the bucket as the time window, Tab cycles 10s / 1m / 5m.
- `Ctrl-K` (and command palette *Clear All Rules*) clears every Filter / Exclude / Highlight after a centered confirm; lock, time window, and bookmarks are kept.
- Per-file session restore: Filter / Exclude / Highlight / lock / time window / view focus / cursor / bookmarks are saved under `$config_dir/sessions/` and restored the next time the same file is opened. Disable with `restore_session = false`; any CLI filter flag skips read and write for that run.
- Unified Manage: `Ctrl-T` bulk enable / disable.
- `--hdc` follows live hilog from an empty buffer instead of replaying the device ring buffer, so startup no longer stalls.
- Internal: TUI split into `app/`, `ui/`, `handlers/`, `keymap/`, `store/` modules; one declarative action table, a single pending-chord slot, and one overlay precedence for key routing and drawing. Key and draw now agree when Detail is open under Summary / Histogram.

## 0.2.4 — 2026-08-18

- Two-level Help (`?`): Home plus seven zone pages, design contracts, and ignore-case `/` search.
- Command palette on `C-p` from LogList and Filter / Exclude / Highlight strips.
- Severe (E/F/crash) tag and message use theme error red; Open File / Stream on `C-f` / `C-g`.
- Status bar reweighted toward resident follow and device state; remaining chrome copy in English.
- Live disconnect indicator is a single-cell plug.
- Pushing a `v*` tag publishes `alnav-core` then `alnav` to crates.io and opens a GitHub Release.
- loggrep-analyzer skill retargeted at `alnav grep`.

## 0.2.3 — 2026-08-13

- Dual-language README: English default (`README.md`) with a title link to [中文](README.zh.md).
- Side-by-side Dashboard and Log list screenshots.
- Shorter homepage: dropped architecture internals, exit-code table, and the bundled skill unzip block.

## 0.2.2 — 2026-08-13

- Nine builtin TUI palettes via `config.toml` `theme` (`default`, OneDark, Dracula, Everforest, Tokyo Night, Catppuccin Mocha, Gruvbox dark, Nord, Kanagawa Wave).
- Per-theme signature accent and Dashboard Unicode wordmark ramp.
- Optional `theme.toml` overlay (`[palette]` then semantic tokens). Templates: [`alnav/examples/config.toml`](alnav/examples/config.toml), [`alnav/examples/theme.toml`](alnav/examples/theme.toml).
- CLI (`alnav grep`) colors unchanged (`alnav-core::logcolor`).
