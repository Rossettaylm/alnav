# TUI nucleo fuzzy search

## Goal

将 alnav TUI 的全部检索/过滤键入场景统一为 **nucleo 模糊匹配**（ignore-case），替代现有子串/`Regex` 字面匹配；CLI（`alnav grep`）保持现状。用户在 TUI 内获得一致的 fuzzy 手感，无需外挂 fzf。

## Background / Confirmed Facts

- 当前 Picker/MsgChip/Time 候选过滤为 ignore-case **子串**（`PickerSession::filtered_indices`）。
- Highlight/Search 经 `HighlightGroup` 编译为 **转义后的 ignore-case Regex**（对用户而言是字面子串，非开放正则）。
- Filter/Exclude 文本条件经 `Expr::from_filters` + AST 求值（字面子串）；`pid`/`tid`/`level` 为精确语义。
- File 模式：`FileStore` mmap + 后台行索引；过滤时后台扫 `Visible::Subset`（见 `.trellis/spec/alnav/backend/file-store.md` / `async-scans.md`）。
- Stream 模式：环形缓冲 + `matched` 双写（见 `stream-visible-ingest.md`）。
- grilling（本会话）已收敛产品决策；不做 fzf HTTP，不做 matcher 配置项。

## Requirements

### R1 — 唯一引擎（TUI）

- TUI 文本匹配 **仅** 使用 **`nucleo-matcher`**（ignore-case fuzzy）：
  - 日志行：在既有 File 后台 Filter/Highlight 扫描与 Stream 求值路径上 **per-row** 调用
  - 小候选列表：同步 `fuzzy_label_indices`
- **MVP 不要求** 高阶 `nucleo` 异步 corpus / `FuzzyIndex`（记为后续优化；契约见 `fuzzy-matching.md`）。
- **不做** fzf 二进制依赖、HTTP `--listen`、启动探测回退链。
- **不做** TUI 正则逃生口（无 `/re:` 等模式）。
- `config.toml` **不新增** matcher 相关键；大小写固定 ignore-case（与现 TUI 一致）。

### R2 — 覆盖场景

| 场景 | 匹配文本 |
|------|----------|
| Search / Highlight | `tag` + 分隔符 + `msg`；二者皆空或未解析 → **raw 整行** |
| Filter / Exclude 文本 chip（`tag`/`msg`/`pkg`） | **仅该字段**；字段空则该 chip 对该行不命中（raw 回退仅用于无字段的 Search/Highlight 路径，或文本 Filter 在字段空时对 raw 的规则见 design） |
| `pid` / `tid` / `level` | **精确匹配**（非 fuzzy） |
| Picker Manage / MsgChip / Time 日期候选 | 候选 label 的 fuzzy（`nucleo-matcher`） |
| 启动 CLI → TUI 初始 Filter 组 | 与交互 chip **同一套 fuzzy** |

组合语义保持现模型：组内 chip **AND**、组间 **OR**；Exclude 全局 **AND NOT**；其后仍 AND lock / time_bound / view_focus。

### R3 — 索引与渐进结果

- `-f`：随行索引/解析进度 **后台灌满** nucleo；未完成时已注入行可查，status 显示进度（如 `idx 120k/500k`），完成后自动刷新命中集。
- `--adb` / `--hdc`：`drain` 注入；环形淘汰时从 matcher 侧失效或等价重建，避免幽灵命中。

### R4 — 高亮与导航

- 启用的 Highlight/Search 在日志渲染上使用 fuzzy **positions**，按拼接规则 **映射回 tag / msg 列** 再上色（跨分界拆段）。
- `n`/`N`、active highlight、minimap search 标记改为基于 fuzzy 命中集，不再依赖 `Regex::find_iter`。

### R5 — CLI 分叉与导出

- `alnav grep` / `alnav-core` FilterChain / `-e` **不改**。
- `yc` 仍导出字面近似 `alnav grep` 命令；提示用户 **TUI fuzzy ≠ CLI**（flash 或等价短提示，如 `approx`）。

### R6 — 破坏性（TUI）

- 曾依赖「必须连续子串」或「Regex 元字符被转义后的字面」的命中集会变为 fuzzy 排序/命中（例如 `abr` 可命中含 `a…b…r` 的行）。
- Help / 状态文案中不再暗示「子串/正则」为 TUI 检索模型。

## Acceptance Criteria

- [ ] **AC1** Picker / MsgChip / Time 日期候选：键入非连续字符仍可按 nucleo 规则命中；空 query 显示全部；ignore-case。
- [ ] **AC2** Search/Highlight：对 `tag+msg`（或 raw 回退）fuzzy；命中行可 `n`/`N`；字符级高亮落在正确字段。
- [ ] **AC3** Filter/Exclude：`tag:foo` 只约束 tag 列 fuzzy；`msg:foo` 只约束 msg；多 chip AND、多组 OR、Exclude NOT 行为与现结构一致；pid/tid/level 仍精确。
- [ ] **AC4** 大文件打开后 status 可见索引进度；索引中即可部分命中；完成后命中集自动一致。
- [ ] **AC5** Stream 淘汰后，不可再跳到已淘汰行的 fuzzy「幽灵」命中。
- [ ] **AC6** 启动带 `--tag`/`--msg` 进 TUI 的初始组按 fuzzy 过滤（与交互 chip 一致）。
- [ ] **AC7** `yc` 仍可复制命令，并有「近似 / 非 fuzzy」提示；CLI 集成测试不因本任务而改为 fuzzy。
- [ ] **AC8** 无 fzf 依赖；`config.toml` 无新 matcher 键；相关单元测试从 Regex/子串断言迁移为 fuzzy 行为。

## Out of Scope

- fzf HTTP / 外挂进程 / 启动校验 fzf
- CLI `alnav grep` 改为 fuzzy 或废弃 `-e` 正则
- `config.toml` matcher 开关、smart-case 配置
- Windows 专门支持、stdin 管道 TUI
- 相对时间、多文件 glob 等既有 YAGNI 项

## Open Questions

（无阻塞项；grilling Q1–Q14 已确认。）
