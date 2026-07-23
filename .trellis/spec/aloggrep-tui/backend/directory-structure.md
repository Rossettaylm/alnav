# Directory Structure

> How aloggrep-tui backend code is organized in this project.

---

## Overview

aloggrep-tui is a ratatui/crossterm TUI built atop `aloggrep-core`. Modules
are flat under `src/`, each owning one concern of the state machine, render
pipeline, or input dispatch. Cross-module state flows through `App`.

---

## Directory Layout

```
aloggrep-tui/src/
├── main.rs         # CLI entry, terminal lifecycle, event loop, key dispatch
├── app.rs          # App state machine: rows/visible/groups/bookmarks/picker/focus
├── model.rs        # EntryRow: owned line model, from_line()/as_log_entry()
├── filter_model.rs # Group/GroupList (chip filter, AND-inside / OR-across)
├── highlight_model.rs # HighlightGroup/HighlightGroupList
├── picker.rs       # PickerSession/PickerKind/PickerMode/UnifiedKind/UnifiedItem
├── input.rs        # ChipField/Chip/InputBox/Popup (Enter two-phase)
├── ui.rs           # Render: log list, strips, picker, minimap, modals
├── theme.rs        # SINGLE color source (UiTokens + logcolor derivation)
├── bookmark.rs     # Bookmark/BookmarkList/JumpResult/label helpers
├── help.rs         # L1/L2 context help strings
├── export.rs       # H10 yc CLI export
├── config.rs       # theme.toml/config.toml loading
├── preview.rs      # H1 preview sampling
└── ingest.rs       # spawn_file_ingest / spawn_hdc_ingest
```

---

## Module Organization

### Picker session dispatch (Manage-by-kind)

`PickerSession` carries a `kind: PickerKind` and `mode: PickerMode`. The
Manage mode is dispatched **by `session.kind`** in two places:

- `picker_render_data` (`main.rs`): builds the candidate list per kind.
  `Unified` aggregates Filter+Highlight+Exclude; `Bookmark` builds a
  bookmark-only list (no `[Bookmark]:` prefix). Future per-kind Manage
  panels branch here.
- `handle_picker_key` Manage branch (`main.rs`): routes keys per kind.
  `Unified` supports Tab multi-select + Ctrl-X edit; `Bookmark` disables
  edit (Tab = no-op, Ctrl-X = flash) and binds Enter = jump, Delete /
  Ctrl-Backspace = delete-via-`ConfirmKind::DeleteBookmark`.

**Convention**: to add a new per-kind Manage panel, add a `PickerKind`
variant, branch in both `picker_render_data` and `handle_picker_key`, and
provide a `*_visible_indices`/`*_selected_index` helper pair. Do NOT
reintroduce a `UnifiedKind` variant for it — `UnifiedKind` is the
aggregate-panel item taxonomy only.

### Bookmark row-id cache

`App.bookmark_row_ids: HashSet<u64>` mirrors `BookmarkList.items` row_ids
for O(1) LogList bg lookup. It is mutated in lockstep with every
`BookmarkList` mutation (`bookmark_add_current`, `bookmark_remove_current`,
`delete_bookmark_at_index`, `clear_bookmarks`). Any new mutation site on
`BookmarkList` MUST sync this set — `render_log_list` reads it every frame.

---

## Naming Conventions

- `*_indices` helpers return indices into the backing `Vec` (not display
  order). When display order differs from storage (e.g. bookmarks
  newest-first), the helper maps display→storage internally and returns
  storage indices.
- `PickerKind` = which panel kind; `PickerMode` = Manage/New/Edit within
  that panel; `UnifiedKind` = item taxonomy inside the Unified aggregate
  panel only.
- theme.rs accessors are `pub fn <thing>_style()` / `pub fn <thing>_color()`;
  glyphs are `pub const GLYPH_*`.

---

## Examples

- `bookmark_visible_indices` / `bookmark_selected_index` (`main.rs`):
  newest-first display, maps back to real `app.bookmarks.items` index.
- `unified_picker_items` (`main.rs`): aggregate for the Unified panel only
  (Filter+Highlight+Exclude; Bookmark removed in 07-23-bookmark-ux).
