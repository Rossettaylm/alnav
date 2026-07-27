# Journal - lymanyang (Part 1)

> AI development session journal
> Started: 2026-07-22

---



## Session 1: Picker mid-cursor editing + hardware caret

**Date**: 2026-07-23
**Task**: Picker mid-cursor editing + hardware caret
**Package**: aloggrep-core
**Branch**: `master`

### Summary

Implemented TextField mid-cursor editing for all Picker drafts with Manage key remaps (Ctrl-X / Delete / Ctrl-Backspace); then switched draft caret to terminal hardware cursor via Frame::set_cursor_position.

### Main Changes

- Detailed change bullets were not supplied; see the summary above.

### Git Commits

| Hash | Message |
|------|---------|
| `37007f0` | (see git log) |
| `b7fe30b` | (see git log) |

### Testing

- Validation was not recorded for this session.

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 2: TUI global time window (-f)

**Date**: 2026-07-24
**Task**: TUI global time window (-f)
**Package**: aloggrep-core
**Branch**: `master`

### Summary

Grilled and shipped App.time_bound with ts/tu panel (date candidates from rows, HH:MM:SS clamp); hdc hard-hide; filter_active/yc/TIME badge; Trellis session-filters spec; 351 tests green.

### Main Changes

- Detailed change bullets were not supplied; see the summary above.

### Git Commits

| Hash | Message |
|------|---------|
| `fcded9b` | (see git log) |

### Testing

- Validation was not recorded for this session.

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 3: Grill split hdc/mmap Trellis plans

**Date**: 2026-07-24
**Task**: Grill split hdc/mmap Trellis plans
**Package**: aloggrep-core
**Branch**: `master`

### Summary

Grilled mmap perf task; chose S1+P-after drop-oldest for hdc; B-gate for file; split into 07-24-tui-hdc-stream-visible then 07-24-tui-mmap-file-backend; planning artifacts written; not started.

### Main Changes

- Detailed change bullets were not supplied; see the summary above.

### Git Commits

(No commits - planning session)

### Testing

- Validation was not recorded for this session.

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 4: HDC Visible::All + drop-oldest ring

**Date**: 2026-07-24
**Task**: HDC Visible::All + drop-oldest ring
**Package**: aloggrep-core
**Branch**: `master`

### Summary

Implemented and checked Visible::All (O(1) eviction) plus P-after hdc DropOldestRing CAP=8192; committed; archived tui-hdc-stream-visible; mmap sibling remains planning.

### Main Changes

- Detailed change bullets were not supplied; see the summary above.

### Git Commits

| Hash | Message |
|------|---------|
| `95a701f` | (see git log) |

### Testing

- Validation was not recorded for this session.

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 5: mmap FileStore land + All-scan grill

**Date**: 2026-07-24
**Task**: mmap FileStore land + All-scan grill
**Package**: aloggrep-core
**Branch**: `master`

### Summary

Committed mmap FileStore (RowStore, bg filter, Subset). Grilled All-scan A1 (Vis/Inc, LogList L1+T+Free); next: new tui-file-async-scans task.

### Main Changes

- Detailed change bullets were not supplied; see the summary above.

### Git Commits

| Hash | Message |
|------|---------|
| `0dca14d` | (see git log) |

### Testing

- Validation was not recorded for this session.

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 6: File async Vis scans

**Date**: 2026-07-24
**Task**: File async Vis scans
**Package**: aloggrep-core
**Branch**: `master`

### Summary

Background Vis+Inc highlight hits, filter UI-parse reinforcement, severe prefetch, LogList L1+T+Free loading; archived tui-file-async-scans.

### Main Changes

- Detailed change bullets were not supplied; see the summary above.

### Git Commits

| Hash | Message |
|------|---------|
| `2dffcda` | (see git log) |

### Testing

- Validation was not recorded for this session.

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 7: TUI popup rounded borders + compact picker

**Date**: 2026-07-27
**Task**: TUI popup rounded borders + compact picker
**Package**: aloggrep-core
**Branch**: `master`

### Summary

Restored rounded four-sided popup chrome (modal/candidate/preview) with 1-cell gaps; half-width picker when preview hidden; confirm dialog anchors to actual picker frame. Spec: popup vs strip chrome contract.

### Main Changes

- Detailed change bullets were not supplied; see the summary above.

### Git Commits

| Hash | Message |
|------|---------|
| `c895275` | (see git log) |

### Testing

- Validation was not recorded for this session.

### Status

[OK] **Completed**

### Next Steps

- None - task complete
