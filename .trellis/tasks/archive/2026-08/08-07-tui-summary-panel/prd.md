# TUI 日志统计面板

## Goal

`Leader i` 打开一个只读浮层，基于当前 `visible`（过滤后结果）算一份统计快照：级别分布、Top 10 tags、Top 10 错误模式、崩溃计数、时间范围，关键数值项用横向 Unicode block 柱状图呈现，风格与现有 `theme.rs`/手搓 `Line`/`Span` 渲染一致（不引入 ratatui `BarChart`/`Sparkline`）。

## Background

- `alnav-core::summary::Summary` 已实现 CLI `--summary` 的全部统计逻辑（`record(&LogEntry)` 增量累加 + `finish()`/序列化输出 level 分布/Top 10 tags（含各级别子分布）/Top 10 错误模式（`Deduper` 归一化 pattern + count + tag + sample）/崩溃计数/时间范围），TUI 侧完全没有等价视图。
- `Summary` 只能整份重算（linear scan `record()` 逐行喂），不能对切片"增量更新"，且 `visible` 可能是百万行级（File 模式无淘汰上限）。
- 已有的 File 模式后台 worker 基础设施（mmap + `LineSpan` + `Arc<RwLock<Vec<LineSpan>>>`，参照 `.trellis/spec/alnav/backend/async-scans.md` 与 `store.rs::spawn_filter_scan`）是本任务复用的后台计算能力，**不新起一套线程模型**。
- Leader 语境（`KeyContext::Leader`）已占用 `Space`(manage)/`w`(preset_save)/`o`(preset_open)/`Esc`(cancel)，`i` 未占用。

## Requirements

### R1 — 数据范围与刷新策略

- 统计范围固定为 `visible`（当前过滤结果；未过滤时等于全量）。
- 打开时算一次**静态快照**，不随后续新行/过滤变化自动刷新；用户需要新数据必须关闭后重开（再按 `Leader i`）。

### R2 — 计算方式（统一走后台）

- 复用现有 File 异步 worker 基础设施：后台线程共享 `Arc<Mmap>` + `Arc<RwLock<Vec<LineSpan>>>`，按 `visible` 的行集合逐行解析并喂给一份新建的 `alnav_core::summary::Summary` 实例，完成后通过既有的 file-event 通道（类似 `FileEvent`）把结果送回主线程，`poll_file_store` 一并轮询。
- Stream 模式：`visible`/`matched` 已是内存中的 `Vec<EntryRow>`（`Clone`），后台线程用一份克隆快照做同样的 `Summary::record` 循环，避免长时间持锁阻塞 ingest。
- 面板打开期间显示"计算中…"占位（参照 `theme::log_loading_style`/`numbered_title_with_loading` 的既有 loading 视觉语言）；计算完成后渲染结果。
- 用**生成号（gen）机制**防止过期结果覆盖新请求（面板关闭又重开时，旧的后台计算结果到达应被丢弃）——沿用 `CandidateMatchService`（`candidate_match.rs`）的 `gen: u64` + mpsc channel 思路，不要求完全同构，但不允许新起无生成号保护的裸线程。

### R3 — 触发键

- 新增 `ActionId::LeaderSummary`（`KeyContext::Leader`，`toml_key: "summary"`，默认绑定 `i`），走 `keymap.rs` 注册表，不硬编码。

### R4 — 内容与呈现

- 复用 `render_modal_shell` 靠上模态壳。
- 字段对齐 CLI `--summary`：总行数（`visible.len()`，不单独区分 total/matched 两个数字）、级别分布、Top 10 tags（含各级别子分布）、Top 10 错误模式（pattern + count + tag + sample）、崩溃计数、时间范围（首/尾时间戳）。
- **横向柱状图**（手搓 Unicode block `█`，按 `count / max_in_section` 比例，`Line`/`Span` 拼接，不用 ratatui `BarChart`）：
  - 级别分布：每个级别一行，柱形颜色取该级别的 `logcolor`/`theme::level_badge_style` 颜色。
  - Top tags：柱形统一用 `theme::ACCENT`。
  - Top errors：**不加柱形**，只列表 + 数字（pattern 文本较长，避免拥挤，且已有 Top tags 柱形做视觉参照）。
- 内容超出面板高度可 `j`/`k` 滚动。
- `Esc` 只关面板，不 `resume_following`（跟 Fields/Pretty/Detail 现状一致）。

### R5 — 范围边界（明确不做）

- 不做"跟随 visible 变化持续刷新"。
- 不做统计范围切换开关（全局 vs 过滤后），固定基于 `visible`。
- 不引入 ratatui `BarChart`/`Sparkline`/`Gauge` 等新 widget 类型。
- 不做导出/yank（跟崩溃详情浮层一致，view-only）。

## Acceptance Criteria

- [ ] `Leader i` 打开面板，展示级别分布（带柱状图）、Top 10 tags（带柱状图）、Top 10 错误模式（无柱状图）、崩溃计数、时间范围
- [ ] File 模式大文件（模拟 ≥10 万行 `visible`）下，打开面板不阻塞主线程渲染（UI 仍可 `j`/`k`、无明显卡顿），显示"计算中…"占位直到结果就绪
- [ ] Stream 模式下同样能打开并正确统计当前 `matched`/`rows`（视过滤状态）
- [ ] 面板打开期间过滤条件变化，不自动重算（保持快照）；关闭重开后拿到基于新 `visible` 的新快照
- [ ] 快速连续 关闭→重开 时不出现"旧计算结果覆盖新请求"的错乱（gen 校验生效）
- [ ] Help 面板 `Leader` 层级能查到 `i` 的提示
- [ ] `cargo test -p alnav` 全绿

## Notes

- 本任务与 07-31-tui-custom-keymap、07-31-candidate-panel-slo 有基础设施依赖（`ActionId` 注册表、async gen-cancel 模式），但不阻塞——`keymap.rs` 注册表已落地可直接用，`candidate_match.rs` 的 gen 模式作为设计参照而非强耦合代码依赖。
