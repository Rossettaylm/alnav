# Implement: TUI command palette + ActionStore

## Checklist

1. **Extract action registry**
   - Move `ActionId`, `ActionMeta`, `KeyContext`, `Capability`, `ActionKind` into `alnav/src/action.rs` (or keep types in `keymap.rs` for one commit if the move fails `cargo test` noise — still add palette fields on `ActionMeta`).
   - `keymap.rs` re-exports so existing `keymap::ActionId` compiles during the transition.
   - Add `in_palette` / `palette_title` / `palette_icon` on every `ActionMeta` (false / `""` / `""` for non-catalog).
   - Register v1 catalog titles/icons from design.md. New glyph(s) in `theme.rs` only.
   - Snapshot/unit: `in_palette` set equals the design table; live `allowed` still gates Time vs Clear Live.

2. **`dispatch(app, ActionId)`**
   - Move Normal-mode side effects from `handle_normal_key` / leader / yank / lock / time / bookmark / strip-d / visual into `action::dispatch`.
   - `main.rs` keeps pending/chord `km_code` resolution, then `dispatch`.
   - Prefix actions dispatch to existing `pending_* = true` helpers.
   - Tests: rebound key still hits the same App effect (existing keymap tests); `dispatch(GlobalFilterNew)` opens Filter New without a key.

3. **Command palette state**
   - `command_palette.rs` + `App.command_palette`.
   - `open_command_palette` / `close_command_palette` (no `resume_following`).
   - `GlobalCommandPalette` default `C-p` ; `KeyContext` + keymap section for palette internals.
   - `help_available` false; `ContextKind::CommandPalette`; LogList Help catalog includes open action.
   - `--init` serialize includes the new section.

4. **Palette input + filter**
   - Reuse `TextField` + `apply_text_field_key`.
   - Empty query → no ids; non-empty → `when` filter then `fuzzy_label_indices` on titles.
   - Up/Down / Enter / Esc / Ctrl+C. Opening clears `pending_*`.
   - Unit tests: hide Time on live; hide Lock PID with no row; `bookmark` query does not treat `k` as Down; zero-match Enter no-op.

5. **Render**
   - `ui.rs::render_command_palette` using `top_modal_rect` + optional `stack_below_rect_gapped`.
   - Input-only height when empty; ≤10 candidate rows; dim zero-match line; icon + title + right key hint.
   - `theme::GLYPH_TITLE_PALETTE`; candidate selected/match styles; no `Color::*`, no inline glyphs.
   - Render tests (buffer `cell_text`): empty open has no `Add Filter`; query `filter` shows it; key hint `;` (or current Filter New binding) on the right.

6. **Docs**
   - `AGENTS.md` / `CLAUDE.md`: ActionStore + `C-p` palette; idle status still 1–2 keys.
   - Spec updates (`directory-structure.md`, `status-help.md`) in Phase 3.3 after code lands — not this planning step.

7. **Validate** (commands below)

## Validation commands

```bash
cargo test -p alnav --bin alnav action::
cargo test -p alnav --bin alnav command_palette::
cargo test -p alnav --bin alnav keymap::
cargo test -p alnav --bin alnav help::
cargo test -p alnav --bin alnav ui::
cargo test -p alnav --bin alnav
cargo fmt -p alnav --check
```

## Risky files

| File | Risk |
|------|------|
| `alnav/src/main.rs` | Dispatch extract can drop a pending-chord edge (flash hints, visual fallthrough) |
| `alnav/src/keymap.rs` | `--init` / merge / prefix-tree tests if new `C-p` steals a binding |
| `alnav/src/ui.rs` | Overlay vs Help/Picker z-order; width clamp on 80-col |
| `alnav/src/help.rs` | Accidentally adding `C-p` to idle status two-hint set |

## Rollback

Single feature branch. Revert the new modules and restore `handle_*` bodies. No config migration.

## Before `task.py start`

- [x] `prd.md` / `design.md` / `implement.md` written from grill 2026-08-14
- [ ] User approved **this** planning summary (grill confirm is not implementation approval)
- [x] `implement.jsonl` / `check.jsonl` curated
