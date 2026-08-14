# Design: TUI status bar hierarchy

## Architecture

Keep `help.rs` as the only keybinding copy source. Split **which subset the status bar shows** from **what Help Active shows**.

```
help.rs
  context_entries(app)          // FULL L1/L2 — Help Active + catalog (unchanged)
  status_hint_entries(app)      // status bar only: idle curated 1–2, else full
  context_hint_spans(app, max)  // consume status_hint_entries, same dim-key style

theme.rs
  status_pill / status_pill_value   // filled bg + contrast_fg
  status_icon_dim                   // off-state follow
  status_flash_pill                 // filled toast

ui.rs::render_status_bar
  left cluster (never yields)
  flash pill (middle, FLASH_MIN ≈ 12)
  pad + right-aligned hints (hide first)
```

## Data contracts

### `status_hint_entries`

| `context_kind` | Status bar set |
|----------------|----------------|
| `LogList` / `LogListLive` | `GlobalOpenHelp` → `help`, `GlobalFilterNew` → `filter` |
| `ChipStrip` / `ExcludeStrip` / `HighlightStrip` | strip Help + `StripPendingD` → `del…` |
| Operator-pending, Picker, Confirm, TimePanel, Detail, HighlightModal, Input, Leader, … | `context_entries(app)` unchanged |

Idle LogList must **not** be implemented by truncating `l1_loglist` to the first two items (`j/k` + `Esc follow`). Curate explicitly.

Keys still come from `key_of` / `App.keymap`. Labels stay English.

### Theme helpers

| Helper | Render |
|--------|--------|
| `status_pill(glyph, fg)` | ` format!(" {glyph} ") ` with `bg=fg`, `fg=contrast_fg(fg)`, BOLD |
| `status_pill_value(glyph, value, fg)` | same fill, ` {glyph} {value} ` |
| `status_icon_dim(glyph)` | ` {glyph} ` + DIM, no background |
| `status_flash_pill(text)` | filled pill; warning fill if message contains `FAILED`, else success fill |

Reuse `palette::contrast_fg` (luminance ≥ 140 → black, else white). Do not invent a second contrast function.

`status_icon` / `status_icon_value` remain for any non-status-bar callers; the session status bar left cluster uses pill/dim.

### Left cluster order

1. dim `cursor+1/visible.len()`
2. highlight match stats (pill_value, accent) if any
3. follow: pill(success) if `following` else dim glyph
4. source/device: disconnect warning pill **or** source accent pill
5. lock / time / view-focus / progress (pill_value, existing semantic colors) if any
6. visual (accent pill) if visual mode — **not** pending prefixes

No `c…`/`f…`/`SPC…` in this cluster.

### Width algorithm

```
left_w  = sum(left spans)
flash_w = flash pill width or 0
avail   = area.width.saturating_sub(left_w)

if flash:
    floor = min(FLASH_MIN, flash_w)           // FLASH_MIN = 12
    hint_budget = avail.saturating_sub(flash_w).saturating_sub(1)
    if hint_budget < MIN_HELP_WIDTH (8):
        hide hints
        if left_w + flash_w > area.width:
            shrink flash text to avail (may go below floor only if otherwise clipped)
    else:
        hints = context_hint_spans(app, hint_budget)
else:
    hints = context_hint_spans(app, avail.saturating_sub(1))  // still hide if < 8

pad so hints sit at the right edge of `area`.
```

Pending L2 does **not** steal the flash slot. Trailing L2 keys drop via existing greedy fit.

## Compatibility

- Help open/close/scroll, `help_available`, keymap, `FAST_SCROLL_STEP`: unchanged.
- Disconnect vs file-mode gate (`export_source.is_live() && ingest_done`): unchanged.
- Flash lifetime 3s / English tokens: unchanged.
- Dashboard flash row (`dashboard_flash_style`) is a different surface; leave it.

## Trade-offs

- Idle LogList loses on-bar `j/k` / `Esc follow`. Follow state is the resident icon; navigation stays in `?`.
- Yank L2 may truncate on 80-col with lock+time+flash. Accepted; common fields stay first.
- File source uses accent (pill or equivalent weight) rather than a dim/off pair so `-f` does not look “disconnected”.

## Rollback

Revert `theme.rs` pill helpers, `ui.rs::render_status_bar`, `help.rs` status subset + tests. No data migration.
