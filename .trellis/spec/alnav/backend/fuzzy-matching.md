# TUI Fuzzy Matching (nucleo-matcher)

> Executable contracts for interactive text matching (task `07-29-tui-nucleo-fuzzy`).

---

## Scenario: TUI text match via `fuzzy.rs`

### 1. Scope / Trigger

- Trigger: all TUI keystroke → narrow-set paths (Picker Manage/New, MsgChip, Time dates, vocab completion, field/level candidates, Search/Highlight, Filter/Exclude text chips, candidate-list match paint).
- Cross-module: `fuzzy.rs`, `vocab`, `filter_model`, `highlight_model`, `input`, `picker`, `time_panel`, `ui` paint + candidate spans, `scan`/`store` File preds, `export`/`yc` flash.
- **Out of scope**: CLI `alnav grep` / `alnav-core` `FilterChain` / `-e` regex (unchanged).

### 2. Signatures

```rust
// fuzzy.rs
pub const TAG_MSG_SEP: char; // '\t'

/// Batch standard: one query × many haystacks (reuse Pattern + Matcher + Utf32 buf).
pub struct FuzzyScorer;
impl FuzzyScorer {
    pub fn new(query: &str) -> Self;
    pub fn score(&mut self, haystack: &str) -> Option<u32>;
    pub fn char_indices(&mut self, haystack: &str) -> Vec<u32>;
}

pub fn fuzzy_score(haystack: &str, pattern: &str) -> Option<u32>; // thin one-shot wrapper
pub fn fuzzy_match(haystack: &str, pattern: &str) -> bool;
pub fn fuzzy_char_indices(haystack: &str, pattern: &str) -> Vec<u32>; // thin one-shot wrapper
pub fn search_haystack(tag: &str, msg: &str, raw: &str) -> String;
pub fn map_search_positions(tag: &str, msg: &str, raw: &str, pattern: &str) -> Vec<FieldSpan>;
pub fn matches_search_row(row: &EntryRow, pattern: &str) -> bool;
pub fn chip_matches_row(chip: &Chip, row: &EntryRow) -> bool;
pub fn chips_match_row(chips: &[Chip], row: &EntryRow, op: SameFieldOp) -> bool;
pub fn fuzzy_label_indices(labels: &[String], query: &str) -> Vec<usize>;
pub fn fuzzy_str_labels(labels: &[&str], query: &str) -> Vec<String>;

pub enum SameFieldOp { And, Or } // interactive And; startup CLI multi-value Or

// vocab.rs — New-panel completion
pub enum CandidateScope { All, Tag, Pkg, Msg }
fn filter_sort_entries(entries, query, cancel, check_every) -> Vec<String>; // uses FuzzyScorer
fn filter_all_entries(entries, query, cancel, check_every) -> Vec<String>; // dedupe + FuzzyScorer

// candidate_match.rs — Picker New UI path
CandidateMatchService::request / poll / clear  // gen-cancel async
```

### 3. Contracts

| Topic | Rule |
|-------|------|
| Engine | TUI **candidate** text = **`nucleo-matcher` `Pattern::parse`** (ignore-case, Smart normalization). Whitespace splits atoms **AND**; per-atom fzf syntax `'…` / `^…` / `…$` / `^…$` / `!…` (bare atoms stay Fuzzy). Never match user queries via a single `Atom` (literal spaces). No fzf process. No TUI regex escape hatch. Log-row Filter/Search/Highlight stay **substring** (`substr_match`), not `Pattern::parse`. |
| Case | Always ignore-case. No `config.toml` matcher keys. |
| Search/Highlight haystack | `tag + '\t' + msg`; if both empty → `raw`. |
| Search/Highlight match (LogList) | **Contiguous ignore-case substring** (`substr_match`), whitespace atoms AND. **Not** fuzzy — avoids `guild` matching scattered `gu`…`i`…`ld`. |
| Search/Highlight paint | `substr_byte_ranges` → `map_search_positions`; not `fuzzy_char_indices`. |
| Filter/Exclude text chip | **Contiguous substring on that field only** (`substr_match`); empty field → chip does **not** match (no raw spoof). Same rationale as Search — hex/codes like `0x1100` must not stitch from scattered digits. |
| pid / tid | Exact string equality. |
| level (row match) | Minimum level (`Level::from_str` + `>=`), same idea as CLI LevelGte. |
| level (New candidates) | Fuzzy over `V/D/I/W/E/F` via `fuzzy_str_labels`. |
| Vocab New completion | `tag`/`pkg`/`msg`/`all_candidates` use Pattern fuzzy; empty query → freq desc **top-N**; else score desc then freq **top-N**. **Picker New UI** runs matching via `candidate_match` (async + gen-cancel + Arc snapshot reuse); sync `Vocab::candidates` remains for tests / empty-query fast path. |
| Batch fuzzy score | **One query × many haystacks** MUST use `FuzzyScorer` (reuse `Pattern` + `Matcher` + Utf32 buf). `fuzzy_score` / `fuzzy_char_indices` are one-shot wrappers only. `filter_sort_entries` / `filter_all_entries` / `fuzzy_label_indices` go through `FuzzyScorer`. Do **not** rebuild Pattern/Matcher per vocab entry. |
| Field keyword candidates | `InputBox::field_candidates` fuzzy on keywords (`tag`/`msg`/…). |
| Highlight history candidates | `HighlightBox::candidate_indices` fuzzy on patterns; cap 6; score-ordered. |
| Group compose | Chips AND (interactive) / same-field OR at startup; groups OR; excludes AND NOT; then lock → time_bound → view_focus. |
| Candidate lists | `fuzzy_label_indices` / vocab filter / `contains_indices`: empty → stable/freq order **truncated to `CANDIDATE_RESULT_CAP` (256)**; else score-sorted then truncate. |
| Paint (log) | Substring ranges mapped to `FieldSpan` on tag/msg (or Raw); `ui` must not use fuzzy gaps or `Regex::find_iter`. |
| Paint (candidate list) | `candidate_label_spans` uses `fuzzy_char_indices` ranges — not substring `contains`. **ViewportPaint**: `render_candidate_list` only builds items for the visible window (`candidate_viewport_range`). |

