# Design: 候选面板检索一级指标

## Boundaries

- **In**: `fuzzy`/`vocab`/`candidate_match`/`picker`/`time_panel`/`ui::render_candidate_list`/`main` picker render、Preview 节流、Trellis spec。
- **Out**: File/Stream 全量 log 行异步扫描、CLI grep。

## Contracts

| Name | Rule |
|------|------|
| `CANDIDATE_RESULT_CAP` | `256`，定义在 `fuzzy.rs`（或薄 re-export），所有 narrowing 出口 truncate |
| Empty query | top-N by freq（vocab）或稳定原序（indices） |
| Paint | viewport (+ small overscan) only |
| Snapshot | `Arc<[(String,u32)]>` per scope，query 变 scope 不变时复用 |

## Data flow

```
keystroke → CandidateMatchService::request (reuse Arc snap)
         → worker filter_* + truncate(256)
         → cache labels
         → render_candidate_list viewport paint
```

## Compatibility

- Highlight history 仍 cap 6。
- Score/freq 排序语义不变，仅截断尾部。
- j/k / Enter 只在截断后的列表内导航（可接受）。

## Rollout / rollback

- 纯 TUI 行为；回滚即还原 ResultCap/ViewportPaint/Arc snap 提交。
