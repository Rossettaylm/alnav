# TUI command palette + ActionStore

## Goal

From Normal LogList / Filter·Exclude·Highlight strip, `C-p` opens a VS Code-style command palette. The user types to fuzzy-search **intent commands** (Add Filter, Add Highlight, …) and Enter runs the same handler as the keybinding. `ActionStore` is the unique source of action metadata, `when`, palette catalog, and `dispatch`. `KeymapStore` only maps keys → `ActionId`.

## Background / Confirmed Facts

- `alnav/src/keymap.rs` already owns `ActionId` (~130 variants), `ActionMeta` (context, default binding, prefix/leaf, `label`/`detail`, `Capability`), and `KeymapStore` (merge `keymap.toml`, `matches_code` / `display`).
- Execution is **not** centralized: `main.rs` still branches `if km_code(app, ActionId::…)` inside `handle_*_key`. Keymap task design mentioned a handler id; it was not landed.
- Short Help `label`s collide (`filter`, `move`) and cannot be palette titles.
- Picker already has nucleo fuzzy (`fuzzy::fuzzy_label_indices`), `top_modal_rect`, `stack_below_rect_gapped`, `render_modal_shell`, candidate row tokens, and `apply_text_field_key` / `TextField`.
- Nerdfont glyphs live in `theme.rs`; `ui.rs` must not inline glyph literals or `Color::*`.
- Grill 2026-08-14 locked the decisions below. One wording fix vs grill Q9: list motion is **Up/Down only**. `j`/`k` are query characters (Picker parity). Typing `bookmark` must not move the selection on `k`.

## Requirements

### R1 — ActionStore is the action authority

- `ActionId` remains the only action enum. No parallel `CommandId`.
- `ActionStore` owns: registry metadata (including palette fields), `when` evaluation, filtered catalog, and `dispatch(app, ActionId)` (`match`, not `Box<dyn Fn>`).
- `KeymapStore` owns only bindings, chord matching, `display()`, load/merge/`--init` serialize.
- This task extracts **all** current Normal-mode `handle_*` bodies into `dispatch`, including navigation (`j`/`k`). Palette is one caller of `dispatch`. Do not extract only `in_palette` actions.
- Prefix actions still dispatch (they arm `pending_*`). They are not in the palette.

### R2 — Palette catalog is intent commands + `when`

Include session-level and context-level **leaf** commands listed in design.md (Add Filter/Highlight/Exclude, Open Help, Quit, Toggle Wrap, Fields/Pretty, Clear Live, Resume Follow, Unified Manage, Preset Save/Open, Summary, Open File/Stream, Set/Clear Time, Lock PID/TID/Highlight/Severe/Clear, Bookmark Add/Remove/Manage, Yank CLI, Yank Message, Strip delete/disable).

Exclude: movement/paging/next-match, Prefix operators, `*Cancel`, Picker/Help/Input/TimePanel/Confirm/Detail internals, Focus 1–5, six `ChipField*`, six field `Yank*` (keep `YankCli` + message yank).

Unavailable commands are **omitted** (not dimmed, not scored): `Capability` (file/live) plus App `when` (no current row, empty strip, live Time, already following, etc.).

### R3 — Open / close / following

- Open only from Normal surfaces: `Focus::LogList` / `ChipStrip` / `ExcludeStrip` / `HighlightStrip`, and only when no Picker / Help / Time / Detail / Confirm / Highlight-edit / Input / Dashboard / source panels are open.
- Default binding `C-p` on a new `GlobalCommandPalette` action (`keymap.toml` overridable). Not `in_palette`.
- Open: clear all `pending_*` / `pending_leader`, `following = false`.
- Esc / Ctrl+C: close palette, keep prior focus, **do not** `resume_following`.
- Enter: close palette, then `dispatch(selected)`. Following afterwards is that action’s existing behavior (e.g. Add Filter opens Picker).
- Dashboard `C-p` is out of scope.

### R4 — Palette widget (new control, not Picker)

- New `command_palette` module + `App.command_palette: Option<CommandPalette>`. Do not reuse `PickerSession`.
- Top-centered via `top_modal_rect`. Width ≈ 60% of frame, clamped 40–72 columns.
- Empty query: input shell only (rounded `render_modal_shell`, nerdfont title). No dropdown, no MRU.
- Non-empty with hits: list stacks below with `stack_below_rect_gapped`; at most 10 visible rows; extra results scroll inside the list.
- Zero hits: dropdown still opens with one dim `No matching commands` row; Enter is a no-op.
- Row: nerdfont icon + `palette_title` + right-aligned `KeymapStore::display` (empty if unbound). Search haystack is `palette_title` only (`fuzzy_label_indices`). No aliases, no side Preview.
- Query editing: reuse `TextField` + `apply_text_field_key` (printable, Backspace, Left/Right/Home/End, Ctrl-A/E/U). Up/Down move the candidate; `j`/`k` type. Enter submits; Esc/Ctrl+C close.
- Paint only through `theme.rs` (candidate tokens, `plain_title`, new glyph constant for the palette title).

### R5 — Help / status

- Help Active + catalog for LogList include Open Command Palette (key from store).
- Idle status bar stays the curated two hints (`? help`, `; filter` / strip `d del…`). Do **not** add a third idle `C-p` hint.
- While the palette is open: `help_available` is false; status right slot uses the palette context’s full L2 (Esc / Enter / Up/Down).

### R6 — `--init` / keymap.toml

- New actions and `[command_palette]` (or equivalent context section) serialize from the registry. Existing user `keymap.toml` without the new keys keeps builtin `C-p` and palette internals.

## Acceptance Criteria

- [ ] AC1: From idle LogList, `C-p` opens a top-centered input-only shell; no candidate list until the query is non-empty.
- [ ] AC2: Query `filter` ranks `Add Filter`; Enter closes the palette and opens Filter New (same as `;`).
- [ ] AC3: Query `bookmark` types the letters `b,o,o,k,…` without moving the selection on `k`; Up/Down change the selected row.
- [ ] AC4: Live session omits Set/Clear Time from matches; file session with no current row omits Lock PID / Add Bookmark; empty Filter strip omits Strip delete.
- [ ] AC5: `j`/`k` on LogList still move the cursor (dispatch parity). Palette open steals keys as in R4.
- [ ] AC6: Esc from the palette does not call `resume_following`. Opening clears pending chords (`c` then `C-p` opens the palette, does not wait for a field letter).
- [ ] AC7: Help catalog lists the `C-p` binding; idle status bar still shows only `? help` and `; filter`.
- [ ] AC8: Unbound palette command still appears (no key on the right) and runs via Enter.
- [ ] AC9: `cargo test -p alnav --bin alnav` and `cargo fmt -p alnav --check` pass; no `Color::*` / inline glyphs in `ui.rs`.
- [ ] AC10: `--init` output contains `command_palette` (or the chosen section) and `command_palette` / open-palette action keys.

## Out of Scope

- Opening the palette from Picker / Help / Time / Input / Detail / Confirm / Dashboard.
- Empty-query full catalog or MRU/recent commands.
- Search aliases, category prefixes (`Filter: Add Group`), side Preview.
- User-defined commands in TOML.
- `HashMap<ActionId, Box<dyn Fn>>` handlers.
- Focus 1–5 and per-field Chip/Yank leaves in the catalog.
- Mouse bindings, keymap hot-reload, ASCII fallback without Nerd Fonts.
- Changing idle status-bar hint count.

## Open Questions

None. Grill 2026-08-14 locked R1–R6; Q9 `j`/`k` vs query resolved to Picker parity.
