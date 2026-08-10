# TUI Dashboard nvim-style refinement

## Goal

Refine the existing one-shot, no-source TUI Dashboard so it reads as a
purpose-built Neovim-style start screen inspired by
`nvimdev/dashboard-nvim`, while preserving the existing source-selection
behavior and fast keyboard workflow.

## Background

- The no-source Dashboard and runtime source switching are already implemented.
- The current Dashboard renders a full-frame rounded modal titled `alnav`.
- Its body is a plain top-aligned paragraph containing one instruction line,
  `Stream` and `Files` section labels, and a flat selectable list.
- Existing activation keys are `h`, `a`, `o`, and `1`-`9`; `j`/`k`,
  arrows, `g`/`G`, `Enter`, `q`, and Ctrl-C are already supported.
- `dashboard-nvim` exposes two established visual models:
  Hyper (header + shortcuts + projects/MRU + footer) and Doom
  (header + centered action list + footer).

## Requirements

### R1 — Preserve behavior

- Keep the Dashboard one-shot: it is visible only before the first source bind.
- Preserve all existing Dashboard navigation and activation keys.
- Preserve source ordering and actions: HDC, ADB, Open file, recent files.
- Do not change recent-file persistence, source binding, or runtime `of` / `os`
  behavior.

### R2 — Neovim-style start-screen composition

- Replace the generic modal/list appearance with a vertically composed start
  screen using a branded header, action/source area, recent-file area, and a
  compact footer/hint area.
- Keep the page borderless. Sections use semantic glyphs, titles, and whitespace
  only; do not add section boxes or horizontal rules.
- Use the dashboard-nvim **Hyper** composition: header, Quick Actions, Recent
  Files, then footer.
- Keep Quick Actions as three vertical rows so `j`/`k` retains a literal
  up/down navigation meaning.
- Full and Compact layouts render action titles plus dim descriptions:
  `HDC — HarmonyOS hilog`, `ADB — Android logcat`, and
  `Open file — Browse recent or local logs`. Minimal layout may hide
  descriptions.
- Use a compact fixed `ALNAV` ASCII logo plus the
  `App / Android Log Navigator` subtitle. On constrained terminals, collapse
  the logo to a single-line `alnav` wordmark before hiding actionable content.
- The full header uses a five-line, ASCII-only line-art logo approximately
  36-40 display columns wide.
- Keep the screen visually centered on normal terminal sizes.
- Cap the centered content column at 72 display columns; narrower terminals use
  the available frame width minus two columns of margin on each side.
- Center a fixed-height full presentation frame sized for nine recent rows so
  the header and Quick Actions do not shift as recent history grows. On
  constrained terminals, switch to top-aligned Compact/Minimal rendering with
  a one-row margin.
- Keep selected-row treatment consistent with theme tokens and the existing
  single-accent/dim visual language.
- Selected rows use the existing soft candidate background within the centered
  content column, retain the selection marker, and emphasize the item hotkey
  with the accent color.
- Use semantic Nerd Font glyph constants from `theme.rs`; do not inline glyphs
  or colors in `ui.rs`.
- Footer content is limited to concise navigation hints plus the current
  package version. Item-local `h`/`a`/`o`/number shortcuts are not duplicated
  in the footer.
- Item hotkeys render right-aligned as `[h]`, `[a]`, `[o]`, or `[1]`-`[9]`,
  following dashboard-nvim's description-plus-key structure.

### R3 — Responsive rendering

- The Dashboard must remain usable in narrow or short terminals.
- Decorative content may collapse before actionable rows or the selected row
  becomes inaccessible.
- Constrained layouts prioritize the three Quick Actions and the selected
  recent row. Degrade in this order: shrink whitespace, reduce recent capacity,
  hide subtitle/action descriptions, replace ASCII logo with the single-line
  wordmark, then hide footer/section titles if still necessary.
- Recent paths must not destroy alignment or overflow the available width.
- Marker width, padding, centering, hotkey alignment, and path truncation use
  terminal display width rather than byte or character counts.
- Recent rows emphasize the file name and render the parent directory in a dim
  style. Width pressure truncates the parent path before the file name.
- Parent directories under the user's home directory use `~`. Overlong parent
  paths use a middle ellipsis so both the root context and nearest directories
  remain identifiable.
- An empty recent list keeps the section visible and renders a dim
  `No recent files yet` placeholder.
- Show at most nine recent-file rows at once on normal terminals. When more
  history exists, `j`/`k` scrolls a cursor-aware window across the complete
  persisted list; `1`-`9` continue to activate the newest first nine entries.
- When the recent list exceeds its visible window, the section title shows the
  visible one-based range and total, for example `Recent Files  1-9 / 20`.
- If an invalid recent entry is removed while the Dashboard stays open, clamp
  the cursor to a valid remaining item before the next render.

### R4 — Dashboard feedback

- Reserve one stable message row above the footer.
- Render `App` flash text in that row so live-source connection failures and
  invalid recent-file errors are visible while the Dashboard remains active.
- Use existing semantic warning/soft styles; the message row must not move the
  surrounding layout when it appears or expires.

### R5 — Scope boundary

- This task changes Dashboard presentation and presentation-focused model
  helpers only.
- Keep Dashboard interaction keyboard-only; do not add mouse hit-testing,
  hover, click, or wheel behavior.
- It does not redesign the Open file or Stream source picker.
- It does not add new source types, persistence formats, or runtime actions.

## Acceptance Criteria

- [x] AC1: No-source startup displays the refined start screen; explicit
  `-f` / `--hdc` / `--adb` startup remains unchanged.
- [x] AC2: `h`, `a`, `o`, `1`-`9`, `j`/`k`, arrows, `g`/`G`, `Enter`, `q`,
  and Ctrl-C retain their current behavior.
- [x] AC3: A normal terminal renders the borderless Hyper composition: five-line
  ALNAV logo, vertical Quick Actions with descriptions, Recent Files, reserved
  flash row, and navigation/version footer in a centered 72-column frame.
- [x] AC4: Narrow/short terminal rendering keeps actionable content visible
  before decorative content, keeps the selected recent row in view, and does
  not panic; CJK/emoji paths and Nerd Font markers do not break alignment.
- [x] AC5: Recent Files shows at most nine rows, exposes the full history through
  cursor-aware scrolling, displays range/total when windowed, and formats each
  entry as emphasized basename plus dim home-relative parent with safe
  display-width truncation.
- [x] AC6: New render/model helpers have regression tests where practical;
  `cargo test -p alnav --bin alnav` and `cargo fmt -p alnav --check` pass.
- [x] AC7: New Dashboard colors/styles/glyphs are defined in `theme.rs`, with
  no raw `Color::*`, hard-coded `Style`, or inline Nerd Font literals added to
  `ui.rs`.
- [x] AC8: A Dashboard-time `App` flash is visible in a reserved message row
  without shifting the page layout.
- [x] AC9: Removing an invalid last recent entry leaves the Dashboard cursor on
  a valid visible item.

## Out of Scope

- Returning to Dashboard after a source is bound
- Source-picker redesign
- Device selection or new source backends
- Recent-file storage changes
- Configurable Dashboard themes in `config.toml`
