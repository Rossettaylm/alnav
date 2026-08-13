# Design: TUI open-file log_dirs nucleo search

## Overview

把 Open file 从「recent + `path_complete`」改成「recent + 配置目录异步语料 + nucleo」。扫描与 Preview 一样走 `mpsc` + 后台线程；匹配复用 `fuzzy::FuzzyScorer` / `fuzzy_label_indices` 模式。

## Components

### 1. Config（`config.rs` + `--init`）

```toml
# Directories recursively scanned for Open-file fuzzy corpus (empty = recent-only).
log_dirs = []
# Case-insensitive suffix filter (include the dot).
log_extensions = [".log", ".txt"]
```

- `AppConfig` 新增 `log_dirs: Vec<String>`、`log_extensions: Vec<String>`。
- 加载时：extensions 空则回退默认；dirs 做 `trim`，空串丢弃；`~` 在扫描侧展开（与 `path_complete::expand_user` 同源或抽一小函数到共享处，避免 Open file 再依赖完整 path_complete UI）。

### 2. Corpus index（新模块建议 `log_corpus.rs`）

```text
LogCorpus
  roots: Vec<Root>          # expanded abs path + leaf name for prefix
  entries: Vec<CorpusEntry> # { abs, label }  label = "{leaf}/{rel}"
  status: Idle | Scanning { found } | Ready | Cancelled
  generation: u64           # bump on refresh / cancel
```

- `CorpusEntry.label` 供 nucleo haystack 与列表展示；打开用 `abs`。
- 多根：`label = format!("{}/{}", root.leaf_name, rel.strip_prefix...)`；单根同样带 leaf 前缀（行为一致、实现简单）。
- 遍历：`walkdir` 或手写 `read_dir` 栈；**不** follow symlink；跳过 `file_name` 以 `.` 开头；后缀用 `log_extensions`（规范化小写比较）。
- 批次推送：每 N 条（如 256）或定时 `try_send` 一批 `ScanBatch { gen, entries }`；结束发 `ScanDone { gen, total }` / 错误。
- **无软上限**；取消：丢弃旧 `gen` 的消息即可（线程可早退若共享 `AtomicBool`/`gen` 比对）。

依赖：若引入 `walkdir`，仅 `alnav` crate；否则手写递归以免新依赖——优先手写 DFS（与「不 follow symlink」一致，用 `symlink_metadata` / `file_type().is_symlink()` 跳过）。

### 3. OpenFilePanel 行为（`source_panel.rs`）

| 状态 | choices |
|------|---------|
| draft 空 | 仅 `OpenFileChoice::Recent` |
| draft 非空 | 对 `recent.paths` 的展示串 ∪ `corpus.labels` 做 nucleo；结果映射回 Recent 或 Corpus；同分 recent 先 |

- 删除 / 停用：`looks_like_path_query` + `Path` 候选 + `apply_tab_complete` 路径分支。
- `OpenFileChoice` 可简化为 `Recent(String)` | `Corpus { abs, label }`（或统一 `File { abs, label, from_recent }`）。
- 面板持有或借用 `App` 上的 `LogCorpus` 句柄；`refresh_choices` 在收到 batch 后重算（仅当 draft 非空或需要更新进度文案）。
- Preview：继续对 `abs` 读 head（与现逻辑相同）。

### 4. App / main 集成

- `App` 持有 `log_corpus: LogCorpus`（或 `Option` + 懒启动）。
- `open_file_source_panel`：若 corpus Idle 且 `log_dirs` 非空 → `start_scan`；若 Ready → 复用；Scanning 则只挂面板。
- `Ctrl-r`：在 Open file 聚焦时 `corpus.refresh()`（bump gen、清空 entries、重扫）。
- 关面板：`cancel_scan()`（bump gen / set flag）；**保留**已 Ready 的缓存供下次打开（除非正在 Scanning 被取消则回到 Idle/部分结果策略：**取消时保留已收到条目并标 Ready**，避免每次 Esc 丢进度——推荐保留已扫部分为 Ready）。
- flash：无 dirs 且 recent 空；或 dirs 全无效。

### 5. UI / Help

- 底部 draft 旁或 Preview 标题旁显示 `scanning… N` / `N files`。
- Help：`of` 描述更新；增加刷新键。
- Dashboard Open file 副文案更新。

### 6. 测试要点

- 后缀过滤、点目录跳过、symlink 跳过（tempdir fixtures）。
- 多根 label 前缀。
- 空 query 不含 corpus；非空 query 混排与 recent 优先（构造相同 score 困难时测「recent 在候选中且可命中」+ 排序稳定性单测对 scorer 包装函数）。
- 配置加载默认 / 覆盖。
- 取消 gen：旧 batch 不污染新 gen。

## Data flow

```text
of / Dashboard Open file
  → open panel
  → (first time) spawn walk(log_dirs) ──batch──► LogCorpus.entries
  → draft empty? recent only
             else fuzzy(recent labels ∪ corpus labels)
  → Enter → open abs path → record recent → FileStore
Ctrl-r → clear + rescan
Esc    → close panel; cancel in-flight walk
```

## Risks

| 风险 | 缓解 |
|------|------|
| 超大目录内存 | 产品已接受无上限；异步保 UI；文档/注释标明 |
| 根目录末级重名 | grilling 接受；用户改路径区分 |
| `Ctrl-r` 键冲突 | 实现前查 `keymap`；冲突则换 `Ctrl-g` 等并写 Help |
| `path_complete` 模块变死代码 | 若仅 Open file 使用则删除或保留给将来辅路径；以 grep 为准 |

## Out of scope (design)

外部工具、软上限、watch、压缩包、每根 extensions。
