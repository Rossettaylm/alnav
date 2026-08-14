# Design: TUI filter presets

## Boundaries

| In | Out |
|----|-----|
| Named preset CRUD under `presets/` | Session snapshot, bookmarks, time, lock |
| TOML v1 schema | CLI `alnav grep` loading presets |
| Leader `Space w` / `Space o` | New bare keys conflicting with `p`/`P` |

## Module layout

New module `alnav/src/preset.rs`:

- `Preset` struct + TOML serde
- `validate_name` / slug = name
- `presets_dir(config_dir)`
- `list_presets` → `(Vec<Preset>, skipped: usize)`
- `save_preset` / `delete_preset` / `rename_preset`
- `capture_from_app` (enabled groups only)
- `apply_to_app` (replace F/E/H, following false, cursor retain)

Wire into:

- `app.rs` — open/save/rename/delete/apply actions; cursor retention helper
- `main.rs` — Leader `w` / `o` dispatch; picker keys for rename/delete
- `picker.rs` — `PickerKind::Preset` (Manage-only; no auto-New)
- `ui.rs` — save name modal (simple); open Preview as chip strips
- `help.rs` — Leader hints + Help catalog
- `config.rs` — expose/reuse config dir (already resolved at startup)

## Data contracts

```toml
version = 1
name = "crash-login"

[[filters]]
chips = [
  { field = "tag", value = "MyApp" },
  { field = "level", value = "E" },
]

[[excludes]]
chips = [
  { field = "tag", value = "Spam" },
]

[[highlights]]
pattern = "error|fail"
```

- `field` maps to `ChipField` string form used elsewhere.
- On load, rebuild `Group` / `ExcludeEntry` / `HighlightGroup` via existing
  builders (`build_group` / highlight constructors). All applied as enabled.

## UI flows

### Save (`Space w`)

1. If capture empty → flash, abort.
2. Open name dialog (reuse draft-line UX; no candidate list, no Preview).
3. Enter → validate name; if file exists → Confirm overwrite; else write.
4. Success → close, flash saved.

### Open (`Space o`)

1. `list_presets`; if zero valid → flash, abort.
2. If skipped > 0 → flash skip count (can combine with open).
3. Picker Manage: fuzzy on `name`; Preview = strip-like F/E/H render of
   selected preset (not live app state).
4. Enter → `apply_to_app`, close picker.
5. Ctrl-X → name dialog prefilled → rename file (+ Confirm if target exists).
6. Delete/Ctrl-Backspace → Confirm → unlink file, refresh list; if list empty
   close picker.

### Apply cursor policy

- `following = false`
- Remember current row identity (stream: `row_id`; file: line index / row_id)
- After refilter, select that row if still in `visible`, else clamp

## Compatibility

- Existing picker Confirm / modal chrome patterns.
- Theme: no raw `Color::*` in `ui.rs`; reuse chip/strip styles.
- Config dir lazy-create `presets/` on first successful save.

## Tradeoffs

- Name = filename keeps overwrite/rename simple; no Chinese names (accepted).
- Open cannot create (Manage-only) — clearer split from `Space w`.
