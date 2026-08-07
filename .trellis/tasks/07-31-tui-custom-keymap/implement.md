# Implement: TUI customizable keymap

## Order

### 1. Core keymap types + DSL

- [x] Add `alnav/src/keymap.rs` with `KeyStroke`, `Binding`, parse/format (`C-`/`S-`/`M-`, specials).
- [x] Reject bare `"J"` for Shift+j; accept `"S-j"`.
- [x] Unit tests: parse round-trip, invalid DSL.

### 2. Action registry (defaults = current behavior)

- [x] Define `ActionId`, `KeyContext`, `ActionMeta` (prefix/leaf, labels, capabilities).
- [x] Register **all** current TUI keyboard actions with defaults matching today’s `main.rs` / help tables.
- [x] Golden / table test: default bindings snapshot for critical contexts (LogList, Leader, ChipField, Yank, Lock, Time, Picker, strips).

### 3. Merge + KeymapStore

- [x] Load TOML → deep-merge per action; unbind `null`/`""`.
- [x] Layered errors: parse/type → Fallback; unknown → warn+skip; steal/prefix illegal → Fallback.
- [x] `KeymapStore` lookup + `display()` for Help/status.
- [x] Chord prefix tree / matcher unit tests (Space / Space Space; c + field).

### 4. Config integration + startup

- [x] `config.rs` (or keymap load helper): read `keymap.toml`, return store + status/warnings.
- [x] Wire into TUI startup next to `load_config` / theme; keep on `App`.
- [x] Capability filter for live sources.

### 5. Dispatch refactor

- [x] Introduce dispatch path in `main.rs` using store + context + pending.
- [x] Keep hard-reserve `C-c`; draft printable bypass.
- [x] Migrate focus areas incrementally if needed, but land with full parity.
- [x] Existing app/unit tests still pass; add dispatch tests for rebound key.

### 6. Help + status

- [x] Rebuild L1/L2/Help from registry labels + store key strings.
- [x] Aggregation for paired move keys where registry declares groups.
- [x] Live catalog omits file-only actions.
- [x] Update help unit tests that assert key literals.

### 7. `--init` / `--force`

- [x] Clap flags; write missing `config.toml` + `keymap.toml`; `--force` overwrite; no `theme.toml`.
- [x] English comments; exit without TUI.
- [x] Test serialize output contains expected sections/actions; init skip/overwrite behavior with temp dir.

### 8. Docs / examples (minimal)

- [x] Registry serialize is authoritative (`--init`); status-help spec updated.
- [x] `--help` mentions config dir includes `keymap.toml`.

## Validation

```bash
cargo test -p alnav
cargo test --workspace
cargo run -p alnav --bin alnav -- --init --config-path /tmp/alnav-init-test
```

## Review gates

- No raw key literals left as the only source of truth in help tables for registered actions.
- No `Color::*` regressions in ui (unchanged rule).
- Do not expand into mouse keymap or hot-reload.
