# File Session Restore

> Executable contracts for per-file analysis sessions under
> `$config_dir/sessions/` (task `09-15-file-session-restore`).

---

## Overview

`-f` file sessions persist the analysis scene on exit and restore it the next
time that same file is opened: Filter / Exclude / Highlight rules (including
disabled ones), lock, global time window, view focus, cursor, and bookmarks.

Live sources (`--hdc` / `--adb`) never participate — an unbounded stream has no
stable position, and `bind_live_source` actively clears `time_bound`.

---

## 1. Scope / Trigger

Update this spec when changing:

- `session.rs` wire format, path keying, atomic write, or LRU eviction
- restore / persist hook sites in `main.rs`
- `App::capture_session` / `apply_session` / deferred cursor consumption
- `AppConfig.restore_session`, or which CLI flags suppress sessions
- `ActionId::ClearSession`

**Out of scope**: `preset.rs` (named rule presets), `recent_files.toml`,
live sessions, CLI `alnav grep`.

---

## 2. Signatures

```rust
// session.rs — sole owner of the on-disk shape
pub const SESSION_VERSION: u32 = 1;
pub const SESSIONS_DIR_NAME: &str = "sessions";
pub const MAX_SESSION_FILES: usize = 50;

pub fn sessions_dir(config_dir: &Path) -> PathBuf;
pub fn session_path(config_dir: &Path, source: &Path) -> PathBuf; // <fnv1a64>.toml
pub fn normalize_source(path: &Path) -> Option<String>;

pub struct SessionFile { /* version, source, filters, excludes, highlights,
    active_highlight, lock_pid, lock_tid, time_since, time_until,
    view_focus_highlight, view_focus_severe, cursor_row_id, list_offset,
    bookmarks */ }
impl SessionFile {
    pub fn is_degenerate(&self) -> bool;
    pub fn is_active(&self) -> bool;
    pub fn time_bound(&self) -> Option<TimeBound>;
}

pub enum SessionLoad { Missing, Loaded(SessionFile), Invalid(String) }

pub fn load(config_dir: &Path, source: &Path) -> SessionLoad;
pub fn save(config_dir: &Path, source: &Path, file: &SessionFile) -> Result<(), String>;
pub fn clear(config_dir: &Path, source: &Path) -> Result<(), String>;
pub fn exists(config_dir: &Path, source: &Path) -> bool;

// app.rs
pub pending_restore_cursor: Option<u64>;
pub session_restore_disabled: bool;
pub session_restored: bool;

pub fn capture_session(&self, source: &Path) -> Option<session::SessionFile>;
pub fn apply_session(&mut self, s: &session::SessionFile) -> Result<(), String>;
pub fn has_session_worth_clearing(&self) -> bool;
pub fn clear_session_for_current_file(&mut self);
pub fn file_index_done(&self) -> bool;   // false for Stream

// main.rs
fn cli_has_filter_args(cli: &TuiCli) -> bool;
fn persist_current_file(app: &App);          // silent (teardown)
fn save_session_or_flash(app: &mut App);     // flashes (source switch)
fn restore_session(app: &mut App, path: &Path) -> bool;
fn restored_label(app: &App) -> String;
```

---

## 3. Contracts

| Topic | Rule |
|-------|------|
| Key | FNV-1a 64 of the normalized absolute path → `sessions/<hex>.toml`. Hand-written hash, no new dependency. |
| Self-check | The file records its own `source`; a mismatch is `Invalid`, not a silent cross-file restore. |
| Capacity | 50 files; oldest mtime evicted after a successful save. Eviction failure must not fail the save. |
| Write | Temp file (`.tmp-<stem>-<pid>`) + `fs::rename`. Temp is removed on any error. |
| Version | `version = 1`. Mismatch → `Invalid`; **no** migration, **no** delete. |
| Restore trigger | Only the **first** file bind of the process: `alnav -f`, or Dashboard open. `switching == true` (`C-f`) does **not** restore. Live binds never restore. |
| Persist trigger | After `run()` returns (covers `q` / Ctrl+C / Dashboard Quit); before `reset_for_source_switch` on `C-f`; before reset on file→live. Panic does **not** save (it unwinds past the hook). |
| Persisted | Filter groups + `enabled` + `same_field_op`; excludes + `enabled`; highlights + `enabled`; `active_highlight`; lock pid/tid; time window; view focus; `cursor_row_id`; `list_offset`; bookmarks (`raw` + `row_id`). |
| Never persisted | `following` (restore forces `false`), `collapsed_view`, `visual_anchor`, all `pending_*`, modal state, search drafts, `detail`, Dashboard cursor. |
| Suppression (R5) | `restore_session = false` **or** any of `--tag/--msg/--level/--pkg/--pid/--tid/--since/--until` ⇒ neither read nor write. One flag gates both directions so a one-shot CLI run cannot clobber a curated session. `-i` and `--max-lines` do not suppress. |
| Degenerate (R6) | No filters, no excludes, no highlights, no bookmarks, **no lock, no time window, no view-focus bit**, and `cursor_row_id <= 1` ⇒ skip the write, keep any existing archive. Lock / time / view-focus count as configuration — omitting them would silently discard a scene the user built with only those. |
| Lock invariant | `lock_pid` and `lock_tid` are mutually exclusive; a hand-edited file with both keeps `pid`. |
| Bookmarks | Rebuilt via `EntryRow::from_line_or_raw` with `row_id` overwritten and `severe` recomputed; pushed through `BookmarkList::try_add` so the 16-cap and dedup hold. `bookmark_row_ids` is rebuilt in lockstep. |
| Degradation (R8) | Path existence only — **no** content/size/mtime fingerprint (append is normal for logs and would make restore never fire). Out-of-range cursor / offset clamp; out-of-range pins take the existing `BOOKMARK EVICTED` path. A restored window that filters to nothing is replayed faithfully. |
| Preset reuse | `preset.rs`'s wire format must **not** be reused: `capture()` drops disabled entries, a session must round-trip `enabled`. |

