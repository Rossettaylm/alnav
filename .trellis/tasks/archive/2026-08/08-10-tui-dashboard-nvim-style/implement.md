# Implement: Hyper-style TUI Dashboard

## Ordered Checklist

1. [x] Add pure Dashboard presentation helpers in `alnav/src/dashboard.rs`:
   responsive tier, right-aligned hotkey metadata, and cursor-aware recent-file
   window capped at nine visible rows; add cursor clamping for recent-list
   shrinkage.
2. [x] Add semantic Dashboard style accessors and any reusable branding constants
   in `alnav/src/theme.rs`; reuse existing source glyph constants.
3. [x] Rewrite `alnav/src/ui.rs::render_dashboard` as a borderless, centered Hyper
   composition:
   - five-line ASCII ALNAV header and subtitle
   - vertical Quick Actions with dim descriptions
   - Recent Files with range/total title
   - reserved Dashboard flash row
   - footer hints and package version
4. [x] Add display-width formatting for recent paths:
   - emphasized basename
   - dim home-relative parent
   - middle ellipsis under width pressure
   - selected recent item always visible when windowed
   - Unicode display-width alignment for marker, padding, and hotkey badges
5. [x] Preserve `dashboard::handle_key` and
   `main.rs::handle_dashboard_key` behavior unless a test-only extraction is
   necessary; after invalid recent removal, invoke the new cursor clamp helper.
6. [x] Extend `dashboard.rs` unit tests for windowing and presentation tiers.
7. [x] Add `ui.rs` `TestBackend` tests for:
   - normal Hyper composition
   - empty recent list
   - narrow/short fallback
   - long-path ellipsis
   - selected recent visibility
   - right-aligned hotkeys
   - empty recent placeholder
   - Dashboard flash visibility without layout shift
   - 0/1/9/20/200 recent rows and cursor clamp after invalid-entry removal
8. [x] Run focused tests, full TUI tests, and formatting checks.

## Precise Change Locations

- `alnav/src/dashboard.rs`
  - `DashboardItem` presentation metadata
  - `DashboardState` pure viewport/layout helpers
  - existing `tests` module
- `alnav/src/ui.rs`
  - replace the body of `render_dashboard`
  - replace or internalize `recent_hot` if hotkey metadata moves to the model
  - add Dashboard render helpers adjacent to `render_dashboard`
  - add `TestBackend` regressions in the existing `tests` module
- `alnav/src/theme.rs`
  - Dashboard semantic style functions near candidate/help presentation styles
  - reuse `GLYPH_SOURCE_*`; no duplicate icon constants
- `alnav/src/main.rs`
  - after invalid recent removal in `handle_dashboard_key`, clamp the Dashboard
    cursor; no source-binding behavior change
- `alnav/Cargo.toml` / `Cargo.lock`
  - add direct `unicode-segmentation` support for grapheme-safe truncation

## Validation Commands

```bash
cargo test -p alnav --bin alnav dashboard::
cargo test -p alnav --bin alnav ui::
cargo test -p alnav --bin alnav
cargo test --workspace
cargo check --workspace
cargo fmt -p alnav --check
git diff --check
```

## Risky Areas

- Ratatui layout arithmetic on frames smaller than the intended composition
- Unicode display-width truncation and Nerd Font alignment
- Windowing math when the cursor moves between Quick Actions and recent files
- Reading the existing timed flash without duplicating status-bar state
- Accidental behavior drift in existing one-key activation

## Rollback Points

- Presentation helpers are additive and can be removed independently.
- `render_dashboard` can be restored without touching source binding,
  persistence, or runtime source switching.

## Pre-start Gate

- PRD, design, and implementation plan reviewed by the user.
- Explicit implementation approval received after the final planning summary.
