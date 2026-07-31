# Design: Picker vocab async candidate match

## Architecture

```
  Keystroke (draft/query change)
           │
           ▼
  App.candidate_match.request(scope, query)
           │  gen++, cancel prev, snapshot (key,freq)[]
           ▼
  background thread ──fuzzy_score / sort──► mpsc Result{gen, labels}
           │
           ▼
  main loop drain → if gen==current → cache.labels
           │
           ▼
  picker_render_data / Tab·Down  read cache (no sync vocab scan)
```

## New module

`alnav/src/candidate_match.rs`

| Type | Role |
|------|------|
| `CandidateScope` | `All` \| `Tag` \| `Pkg` \| `Msg` |
| `CandidateMatchRequest` | scope + query + gen |
| `CandidateMatchResult` | gen + `Vec<String>` labels |
| `CandidateMatchCache` | last applied query/scope/labels；optional `pending: bool` |
| `CandidateMatchService` | gen, cancel flag, rx; `request()`, `poll()`, `cancel()` |

## Snapshot + worker

1. `request`: from `Vocab` clone `(String, u32)` entries for the scope (All = tag∪pkg∪msg with same dedup as `all_candidates`).
2. Spawn thread with `Arc<AtomicBool> cancel`, owned snapshot, query, gen.
3. Reuse scoring/sort from `vocab` (extract `filter_sort_entries` / `sort_scored` for owned slices; cancel check every ~4096 entries).
4. On finish, `try_send` result; if cancel, drop silently.

## App / main wiring

- `App` holds `CandidateMatchService` + cache.
- On Highlight draft / Filter field draft change (and open New with non-empty draft): `request`.
- Event loop: after ingest/FileEvent drain, `poll` match results → update cache if gen matches.
- `picker_render_data`: for vocab paths, use cache labels when scope+query match; if pending and cache query differs, still show cache (stale); if never matched, show empty until first result.
- Tab / Down: use cache only — **never** call `vocab.*_candidates` on UI thread for these paths.
- `close_picker`: `cancel()` + clear cache.

## Empty query

- Empty query: may complete synchronously from snapshot (freq sort, cheap relative to fuzzy) **or** still async; prefer sync path for empty to avoid flicker on field pick with empty draft.

## Concurrency vs vocab.feed

- Snapshot at request time; concurrent `feed` does not affect in-flight job (acceptable: next keystroke refreshes).

## Spec impact

- Update `.trellis/spec/alnav/backend/fuzzy-matching.md`: Vocab New completion may be async with gen-cancel; match text contracts unchanged.
- Update `directory-structure.md` to list `candidate_match.rs`.

## Non-goals

- Persist nucleo corpus across sessions
- Async Manage / MsgChip filtering
- Change Preview sampling
