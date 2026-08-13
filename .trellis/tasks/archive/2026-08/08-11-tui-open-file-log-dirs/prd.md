# TUI open-file log_dirs nucleo search

## Goal

强化 alnav TUI 的 Open file（`of` / Dashboard「Open file」）操作流：在 `config.toml` 声明的日志目录语料上，用进程内 **nucleo** fuzzy 找出目标日志并打开。Recent 仍作空态快捷入口；**不做**外部 fzf / yazi / Finder，**不做**语料外路径补全打开。

## Background / Confirmed Facts

- 现有 `OpenFilePanel`（`source_panel.rs`）：空 draft → recent；路径形 draft → `path_complete` 前缀补全；recent 已用 `fuzzy::fuzzy_label_indices`；选中后在 alnav 内切换 `-f` 源；右侧文件头 Preview。
- `RecentFiles` 持久化在配置目录 `recent_files.toml`；`AppConfig.recent_files_limit` 已存在。
- TUI 文本匹配门面为 `alnav/src/fuzzy.rs`（`nucleo-matcher`）；契约见 `.trellis/spec/alnav/backend/fuzzy-matching.md`。
- grilling（本会话）已收敛产品决策（见下方 Requirements）。

## Requirements

### R1 — 语料来源

- 候选文件语料 **仅** 来自 `config.toml` 的 `log_dirs: Vec<String>`（支持 `~` 展开）。
- 扫描时按 `log_extensions`（可配，默认 `[".log", ".txt"]`）过滤；大小写不敏感比后缀。
- **不做** cwd 回退扫描；**不做**压缩包（`.log.gz` 等）纳入。

### R2 — 匹配与列表语义

- 匹配引擎：进程内 nucleo（复用 `fuzzy.rs` → `Pattern::parse`），ignore-case；支持多段 AND 与 `'…` / `^…` / `…$` / `^…$`。
- haystack / 列表标签：相对配置根的相对路径；多根时前缀为 **末级根目录名**，形如 `bugly/2026-08-10/crash.log`。
- **空 query**：recent（最新在前）+ 全部语料文件（按 abs 去重）。
- **非空 query**：`recent ∪ 语料` 一起 fuzzy，按 score 降序；同分 **recent 优先**。
- **列表展示**：`basename · parent/path`（文件名优先，避免左截断看不清名）；Open-file 左栏至少约 55% 宽。
- 打开成功的路径仍写入 recent（既有行为保持）。

### R3 — 空语料 / 未配置

- 未配 `log_dirs`、目录不存在、或扫出 0 文件：仍可打开面板，只显示 recent。
- 若 recent 也为空：空列表 + flash 提示配置 `log_dirs`（短文案即可）。

### R4 — 语料外打开（MVP 明确不做）

- 移除 Open file 面板对 `path_complete` / Tab 路径补全 / 路径形 query 分支的依赖（或降级为无效）。
- **不做** yazi / macOS Finder / 外部 fzf 壳出。
- 用户只能打开：recent 中的路径，或语料扫描命中的文件。

### R5 — 异步扫描与缓存

- 首次打开 Open file 时启动后台递归扫描；边扫边把批次结果灌入面板可匹配集并刷新列表。
- 结果 **进程内缓存**；再次打开复用缓存。
- 手动刷新：`Ctrl-r`（或等价已占用则选未冲突键，写入 Help）丢弃缓存并重扫。
- 关闭面板可取消进行中的扫描（避免无用后台线程空转）。
- **无软上限**：扫到目录穷尽（接受大语料内存成本；用异步避免冻 UI）。

### R6 — 遍历规则

- **不**跟随 symlink。
- 跳过以 `.` 开头的文件与目录。

### R7 — 配置与 `--init`

- `config.toml` 扁平字段：
  - `log_dirs = ["~/logs", "..."]`
  - `log_extensions = [".log", ".txt"]`
- `--init` 写出带英文注释的默认值（`log_dirs` 默认空数组；extensions 为上述默认）。
- 坏配置回退行为与现有 `config.toml` 一致（builtin + status 提示）。

### R8 — UX 附属

- 扫描进行中：面板或 status 可见短进度（如已发现文件数 / scanning…），不阻塞键入与选择。
- 文件 Preview 行为保持（对选中文件读 head）。
- Help / Dashboard 文案从「Browse recent or local logs」改为反映「recent + configured log dirs」。

## Acceptance Criteria

- [ ] **AC1** 配置 `log_dirs` 后打开 `of`：空 draft 只见 recent；键入后可 fuzzy 命中语料内相对路径（含多根末级前缀）。
- [ ] **AC2** `log_extensions` 生效：默认只收 `.log`/`.txt`；改配置后刷新可看到扩展变化。
- [ ] **AC3** 未配置 / 空语料：面板可开；仅 recent；无 recent 时 flash 提示配 `log_dirs`。
- [ ] **AC4** 首次打开异步灌入；再次打开不重扫（除非 `Ctrl-r`）；关面板取消未完成扫描。
- [ ] **AC5** 不跟随 symlink；点文件/点目录不进语料。
- [ ] **AC6** Open file 无法通过路径补全打开语料外文件（MVP）；无 fzf/yazi/Finder 依赖。
- [ ] **AC7** `--init` / 默认 config 含 `log_dirs` / `log_extensions` 注释与默认；相关单元测试覆盖扫描过滤、相对标签、空 query/有 query 排序、缓存刷新。

## Out of Scope

- 外部 fzf / yazi / Finder / 任意文件浏览器集成
- cwd 或 recent 父目录自动扩张语料
- 软文件数上限、`log_scan_max_files`、深度硬上限
- 压缩日志、多文件合并、目录 watch 自动失效
- 每根独立 extensions / alias 字段
- CLI `alnav grep` 行为变更
- Windows 专门支持

## Open Questions

- （无）grilling 已关闭产品决策；实现期若 `Ctrl-r` 与现有绑定冲突，在 design 内改选未占用组合并更新 Help。
