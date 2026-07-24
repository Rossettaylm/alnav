# TUI -f mmap file backend for huge logs

## Goal

让 `aloggrep-tui -f <大文件>` 能（近）秒开、可交互、内存与文件大小基本解耦。以只读文件为前提，用 **mmap + 行偏移索引 + 惰性解析** 替换当前“后台线程逐行解析成 owned `EntryRow` → channel → `VecDeque` 按 `max_lines` 淘汰”的 file 摄入模型。

`--hdc` 活流 **不在本任务范围**（见已拆出的 `07-24-tui-hdc-stream-visible`）。本任务在 hdc 任务完成之后实施，并复用其 `Visible::All`；File 过滤结果用 `Visible::Subset`（命中行号）。

背景：700MB（≈400 万行）文件 `-f` 打开直接卡死。主因包括 O(n²) 淘汰（由 hdc 任务先行修掉 Stream/共享 `push_row`）、无界 channel 峰值、以及逐行 owned 物化。此外超出 `max_lines` 的历史行被物理淘汰，**无法浏览全文件**。

## Requirements

### R1 — File 后端改为 mmap + 行索引 + 惰性解析
- `-f` 打开走 `memmap2::Mmap`，不做全量读盘。
- 建立**行偏移索引**（每行字节范围），用于随机访问与行数统计。
- 行内容**惰性解析**：只对可见窗口（屏幕高度 + minimap 采样）从 mmap 切片临时解析成 `EntryRow`。
- 任何阶段**不为全文件物化 owned `EntryRow`**。

### R2 — 移除 file 场景的 `max_lines` 上限，可浏览全文件
- file 模式无淘汰；`g`/`G`/滚动/跳转可到达文件任意行。
- 书签可锚定任意行（进程内）。
- `--max-lines` 对 file 模式无意义（保留 flag，对 `--hdc` 仍生效）或文档标注。

### R3 — 过滤/搜索/vocab/severe 不再逐行物化
- filter 变化时以“**命中行号集**”表示可见集（`Visible::Subset`），不克隆 owned 行进 `matched`。
- O(n) 解析扫描不可回避，但需：① 可**后台执行**、② 可**取消/被新 filter 抢占**、③ **增量出结果**（边扫边显）。
- vocab 通过后台扫描（可采样）构建；severe 惰性计算 + 缓存。

### R4 — 加载期可交互（验收门在 Phase C）
- 索引/过滤进行中 UI 不阻塞：可滚动已索引部分、可响应按键、显示进度（行数/百分比）。
- 索引完成后 `G` 可跳真正末尾；following 语义（file 静态）= 钉底。
- **B-gate**：同步 FileStore（Phase B）可作为工程里程碑与烟测点，**不**视为本任务验收通过；验收以后台化 Phase C 为准。

### R5 — 现有 TUI 行为与契约不变
- Filter/Exclude/Highlight/lock/`time_bound`/bookmark/`yc` 导出/minimap/preview 语义与既有一致。
- 遵守 `session-filters.md`（time 只在 `App.time_bound`）、`quality-guidelines.md`。
- File 模式不可解析行：保留为 raw-only `EntryRow`（与 stream 丢弃不可解析行的行为有意不同）。

### R6 — 依赖与共存
- **依赖**：`07-24-tui-hdc-stream-visible` 已落地（`Visible::All` + Stream O(1) 淘汰）。
- `--hdc` 行为以该任务为准；本任务将 Stream 收进 `RowStore::Stream` 时不得回归。

## Out of scope

- `--hdc` 背压 / `Visible::All` 引入（→ hdc 任务）
- 文件增长实时 tail（file 视为静态快照）
- 多文件 glob / 归并排序
- 磁盘持久化的全文件日期索引
- stdin 管道流式
- Windows 专门优化（memmap2 跨平台可用即可）

## Acceptance Criteria

- [ ] 前置：`07-24-tui-hdc-stream-visible` 已完成（或同等 `Visible::All` 已在主干）。
- [ ] 用约定 700MB 样例 `-f` 打开：≤ 数秒可见首屏并可交互，不卡死。
- [ ] 进程 RSS 远小于文件大小（不出现 owned 行 >数百 MB / channel >1GB 峰值）。
- [ ] 可 `G` 跳到文件真实末尾、`g` 回首行、任意滚动到中段（不受 500k 限制）。
- [ ] 施加 Filter/Exclude/lock/time_bound 后可见集正确；大文件下过滤不冻结 UI（后台/增量 —— Phase C）。
- [ ] Highlight、search `n/N`、minimap、preview、bookmark、`yc` 与小文件一致（回归）。
- [ ] `--hdc` 不回归（复用 hdc 任务验收）。
- [ ] 小文件测试全绿；`cargo test --workspace`；`cargo fmt -p aloggrep-tui --check` 干净。
- [ ] 非 UTF-8 / 无末尾换行 / 空文件 / 全不可解析行等边界不 panic。

## Notes

- 2026-07-24 grilling：方案 B（mmap）；与 hdc 拆任务；顺序 **hdc → file**；File 验收 **B-gate**（B 里程碑，C 验收）。
- 详细设计见 `design.md`；执行顺序见 `implement.md`。
