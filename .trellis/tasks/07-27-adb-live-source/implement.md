# Implement: adb live log source

Each behavior follows red-green-refactor and each phase ends with its focused
test suite green.

## Phase 1 — Shared core live session and adb backend

1. Add failing tests for backend-neutral startup filtering and adb command
   construction (`-s`, device clock query, `logcat -v threadtime`).
2. Extract `LiveFilter`, `LiveLines`, and `LiveSession`; preserve HDC aliases.
3. Implement `adb::now_marker` and `adb::spawn_logcat`.
4. Add `Cli.adb`, then update CLI fixture literals.
5. Add failing validation tests, then apply live-source conflicts and share the
   existing core live run path across HDC and ADB.
6. Gate: `cargo test -p aloggrep-core`.

## Phase 2 — TUI live-source integration

1. Add failing export and dispatch tests for `ExportSource::Adb`.
2. Generalize HDC ingest to `spawn_live_ingest(LiveSession)` and retain bounded
   drop-oldest behavior.
3. Add `Cli.adb`, three-way source validation, adb session spawn, and a generic
   live child guard.
4. Generalize Help context, Ctrl-L handling, and interactive time-window gating
   from HDC to live mode.
5. Gate: `cargo test -p aloggrep-tui`.

## Phase 3 — Documentation and full verification

1. Update README, project architecture guidance, and executable TUI specs.
2. Run:

```bash
cargo fmt --check
cargo test -p aloggrep-core
cargo test -p aloggrep-tui
cargo test --workspace
cargo clippy --workspace --all-targets
```

3. If an Android device is available, smoke-test
   `cargo run -p aloggrep-tui -- --adb [--device SERIAL]`. Otherwise record the
   unavailability without weakening automated verification.

## Rollback points

- `live.rs` retains HDC aliases; reverting adb call sites leaves HDC behavior
  intact.
- TUI data-plane modules (`app.rs`, `store.rs`) are not redesigned.
- No device-side log buffer is mutated.

## Review gates

- Confirm no explicit HDC-only checks remain where live-mode behavior is
  required.
- Confirm `ExportSource` matches are exhaustive.
- Confirm every new public function has a focused test and all new tests were
  observed failing before implementation.
