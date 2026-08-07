# PRD: 候选面板检索一级指标

## Goal

把候选面板的性能与结果有界性提升为检索**一级指标**（与 match 语义同级），消除 Filter/Highlight New 等路径在大词表 + 快打字下的 UI 卡顿。

## Requirements

1. **ResultCap**：所有候选面板 UI 可见结果 ≤ 256（`CANDIDATE_RESULT_CAP`）。
2. **ViewportPaint**：候选绘制只对当前 List 视口行做 `fuzzy_char_indices` / `ListItem`。
3. **UiThreadBudget**：100k Msg 词表 + 短 query 下，候选相关 UI 工作（请求侧 snapshot + 组装 + paint）release 基准 &lt; 8ms。
4. **LargeMatchAsync**：Msg/All（及大词表）narrowing 走 gen-cancel worker；UI 读 stale-while-revalidate 缓存。
5. **EmptyQuery**：空 query 展示 top-N（freq / 稳定序），不再展示全表。

## Coverage（全部遵守）

- Picker New/Edit：Filter/Exclude vocab、Highlight All、Level
- Picker Manage / Unified / MsgChip / Bookmark / Preset（`filtered_indices`）
- 独立浮层：field 关键字、Highlight history、一切 `render_candidate_list` 调用方
- Time 日期候选

## Acceptance

- [x] 100k Msg + 短/空 query → `display_labels().len() ≤ 256`
- [x] `filtered_indices` / time dates 截断到 ≤ 256
- [x] `render_candidate_list` 不对全结果集建 ListItem + fuzzy paint
- [x] Vocab request 同 scope 复用 Arc snapshot，避免每键满表克隆
- [x] Filter draft Preview 有节流，不与候选同帧无界叠峰
- [x] Spec（fuzzy-matching / index checklist / quality-guidelines）写入 Candidate panel SLOs
- [x] 相关单测 / release 预算守卫通过

## Out of scope

- LogList 行匹配仍为 substring
- 不上完整 nucleo FuzzyIndex 语料库
- Stream 全量 Filter 异步
