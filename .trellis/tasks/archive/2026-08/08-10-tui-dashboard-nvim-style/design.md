# Design: Hyper-style TUI Dashboard

## Architecture and Boundaries

This is a presentation-only refinement of the existing unbound-source
Dashboard.

```text
DashboardState (items, cursor, recent)
        │
        ├─ layout helpers: responsive tier + visible recent window
        │
        └─ ui::render_dashboard
              ├─ Header: ALNAV logo + subtitle
              ├─ Quick Actions: HDC / ADB / Open file
              ├─ Recent Files: selected-window slice
              └─ Footer: navigation hints
```

Source binding remains owned by `main.rs::handle_dashboard_key`; this task does
not change `DashboardAction`, recent-file persistence, or source panel behavior.

## Target Composition

Use the `dashboard-nvim` Hyper visual hierarchy:

1. Borderless, full-frame start page rather than a full-screen popup shell.
2. Horizontally centered content column capped at 72 display columns.
3. Five-line ASCII-only `ALNAV` logo and dim product subtitle.
4. `Quick Actions` section containing three vertical HDC, ADB, and Open file
   rows with title + dim description.
5. `Recent Files` section containing newest-first paths, at most nine visible.
6. A reserved one-line flash area for source/open failures.
7. Compact footer containing navigation hints and package version.

The Dashboard remains one flat keyboard selection model. Visual sections do not
introduce additional focus state.

Item hotkeys render as right-aligned bracketed badges. Selected rows use the
existing soft candidate background within the content column, plus the existing
selection marker and an accent hotkey.

## Responsive Layout

Rendering chooses a tier from the available width and height:

- **Full**: ASCII logo, subtitle, section gaps, footer, and up to nine recent
  rows.
- **Compact**: single-line `alnav` header, reduced gaps, sections, footer.
- **Minimal**: single-line header and actionable rows; decorative subtitle and
  footer may be omitted.

The Full frame is sized from the terminal's maximum visible recent capacity so
the header and Quick Actions remain vertically stable as history grows. It is
vertically centered when it fits. Constrained tiers use a one-row top margin.

Degradation is usability-first: shrink whitespace, reduce recent capacity, hide
subtitle/action descriptions, collapse the logo to a single line, then hide
footer/section titles if required. Action rows and the selected row always win
over decoration.

Recent rows use a cursor-aware window across the full persisted history. If
windowed, the section title shows the one-based range and total. Long path
components are display-width truncated by reusing `unicode-width`. Centering,
padding, selection-marker width, and right-aligned key badges use the same
display-width calculation.

## Model Helpers

`dashboard.rs` continues to own item ordering and navigation. Add pure helpers
only where they make rendering deterministic and testable, for example:

- responsive presentation tier
- cursor-aware recent-file range
- item hotkey text

Do not move source binding or terminal rendering into the model.

## Theme Contract

All Dashboard-specific semantic styling is exposed by `theme.rs`, reusing
existing `UiTokens`:

- header/logo: accent, optionally bold
- subtitle/footer/empty state: muted or dim
- section title: accent + dim/bold according to existing visual language
- selected row: existing soft candidate selection treatment
- hotkey: accent emphasis

Existing source glyph constants remain the icon source. `ui.rs` must not add
raw `Color::*`, hard-coded `Style`, or inline Nerd Font glyphs.

## Empty and Constrained States

- No recent files: render a non-selectable dim `No recent files yet` row; Open
  file remains available under Quick Actions.
- Recent file rows emphasize the basename and render a home-relative (`~`) dim
  parent path. Truncate the parent first with a middle ellipsis.
- More recent files than fit: render only the cursor-aware window and show
  `start-end / total` in the section title.
- Dashboard-time `App` flash text renders in a reserved row above the footer;
  appearance/expiry must not shift the composition.
- When opening an invalid recent entry removes it from the list, clamp
  `DashboardState.cursor` before the next render so the selection cannot point
  past the new list end.
- Extremely small frame: avoid panics and render the maximum useful prefix of
  the minimal composition.

## Compatibility

- Existing Dashboard key handling and source actions are unchanged.
- Explicit-source startup still bypasses Dashboard.
- Open file and Stream source pickers are unchanged.
- No config, persistence, or migration changes.
- No mouse routing or hit-testing is added.

## Validation Strategy

- Model tests for unchanged item/action behavior and recent-window boundaries.
- Model tests cover 0, 1, 9, 20, and configured-maximum recent counts plus
  cursor clamping after removal.
- Ratatui `TestBackend` render tests at normal, narrow, short, and empty-recent
  sizes.
- Assertions cover visible section labels, compact fallback, selected recent
  visibility, home-relative/middle-ellipsis paths, right-aligned hotkeys, and
  Dashboard flash visibility.
- Run `cargo test -p alnav --bin alnav` and `cargo fmt -p alnav --check`.

## Risks and Rollback

- Nerd Font and block-character widths vary by terminal; keep structural
  alignment based on Unicode display width and retain a plain single-line
  fallback.
- Over-decorating can hide actions on a 24-row terminal; responsive tiers must
  reserve action rows first.
- The change is isolated to Dashboard model/render/theme code and can be
  reverted without affecting source switching.
