# TUI file async scans (highlight / filter / severe) + LogList loading

## Goal

在 `-f` mmap（`FileStore`）已落地的前提下，消除「启动不卡、加 Highlight/Filter 等全盘扫描就卡」：凡 A1 范围内的全可见集扫描一律走 **可取消后台任务 + 增量结果**，主线程禁止 O(visible) `row_at` 全扫。LogList 框内用主题化 loading 文案展示 index / filter / highlight 进度，**不阻挡**滚动与按键。

前置：`07-24-tui-mmap-file-backend` 已 archive（commit `0dca14d`）。Filter 后台扫描已有；本任务加固其 UI 路径，并补齐 Highlight（含 `n`/`N`）与 severe。

## Requirements

### R1 — All-scan 架构（A1 首版范围）
- 统一可取消后台扫描模型（`ScanJob` 或等价），至少覆盖：
  - **F**：File filter（加固：批次/完成不得触发 UI 全量 parse）
  - **H**：Highlight 命中索引（统计 + `n`/`N`）
  - **S**：severe 查找 / 预热不在 UI 全量 `row_at`
- 首版 **不做**：vocab 全量、`ts` 全文件日期索引、独立 Search 管线（若 Search≡Highlight 则随 H）。

### R2 — Highlight：Vis 域 + Inc
- 扫描域 = 当前 **`visible`**（`All` 或 `Subset` 源行），与现 `compute_match_stats_inner` 语义一致。
- **增量**：对新增 visible 槽位继续扫并 append 命中；`filter_gen` / active highlight / visible 世代变化 → cancel + 作废重来。
- 结果为命中下标结构，供 status 计数与 `n`/`N`；主线程 **禁止** 再跑 O(visible) `row_at` 统计。

### R3 — Filter 加固
- `FilterBatch` / `FilterDone` 路径不得调用会全量解析 visible 的逻辑。
- Highlight job 与 filter 进度协调：Inc 可在 Subset 增长时跟扫，不得阻塞 UI。

### R4 — Severe（S）
- File 模式：`find_severe` / minimap severe 依赖懒 cache 或后台预热；不得在按键路径上对全文件/`visible` 同步 `row_at` 扫一遍。

### R5 — LogList loading UI（L1 + T + Free）
- 覆盖 job：**index / filter / highlight**。
- 形态：Log 圆角框 **标题或顶栏** 进度文案（如 `Indexing 12%…` / `Filtering…` / `Highlight…`）。
- 样式仅走 `theme.rs`（可 theme.toml），禁止 `ui.rs` 硬编码 `Color`。
- **Free**：扫描中仍可滚动与按键；不使用挡操作的居中遮罩。

### R6 — `--hdc` / Stream 不回归
- Stream 可继续用已有 owned 行做同步统计（集合有界）；不得破坏 `Visible::All` / drop-oldest ring。
- 若 Stream 与 File 共享 API，File 走异步、Stream 保持现语义或同样非阻塞。

## Out of scope

- 重做 mmap / 行索引
- P-late ingest、活流落盘
- vocab / `ts` 全文件日期索引后台化（后续）
- Windows 专门优化

## Acceptance Criteria

- [ ] 700MB 样例（或同等）`-f`：打开后加 Filter、加 Highlight、连续 `n`/`N`，UI 保持可交互（无明显多秒冻帧）。
- [ ] Highlight 统计与 `n`/`N` 结果与小文件语义一致（相对于 visible）。
- [ ] Filter 变更可取消旧扫描；无 FilterBatch 触发的 UI 全量 parse。
- [ ] LogList 在 index/filter/highlight 进行中显示 loading 文案；仍可 `j/k` 滚动。
- [ ] severe 跳转在大文件上不因同步全扫卡死。
- [ ] `cargo test -p aloggrep-tui` 绿；`cargo fmt -p aloggrep-tui --check` 干净。
- [ ] `--hdc` 既有测试/行为不回归。

## Notes

- 共识来自 2026-07-24 grilling：All-scan → A1 → Vis → Inc → L1+T+Free → 新任务 → Commit-first（mmap 已先合）。
- 样例路径：`/Users/lyman/Downloads/extracQQXLog_HarmonyQQ_2026.07.22.10-2026.07.24.10/merged_2026.07.22-24.log`。
