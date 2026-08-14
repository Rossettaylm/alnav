# Status Bar + Help Panel

> Executable contracts for the English status bar and read-only Help (`?`).

---

## Overview

`help.rs` owns keybinding **labels/details** as structured `HintEntry` data.
**Key strings** come from `App.keymap` (`keymap.toml` / builtin registry).
The status bar and Help panel both render from that source — do not maintain
a second Chinese/`key:label` string table or hard-code key glyphs in `ui.rs`.

---

## 1. Scope / Trigger

Update this spec when changing:

- Status-bar left icons / right L1–L2 hints
- Help panel open/close/scroll keys
- Flash toast language on the status bar
- `?` keybinding availability

---

## 2. Signatures

| Item | Location | Contract |
|------|----------|----------|
| `HintEntry { key, label, detail }` | `help.rs` | `key` from `KeymapStore::display`; Status uses `key`+`label`; Help uses `detail` |
| `context_kind(app) -> ContextKind` | `help.rs` | modal/confirm > pending > focus |
| `context_entries(app) -> Vec<HintEntry>` | `help.rs` | Full L1 or L2 set — Help Active + catalog only |
| `status_hint_entries(app) -> Vec<HintEntry>` | `help.rs` | Status bar subset: idle LogList/Strip curated 1–2 keys; else full |
| `keymap.toml` / `KeymapStore` | `keymap.rs` | Startup deep-merge; `--init` serializes defaults |
| `context_hint_spans(app, max)` | `help.rs` | Consumes `status_hint_entries`; dim key + label; gap `"  "`; no `:`/`\|` |
| `help_available(app) -> bool` | `help.rs` | Gate for opening Help; **false** when command palette is open |
| `ContextKind::CommandPalette` | `help.rs` | Palette open → status L2 is palette keys (Esc/Enter/Up/Down) |
| `GlobalCommandPalette` | `keymap.rs` | Default `C-p`; listed in Help catalog, **not** idle status |
| `help_body_lines(app)` | `help.rs` | Active block + fixed catalog |
| `FAST_SCROLL_STEP` | `help.rs` (`pub const`, value `7`) | Shared by LogList `J`/`K` and Help `J`/`K` |
| `App.help_open` / `help_scroll` | `app.rs` | Panel state; `close_help` does **not** `resume_following` |
| `handle_help_key` | `main.rs` | Esc/`?`/Ctrl+C close; `j`/`k` ±1; `J`/`K` ±`FAST_SCROLL_STEP` |
| `status_pill` / `status_pill_value` / `status_icon_dim` / `status_flash_pill` | `theme.rs` | Status-bar left cluster + flash; on-pill fg via `contrast_fg` |
| `status_icon` / `status_icon_value` / `status_soft` | `theme.rs` | Kept for non-status-bar callers |

---

## 3. Contracts

### Status bar three zones (single row)

Left (never yields) → middle flash pill → pad + right-aligned hints.

| State | Render |
|-------|--------|
| follow | Always a slot: on = `status_pill` success; off = `status_icon_dim` (same glyph, DIM, no fill) |
| device | Always a slot: live connected = source glyph accent pill; live `ingest_done` = `GLYPH_DISCONNECT` warning pill; `-f` = file glyph accent pill (never disconnect) |
| lock / time / view focus / progress | When active: `status_pill_value` — no LOCK/TIME word prefix; view focus uses `GLYPH_VIEW_FOCUS` + `HL`/`ERR` |
| visual | When active: accent `status_pill` — no VISUAL word |
| highlight hits | Search glyph + `k/total` as accent pill_value — **no** `[brackets]` |
| cursor `n/N` | Dim text, not a pill |
| pending prefixes | **Dropped** (`c…` / `SPC…` etc. are not in the left cluster) |
| flash | Middle filled pill (`status_flash_pill`); `FAILED` → warning fill, else success; 3s via `set_flash` |

### Status bar right hints

- English only; key dim, label normal weight; entries separated by spaces only.
- Idle **LogList / LogListLive**: exactly `? help` and `; filter` (from keymap via `status_hint_entries`).
- Idle **ChipStrip / ExcludeStrip / HighlightStrip**: exactly `? help` and `d del…`.
- Operator-pending and modal (Picker / Time / Detail / Confirm / Highlight-edit / Input / Leader / **CommandPalette**): full `context_entries`.
- Do **not** add idle `: palette` / `C-p palette` — Open Command Palette is Help-catalog only.
- Help Active + catalog still use the full `context_entries` list — do not shrink that source.
- Hints hide first when budget `< MIN_HELP_WIDTH` (8); flash keeps a ~12-column floor (`FLASH_MIN`) while visible.

