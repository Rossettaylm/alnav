# Add adb live log source

## Goal

Add an Android `adb logcat` live input backend to both `aloggrep` and
`aloggrep-tui`. It must behave like the existing HarmonyOS `--hdc` backend
except for the device command used to produce logs.

## Background

- `aloggrep-core` already owns the `--hdc` child-process session and live-start
  filtering used by both binaries.
- `aloggrep-tui` already has a bounded drop-oldest ring and StreamStore path for
  live input.
- Android logcat's `threadtime` format is already accepted by
  `LogEntry::parse()`.

## Requirements

- Both binaries expose `--adb`.
- `--adb`, `--hdc`, and file input are mutually exclusive. Core keeps stdin as
  its default when no explicit source is selected.
- `--device SERIAL` works with either live backend; adb maps it to `-s SERIAL`
  and hdc continues to map it to `-t SERIAL`.
- The adb backend runs `adb logcat -v threadtime`.
- The adb backend queries device time before capture and suppresses buffered
  entries older than startup, matching `--hdc`. If the query fails, capture
  continues with buffered history and the core CLI prints a warning.
- Core live capture retains Ctrl-L terminal clearing, Ctrl-C handling,
  filtering, formatting, sampling, summary, and child cleanup.
- TUI live capture retains the 8192-entry drop-oldest ingest ring,
  StreamStore/Visible behavior, Ctrl-L local-buffer clearing, hard-disabled
  interactive time window, Help hints, and child cleanup.
- TUI `yc` export emits `aloggrep --adb [--device SERIAL] ...` for adb sessions.
- Existing file, stdin, and hdc behavior remains compatible.

## Acceptance Criteria

- [ ] `aloggrep --adb` and `aloggrep-tui --adb` are accepted by clap.
- [ ] `--adb --device SERIAL` builds adb commands with `-s SERIAL`.
- [ ] adb output is forced to `threadtime` and parses through the current log
      model.
- [ ] startup-time filtering excludes older buffered lines and preserves
      unparsed continuation lines after the first live entry.
- [ ] invalid source combinations exit with code 2 and an actionable message.
- [ ] Ctrl-L, time-window gating, Help, ingest backpressure, export, and child
      cleanup treat adb and hdc as equivalent live modes.
- [ ] Core and TUI automated tests cover the new backend and all workspace
      tests, formatting, and clippy checks pass.
- [ ] Documentation and executable TUI specs describe both live backends.

## Out of Scope

- adb buffer selection, server/device discovery UI, wireless pairing, automatic
  reconnect, or adb-side filter pushdown.
- Clearing the device logcat ring.
- Changing existing `MM-DD` startup-marker comparison behavior at year
  boundaries.
