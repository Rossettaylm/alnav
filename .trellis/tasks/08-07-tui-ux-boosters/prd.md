# TUI 体验增强四件套

## Goal

在不新增大型子系统的前提下，补齐四个 TUI 体验缺口——三个是"CLI 已有能力、TUI 没做等价视图"（崩溃详情、统计面板），一个是"状态完全没有 UI 反馈"（断线），一个是"日志区显示密度没有可调项"（单行折叠）。四者互相独立，可分别排期、实现、验收。

## Background

- 探索来源：对 `alnav-core`/`alnav` 现状盘点后发现的功能缺口，非用户明确报的 bug。
- 已避开当前 6 个 in_progress 任务的范围（rename / jump-view-focus / nucleo-fuzzy / candidate-panel-slo / custom-keymap / filter-presets）及 CLAUDE.md 的 YAGNI 清单（历史/相对时间/多文件归并/Windows 等）。
- 07-31-tui-custom-keymap 已经落地 `alnav/src/keymap.rs` 的 `ActionId` 注册表（`KeymapStore`），四个子任务的新按键**一律**通过该注册表新增 `ActionId`，不得在 `main.rs`/`app.rs` 里硬编码 `match KeyCode::Char(...)`。
- 07-31-candidate-panel-slo 建立的 async gen-cancel worker 模式（详见 `.trellis/spec/alnav/backend/async-scans.md` 或等价文档）是统计面板、崩溃详情大文件扫描的复用对象，不重新发明一套后台线程机制。

## Subtasks

1. `08-07-tui-crash-detail-overlay` — 崩溃/ANR 结构化详情浮层，复用 `P`（Pretty）键
2. `08-07-tui-summary-panel` — 日志统计面板，`Leader i` 打开，基于 `visible` 静态快照
3. `08-07-tui-disconnect-indicator` — 设备断线状态栏图标，复用现有 `App.ingest_done`
4. `08-07-tui-line-wrap-toggle` — 单行/多行折叠切换，`w` 键，会话级、不持久化

## Cross-cutting Requirements

- 新键位一律走 `keymap.rs` `ActionId` 注册表；Help 面板（L1/L2 提示 + `?` 全目录）需同步收录新按键。
- 新增浮层（崩溃详情、统计面板）复用 `render_modal_shell` 靠上模态壳，颜色/图标一律经 `theme.rs`，禁止在 `ui.rs` 硬编码 `Color::*`。
- 四个子任务互不阻塞，可并行实现；每个子任务独立验收（各自 `cargo test -p alnav`），不要求同一 PR 交付。

## Acceptance Criteria

- [ ] 4 个子任务全部 `completed` 并归档
- [ ] `cargo build --workspace` 与 `cargo test --workspace` 全绿
- [ ] 四个新功能均有对应的 Help 提示（L1/L2 或 `?` 目录可查到按键）
- [ ] 无新增 `Color::*`/硬编码 RGB 落在 `ui.rs`（复查 `theme.rs` 唯一色源约束）

## Notes

- 本 PRD 只做需求索引与验收总控，不直接产出代码；实现细节见各子任务 `prd.md`（及复杂子任务的 `design.md`/`implement.md`）。