### Help panel (`?`)

- **Read-only** — never executes commands / never replaces Picker.
- Open when: focus ∈ {LogList, ChipStrip, ExcludeStrip, HighlightStrip} AND no picker/time/detail/highlight edit/**command palette** AND no `pending_*` / `pending_leader`.
- Content: top **Active** (current context detailed) + **All commands** catalog; active catalog section emphasized.
- Close: Esc / `?` / Ctrl+C → `close_help()`; does **not** resume following.
- Scroll: `j`/`k` (and arrows) = 1 line; `J`/`K` = `FAST_SCROLL_STEP` (7), same as LogList.

### Keybinding note

- `?` opens Help. `/` remains Highlight New (`open_picker_new`). `C-p` opens the command palette (not Help).
- Do **not** rebind `?` to Highlight New.
- LogList L1: `f` label is `focus` (lock + view focus); L2_LOCK includes `p`/`t`/`h`/`e`/`u`.
- L2_TIME: `t` set / `u` clear (open key is `tt`, not `ts`). Catalog session: `f h/e`, `t t/u`.

### Flash language

All `set_flash` / TimePanel flash strings that appear on the status bar are
English. Prefer short uppercase tokens (`EXISTS`, `NO ROW`, `UNKNOWN FIELD`).

---

## 4. Validation & Error Matrix

| Condition | Behavior |
|-----------|----------|
| `?` while `pending_yank` (etc.) | Help does not open (pending handler consumes key) |
| `?` while Picker/Time/Detail open | Help does not open |
| Help Esc | Close only; `following` unchanged |
| Narrow terminal | Right hints hide when budget `< MIN_HELP_WIDTH` (8); left icons win |

---

## 5. Good / Base / Bad Cases

- **Good**: LogList Normal `?` → Help; `J` scrolls +7; Esc closes; still not following.
- **Base**: Wide idle LogList status shows `? help  ; filter` without colons, not the long L1 list.
- **Bad**: Reintroducing `j/k:移` Chinese colon strings, or `FOLLOWING` word badges, or `?` → Highlight New.

---

## 6. Tests Required

- `help::` — context kind priority, live L1 for HDC and ADB (no `t`, has `^L`), idle status spans are `help`+`filter` (not `j/k move`), pending chip lists `tag`/`msg`, catalog includes Active + Navigation + `j/k` move, `FAST_SCROLL_STEP` matches catalog text.
- `dispatch_tests` — `?` open/Esc no-follow; `?` ignored when pending; `/` still Highlight New; Help `J`/`K` ±7; `j`/`k` ±1.
- `ui::` status bar — match stats without `[]`; wide idle shows help+filter not `j/k`; follow glyph when paused; pending has no `c…`; flash pill visible with pending L2; narrow keeps follow glyph and hides hints.

---

## 7. Wrong vs Correct

#### Wrong

```rust
// Separate Chinese status string + hard-coded Help paragraphs
const L1: &str = "j/k:移 Esc:随";
app.set_flash("已存在");
theme::status_badge(GLYPH_FOLLOWING, "FOLLOWING", success());
```

#### Correct

```rust
// Shared HintEntry; status subset vs full Help; English flash pill
status_hint_entries(app); // idle: help + filter
context_entries(app);     // Help Active stays full (j/k move, …)
app.set_flash("EXISTS");
theme::status_pill(GLYPH_FOLLOWING, success()); // on
theme::status_icon_dim(GLYPH_FOLLOWING);        // off
theme::status_flash_pill("EXISTS");
// Help J/K shares help::FAST_SCROLL_STEP with LogList
```

---

## Design Decision: Single Hint Source

**Context**: Status bar and Help must stay consistent after English redesign.

**Decision**: `help.rs` is the only keybinding copy source. `context_entries` stays full for Help; `status_hint_entries` is the status-bar subset. UI only styles/spans.

**Why**: Prevents Help catalog from shrinking when the status bar curates idle hints, and keeps dim-key rendering data-driven.
