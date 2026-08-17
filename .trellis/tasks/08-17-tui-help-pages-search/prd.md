# TUI help panel pages and search

## Goal

Make `?` Help a two-level, searchable reference: Home explains *where you are* in a short Active block and lists seven zone pages; each page carries a short design contract plus that zone’s full key table. Users can jump pages and `/`-search the whole corpus with highlighted hits.

## Background / Confirmed Facts

- Today Help is one scrollable modal (`App.help_open` + `help_scroll`): **Active** (full `context_entries`) + **All commands** catalog (`SectionId`: Navigation / Leader & pickers / Filter operators / Session / Overlays / Help). No zone prose, no sub-pages, no search (`alnav/src/help.rs`, `ui.rs::render_help_panel`, `main.rs::handle_help_key`).
- Open gate (`help_available`): focus ∈ LogList / ChipStrip / ExcludeStrip / HighlightStrip; no picker / time / detail / highlight-edit / command palette; no `pending_*`. Unchanged.
- Close today: `Esc` / `?` / `Ctrl+C` → `close_help()`; does **not** `resume_following`. Grill kept this for the whole panel (not a back stack).
- Keys for Help live in `KeyContext::Help`. LogList `/` remains Highlight New; Help `/` is a different context and must not steal LogList `/`.
- Status/Help copy is English-only and must stay in `help.rs` (`HintEntry`). Paint goes through `theme::*`. Modal chrome is rounded `render_modal_shell`.
- Grill 2026-08-17 locked the decisions below. User confirmed the shared-understanding draft including: sub-page `1`–`7` jumps without returning Home first; ignore-case substring search (not nucleo).

## Requirements

### R1 — Two-level Help (Home + seven pages)

- Home and exactly seven pages: **Filter / Exclude / Highlight / Log / Session / Picker / Overlays**.
- No eighth “Help keys” page. This panel’s chrome (`/`, `n`/`N`, `h`, `Esc`) lives on the Home footer.
- Help stays **read-only**: never dispatches LogList/Picker commands, never replaces Picker or the command palette.

### R2 — Home layout

- **Top (pinned on short frames):** `Active <ContextKind.title>` plus at most the first **4** entries of `context_entries` (existing order; do not maintain a second Home-only table).
- **Middle (scrolls on short frames):** numbered TOC `1`–`7`. Opening Help preselects from current `Focus` (ChipStrip→Filter, ExcludeStrip→Exclude, HighlightStrip→Highlight, LogList→Log). Session / Picker / Overlays have no Focus mapping; they are not auto-selected.
- **Bottom (pinned):** chrome line `/ search    n/N next    h back    Esc close`. `h` is listed on Home even though it is a no-op there.
- Home `j`/`k` (and arrows) move the TOC highlight, not a long body. `Enter` opens the highlighted page. `J`/`K` jump the TOC by `FAST_SCROLL_STEP`, clamped.

### R3 — Page content

- Each page: English **title + at most 5 lines of design contract + that zone’s complete key table**.
- Contracts describe mental model and boundaries only (AND/OR, what the zone is not). No step-by-step “press `;` then Enter”.
- Do not author separate file vs live essays. Live-only / file-only keys still hide via existing `ActionId::meta().allowed(file_mode)` (and the current `live` catalog branches such as time vs clear).
- Design-contract copy is owned in `help.rs` (or a sibling constants table in that module), not in `ui.rs`.

### R4 — Navigation (split Esc)

- Any layer: `Esc` / `?` / `Ctrl+C` → `close_help()`; `following` unchanged.
- Sub-page → Home: `h` and `Backspace` (`HelpBack` / `HelpBackAlt`). Restore TOC highlight on the page just left. Home: those keys are no-ops (do not close).
- `1`–`7` open the corresponding page from **Home and from any other page**, except while the search prompt is active.
- Sub-page `j`/`k`/`J`/`K`/`g`/`G` scroll that page’s body (`J`/`K` still `FAST_SCROLL_STEP`).

### R5 — Global `/` search with highlight

