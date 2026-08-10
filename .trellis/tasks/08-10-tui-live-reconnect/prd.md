# TUI Live Stream Auto-Reconnect

## Goal

In Stream mode (`--hdc` / `--adb`), when the live child disconnects, show the existing disconnect icon and automatically respawn the capture session when the device is available again. On success, keep the old log buffer and append new lines (strategy A).

## Background

- Today `spawn_hilog` / `spawn_logcat` runs once at TUI start. EOF → `ingest_done=true` permanently; no respawn.
- Disconnect icon already exists: `!store.is_file() && ingest_done` → `GLYPH_DISCONNECT` in the status bar.
- User value: plug/unplug or transient hdc/adb death no longer requires restarting alnav.

## Requirements

### R1 — Disconnect indication

While live ingest is disconnected (`ingest_done` in Stream mode), status bar shows the existing disconnect glyph. No new glyph/color.

### R2 — Auto-reconnect with fixed backoff

When `export_source.is_live() && ingest_done`, every **2s** attempt `spawn_hilog` or `spawn_logcat` (same backend/device as session start). Success swaps ring + child; failure stays disconnected and retries.

### R3 — Buffer strategy A

On reconnect: do **not** clear `rows` / `matched` / `visible` / filters / lock / highlights / bookmarks. New lines append. User may `Ctrl-L` to clear manually.

### R4 — UX on success

Clear `ingest_done`, flash `RECONNECTED` (English, 3s via `set_flash`). Disconnect icon disappears. `following` unchanged (existing `follow_tick` pins bottom when following).

## Acceptance Criteria

- [x] AC1: After live child EOF, disconnect icon appears (existing path).
- [x] AC2: While disconnected, spawn is not attempted more often than every 2s.
- [x] AC3: Successful respawn clears `ingest_done`, flashes `RECONNECTED`, and new drained rows append without wiping the prior buffer.
- [x] AC4: Failed spawn leaves disconnected state and icon; later success still recovers.
- [x] AC5: File mode (`-f`) never attempts reconnect or shows reconnect flash from this path.
- [x] AC6: `cargo test -p alnav --bin alnav` green.

## Out of Scope

- Device-list polling / multi-device switching
- Configurable backoff
- Distinguishing disconnect reasons
- Detecting hung hdc with no EOF
- CLI (non-TUI) live reconnect

## Key Decisions

| Decision | Choice |
|----------|--------|
| Buffer on reconnect | Keep + append (A) |
| Reconnect signal | Spawn success (not device list) |
| Backoff | Fixed 2s |
| First retry after disconnect | Immediate (no forced initial wait) |
| Reconnect success criteria | Device probe (`now_marker`) OK **and** capture child still alive after 150ms grace |
| False spawn | Do **not** flash `RECONNECTED`; keep disconnect icon; retain backoff stamp |
