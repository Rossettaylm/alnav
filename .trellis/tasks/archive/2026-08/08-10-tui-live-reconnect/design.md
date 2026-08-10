# Design: Live Stream Auto-Reconnect

## Architecture

`LiveIngestCtl` owns the replaceable live session for TUI Stream mode:

- `backend`: Hdc | Adb
- `device`: optional serial
- `ingest`: `Option<IngestHandle>` (Ring)
- `child`: `LiveChildGuard` with `replace`
- `last_reconnect_at`: backoff clock

File mode passes `None` ctl (or ctl with no backend); no reconnect.

## Data flow

1. Frame: `drain(ingest)` → may set `ingest_done`
2. If live + `ingest_done`: `try_reconnect`
   - backoff gate (2s since last attempt; `None` → try now)
   - `spawn_hilog` / `spawn_logcat`
   - on Ok: `spawn_live_ingest` → replace ring + child → `App::mark_live_reconnected`
   - on Err: keep disconnected
3. Optional same-frame second `drain` after successful reconnect

## Contracts

- Reuse `alnav::hdc::spawn_hilog` / `alnav::adb::spawn_logcat` (fresh `now_marker`).
- Old producer finishes independently; dropping old `IngestHandle` Arc is fine.
- `LiveChildGuard::replace` kill/wait old before taking new; Drop still cleans final child.
- UI disconnect path unchanged (`ingest_done`).

## Trade-offs

- Spawn-as-probe is simpler than `hdc list targets` / `adb devices` and matches "can we capture again?".
- Immediate first retry after disconnect; subsequent failures wait 2s.
- No clear-on-reconnect (user Ctrl-L).

## Test seams

- `try_reconnect` accepts injectable spawn closure for unit tests.
- `LiveChildGuard::replace` tested with short-lived processes.
