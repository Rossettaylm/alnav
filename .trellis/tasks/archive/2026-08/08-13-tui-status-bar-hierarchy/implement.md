# Implement: TUI status bar hierarchy

## Checklist

1. **`theme.rs` pills**
   - Add `status_pill`, `status_pill_value`, `status_icon_dim`, `status_flash_pill`.
   - Reuse `contrast_fg`. No new glyph constants unless a visual gap appears (should not).
   - Unit-test: on-state has `bg`, off-state is DIM without `bg`; FAILED flash uses warning.

2. **`help.rs` status subset**
   - Add `status_hint_entries` (idle LogList = help+filter; idle strip = help+del; else `context_entries`).
   - Point `context_hint_spans` at that subset.
   - Keep `context_entries` / `help_body_lines` full.
   - Replace `hint_spans_fit_without_colon` assertions: idle LogList contains `help` + `filter`, not `j/k move`.
   - Add tests: pending chip field still lists `tag`/`msg`; Help body still contains `j/k` / `move cursor`.

3. **`ui.rs::render_status_bar`**
   - Three-zone layout + right-align hints.
   - Follow always rendered; drop pending prefixes; flash via `status_flash_pill`.
   - Active lock/time/visual/progress/search-stats use pills.
   - Update existing status-bar tests (wide help content, narrow follow-wins, disconnect/source).
   - Add: follow visible when `following=false`; idle wide bar lacks `j/k`; pending has no `c…` and has field letters; flash pill present with pending.

4. **Docs**
   - `AGENTS.md` / `CLAUDE.md`: status cluster is follow on/off + device pills; idle hints are 1–2; pending L2 replaces left prefixes; flash is a pill (toast later).
   - After code lands, update `.trellis/spec/alnav/backend/status-help.md` (Phase 3.3).

5. **Validate**
   - `cargo test -p alnav help::`
   - `cargo test -p alnav ui::`
   - `cargo test -p alnav --bin alnav`
   - `cargo fmt -p alnav --check`

## Validation commands

```bash
cargo test -p alnav help::
cargo test -p alnav --bin alnav ui::
cargo test -p alnav --bin alnav
cargo fmt -p alnav --check
```

## Risky files

| File | Risk |
|------|------|
| `alnav/src/ui.rs` | Width/pad math; many render tests parse `cell_text` |
| `alnav/src/help.rs` | Easy to shrink Help Active by mistake if `context_entries` is reused |
| `alnav/src/theme.rs` | Pill contrast on `default` vs named palettes |

## Rollback

Single logical change-set on the three files above plus tests/docs. No migration.

## Before `task.py start`

- [x] `prd.md` / `design.md` / `implement.md` written from grill 2026-08-13
- [x] User approved the grill summary and asked to create the task and execute
- [x] `implement.jsonl` / `check.jsonl` curated