- `/` opens a vim-style query prompt at the Help footer (reuse `TextField` + `apply_text_field_key`). Haystack = Home Active + TOC titles + chrome + all seven page titles, contracts, and key details.
- Match: **ignore-case substring**, not nucleo fuzzy. Highlight match spans in the visible body via `theme` (no `Color::*` in `ui.rs`).
- While the prompt is open: printable characters (including `j`/`k`/`h`/`1`–`7`/`n`) edit the query; Backspace deletes query text (not HelpBack); `↑`/`↓` move among hits without inserting; live-jump to the first / current hit (may change page + scroll).
- `Enter` with a non-empty query: dismiss the prompt, **keep** highlights and current hit; then `n`/`N` walk hits (cross-page). `Enter` with an empty query: exit search and clear highlights.
- `Esc` while the prompt is open **or** after a committed search with highlights: clear search (query + highlights), stay in Help on the current page. A following `Esc` closes Help. `?` / `Ctrl+C` always close the whole panel even during search.
- No match: flash `NO MATCH`; panel stays open; do not jump.
- No search history. Re-pressing `/` reopens the prompt with the current query.
- Help `/` must not change LogList Highlight New (`/`) when Help is closed.

### R6 — Keymap / `--init` / status

- New Help-context actions serialize in `--init` / `keymap.toml` merge (back, search, next/prev hit, enter page). Digits `1`–`7` may be handled in `handle_help_key` without seven ActionIds; document them as literals in Help chrome / tests.
- Idle status bar two-hint set is unchanged (`? help` + `; filter` / strip `d del…`).
- Help catalog on zone pages lists that zone’s keys; Home footer lists this panel’s keys. Do not shrink `context_entries` used by status L2 elsewhere.

## Acceptance Criteria

- [ ] AC1: LogList `?` opens Home with shortened Active (≤4 lines of keys) + numbered TOC + chrome footer; `Esc` / `?` / `Ctrl+C` close without resuming follow.
- [ ] AC2: From Exclude strip, `?` preselects Exclude; `Enter` or `2` opens the Exclude page (title + ≤5-line English contract + exclude keys).
- [ ] AC3: On a sub-page, `h` or Backspace returns to Home with that page still TOC-selected; Home `h`/Backspace does not close Help; `Esc` on a sub-page **does** close Help.
- [ ] AC4: From the Filter page, `7` jumps to Overlays without visiting Home; `j`/`k` scroll Overlays, not the TOC.
- [ ] AC5: `/chip` (ignore-case) highlights substring hits across pages; `n`/`N` after Enter walk hits and may change page; typing `k` while the prompt is open inserts `k` (does not scroll).
- [ ] AC6: Empty `/` then Enter clears search; `Esc` in the prompt clears search and leaves Help open; a second `Esc` closes Help.
- [ ] AC7: Query with no substring match flashes `NO MATCH` and does not close Help or change page.
- [ ] AC8: LogList `/` with Help closed still opens Highlight New; Help remains unavailable while Picker / palette / pending operators are active.
- [ ] AC9: Short Help frame keeps Active + chrome visible while the TOC scrolls; no `Color::*` / inline glyphs in `ui.rs`; Help copy stays in `help.rs`.
- [ ] AC10: `cargo test -p alnav --bin alnav` and `cargo fmt -p alnav --check` pass; `--init` includes new Help search/back bindings.

## Out of Scope

- Fuzzy / nucleo Help search; search history; filtering the body down to matching lines only.
- A third Help layer, wiki links, or Chinese Help copy.
- Mouse clicks on TOC.
- Opening Help from Picker / Time / Detail / palette / Dashboard.
- Changing idle status hints or making Help execute actions.
- Windows-specific Help; stdin-pipe TUI.

## Key Decisions

| Decision | Choice |
|----------|--------|
| IA | Two-level Home + 7 zone pages |
| Pages | Filter, Exclude, Highlight, Log, Session, Picker, Overlays |
| Esc | Always `close_help`; back is `h` / Backspace |
| Search | Global ignore-case substring highlight; vim prompt |
| Home Active | First 4 `context_entries`; contracts only on pages |
| Language | English, ≤5-line contracts, no tutorials |
