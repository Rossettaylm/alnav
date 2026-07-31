# Picker vocab candidate match — async + cancel

## Goal

Picker New 面板在词表候选（Highlight `all_candidates` / Filter·Exclude `tag|pkg|msg_candidates`）上粘贴或快速输入时，UI 不再因同步全量 fuzzy 而卡顿：匹配改为后台异步，下一键取消上一轮，主线程只更新草稿并渲染缓存结果。

## Background

- 现状：`picker_render_data` 每帧同步调用 `vocab.all_candidates` / `*_candidates`；msg 词表上限 10 万，Highlight 合并约 10.7 万条，每键一次 `fuzzy_score` 扫全表。
- Tab/Down 路径也会再扫一遍词表取 `len()` / 第 N 项。
- File 行过滤已有 gen+cancel 异步扫描；词表补全仍在 UI 线程（`fuzzy-matching.md` 曾 defer 的 corpus worker 的轻量版）。

## Requirements

1. **R1 — 异步匹配**：词表候选过滤在后台线程执行；主线程按键只改 draft/query，不阻塞在全量 fuzzy。
2. **R2 — 取消上一次**：新输入 / Backspace / 关面板时 bump generation + cancel；过期结果丢弃。
3. **R3 — 语义不变**：空 query → freq 降序；非空 → nucleo fuzzy score 降序再 freq（与现 `filter_sort` / `all_candidates` 一致，ignore-case，多词 AND）。
4. **R4 — 渲染读缓存**：`picker_render_data` 与 Tab/Down 只用已完成的缓存 labels；匹配中采用 stale-while-revalidate（保留上一帧列表）。
5. **R5 — 范围**：仅 Picker New 词表补全（Highlight / Filter·Exclude 字段词表）。Manage 小列表、MsgChip、日志行 Filter/Highlight 扫描、完整 `nucleo` FuzzyIndex **不做**。

## Acceptance Criteria

- [x] 粘贴/连打时草稿即时更新，主线程不因词表全量 fuzzy 明显卡顿。
- [x] 快速连续改 query 时，只应用最后一次匹配结果（中间 gen 被取消/丢弃）。
- [x] 同一 query 下候选顺序与现同步 `tag_candidates` / `all_candidates` 一致（单测覆盖）。
- [x] 关闭 Picker 取消进行中的 job，无悬挂线程写回已关闭会话。
- [x] `cargo test -p alnav` 与 `cargo fmt -p alnav --check` 通过。

## Out of scope

- Manage / ActionList / Bookmark / MsgChip 列表过滤异步化
- 日志行级 FilterBatch / Highlight Inc 改动
- 引入 high-level `nucleo` FuzzyIndex / 持久化词表索引

## Notes

- UX（已确认）：匹配进行中显示上一帧候选，不强制清空或 loading 文案。
- 复用 File 扫描的 gen + `AtomicBool` cancel + mpsc 模式（见 `async-scans.md`）。
