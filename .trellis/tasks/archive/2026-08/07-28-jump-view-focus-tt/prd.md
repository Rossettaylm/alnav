# Jump no-wrap + fh/fe view focus + tt time

## Goal

在已有 Filter（及 lock/time）结果上，用全局会话视图状态快速缩到「仅高亮行 / 仅错误行」；同时让 `n/N`、`e/E` 跳转不再环绕列表，并把时间窗打开键从 `ts` 改为 `tt`。

## Background

- 今日（用户确认）：方案 A — 全局会话状态，不进 Filter strip；典型路径是先 filter 再 `fh`/`fe`。
- 仓库现状：`find_match` / `find_severe`（stream）与 `HighlightScanState::find_next`（file）均按 vim `wrapscan` 环绕；`f` 为 lock 操作符（`p`/`t`/`u`）；时间窗打开为 `t`+`s`。

## Confirmed Facts（仓库）

- Stream 跳转环绕：`alnav/src/app.rs` `find_match` / `find_severe` 使用 `rem_euclid`，循环 `1..=n`。
- File 高亮跳转环绕：`alnav/src/scan.rs` `HighlightScanState::find_next` 在 `done` 时回到首/末 hit。
- 过滤链顺序：`groups` → lock → `time_bound`（`row_passes_filter_parts`）；`filter_active` 决定 stream 是否双写 `matched`。
- `f` pending：`main.rs` `pending_lock` 二键；`e/E` 无命中 flash `NO ERROR`；`n/N` 无命中静默。
- 时间窗：`pending_time` + `'s'` 打开面板；Help `L2_TIME` / catalog 仍写 `t s/u` / `ts`。

## Requirements

### R1 — 跳转不环绕

- `n`/`N`：在当前 `visible`（及 file 的 highlight hit 索引）内向后/向前找下一命中；到边界且无更远命中时停止，光标不动。
- `e`/`E`：同上，针对 severe（E/F/crash）；已有「全无」时 `NO ERROR` 保持。
- 到边界「有命中但已是最后一个/第一个」时：flash `NO MORE`（与 `NO ERROR` 同为英文 soft flash）。

### R2 — 全局视图焦点 `fh` / `fe`

- `App` 持有全局状态：`None | HighlightOnly | SevereOnly`（名称以实现为准）。
- 叠在现有 Filter/Exclude/lock/time **之后** AND 收窄 `visible`；不创建 strip 组；不进入 `yc` 导出。
- `fh`：保留任一 **enabled** highlight 组命中（tag/msg）；无启用组 → flash `NO HIGHLIGHT`，状态不变。
- `fe`：保留 `severe` 行（与 `e`/`E` 同判定）。
- Toggle：再按同键关闭；`fh`↔`fe` 互斥替换。
- 开启/切换/关闭均 `following=false` 并 `rebuild_visible`；Esc resume following **不**清除该状态。
- 计入 `filter_active`（单独开启时也要走 matched/Subset 路径）。
- 键位：复用 `f` operator，二键 `h`/`e`（与现有 `p`/`t`/`u` 并存）；Help/L2/status 同步。
- 无硬门槛「必须先有 chip filter」；无 filter 时对全量收窄亦可。
- status 左侧有短图标/短值提示当前视图焦点（theme token，禁止在 `ui.rs` 硬编码色）。

### R3 — 时间窗 `tt`

- 打开时间窗：`t`+`t`（文件模式）；弃置 `t`+`s`（按后 flash `UNKNOWN`）。
- `t`+`u` 清除不变；live 模式仍硬隐藏 `t`。
- Help / 注释 / 单测中的 `ts` 打开语义全部改为 `tt`。

## Acceptance Criteria

- [ ] AC1：在仅有两处 highlight 命中时，从第二处按 `n` 不再回到第一处；光标不变，出现 `NO MORE`。`N` 在第一处同理。
- [ ] AC2：`e`/`E` 在唯一 severe 行上再按同向不再环绕，出现 `NO MORE`；全无 severe 仍为 `NO ERROR`。
- [ ] AC3：file 模式 highlight 扫描完成后，`n`/`N` 亦不环绕（改 `HighlightScanState::find_next`）。
- [ ] AC4：有 chip filter 后 `fh` 仅显示该过滤结果中的 highlight 命中行；再 `fh` 恢复为仅 chip filter 结果。
- [ ] AC5：`fe` 仅显示 severe；与 `fh` 互斥；Esc resume 不关闭视图焦点。
- [ ] AC6：无启用 highlight 时 `fh` → `NO HIGHLIGHT`，列表不变。
- [ ] AC7：`tt` 打开时间面板；`ts` 不再打开；Help 文案为 `t t/u`。
- [ ] AC8：相关单测更新并通过（含原 wrap 用例改为 no-wrap）。

## Out of Scope

- 相对时间、live 交互时间窗、从光标行派生时间
- 把视图焦点做成 Filter strip chip / 持久化配置 / `yc` 导出
- 搜索/过滤历史；Windows 专门支持
- 改 `n`/`N` 的 active-highlight 语义（仍只跳 active 组；`fh` 用全部 enabled 组）

## Key Decisions

| 决策 | 选择 |
|------|------|
| 视图焦点形态 | A：全局会话状态，不进 strip |
| 与 filter 关系 | AND 叠在现有过滤之后；典型「先 filter 再 fh/fe」 |
| `fh` 匹配范围 | 所有 enabled highlight 组（OR），非仅 active |
| 到边界反馈 | flash `NO MORE` |
| `ts` 兼容 | 不保留；`s` → `UNKNOWN` |

## Open Questions

（无阻塞项）
