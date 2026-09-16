# Design — File Session Restore

## Boundaries

- **In**: `alnav` (TUI crate) only — new `session.rs`, `AppConfig.restore_session`,
  binding/persist hooks in `main.rs`, restore/pending-cursor plumbing in `app.rs`,
  one command-palette action.
- **Out**: `alnav-core` (no CLI surface change), live sessions, preset wire format,
  `recent_files.toml`, theme.
- **Reused**: `$config_dir` resolution, `Expr`/`Chip`/`Group` model, existing
  `set_flash` + `status_flash_until`, existing `BOOKMARK EVICTED` path,
  `App::rebuild_visible`, `App::poll_file_store`.

## Module: `alnav/src/session.rs` (new)

Owns the wire format, path keying, atomic write, and LRU eviction. Nothing else
in the codebase knows the on-disk shape.

```rust
pub const SESSION_VERSION: u32 = 1;
pub const SESSIONS_DIR_NAME: &str = "sessions";
pub const MAX_SESSION_FILES: usize = 50;

/// FNV-1a 64-bit over the normalized absolute path. No new dependency.
fn fnv1a64(s: &str) -> u64;

pub fn sessions_dir(config_dir: &Path) -> PathBuf;              // config_dir/sessions
pub fn session_path(config_dir: &Path, source: &Path) -> PathBuf; // <hash>.toml

/// Normalized absolute path (canonicalize, else cwd-join). Mirrors
/// `recent::normalize_path`; returns `None` when the path is unusable.
pub fn normalize_source(path: &Path) -> Option<String>;

#[derive(Serialize, Deserialize)]
pub struct SessionChip { field: String, value: String }

#[derive(Serialize, Deserialize)]
pub struct SessionFilterGroup { chips: Vec<SessionChip>, enabled: bool, same_field_op: String }

#[derive(Serialize, Deserialize)]
pub struct SessionHighlight { pattern: String, enabled: bool }

#[derive(Serialize, Deserialize)]
pub struct SessionBookmark { row_id: u64, raw: String }

#[derive(Serialize, Deserialize)]
pub struct SessionFile {
    pub version: u32,
    pub source: String,
    pub filters: Vec<SessionFilterGroup>,
    pub excludes: Vec<SessionFilterGroup>,   // single-chip; reuses SessionFilterGroup
    pub highlights: Vec<SessionHighlight>,
    pub active_highlight: Option<usize>,
    pub lock_pid: Option<String>,
    pub lock_tid: Option<String>,
    pub time_since: Option<String>,
    pub time_until: Option<String>,
    pub view_focus_highlight: bool,
    pub view_focus_severe: bool,
    pub cursor_row_id: u64,
    pub list_offset: usize,
    pub bookmarks: Vec<SessionBookmark>,
}

impl SessionFile {
    /// True when nothing worth persisting is present (R6).
    pub fn is_degenerate(&self) -> bool;
    pub fn is_active(&self) -> bool;  // !is_degenerate — drives the RESTORED flash
}

pub enum SessionLoad {
    Missing,
    Loaded(SessionFile),
    Invalid(String),   // drives `SESSION fallback: …`
}

pub fn load(config_dir: &Path, source: &Path) -> SessionLoad;
pub fn save(config_dir: &Path, source: &Path, file: &SessionFile) -> Result<(), String>;
pub fn clear(config_dir: &Path, source: &Path) -> Result<(), String>;
fn evict_oldest(config_dir: &Path);  // mtime LRU, called after a successful save
```

### Wire format

```toml
version = 1
source = "/abs/path/a.log"
active_highlight = 1
lock_pid = "1234"
time_since = "09-15 10:00:00"
view_focus_highlight = false
view_focus_severe = true
cursor_row_id = 8421
list_offset = 17

[[filters]]
enabled = true
same_field_op = "and"          # "and" | "or"
chips = [{ field = "tag", value = "MyApp" }]

[[excludes]]
enabled = true
chips = [{ field = "tag", value = "Spam" }]

[[highlights]]
pattern = "error"
enabled = true

[[bookmarks]]
row_id = 8123
raw = "09-15 10:00:00.000  1234  5678 E MyApp   : boom"
```

`field` strings match `ChipField::keyword()` exactly (`tag`/`msg`/`pkg`/`pid`/
`tid`/`level`) so `parse_field` can be shared in spirit with `preset.rs` but lives
locally (session must keep `enabled`, preset drops it — different contract).

`same_field_op` accepts `"and"` / `"or"` case-insensitively; anything else →
`Invalid`.

### Atomic write

Write `sessions/.tmp-<hash>-<pid>` then `fs::rename` over the target. On any I/O
error remove the temp file and return `Err`. Directory created with
`create_dir_all` on first save.

Rust has no `tmpnam`-style helper here and the process is single-threaded for
writes (one save per exit / source switch), so `<pid>` is enough to disambiguate.

### LRU eviction

After a successful save, `read_dir(sessions)` → collect `(*.toml, mtime)` →
if `len > MAX_SESSION_FILES`, sort ascending by mtime and remove the excess.
Best-effort: eviction errors are ignored (a failed eviction must not fail the
save). Temp files (`.*` prefix) are skipped when counting.

