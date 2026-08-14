# Optimize TUI status bar hierarchy

## Goal

Make the TUI status bar read as **status first, hints second**. Follow and device-link icons stay visible and high-contrast; keybinding hints shrink on the idle LogList/Strip; flash toasts become a filled pill in a reserved middle slot. A later top-right toast overlay is explicitly out of scope.

## Background / Confirmed Facts

- Status bar is a single row in `alnav/src/ui.rs::render_status_bar`.
- Left cluster today: dim `cursor/total`, highlight `k/total`, follow **only when `following`**, source/disconnect, lock/time/view-focus/progress, visual **or** pending prefix (`c…`/`f…`/…), then DIM flash (`theme::status_soft`), then right hints **greedily fill remaining width**.
- Follow disappearing on pause is easy to miss. Flash uses `Modifier::DIM`, so toasts are weaker than hints.
- `help.rs::context_kind` already ranks modal/confirm > pending > focus. `context_entries` is the full L1/L2 set used by **both** the status bar and Help Active. Help catalog must stay full; status bar must not shrink that source.
- `help_available` is false for Picker / Time / Detail / Highlight edit / any `pending_*`. Those surfaces cannot open `?`.
- Live disconnect already replaces the source glyph with `GLYPH_DISCONNECT` + `theme::warning()` when `export_source.is_live() && ingest_done`. File mode never shows disconnect.
- Grill decisions (2026-08-13) are locked below.

## Requirements

### R1 — Three zones, single row

- Left: cursor + resident status icons (never yields).
- Middle: flash pill when `status_msg` is set; reserved min width ~12 columns.
- Right: keybinding hints; hide first when the row is tight.
- Hints are right-aligned in the leftover width. No second status row. No overlay toast this task.

### R2 — Resident follow + device icons

- Follow **always occupies a slot**: on = filled success pill; off = same `GLYPH_FOLLOWING` with DIM, no fill.
- Device **always occupies a slot**:
  - live connected = source glyph (`HDC`/`ADB`) as accent pill;
  - live disconnected = `GLYPH_DISCONNECT` as warning pill (replaces source glyph, same as today);
  - `-f` file source = file glyph + accent (always-on source, **not** an on/off pair, not DIM).
- lock / time / view-focus / progress / visual: still appear only when active; when shown, use the same filled-pill family.
- Highlight match stats stay in the left cluster (icon + `k/total`, no `[]`).
- Cursor `n/N` stays dim text, not a pill.

### R3 — Idle hints are curated 1–2 keys

- Idle **LogList / LogListLive**: exactly `? help` and `; filter` (keys from `App.keymap`, not hard-coded glyphs).
- Idle **ChipStrip / ExcludeStrip / HighlightStrip**: exactly `? help` and `d del…`.
- Do **not** vary idle LogList hints by following / highlight-hit / last action.
- Help panel Active + catalog still use the full `context_entries` list. Status bar uses a separate `status_hint_entries` (name may differ) so Help does not shrink.

### R4 — Pending and modal hints expand

- Operator-pending (`c`/`C`/`f`/`t`/`m`/`y`/`o`/`d`/`Space`…): right slot shows the **full** L2 from `context_entries`.
- Drop left pending prefixes (`c…`, `SPC…`, …). L2 on the right is the only pending cue.
- Picker / Time / Detail / Confirm / Highlight-edit / Input: treat as modal — expand full current `context_entries` (because `?` is unavailable).

### R5 — Width collision

- Left icons never yield.
- If the row is too narrow: drop/truncate hints first, then truncate flash text, keeping a ~12-column flash floor while any toast is visible. If even the floor cannot fit, flash may shrink further rather than overlapping icons.
- Pending L2 uses remaining width after left + flash and truncates with the existing greedy `context_hint_spans` algorithm (keep current field order; drop trailing keys).

### R6 — Flash pill (interim)

- Flash is a filled pill in the middle slot (not `status_soft` + DIM).
- Failure-like messages (`YANK FAILED` and other `FAILED` / hard errors already using warning fg) use `theme::warning()` fill; other flashes use `theme::success()` fill.
- Still 3 seconds via `App::set_flash` / `tick_flash`. Copy stays English short tokens.
- Top-right toast is **later**; do not add a second row or overlay this task.

### R7 — Theme-only paint

- New pill helpers live in `theme.rs`. `ui.rs` must not hard-code `Color::*` or inline glyph literals.
- Reuse `palette::contrast_fg` for on-pill foreground (same rule as chip pills).
- `status_icon` / `status_icon_value` may remain for non-pill callers; status bar left cluster switches to the pill/dim pair.

## Acceptance Criteria

- [ ] AC1: LogList idle status bar shows `? help` and `; filter`, and does **not** show the long L1 list (`j/k move`, `Esc follow`, `Space menu`, …) even on a wide terminal.
- [ ] AC2: Help (`?`) Active + catalog still list the full LogList command set (including `j/k move`).
- [ ] AC3: Follow glyph is visible when `following=false` (DIM) and when `following=true` (success filled pill).
- [ ] AC4: Live connected shows source glyph as accent pill; live `ingest_done` shows disconnect warning pill; file mode shows file glyph, never disconnect.
- [ ] AC5: After `c` (or other pending), left cluster has no `c…` prefix; right slot shows the field L2 (`t tag`, `m msg`, …). Esc/cancel returns to the idle 1–2 hints.
- [ ] AC6: Picker/Time/Detail/Confirm open → status hints expand to that context’s full set.
- [ ] AC7: Flash renders as a filled pill; a simultaneous pending L2 does not cover the flash slot; narrow widths hide hints before eating the flash floor.
- [ ] AC8: Strip idle shows `? help` and `d del…` only.
- [ ] AC9: `cargo test -p alnav help:: ui::` and `cargo build -p alnav` pass; no `Color::*` added in `ui.rs`.

## Out of Scope

- Top-right / overlay flash toast (follow-up).
- Two-line status bar, hint rotation, last-action sticky hints.
- Enabling `?` Help inside Picker/Time/Detail/pending.
- Changing keybindings, Help scroll, or Help catalog structure.
- ASCII fallback when Nerd Fonts are missing.
- Dashboard flash row (separate from the session status bar).

## Open Questions

None. Grill 2026-08-13 locked R1–R7.
