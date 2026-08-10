# Design: Dashboard + Source Switch

## Architecture

```
No source ──► DashboardView (App.source = None)
                 │ h/a/Enter(stream) ──► bind LiveSession + StreamStore
                 │ o / Open file… / 1-9 ──► SourcePicker(File) ──► bind FileStore
Bound source ──► Normal TUI
                 │ of ──► SourcePicker(File) [recent+path+preview]
                 │ os ──► SourcePicker(Stream) [centered HDC|ADB]
                 └ Enter ──► reset_for_source_switch() + rebind
```

## New / touched modules

| Module | Role |
|--------|------|
| `recent.rs` (new) | Load/save `recent_files.toml` (or json list) under config dir; push/cap API |
| `path_complete.rs` (new) | `complete(prefix) -> Vec<PathCandidate>` via `read_dir`; `~` + relative |
| `source_switch.rs` or `dashboard.rs` (new) | Dashboard list model + SourcePicker kinds |
| `app.rs` | `source_bound` / optional store; `reset_for_source_switch`; open panels |
| `main.rs` | Relax `validate_source`; deferred bind; operator `o`; live swap |
| `config.rs` | `recent_files_limit` |
| `picker.rs` / `ui.rs` | File source picker + centered stream picker + Dashboard render |
| `preview.rs` | Async file head preview (10 lines) for source file picker |
| `keymap.rs` / `help.rs` | ActionIds + Help entries |
| `theme.rs` | Glyphs / styles for Dashboard items (no raw Color in ui) |

## State model

- `App` gains an explicit unbound mode when `RowStore` is not yet created **or** use a placeholder empty `StreamStore` + `source: Option<ExportSource>` / `DashboardActive` flag.
- Preferred: `app.boot: BootState::{Dashboard, Bound}` so render/input branch cleanly.
- `export_source` updated on every successful bind (for `yc`).

## Switch reset contract

`reset_for_source_switch(&mut self)`:

- Clear: rows/matched/visible, bookmarks, search, lock, time_bound, visual, pending, overlays, picker (except the confirming one), flash as needed, list_offset/cursor.
- Keep: `groups`, `excludes`, `highlight_groups` (+ enabled flags).
- Set `following = true`, `focus = LogList`.
- Tear down prior live child / file mmap before opening the new source.

## File SourcePicker

- Manage candidates = recent paths (nucleo fuzzy).
- Draft/New = typed path; on change, merge path-completion candidates into left list.
- Tab applies selected completion / longest common prefix.
- Right Preview: spawn async reader of first 10 lines; generation token cancels stale results.
- Enter: validate file exists + is file → record recent → bind FileStore.

## Stream SourcePicker

- Centered modal (no preview column): two rows HDC / ADB.
- Enter / or single-letter when focused: spawn live + StreamStore.

## Dashboard

- Not a PickerSession; dedicated render in `ui.rs` filling the frame (or top modal shell + list).
- Key dispatch only while `BootState::Dashboard`.
- “Open file…” opens File SourcePicker; Esc returns to Dashboard (still unbound).

## Config

```toml
recent_files_limit = 20
```

Persist paths in `$ALNAV_HOME/recent_files.toml`:

```toml
files = ["/abs/a.log", "/abs/b.log"]
```

## Compatibility

- CLI `alnav grep` unchanged.
- Existing `-f` / `--hdc` / `--adb` startup paths unchanged (skip Dashboard).
- Compat argv0 aliases unchanged.
