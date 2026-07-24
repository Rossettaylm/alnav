# TUI global time window (-f only)

## Goal

在 `aloggrep-tui` 的 `-f` 文件模式下提供**全局会话时间窗**：与 Filter/Exclude 正交、对所有行 AND；通过 `ts` 面板交互设定、`tu` 清除；`--hdc` 不暴露交互入口。

## Requirements

### R1 — 全局会话时间窗
- 时间窗挂在 `App` 级（非 Filter `Group`）。
- 与 chip include / exclude / lock 组合时：先 chip+exclude，再 lock，再时间窗（或等价：时间与 lock 同为全局 AND）。
- 启动 CLI `--since` / `--until` **并入**该全局窗；Filter 组不再承载时间。
- 时间窗生效时计入 `filter_active`（走 `matched` 保留缓冲）。
- `yc` 导出带上全局 `--since` / `--until`。

### R2 — 仅 `-f` 交互
- `--hdc` 下硬隐藏：`ts` / `tu` 不响应（可 silent no-op 或与其它无效键一致）。
- 启动 `--hdc --since/--until` 行为保持可用（写入全局窗并过滤），只是没有交互入口。

### R3 — 快捷键
- LogList Normal：`t` 进入 operator-pending；`ts` 打开面板；`tu` 清除时间窗。
- 无日期候选时 `ts` 拒绝打开并 flash「无可用日期」。

### R4 — 面板 UX（单次会话壳）
- 一屏两栏 since | until；字段顺序：since 日期 → since 时间 → until 日期 → until 时间。
- Tab / Enter 切换栏位；最后一栏 Enter = 提交；Esc = 取消且不改已生效窗。
- **日期**：候选来自当前 `rows` 去重日期；键入过滤；只能选候选；Enter/Tab = 选中高亮并前进（过滤后唯一则自动收）；无高亮不前进。
- **时间**：`HH:MM:SS` 自由键入；先合法化再夹到**该日在缓冲内的 min/max**；两端齐全时夹**当前编辑端**保证 since ≤ until。
- 允许只设 since 端或只设 until 端；端内必须日期+时间成对，否则 flash、面板不关。
- 空提交 ≠ 清除；清除只用 `tu`。
- `ts` 重开时预填当前全局窗（启动字符串尽力拆分）。

### R5 — 呈现与 Following
- 状态栏徽标 `TIME …`（类似 LOCK）；无窗不显示。
- 打开 `ts`、提交成功、`tu` → `following=false` +（提交/`tu`）`rebuild_visible`。
- 面板 Esc 不 `resume_following`。

### R6 — 启动兼容
- 全局窗内部仍存 since/until 字符串（兼容 CLI 多种格式）。
- `ts` 打开时尽力拆成日期+HMS 预填；面板提交后统一写成「候选日 + HMS」形式。

## Out of scope

- 光标行派生设时间
- `--hdc` 交互时间窗
- 相对时间（last 5m）
- 全文件日期索引（非缓冲）
- 独立 Time strip
- 扩展 `expr.rs` 时间语法

## Acceptance Criteria

- [ ] `-f` 下 `ts` 可打开面板；从候选选日、键入时分秒；提交后日志被时间窗过滤；状态栏显示 `TIME …`
- [ ] 允许只设 since 或只设 until；端内缺日期或时间提交失败并 flash
- [ ] `tu` 清除时间窗，徽标消失，可见行恢复（在其它过滤不变前提下）
- [ ] `--hdc` 下 `ts`/`tu` 不可用
- [ ] 启动 `aloggrep-tui -f file --since … --until …` 仍过滤；时间在全局窗而非 Filter 组；`di` 第 0 组不影响时间窗
- [ ] 时间窗生效时 `filter_active == true`；`yc` 含 `--since`/`--until`
- [ ] 无日期候选时 `ts` flash 拒绝
- [ ] 单元测试覆盖：全局窗匹配、`filter_active`、导出、日期候选提取、时间夹紧/since≤until、`initial_group` 不再把 time 挂组上

## Notes

- 共识来自 grilling 会话（2026-07-23）；用户确认共享理解后建任务并执行。