### Deferred cursor (R7)

Rules / time / lock / view focus / bookmarks apply immediately at bind, then
`rebuild_visible()`. The cursor is deferred to `App::poll_file_store`:

- Consume when `visible_idx_for_row_id(row_id)` resolves → set `cursor`,
  `list_offset = stored.min(cursor)`.
- Give up only once `file_index_done()` → `cursor = 0`, `list_offset = 0`.
- Otherwise leave it pending and retry next poll.

"First non-empty `visible`" is **not** a valid trigger: `FileStore` grows
`line_count` incrementally, so `visible` can be non-empty while the index still
covers only a prefix. Consuming then would resolve a mid-file `row_id` to `None`
and silently drop the restore. The consume-on-success-or-index-done rule is
correct for both orders (index-before-filter and filter-before-index) with no
timer.

### Feedback (R10)

- Restore success → `RESTORED …` flash (rule / time / pin counts) **only** on
  the Dashboard path (`app.dashboard.is_some()` captured before it is cleared).
  `-f` startup is silent, which also avoids racing the
  `THEME/CONFIG/KEYMAP fallback` hints set earlier in `run_tui`.
- `Invalid` archive → `SESSION fallback: <reason>`; the file is kept.
- `ActionId::ClearSession` (palette **Clear Session**, `Global`,
  `Capability::FileOnly`, unbound by default) deletes the current file's
  archive: `SESSION CLEARED`, or `NO SESSION` when already absent. `when` is
  false when suppressed or when no archive exists.

---

## 4. Validation & Error Matrix

| Condition | Behavior |
|-----------|----------|
| Archive missing | `Missing`; silent start |
| Corrupt TOML / bad version / source mismatch | `Invalid`; flash `SESSION fallback: …`; file kept |
| Unknown chip field / empty chip value / bad `same_field_op` | `Invalid` (validated by `materialize_rules`) |
| CLI filter flag present | No read, no write; existing archive untouched |
| `restore_session = false` | No read, no write |
| Degenerate scene at exit | No write; existing archive preserved |
| `cursor_row_id` beyond EOF | Cursor 0 once the index completes |
| Both locks set in the file | `pid` kept, `tid` dropped |
| `> 16` pins in the file | First 16 accepted (`try_add` cap) |
| `> 50` archives | Oldest mtime removed after save |
| Live source at exit | Nothing written; `sessions/` not even created |

---

## 5. Good / Base / Bad Cases

- **Good**: `-f a.log`, build rules + pins + time window, quit; reopen `a.log`
  → same scene, `following == false`, cursor back on the same line.
- **Base**: `-f b.log` is unaffected by `a.log`'s archive; `a.log`'s archive
  still restores afterwards.
- **Bad**: `alnav -f a.log --tag Foo` overwriting yesterday's archive;
  restoring on a `C-f` switch; fingerprinting file content so append breaks
  restore; reusing `Preset` and losing `enabled`; consuming the deferred cursor
  on first non-empty `visible`.

---

## 6. Tests Required

- `session::` — FNV vectors; TOML round-trip; disabled group / exclude /
  highlight preserved; version + source mismatch → `Invalid`; corrupt file kept;
  atomic write leaves no temp; eviction trims to 50 and keeps the newest;
  `clear` idempotent; degenerate detection.
- `app::` — capture/apply round-trip over every persisted field; degenerate →
  `None`; suppressed → `None`; both locks → tid dropped; out-of-range
  `active_highlight` → `None`; pin cap + `bookmark_row_ids` lockstep; deferred
  cursor lands; out-of-range cursor → 0; `reset_for_source_switch` keeps
  suppression and clears transients; Stream `file_index_done()` false.
- `main::` — `cli_has_filter_args` per flag (and **not** for `-i` /
  `--max-lines`); first bind restores; switching bind does not; switching away
  persists the leaving file; file→live persists; disabled → neither direction;
  degenerate does not overwrite; corrupt → fallback flash; live writes nothing;
  `restored_label` content.
- `action::` — `ClearSession` hidden without an archive, hidden on live, visible
  with one; dispatch clears and flashes.
- `keymap::` — `clear_session` meta is Global / unbound / FileOnly / in palette;
  `restore_session = true` in the `--init` config template.
- `config::` — default true; `restore_session = false` parsed.

---

## Design Decision: one suppression rule for both directions

**Context**: `alnav -f a.log --tag Foo` is a one-shot command-line query. If it
read the archive, the CLI filter would compose with yesterday's rules; if it
wrote one, glancing at a log with `--tag` would clobber a curated scene.

**Decision**: a single `session_restore_disabled` flag, computed once in
`run_tui`, gates read **and** write.

**Extensibility**: new one-shot flags join `cli_has_filter_args`; nothing else
changes.

---

## Design Decision: no content fingerprint

**Context**: A size/mtime/hash guard would make restore correct-by-construction
against edits.

**Decision**: validate the path only. Logs are append-mostly, so any content
fingerprint would invalidate nearly every restore. Stale positions degrade
(clamp / `BOOKMARK EVICTED`) instead of blocking the restore.
