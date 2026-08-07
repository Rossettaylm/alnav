# Implement: TUI filter presets

## Ordered checklist

1. **Core IO + model** (`preset.rs`)
   - TOML schema, validate_name, capture/apply, list/save/delete/rename
   - Unit tests: round-trip, empty capture, bad TOML skip, name rules

2. **App integration** (`app.rs`)
   - Methods: begin save/open, confirm save/rename/delete, apply preset
   - Cursor retention after replace + refilter

3. **Picker / keys** (`picker.rs`, `main.rs`)
   - `PickerKind::Preset` Manage-only (disable auto-New)
   - Leader `w` / `o`
   - Ctrl-X / Delete paths + Confirm overwrite/delete

4. **UI** (`ui.rs`)
   - Save/rename name dialog (no candidates/preview)
   - Preset Preview: Filter → Exclude → Highlight chip-strip style

5. **Help** (`help.rs`)
   - Leader L2 + Help catalog entries for `Space w` / `Space o`

6. **Validate**
   - `cargo test -p alnav`
   - `cargo fmt -p alnav --check`

## Validation commands

```bash
cargo test -p alnav
cargo fmt -p alnav --check
```

## Review gates

- Apply does not clear time/lock/bookmarks/search
- Save skips `di` groups
- Open with empty dir does not open panel
- Bad files skipped + flash

## Rollback

Revert new `preset.rs` and call sites; no on-disk migration beyond optional
user `presets/` files left in place.
