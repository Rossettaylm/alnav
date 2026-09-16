# File Session Restore

## Goal

alnav `-f` 文件模式下，退出时自动保存当前分析现场（过滤规则、排除、高亮、
lock、全局时间窗、view focus、光标位置、书签），下次打开**同一个日志文件**时
自动恢复，免去重复搭建规则的成本。

仅涉及文件模式（`-f` / Dashboard 打开文件）。`--hdc` / `--adb` live 会话不参与。

## Requirements

### R1 存储粒度：按文件

- 以**规范化绝对路径**为 key，每个日志文件各存一份现场。
- 路径 `$config_dir/sessions/<fnv1a64(path)>.toml`，`config_dir` 沿用现有解析
  （`--config-path` > `$ALNAV_HOME` > `~/.config/alnav`）。
- 文件内部记录 `source` 绝对路径，加载时校验一致；不一致视为无现场。
- 目录容量上限 **50**，超出按 mtime 淘汰最旧。
- FNV-1a 手写实现（约 10 行），不新增依赖。

### R2 恢复触发点

恢复只发生在**本会话第一次绑定文件源**：

- `alnav -f a.log` 启动绑定 → 恢复
- 无 `-f` 进入 Dashboard，从 Dashboard 打开 a.log → 恢复
- `C-f` Open-file 面板中途换文件（`switching == true`）→ **不恢复**
- `C-g` 切到 live / live 切到 file → **不恢复**

### R3 保存触发点

- 进程退出（`run()` 返回后统一落盘），覆盖 `Ctrl+C` / `q` / Dashboard Quit 全部路径
- `C-f` / Dashboard 切换源**离开旧 file 时**先落盘旧文件的现场
- 从 file 切到 live 也落盘（确实离开了该文件）
- panic **不保存**
- 写盘使用临时文件 + rename 原子替换

### R4 保存内容

| 存 | 说明 |
|---|---|
| `groups.groups` | chips + **`enabled`** + `same_field_op`（disabled 组必须保留） |
| `groups.excludes` | chip + `enabled` |
| `highlight_groups` | pattern + `enabled` |
| `active_highlight` | 当前激活的 search 组序号 |
| `lock_pid` / `lock_tid` | 互斥，各自最多一个 |
| `time_bound` | `since` / `until`，单端也原样保留 |
| `view_focus` | `highlight` / `severe` |
| `cursor_row_id` | file 模式 = `line_index + 1` |
| `list_offset` | 滚动偏移原始值，恢复时 clamp |
| `bookmarks` | 只存 `raw` 字符串 + `row_id` |

**不存**：`following`（恢复后一律 `false`）、`collapsed_view`、Dashboard 光标、
`visual_anchor`、所有 `pending_*`、模态态、search 草稿、`detail` 浮层。

### R5 抑制规则（核心）

以下情况**既不读也不写**现场文件：

- **CLI 显式传了任一过滤参数**：`--tag` / `--msg` / `--level` / `--pkg` /
  `--pid` / `--tid` / `--since` / `--until` 任一非空
- **`config.toml` 的 `restore_session = false`**

理由：CLI 参数是命令行的一次性意图，不是"我攒的现场"；一条规则同时管住读和写
两个方向，避免 `alnav -f a.log --tag Foo` 看一眼退出就覆盖掉昨天的规则。

### R6 退化现场不落盘

保存前检测：无 filter group、无 exclude、无 highlight、无书签、无 lock、
无时间窗、无 view focus **且** `cursor_row_id <= 1` → 跳过写入，保留已有文件。

lock / 时间窗 / view focus 也算"配置"：只用它们搭出来的现场同样值得保留。

### R7 恢复时机（异步）

- 规则 / 时间窗 / lock / view_focus / 书签：绑定文件后**立即**恢复并
  `rebuild_visible()`
- `cursor`：存为 `pending_restore_cursor: Option<u64>` + 配套 `list_offset`，
  在 `App::poll_file_store` 中消费。FileStore 行索引渐进增长，第一次 `visible`
  非空时索引可能只覆盖文件前缀，因此判定条件是**解析成功即消费**，失败则等到
  `file_index_done() && ingest_done`（索引与过滤扫描都收敛）再降级为光标 0；
  两者都未完成时保持等待

### R8 降级策略（不做内容指纹）

- 只校验**路径存在**，不做内容/大小/mtime 指纹（append 是日志常态，指纹会让恢复
  几乎永远失效）
- `cursor_row_id` / `list_offset` 越界一律 clamp
- 书签 `row_id` 越界走现有 `BOOKMARK EVICTED` 路径，不写新逻辑
- **忠实重放**：恢复的时间窗可能筛出空列表，照常恢复，不自动丢弃

### R9 损坏与版本

- 文件含 `version = 1`
- 解析失败 / 版本不符 → 当作无现场 + flash `SESSION fallback: <原因>`，
  **不删文件、不写迁移**

### R10 用户反馈与入口

- 恢复成功 → flash `RESTORED …`（含规则/时间窗/书签计数），
  **仅在 Dashboard 首绑场景打**；`-f` 启动路径静默
- `config.toml` 新增 `restore_session = true`（默认开），
  `keymap::serialize_default_config_toml()` 模板同步加注释
- 命令面板（`C-p`）新增一条 `Clear session`：删当前文件的存档文件

## Acceptance Criteria

- [ ] `alnav -f a.log` 搭规则 → 退出 → 再次 `alnav -f a.log`，规则/时间窗/lock/
      高亮/view focus/光标/书签全部回到退出时的状态
- [ ] `alnav -f b.log` 不受 a.log 现场影响；a.log 的现场仍在，再开 a.log 仍能恢复
- [ ] 无 `-f` 启动 → Dashboard 打开 a.log → 恢复生效，并 flash `RESTORED …`
- [ ] `alnav -f a.log --tag Foo` → 不读现场；退出后 a.log 原现场文件未被改写
- [ ] `restore_session = false` → 不读不写
- [ ] `C-f` 换文件：旧文件现场已落盘，新文件现场不被套用
- [ ] file → live 切换时旧 file 现场已落盘
- [ ] `--hdc` / `--adb` 全程不产生也不读取 sessions 文件
- [ ] 全空现场退出不覆盖已有存档文件（"全空"= 无 Filter/Exclude/Highlight、无书签、
      无 lock、无时间窗、无 view focus、`cursor_row_id <= 1`）
- [ ] 存档 TOML 损坏 → 启动正常 + flash `SESSION fallback: …`，文件保留
- [ ] sessions 目录超过 50 个 → 按 mtime 淘汰最旧
- [ ] 命令面板 `Clear session` 删除当前文件存档，后续启动不再恢复
- [ ] `cargo test --workspace` 通过

## Notes

- `preset.rs` 的 `Preset` wire 格式**不能直接复用**：`capture()` 会丢弃 disabled
  组与 disabled exclude，而现场必须保留 `enabled` 标志。
- `App.storage` 侧 `row_id` 在 file 模式由 `FileStore` 惰性解析时赋值为
  `line_index + 1`（见 `store.rs`），因此 `row_id` 即稳定的行定位。
- 与 `recent_files.toml` 正交：恢复现场不影响最近文件列表更新。
- live 现场不做（无限流无位置概念，`time_bound` 被 `bind_live_source` 主动清空）。
