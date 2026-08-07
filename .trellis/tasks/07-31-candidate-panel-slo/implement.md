# Implement: 候选面板检索一级指标

## Checklist

1. [x] `CANDIDATE_RESULT_CAP` + truncate in `vocab` / `fuzzy_label_indices` / picker / time
2. [x] `render_candidate_list` viewport-only paint
3. [x] `CandidateMatchService` Arc snapshot reuse + bounded empty query
4. [x] Reduce `display_labels().to_vec()` churn where safe (labels already ≤256)
5. [x] Preview throttle for Filter draft
6. [x] Spec updates (fuzzy-matching, index, quality-guidelines)
7. [x] Tests + release budget guard

## Validation

```bash
cargo test -p alnav --bin alnav
cargo fmt -p alnav --check
```

## Review gates

- ResultCap enforced at match exits, not only paint
- ViewportPaint still correct with selection scrolling
- Spec EmptyQuery wording no longer says "show all"

## Rollback

Revert the feature commit(s) on `alnav/` + `.trellis/spec/alnav/backend/`.
