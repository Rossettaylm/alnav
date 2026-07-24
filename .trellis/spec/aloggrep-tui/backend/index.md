# Backend Development Guidelines

> Best practices for aloggrep-tui backend / TUI state machine work.

---

## Overview

Executable contracts for the TUI crate. Prefer these over re-deriving
behavior from CLAUDE.md when implementing filters, pickers, or modals.

---

## Guidelines Index

| Guide | Description | Status |
|-------|-------------|--------|
| [Directory Structure](./directory-structure.md) | Module layout + picker/time_panel ownership | Active |
| [Session Filters](./session-filters.md) | Lock + global `App.time_bound` contracts | Active |
| [Quality Guidelines](./quality-guidelines.md) | Forbidden patterns, testing requirements | Active |
| [Database Guidelines](./database-guidelines.md) | N/A for this crate | Stub |
| [Error Handling](./error-handling.md) | Error types, handling strategies | Stub |
| [Logging Guidelines](./logging-guidelines.md) | Structured logging, log levels | Stub |

---

## Pre-Development Checklist

- [ ] Read [session-filters.md](./session-filters.md) before changing filter/lock/time matching or `yc` export.
- [ ] Read [quality-guidelines.md](./quality-guidelines.md) Forbidden Patterns (theme colors, Group.time, modal Ctrl+C).
- [ ] Touching picker Manage: read Directory Structure "Picker session dispatch".

## Quality Check

- [ ] `Group` has no `time` field; time is on `App.time_bound`.
- [ ] Interactive time keys gated on `is_file_mode()`.
- [ ] New modal key paths handle Ctrl+C as cancel when appropriate.
- [ ] `cargo test -p aloggrep-tui` green; `cargo fmt -p aloggrep-tui --check` clean.

---

**Language**: All documentation should be written in **English**.
