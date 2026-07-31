# Implement: Picker vocab async candidate match

## Order

1. **Extract** owned-entry filter helpers in `vocab.rs` (`filter_sort_entries`, keep public sync APIs as thin wrappers for tests).
2. **Add** `candidate_match.rs` + wire `mod` in `main.rs`.
3. **App**: hold service + cache; `request_candidate_match` / `drain_candidate_match` / cancel on close.
4. **main**: on draft change in Highlight / Filter·Exclude field paths → request; event loop poll; `picker_render_data` + Tab/Down use cache.
5. **Tests**: gen discard, last-wins under cancel, semantic parity with sync `tag_candidates`/`all_candidates`.
6. **Spec**: note async vocab completion in `fuzzy-matching.md` + directory structure.

## Validation

```bash
cargo test -p alnav
cargo fmt -p alnav --check
```

## Risks

| Risk | Mitigation |
|------|------------|
| Snapshot clone cost on every key | Acceptable vs full fuzzy on UI; cancel drops wasted work |
| Stale list briefly wrong | Confirmed UX; Enter still submits draft text, not necessarily selected stale row |
| Tab before first result | No-op if cache empty / query mismatch |
