# Implement — File Session Restore

Active task: `.trellis/tasks/09-15-file-session-restore`

## Preconditions

- Read: `prd.md` → `design.md` → `implement.jsonl` entries.
- Baseline green: `cargo test --workspace` before starting.

## Ordered checklist

### 1. `alnav/src/session.rs` (new)

- [ ] Header doc: wire format ownership, R-numbers it implements.
- [ ] `SESSION_VERSION`, `SESSIONS_DIR_NAME`, `MAX_SESSION_FILES`.
- [ ] `fnv1a64(&str) -> u64` + unit test with known vectors
      (`""` → `0xcbf29ce484222325`, `"a"` → `0xaf63dc4c8601ec8c`).
- [ ] `normalize_source`, `sessions_dir`, `session_path`.
- [ ] `SessionChip` / `SessionFilterGroup` / `SessionHighlight` / `SessionBookmark`
      / `SessionFile` with `Serialize` + `Deserialize`.
- [ ] `SessionFile::is_degenerate` / `is_active`.
- [ ] `chip_to_wire` / `wire_to_chip` (field string ↔ `ChipField`, reject unknown).
- [ ] `same_field_op` ↔ `"and"`/`"or"` (case-insensitive).
- [ ] `load` → `SessionLoad::{Missing, Loaded, Invalid}` with version + source
      validation.
- [ ] `save` → temp file + `fs::rename`; `create_dir_all`; cleanup temp on error.
- [ ] `clear`.
- [ ] `evict_oldest` (mtime ascending, skip dot-dirs, best-effort).
- [ ] Unit tests: round-trip; disabled group/exclude preserved; version mismatch →
      `Invalid`; source mismatch → `Invalid`; atomic write leaves no temp;
      eviction keeps newest 50.

**Validation:** `cargo test -p alnav session::`

### 2. `alnav/src/config.rs`

- [ ] `AppConfig.restore_session: bool`, default `true`.
- [ ] `ConfigToml.restore_session: Option<bool>`; read in `load_config`.
- [ ] Tests: default true; `restore_session = false` parsed.

### 3. `alnav/src/keymap.rs`

- [ ] Add `restore_session = true` block (with comment) to
      `serialize_default_config_toml()`.
- [ ] `ActionId::ClearSession` enum variant + `ActionId::ALL` entry.
- [ ] `ActionMeta` for it: `KeyContext::Global`, `toml_key = "clear_session"`,
      `default` unbound, `kind: Leaf`, `capabilities: &[Capability::FileOnly]`,
      `.with_palette("Clear Session", theme::GLYPH_TITLE_DASHBOARD)`.
- [ ] `(KeyContext::Global, "clear_session") => Some(ActionId::ClearSession)` in
      the toml-key resolver.
- [ ] Confirm `builtin_defaults_roundtrip` / `ALL` metadata tests still pass.

### 4. `alnav/src/app.rs`

- [ ] Fields: `pending_restore_cursor: Option<u64>`, `session_restore_disabled:
      bool`, `session_restored: bool` (+ `App::new` defaults).
- [ ] `capture_session(&self, source: &Path) -> Option<SessionFile>`.
- [ ] `apply_session(&mut self, &SessionFile) -> Result<(), String>`.
- [ ] `clear_session_for_current_file(&mut self)`.
- [ ] `has_session_worth_clearing(&self) -> bool`.
- [ ] `poll_file_store`: consume `pending_restore_cursor` via
      `visible_idx_for_row_id` on success, or on `file_index_done()` → cursor 0
      (R8). Add the `file_index_done()` accessor (`false` for Stream).
- [ ] `reset_for_source_switch`: reset the three new fields' transient parts
      (`pending_restore_cursor = None`, `session_restored = false`); keep
      `session_restore_disabled` (session-scoped).
- [ ] Unit tests: capture/apply round-trip across all fields; degenerate → `None`;
      disabled → `None`; both locks → tid dropped; `active_highlight` out of range
      → `None`; deferred cursor consumed on first non-empty `visible`; out-of-range
      `row_id` → cursor 0; bookmarks restored with cap + `bookmark_row_ids`
      lockstep.

### 5. `alnav/src/action.rs`

- [ ] Add `ActionId::ClearSession` to `PALETTE_IDS`.
- [ ] `when`: `Capability::FileOnly` via meta + `app.has_session_worth_clearing()`.
- [ ] `dispatch`: `ClearSession => app.clear_session_for_current_file()`.

### 6. `alnav/src/main.rs`

- [ ] `cli_has_filter_args(&TuiCli) -> bool`.
- [ ] In `run_tui`: set `app.session_restore_disabled` after config load.
- [ ] `persist_current_file(&App)` / `save_session_or_flash(&mut App)` helpers.
- [ ] `restore_session(&mut App, &Path) -> bool` helper.
- [ ] `bind_file_source`: save leaving file before reset when `switching`; restore
      after `set_file_store` when `!switching`; `RESTORED` flash only when
      `from_dashboard`.
- [ ] `bind_live_source`: save leaving file before reset.
- [ ] `run_tui`: after `run(...)` returns (and after terminal teardown), save the
      current file.
- [ ] Tests: `-f` + no CLI filters → restore; `-f --tag X` → no restore and no
      overwrite; `restore_session = false` → neither; switching saves old; live
      never writes.

### 7. Help / docs

- [ ] `help.rs`: mention `Clear Session` in the palette-relevant page if the
      catalog listing there is generated from `PALETTE_IDS` (verify, do not
      hand-duplicate).
- [ ] `alnav/examples/config.toml` (if present): add `restore_session`.

### 8. Spec

- [ ] `.trellis/spec/alnav/backend/session-restore.md` (new) — contracts: trigger
      points, R5 suppression rule, degenerate guard, deferred cursor, wire version.
- [ ] Link from `.trellis/spec/alnav/backend/index.md` + pre-dev checklist.

## Validation commands

```bash
cargo build --workspace
cargo test -p alnav session::
cargo test -p alnav --bin alnav
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
```

Manual smoke (real TTY):

```bash
cargo run -p alnav --bin alnav -- -f /tmp/a.log   # set rules, quit
cargo run -p alnav --bin alnav -- -f /tmp/a.log   # expect restore
cargo run -p alnav --bin alnav -- -f /tmp/a.log --tag Foo   # expect no restore
ls ~/.config/alnav/sessions/
```

## Review gates

1. After step 1 — `session.rs` tests green before any integration.
2. After step 6 — full manual smoke walkthrough before spec/commit.
3. Before commit — `cargo test --workspace` + `cargo clippy` clean.

## Rollback points

- Steps 1–3 are additive and inert on their own.
- Step 4 adds plumbing that is still inert (nothing calls it).
- Step 6 activates behavior — revert only step 6 to return to today's behavior
  without touching the rest.
