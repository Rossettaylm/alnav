# Quality Guidelines

> Code quality standards for aloggrep-tui.

---

## Overview

These standards are derived from CLAUDE.md "UI 设计指导" and decisions
captured in tasks under `.trellis/tasks/`. They are executable rules, not
aspirations.

---

## Forbidden Patterns

### Don't: hardcode colors in render code

```rust
// DON'T — inline Color in ui.rs / main.rs
item.style(Style::default().bg(Color::Rgb(54, 46, 0)))
```

**Why**: breaks theme.toml override + CLI/TUI color sync. Log colors derive
from `aloggrep::logcolor`; UI chrome from `theme::UiTokens`.

**Instead**:
```rust
item.style(theme::bookmark_row_style())   // reads UiTokens.bookmark_row_bg
```

### Don't: reintroduce `enabled` on entities that have no enable semantics

`Bookmark.enabled` was a zombie field (set on add, toggleable in picker,
but no consumer ever gated on it). It was deleted in `07-23-bookmark-ux`.
Do not re-add `enabled` to a model unless a real consumer gates on it.

### Don't: route a per-kind Manage panel through `UnifiedKind`

`UnifiedKind` is the aggregate-panel item taxonomy only. A dedicated
Manage panel (e.g. Bookmark) keys off `session.kind` + a `*_visible_indices`
helper, never `unified_selected_id`/`UnifiedId`.

---

## Required Patterns

### Per-frame O(1) lookups need a cache

`render_log_list` runs every frame over the viewport. Per-row predicates
that would be O(n) (e.g. "is this row bookmarked?") MUST be backed by a
`HashSet`/`HashMap` on `App`, synced at every mutation site.

Example: `App.bookmark_row_ids: HashSet<u64>` → `is_bookmark_row()` O(1).

### Picker Manage-by-kind dispatch

New per-kind Manage panels branch in BOTH `picker_render_data` (build
labels/actions) AND `handle_picker_key` (key routing). See
`directory-structure.md` "Picker session dispatch".

### Action icons via `ActionKind`, not ad-hoc spans

Candidate rows that have a primary action (Enter) show a right-flush
nerdfont icon via `candidate_label_spans(action, area_width)`. Do not
append raw icon spans in callers.

---

## Testing Requirements

- Every removed field/arm gets its test deleted or rewritten to the new
  contract (e.g. `mm` New→Manage, `toggle_unified_enabled_bookmark`
  deleted).
- New behavioral contract (jump, delete, bg priority) gets a test that
  fails on a plausible regression.
- `cargo test --workspace` must be green before commit; `cargo fmt -p
  aloggrep-tui --check` must be clean. Do NOT run `cargo fmt --all` — it
  touches `aloggrep-core` (out of scope for tui tasks).

---

## Code Review Checklist

- [ ] No `Color::*` literals in new render code (theme.rs only).
- [ ] New `BookmarkList`/`HashSet` mutation sites sync the cache.
- [ ] Picker changes branch on `session.kind` in render AND key dispatch.
- [ ] Deleted fields have no surviving references (grep).
- [ ] `cargo test --workspace` green; `cargo fmt -p aloggrep-tui --check` clean.
