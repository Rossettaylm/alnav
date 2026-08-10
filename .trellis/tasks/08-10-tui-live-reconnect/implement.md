# Implement: Live Stream Auto-Reconnect

## Checklist

1. `LiveChildGuard::replace` in `main.rs`
2. `LiveBackend` + `LiveIngestCtl` + `RECONNECT_BACKOFF` + `try_reconnect`
3. `App::mark_live_reconnected` in `app.rs`
4. Wire `run_tui` / `run` to mutable ctl; reconnect after drain
5. Unit tests: replace, backoff skip, reconnect success preserves buffer
6. `cargo test -p alnav --bin alnav`

## Validation

```bash
cargo test -p alnav --bin alnav
cargo fmt -p alnav --check
```

## Risky files

- `alnav/src/main.rs` — event loop signature
- `alnav/src/app.rs` — flash / ingest_done

## Rollback

Revert ctl wiring; restore single spawn + immutable ingest ref.