### Load validation

`load` returns `Invalid(reason)` for: unreadable file, TOML parse error,
`version != SESSION_VERSION`, `source != normalize_source(source)`, empty
`source`, or any chip whose field/value fails `build_group_from_chips`-equivalent
validation. `Missing` when the file does not exist. Callers treat both `Missing`
and `Invalid` as "no session"; only `Invalid` flashes.

## App integration

### New fields on `App`

```rust
/// Row id to restore once `visible` first becomes non-empty (R7).
pub pending_restore_cursor: Option<u64>,
/// Suppresses session read AND write for this session (R5).
pub session_restore_disabled: bool,
/// Set by `bind_file_source` when a restore was applied (R10 flash gate).
pub session_restored: bool,
```

`session_restore_disabled` is computed once in `run_tui`:

```rust
app.session_restore_disabled = !app.config.restore_session || cli_has_filter_args(&cli);
```

`cli_has_filter_args` = any of `tag`/`msg`/`level`/`pkg`/`pid`/`tid`/`since`/`until`
non-empty. It is **not** `-i` (no-op) and not `--max-lines`.

### Capture / apply — `app.rs`

```rust
impl App {
    /// Snapshot the current session for `source`. `None` when disabled or degenerate.
    pub fn capture_session(&self, source: &Path) -> Option<session::SessionFile>;

    /// Apply a loaded session: rules/time/lock/view/bms now, cursor deferred.
    pub fn apply_session(&mut self, s: &session::SessionFile) -> Result<(), String>;
}
```

`capture_session` returns `None` when `session_restore_disabled` (R5) or when the
resulting `SessionFile::is_degenerate()` (R6). It reads `cursor_row_id` from the
**current row** (`current_row().row_id`), not `self.cursor`, so the stored value is
the stable id; for File mode `row_id == line_index + 1`.

`apply_session`:

1. Replace `groups.groups` / `groups.excludes` / `highlight_groups.groups` wholesale
   (the binding path has already run `reset_for_source_switch()` for `switching`
   binds; for the `-f` / Dashboard first-bind path the lists are empty except for
   `initial_group`, which R5 guarantees is empty when we restore).
2. Set `active_highlight` (clamped to `highlight_groups.len()`; out of range → `None`).
3. Set `lock_pid` / `lock_tid`; **if both present, drop `lock_tid`** (the app
   invariant is mutual exclusion — see `LockKind` handling).
4. Rebuild `time_bound` from `time_since` / `time_until`; `None` when both absent.
5. Set `view_focus`.
6. Rebuild `bookmarks` via `EntryRow::from_line_or_raw(raw)` then overwrite `row_id`;
   push through `BookmarkList::try_add` so the 16-cap and dedup still hold; rebuild
   `bookmark_row_ids` from the accepted set.
7. `pending_restore_cursor = Some(cursor_row_id)` (skipped when `0`).
8. `list_offset` is applied together with the cursor (same deferred step) — storing
   it applied immediately would let `rebuild_visible`'s clamp fight the later cursor
   jump.
9. `following = false`.

### Deferred cursor consumption

In `App::poll_file_store`, at the end of the existing event loop:

```rust
if let Some(row_id) = self.pending_restore_cursor {
    // Consume on success, OR once the line index is complete (then give up).
    match self.visible_idx_for_row_id(row_id) {
        Some(idx) => {
            self.pending_restore_cursor = None;
            self.cursor = idx;
            self.list_offset = self.list_offset.min(self.cursor);
            self.match_stats_stale = true;
        }
        None if self.file_index_done() => {
            // Row is gone (file shrank / rotated) → R8 degrade to top.
            self.pending_restore_cursor = None;
            self.cursor = 0;
            self.list_offset = 0;
        }
        None => {} // index still growing; try again next poll
    }
}
```

`visible_idx_for_row_id` already binary-searches `Visible::Subset` and walks
`Visible::All`, so no new lookup helper is needed.

**Why not `!visible.is_empty()` as the only guard:** `FileStore` grows
`line_count` incrementally, so the first non-empty `visible` can arrive while the
index still covers only a prefix of the file. Consuming then would resolve a
mid/late-file `row_id` to `None` and silently drop the restore. The
consume-on-success-or-index-done rule above is correct in both orders
(index-before-filter and filter-before-index) without a timer.

`file_index_done()` is a small accessor over the existing
`FileStore::index_done()` (`false` for `RowStore::Stream`). This is the only place
the deferred jump is applied.

## Persist hooks (`main.rs`)

| Site | Action |
|---|---|
| `run_tui`, `-f` branch | after `bind_file_source(..., false)` → restore (R2) |
| `bind_file_source(switching == false)` | restore + `session_restored = true` (covers Dashboard) |
| `bind_file_source(switching == true)` | **before** `reset_for_source_switch()` → save the leaving file |
| `bind_live_source(any)` | **before** reset → save the leaving file (R3) |
| `run()` returns | save the current file |

To capture the leaving file, `bind_*_source` needs the old path **before** the
reset: read `self.export_source` while it is still `File(path)`.

