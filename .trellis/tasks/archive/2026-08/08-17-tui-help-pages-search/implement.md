# Implement: TUI help panel pages and search

## Checklist

1. **Help state machine** (`app.rs`, `help.rs`)
   - Add `HelpPage` / `HelpView` / `HelpSearch` / `HelpHit`.
   - Replace lone `help_scroll` with view-owned offsets; keep `help_open`.
   - `open_help` preselects TOC from `Focus`; `close_help` unchanged (no `resume_following`); add `help_pop_to_home`.
   - Unit: open from Exclude strip → TOC index 1; close does not flip `following`.

2. **Copy + page tables** (`help.rs`)
   - Author the seven blurbs from `design.md` (≤5 lines each).
   - Remap `catalog_entries` into per-page `HintEntry` lists (table in design.md). Drop the combined “All commands” dump.
   - Home Active = first 4 `context_entries`.
   - Keep `status_hint_entries` / idle two-hint set untouched.
   - Unit: each blurb line count ≤5; Filter page contains filter-new; Overlays does not list Help `/`.

3. **Keymap** (`keymap.rs`)
   - Register `HelpBack`, `HelpBackAlt`, `HelpSearch`, `HelpSearchNext`, `HelpSearchPrev`, `HelpSubmit` under `KeyContext::Help`.
   - `--init` serializes them. `handle_help_key` branches on view + search prompt; digits `1`–`7` when `!prompt`.
   - Ctrl+C still closes the whole panel during search.

4. **Search** (`help.rs`, `text_field.rs` reuse)
   - Corpus from the same line builders as render.
   - Ignore-case substring hits; live jump on edit; `↑`/`↓` in prompt; `n`/`N` after commit; empty Enter clears.
   - Flash `NO MATCH` on empty hits with non-empty query.
   - Unit: `/CHIP` hits `chip`; prompt `k` does not scroll; Esc in prompt leaves `help_open`.

5. **Render** (`ui.rs`, `theme.rs`)
   - Home: pin Active + chrome; TOC viewport + `toc_off`.
   - Page: title, blurb, keys; scroll body.
   - Search hit / current-hit styles via new `theme::help_search_*` wrappers (reuse highlight / preview tokens).
   - Prompt row uses TextField; no `Color::*`, no inline glyphs.
   - Render/buffer tests: Home shows `1`/`Filter` (or title); after `2` the Exclude blurb keyword is visible; hit style applied when query nonempty.

6. **Retarget existing tests**
   - `help_body_lines` / catalog “All commands” / `help_shift_jk_scrolls_by_fast_step` / `help_jk_scrolls_one_line_*` — Home no longer a long document. Open Log page (`4`) before asserting `J`/`K` ±7 and `j`/`k` ±1.
   - Keep: `?` open/Esc no-follow; `?` ignored when pending; `/` Highlight New when Help closed; ADB hides time.

7. **Docs**
   - `AGENTS.md` / `CLAUDE.md`: Help is two-level + `/` search; Esc still closes; `h`/Backspace back. Spec `status-help.md` update is Phase 3.3 after code lands.

8. **Validate** (commands below)

## Validation commands

```bash
cargo test -p alnav --bin alnav help::
cargo test -p alnav --bin alnav app::
cargo test -p alnav --bin alnav keymap::
cargo test -p alnav --bin alnav ui::
cargo test -p alnav --bin alnav
cargo fmt -p alnav --check
```

## Risky files

| File | Risk |
|------|------|
| `alnav/src/help.rs` | Dual copy (Home vs pages) drifting from `context_entries`; blurbs exceeding 5 lines |
| `alnav/src/main.rs` | Search prompt vs HelpBack Backspace; `n`/`N` vs jump keys; digits vs later search commit |
| `alnav/src/ui.rs` | Short-frame pin math clipping TOC to zero; wrap vs highlight byte ranges (follow existing `wrap_ranges` discipline if lines wrap) |
| `alnav/src/keymap.rs` | `--init` / merge tests if new Help keys collide |

## Rollback

Revert the Help view/search fields and restore single-document `help_body_lines`. No data migration.

## Before `task.py start`

- [x] `prd.md` / `design.md` / `implement.md` written from grill 2026-08-17
- [ ] User approved **this** planning summary (grill confirm + task create are not implementation approval)
- [x] `implement.jsonl` / `check.jsonl` curated
