# Picker search polish

## Goal

Polish the Picker search area with a theme-consistent rounded input border and
balanced spacing while preserving mode semantics, chip display, and hardware
cursor behavior.

## Requirements

- Render only the Picker input line inside a rounded border.
- Keep committed chips above, outside the input border.
- Preserve the mode-specific Manage/New/Edit Nerd Font icon.
- Use one cell of horizontal padding between the border and content, and one
  cell between the mode icon and following content.
- Reserve three rows when no chips are present and four rows when chips are
  present, reducing the candidate area dynamically.
- Reuse existing theme styles; do not hardcode colors or add theme settings.
- Do not change filtering, Picker state, key handling, candidates, Preview, or
  standalone Search/Highlight modals.

## Acceptance Criteria

- [x] The Picker input is enclosed by a rounded border using the active modal
      border style.
- [x] The input content has one-cell left/right padding and one-cell spacing
      after its mode icon.
- [x] Chips remain visible above and outside the input border.
- [x] Candidate and search regions do not overlap with or without chips.
- [x] Manage/New/Edit icons and hardware cursor positions remain correct,
      including narrow areas and horizontally windowed text.
- [x] Picker UI tests, all TUI tests, and workspace tests pass.

## Notes

- Confirmed decisions: input-only border, mode-specific icon, compact one-cell
  spacing, and dynamic three/four-row search area.
- This is a lightweight, single-file rendering change; PRD-only planning is
  sufficient.