### Candidate panel SLOs (first-class)

| SLO | Rule |
|-----|------|
| ResultCap | UI candidate results ≤ `fuzzy::CANDIDATE_RESULT_CAP` (**256**) at every narrowing exit |
| ViewportPaint | No full-result `ListItem` + `fuzzy_char_indices` over the whole labels vec |
| UiThreadBudget | 100k Msg vocab + short query: candidate UI work (request-side snapshot reuse + assemble + viewport paint) &lt; **8ms** release |
| LargeMatchAsync | Msg/All (large vocab) narrowing via gen-cancel worker; UI reads stale-while-revalidate cache |
| EmptyQuery | Empty query shows **top-N**, not the full table |

Applies to: Picker New/Edit/Manage/Unified/MsgChip/Bookmark/Preset, field/level/history popups, Time date candidates, all `render_candidate_list` callers.
| File progressive | MVP: existing File **FilterBatch / Highlight Inc** scans apply substring/chip predicates per row; status may show `idx a/b`. Picker New vocab completion uses a light **async gen-cancel** worker (`candidate_match.rs`); a full high-level `nucleo` corpus worker remains optional future work. |
| Stream | Evaluate against current `rows`/`matched`; eviction drops reachability (no independent fuzzy corpus → no ghost hits). |
| Startup CLI → TUI | Initial group chips use same substring match + `SameFieldOp::Or` for multi-values. |
| `yc` | Still emits literal `alnav grep` approx (encoding / ring truncation); flash notes `approx`. |
| Group model | No `Group.expr` / `ExcludeEntry.expr` for TUI; chips are source of truth. |
| Fuzzy vs substring split | **Candidate Pattern::parse** = UI candidate narrowing (Picker, vocab, field/level/date/history, Open-file lists) — Fuzzy by default + `'^$!` syntax. **Substring** (`substr_match`) = matching against log row content (Filter/Exclude/Search/Highlight). |

### 4. Validation & Error Matrix

| Condition | Behavior |
|-----------|----------|
| Empty Search/Highlight pattern | No match / no paint (not “match all”) |
| Empty Picker query | Show top-N (`CANDIDATE_RESULT_CAP`) by freq / stable order |
| Empty Filter chip field on row | That chip fails |
| Unparsed row (empty tag+msg) | Search/Highlight uses `raw` |
| `yc` success | Flash contains `approx` (e.g. `YANKED (approx)`) |

### 5. Good / Base / Bad

- **Good**: Picker `abr` fuzzy-hits `aXbYr`; Filter `msg:0x1100` does **not** hit lines that only have scattered `0`/`x`/`1`; File filter Subset still line indices only.
- **Base**: Contiguous Filter/Search queries feel like CLI substring/`-i`.
- **Bad**: Reintroduce fuzzy gaps for Filter/Exclude/Search/Highlight on log rows; `HighlightGroup.re: Regex` for TUI paint; compile Filter chips back to `Expr` for matching; spawn fzf `--listen`.

### 6. Tests Required

- `fuzzy_match` non-contiguous (`aXbYc` / `abc`) — candidate/list path only
- Multi-word: `guild viewmodel` ⊨ `GuildFeedListViewModel` (fuzzy + vocab + highlight candidates)
- `fuzzy_label_indices` empty + ignore-case
- Field-scoped chip: tag chip does not match msg-only haystack
- Filter chip hex contiguous (`0x1100` vs scattered digits)
- Highlight metacharacters treated as literals under substring (no regex specials)
- `yc` flash approx assertion (clipboard may fail in CI)

### 7. Wrong vs Correct

| Wrong | Correct |
|-------|---------|
| TUI Filter/Exclude uses fuzzy gaps on log fields | `substr_match` / `chip_matches_row` contiguous |
| TUI Highlight uses fuzzy gaps on LogList | `substr_match` / `map_search_positions` contiguous |
| TUI Highlight uses `Regex::new` + `find_iter` | `fuzzy::map_search_positions` + theme highlight styles |
| `Group.expr` drives `matches` | `chips_match_row` / `chip_matches_row` |
| `Atom::new("guild viewmodel", …)` for user query | `Pattern::new(...)` so space → AND atoms |
| `vocab` / New panel `contains` / `starts_with` | `FuzzyScorer` / `fuzzy_label_indices` |
| Loop `fuzzy_score` over large vocab (rebuild Pattern each row) | `FuzzyScorer::new(query)` once, then `score` per haystack |
| Candidate list substring paint | `fuzzy_char_indices` multi-range paint |
| File materialises all `EntryRow` into a nucleo corpus for MVP | Per-row chip/substring preds inside existing bg Filter/Highlight scans + `Visible::Subset` |
| `yc` claims exact CLI parity | Approx export + flash `approx` |

### Design decision (MVP)

A dedicated `FuzzyIndex` backed by the high-level `nucleo` crate (async inject/tick) was considered and **deferred** for log-row corpus matching. Picker New vocab completion now uses a lighter **snapshot + gen-cancel** worker in `candidate_match.rs` without changing match text / field contracts above.
