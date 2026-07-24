# TUI --hdc Visible::All + drop-oldest ingest ring

## Goal

消除 `--hdc`（及任何仍走 owned `VecDeque` 摄入路径）在触顶淘汰时的 **O(n²) 卡死**，并为洪峰建立 **有界背压**：生产者侧仍解析（P-after），队列满时 **丢弃最旧未 drain 的 `EntryRow`**，不堵 `hilog`。同时将 Stream 的 `visible` 从「物化恒等 `Vec<usize>`」改为 **`Visible::All { len }`**，从结构上消灭无意义的全量 index shift。

本任务 **只覆盖活流 / Stream 路径**。`-f` mmap 大文件后端见兄弟任务 `07-24-tui-mmap-file-backend`（应在本任务之后实施）。

## Background

无 filter 时 `visible` 本为恒等 `0..rows.len()`，但 `shift_visible_after_front_evict` 仍对整表 `-=1`；达 `max_lines` 后每行 O(n)，累计 O(n²)。`spawn_hdc_ingest` 使用无界 `mpsc::channel`，生产者可远快于 `drain`。

## Requirements

### R1 — `Visible::All`（Stream）
- Stream 在 filter inactive / active 两种情况下，对当前 `view_source`（`rows` 或 `matched`）的可见集均为 **恒等映射**。
- 用 `Visible::All { len }`（或等价结构）表示；**不再**为恒等关系物化 `Vec<usize>`，淘汰时 **O(1)** 调整 `len` / `cursor` / `list_offset` / `visual_anchor`。
- `Subset(Vec<…>)` 预留给后续 file 任务的稀疏行号；本任务可不实现 File 语义，但枚举形状应允许后续扩展。

### R2 — P-after 有界丢旧 ring
- hdc ingest：生产者线程仍 `EntryRow::from_line`，再写入 **有界 ring**（非标准 `sync_channel` 阻塞语义）。
- ring 满：`pop_front` 最旧未 drain 行，再写入新行；**不阻塞**生产者读 `hilog`。
- 主线程 `drain` 从 ring 取走行后走现有 `push_row`（vocab / row_id / severe / filter 语义不变）。

### R3 — 行为契约
- `max_lines` / `matched` 保留缓冲 / following / bookmark / filter 语义与现网一致（除「通道积压时可能丢未展示行」这一显式背压行为）。
- `-f` 路径本任务可保持可编译/可测；不要求实现 mmap（file 仍可暂用旧 ingest，但若仍走同一 `Visible`/`push_row`，须同享 O(1) 淘汰，避免 file 继续 O(n²)）。

## Out of scope

- mmap / 行索引 / 惰性解析 / 去掉 file `max_lines`（→ mmap 任务）
- P-late（消费侧再 parse）或独立解析线程
- 堵生产者的背压策略
- 丢行 UI 徽标（可选后续；本任务不强制）

## Acceptance Criteria

- [ ] 无 filter、持续灌入超过 `max_lines` 时，不再出现 O(n²) 可见表平移；CPU/帧时间保持可交互量级。
- [ ] hdc 洪峰下 ring 有界；生产者不因 UI 慢而阻塞；积压时丢最旧未 drain 行，保留较新行。
- [ ] filter active 时 `matched` 保留语义回归（含 `matched_cap` 淘汰）。
- [ ] following / Esc resume / bookmark / Ctrl-L clear 行为不回归。
- [ ] `cargo test -p aloggrep-tui` 绿；`cargo fmt -p aloggrep-tui --check` 干净。

## Notes

- 共识来自 2026-07-24 grilling：S1 + P-after + 丢旧；与 `-f` 拆成独立任务；**本任务先做**。
- 详细设计见 `design.md`；执行顺序见 `implement.md`。
- 排序：完成后才能开始 `07-24-tui-mmap-file-backend`（该任务依赖 `Visible::All` 已存在）。
