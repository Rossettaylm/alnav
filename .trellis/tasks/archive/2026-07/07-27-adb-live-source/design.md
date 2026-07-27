# Design: adb live log source

## Architecture

`aloggrep-core` owns a backend-neutral live line session. Backend modules only
construct their device-time and capture commands:

```text
hdc/adb command -> LiveSession -> core run_lines
                            \-> TUI live ingest ring -> StreamStore -> UI
```

`LiveFilter` keeps the existing startup-marker contract: skip parsed records
older than the device clock marker, then pass all subsequent records including
unparsed stack-trace continuation lines.

## Backend contracts

| Backend | Device selector | Clock query | Capture command |
|---------|-----------------|-------------|-----------------|
| HDC | `-t SERIAL` | `shell date +%m-%d %H:%M:%S` | `hilog --no-block` |
| ADB | `-s SERIAL` | `shell date +%m-%d %H:%M:%S` | `logcat -v threadtime` |

Both return the same `LiveSession` shape:

```rust
pub struct LiveSession {
    pub child: Child,
    pub lines: LiveLines,
    pub used_history_fallback: bool,
}
```

Existing HDC public type names remain re-exported aliases so downstream code
using `aloggrep::hdc::{HdcLiveFilter, HdcSession}` keeps compiling.

## Core CLI

Define `live = cli.hdc || cli.adb`. Validation applies existing HDC restrictions
to either live backend, adds HDC/ADB mutual exclusion, and lets `--device`
require either backend. Backend selection occurs once; all processing after a
session is created remains shared.

## TUI

`ExportSource` remains the mode owner and gains `Adb`. It exposes file/live
predicates so Help, time-window dispatch, and Ctrl-L do not duplicate backend
matches. `spawn_live_ingest` accepts the shared `LiveSession`; the ring,
`App::drain`, StreamStore, Visible, filtering, and rendering are unchanged.

The child guard is renamed from HDC-specific to live-specific and still performs
`kill` followed by `wait` on every exit path.

## Compatibility and errors

- Missing `adb` reports an installation-oriented error and exits 2.
- Failure to query the device clock does not abort; it enables history fallback.
- Child stderr handling remains unchanged from HDC to avoid unrelated behavior
  changes.
- File input and stdin never enter the new live branch.

## Risks

- Device logcat options vary by Android version. `-v threadtime` and adb `-s`
  are established platform-tools interfaces and are the only required options.
- The existing marker is second-granularity, so records within the marker second
  can be included. This matches HDC.
- Cross-year ordering remains limited by the existing `MM-DD` representation.
