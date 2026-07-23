# Implement: picker search mid-cursor editing

## Checklist

1. Add `TextField` (module: prefer `aloggrep-tui/src/text_field.rs` or extend `input.rs`) with unit tests for insert/move/backspace/home/end/kill_to_start/kill_word_back (Unicode char boundaries).
2. Replace `InputBox.draft` / `HighlightBox.draft` / `PickerSession.query`+`draft` string mutation with `TextField` (or equivalent cursor API); update call sites (`push_char` → insert at cursor, etc.).
3. Wire Manage keys in [`main.rs`](aloggrep-tui/src/main.rs): `Ctrl-X` edit; `Delete` + Ctrl-Backspace → delete confirm; remove Ctrl-E/Ctrl-D actions; route edit keys into query `TextField`.
4. Wire New/Edit / MsgChip / legacy modal handlers for the edit keymap; New/Edit Ctrl-Backspace → kill_word_back; Delete noop for item/forward delete.
5. UI: mid-caret + follow-cursor window in picker search line + input/highlight draft renders ([`ui.rs`](aloggrep-tui/src/ui.rs)).
6. Update [`help.rs`](aloggrep-tui/src/help.rs) `L1_PICKER`; adjust CLAUDE.md YAGNI / key notes if they still claim append-only or Ctrl-E/D.
7. Tests: TextField unit tests; Manage remap tests (extend existing Ctrl-E/Ctrl-D picker tests); at least one mid-cursor Backspace / Ctrl-U case on InputBox or picker draft.

## Validation

```bash
cargo test -p aloggrep-tui text_field::
cargo test -p aloggrep-tui input::
cargo test -p aloggrep-tui -- picker
cargo test -p aloggrep-tui
```

Manual (TTY): open Filter Manage → type long query → move left → edit middle; Ctrl-X edit; Delete / Ctrl-Backspace delete with confirm; New draft Ctrl-Backspace kills word.

## Risky files

- [`aloggrep-tui/src/main.rs`](aloggrep-tui/src/main.rs) — large key dispatch; easy to break Manage vs New routing
- [`aloggrep-tui/src/ui.rs`](aloggrep-tui/src/ui.rs) — width/window off-by-one on CJK
- Existing tests that assume `draft.pop()` / Ctrl-E/D

## Before `task.py start`

- [ ] User approved final planning summary
- [ ] `prd.md` / `design.md` / `implement.md` present
- [ ] Curate `implement.jsonl` / `check.jsonl` if dispatching sub-agents