```rust
fn persist_current_file(app: &App) {
    if app.session_restore_disabled { return; }
    let ExportSource::File(path) = &app.export_source else { return };
    if let Some(s) = app.capture_session(Path::new(path)) {
        if let Err(e) = session::save(&app.config_dir, Path::new(path), &s) {
            // flash is not visible during teardown; ignore silently at exit,
            // surface it on the switch path.
        }
    }
}
```

At exit the flash cannot be seen, so the exit-path save ignores errors. On the
switch path the error is surfaced with `set_flash`.

### Restore call

```rust
fn restore_session(app: &mut App, path: &Path) -> bool {
    if app.session_restore_disabled { return false; }
    match session::load(&app.config_dir, path) {
        SessionLoad::Missing => false,
        SessionLoad::Invalid(reason) => { app.set_flash(format!("SESSION fallback: {reason}")); false }
        SessionLoad::Loaded(s) => match app.apply_session(&s) {
            Ok(()) => { app.session_restored = s.is_active(); app.session_restored }
            Err(e) => { app.set_flash(format!("SESSION fallback: {e}")); false }
        },
    }
}
```

Restore runs **after** `set_file_store` (so `FileStore` exists and
`rebuild_visible` has a store to scan) and **before** the trailing
`record_recent_file`, keeping the two orthogonal.

Order inside `bind_file_source`:

```
open FileStore
if switching { persist_current_file(app); app.reset_for_source_switch(); }
app.set_file_store(file_store)
app.export_source = File(path)
app.dashboard = None
app.close_source_panels()
let restored = !switching && restore_session(app, &path_buf);  // R2
app.record_recent_file(&path_buf)
if app.filter_active() { app.rebuild_visible(); }
flash RESTORED when restored && came-from-dashboard
```

### `RESTORED` flash gate (R10)

The flash is only emitted on the Dashboard path. `bind_file_source` can tell:
the Dashboard call site (`handle_dashboard_key` / `handle_open_file_panel_key`)
passes `switching = app.dashboard.is_none()`; the flag `app.dashboard.is_some()`
is therefore still `true` when entering from the Dashboard. Capture it before
`app.dashboard = None`:

```rust
let from_dashboard = app.dashboard.is_some();
...
if restored && from_dashboard {
    app.set_flash(restored_label(&s));  // e.g. "RESTORED 3 FILTERS · TIME · ★2"
}
```

`-f` startup keeps `from_dashboard == false` → silent, which also avoids racing
the `THEME/CONFIG/KEYMAP fallback` hints set earlier in `run_tui`.

## Command palette: `Clear session`

- `ActionId::ClearSession`, `KeyContext::Global`, `toml_key = "clear_session"`,
  `default` unbound (palette-only, same shape as `ClearAllRules`),
  `capabilities = &[Capability::FileOnly]`,
  `.with_palette("Clear Session", theme::GLYPH_TITLE_DASHBOARD)`.
- Add to `PALETTE_IDS` and `ActionId::ALL`.
- `action::when`: `file_mode && app.has_session_worth_clearing()` — true when
  `!session_restore_disabled` and `!matches!(session::load(...), Missing)`.
- `action::dispatch`: `ClearSession => app.clear_session_for_current_file()` on
  `App`, which calls `session::clear` and flashes `SESSION CLEARED` (or
  `NO SESSION` when already absent).
- Unbound default means `KeymapStore` gets a new entry with no stroke; the
  keymap round-trip test asserts every `ActionId::ALL` entry has metadata, so the
  meta table must be updated or `builtin_defaults_roundtrip` fails.

## `config.toml`

`AppConfig::restore_session: bool` (default `true`), `ConfigToml.restore_session:
Option<bool>`, and a commented block in `serialize_default_config_toml()`:

```
# restore_session: remember the last Filter / Exclude / Highlight / lock /
# time-window / cursor / bookmark state per file, and bring it back the next
# time that file is opened. Sessions are skipped entirely when filter flags are
# passed on the command line. Set false to disable. Default true.
restore_session = true
```

## Compatibility / rollback

- Purely additive: no existing file format changes, no CLI surface change.
- `restore_session = false` or `rm -rf $config_dir/sessions` restores today's
  behavior exactly.
- `version != 1` documents are ignored rather than migrated (R9).

## Risks

| Risk | Mitigation |
|---|---|
| `apply_session` replaces lists while a File scan is in flight | restore runs before `rebuild_visible`; `set_file_store` already started the (empty-rule) scan, `rebuild_visible` restarts it with the restored predicate |
| Deferred cursor never fires (filtered result empty, or row evicted) | Guard is consume-on-success-or-`file_index_done`, not one-shot-on-first-poll; on index-done miss it degrades to cursor 0 (R8) rather than hanging |
| `lock_pid` + `lock_tid` both set in a hand-edited file | `apply_session` keeps `pid`, drops `tid` |
| Bookmark `raw` grows the file | 16-pin cap bounds it; line length is the natural unit |
| Saving on every source switch | one small file write per switch; the common path (open once, quit) writes once |
