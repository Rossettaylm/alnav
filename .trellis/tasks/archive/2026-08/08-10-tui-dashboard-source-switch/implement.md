# Implement: Dashboard + Source Switch

## Checklist

1. [x] `recent.rs` + `config.recent_files_limit` + load/save tests
2. [x] `path_complete.rs` + unit tests (`~`, dir trailing `/`, prefix filter)
3. [x] Relax `validate_source`; wire deferred filters into App before bind
4. [x] Dashboard render + key dispatch (`h/a/o/1-9/jk/Enter/gG/q`)
5. [x] Bind helpers: open file / start hdc / start adb (shared by Dashboard + switch)
6. [x] `reset_for_source_switch` + live child teardown / FileStore swap
7. [x] File SourcePicker (`of` + Dashboard Open file) with async 10-line preview
8. [x] Centered Stream SourcePicker (`os`)
9. [x] Operator `o` pending in Normal+LogList; Help (+ hardcoded `o` pending keymap follow-up)
10. [x] Integration/unit tests for validate_source, recent cap, reset keeps F/E/H
11. [x] `cargo test -p alnav --bin alnav` (520 ok); fmt optional

## Validation

```bash
cargo test -p alnav --bin alnav
cargo fmt -p alnav --check
```

## Risky files

- `main.rs` event loop / live session ownership
- `app.rs` store swap + filter rebuild
- `picker.rs` new Kind vs overloading Filter picker

## Rollback

Feature is additive behind new boot path; revert task commits if bind/teardown races appear.
