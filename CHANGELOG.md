# Changelog

## 0.2.4 — 2026-08-18

- Two-level Help (`?`): Home plus seven zone pages, design contracts, and ignore-case `/` search.
- Command palette on `C-p` from LogList and Filter / Exclude / Highlight strips.
- Severe (E/F/crash) tag and message use theme error red; Open File / Stream on `C-f` / `C-g`.
- Status bar reweighted toward resident follow and device state; remaining chrome copy in English.
- Live disconnect indicator is a single-cell plug.
- Pushing a `v*` tag publishes `alnav-core` then `alnav` to crates.io and opens a GitHub Release.
- loggrep-analyzer skill retargeted at `alnav grep`.

## 0.2.3 — 2026-08-13

- Dual-language README: English default (`README.md`) with a title link to [中文](README.zh.md).
- Side-by-side Dashboard and Log list screenshots.
- Shorter homepage: dropped architecture internals, exit-code table, and the bundled skill unzip block.

## 0.2.2 — 2026-08-13

- Nine builtin TUI palettes via `config.toml` `theme` (`default`, OneDark, Dracula, Everforest, Tokyo Night, Catppuccin Mocha, Gruvbox dark, Nord, Kanagawa Wave).
- Per-theme signature accent and Dashboard Unicode wordmark ramp.
- Optional `theme.toml` overlay (`[palette]` then semantic tokens). Templates: [`alnav/examples/config.toml`](alnav/examples/config.toml), [`alnav/examples/theme.toml`](alnav/examples/theme.toml).
- CLI (`alnav grep`) colors unchanged (`alnav-core::logcolor`).
