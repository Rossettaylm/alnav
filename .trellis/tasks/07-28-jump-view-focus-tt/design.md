# Design: Jump no-wrap + view focus + tt

## Boundaries

| Layer | Owns |
|-------|------|
| `App` | `view_focus` 状态；`filter_active` / `row_passes_filter_parts` 串联；`find_match` / `find_severe` 不环绕；toggle API |
| `scan::HighlightScanState` | file 模式 `n`/`N` 命中索引不环绕 |
| `main` 键分发 | `f`+`h`/`e`；`t`+`t`（弃 `s`） |
| `help` / `ui` / `theme` | L1/L2/catalog 文案；status 短提示；glyph/style 走 theme |
| `export` | **不**读取 `view_focus` |

不改 `alnav-core`。不改 Filter strip / Group 模型。

## Data model

```rust
pub enum ViewFocus {
    Highlight, // any enabled highlight group
    Severe,    // row.severe / severe cache
}

// App
pub view_focus: Option<ViewFocus>;
```

### Filter pipeline（扩展后）

```
parsed? → groups.matches → lock → time_bound → view_focus
```

- `view_focus == Highlight`：`highlight_groups.groups.iter().any(|g| g.enabled && g.matches_row(tag,msg))`
- `view_focus == Severe`：`row.severe`（file worker 内可用同步 parse 后的字段；与现有 severe 语义一致）
- `filter_active` 增加 `view_focus.is_some()`，保证单独开启时 stream 走 `matched`、file 走 Subset 扫描。

File `rebuild_visible`：向 `FilterPred` 闭包 clone 所需的 `highlight_groups`（或仅 enabled patterns）+ `view_focus`；与现有 lock/time clone 同一模式。

## Key dispatch

### `f` operator（扩展，保留 `pending_lock` 标志名以降低 churn）

| 二键 | 行为 |
|------|------|
| `p`/`t`/`u` | 现有 lock（不变） |
| `h` | `toggle_view_focus(Highlight)` |
| `e` | `toggle_view_focus(Severe)` |
| 其他 | `UNKNOWN` |

互斥：设 Highlight 时清 Severe，反之亦然；同态再按 → `None`。

### `t` operator

| 二键 | 行为 |
|------|------|
| `t` | `open_time_panel()`（原 `s`） |
| `u` | `clear_time_bound()` |
| `s` | `UNKNOWN`（弃置） |

## Jump no-wrap

### Stream `find_match` / `find_severe`

- 将 `for offset in 1..=n` + `rem_euclid` 改为单向有界扫描：
  - `dir > 0`：`cursor+1 .. n`
  - `dir < 0`：`0 .. cursor` 逆序
- 找到则移动并 `following=false`；否则 `false`（调用方决定 flash）。

### File `HighlightScanState::find_next`

- 去掉 `done` 时 wrap 到 `hits[0]` / `last`；越界 → `None`。
- 扫描未完成（`!done`）时现有「不越过已知边界」行为保留。

### Flash 策略（`main`）

| 键 | 无任何命中 | 有命中但已到边界 |
|----|------------|------------------|
| `e`/`E` | `NO ERROR`（现有） | `NO MORE` |
| `n`/`N` | 静默或可保持静默 | `NO MORE` |

实现：`find_*` 返回枚举或由调用方在 `false` 且「存在至少一个命中」时 flash `NO MORE`。推荐轻量：

```rust
enum JumpResult { Moved, NoMore, None }
```

或 `find_* -> bool` + `has_any_*` 辅助。择改动小者。

## Status / Help

- L2_LOCK 增加 `h`/`e` 短提示；L2_TIME：`s`→`t`。
- Catalog `t s/u` → `t t/u`；session 说明同步。
- status：视图焦点用 `status_icon` / `status_icon_value` + 新 glyph（theme 常量）；文案短（如 `HL` / `ERR`）。

## Compatibility / Rollback

- 无配置文件迁移；纯交互变更。
- 回滚：还原 `view_focus` 管线、`find_*`/`find_next` wrap、键位与 Help。
- Spec 更新（Phase 3.3）：`.trellis/spec/alnav/backend/session-filters.md`（`ts`→`tt` + view focus）、`status-help.md`（键位）。

## Trade-offs

| 选择 | 取舍 |
|------|------|
| `fh` = 全部 enabled HL，非 active | 与「只看高亮行」一致；`n/N` 仍跟 active |
| 不进 `yc` | 会话视图，CLI 无法表达；避免假导出 |
| 扩展 `pending_lock` 而非新 pending | 少一个 pending 位；L2 文案从「lock」扩成 f-ops（Help 可写 lock/focus） |
| file Highlight 走 FilterPred 再 parse | 与现有 filter worker 一致；大文件成本等同「又开一层 filter」可接受 |
