# Design: TUI status bar + help panel

## Architecture

以 `help` 模块为键位文案唯一数据源，状态栏与 Help 面板共用结构化条目，避免「短提示字符串」与「详细手册」两套真相。

```
help.rs
  HintEntry { key, label }          // 短/长 label 可同字段或 short/detail
  context_entries(app) -> Vec<HintEntry>   // 当前 L1 或 L2
  catalog_sections(app) -> Vec<Section>    // 完整目录；可标 active
  render helpers -> Span 列表（dim key + normal label）

ui.rs::render_status_bar
  left: icon / icon+value / pending / flash
  right: spans_from(context_entries) + fit by width

ui.rs::render_help_panel
  Active: detailed lines from context_entries
  Catalog: catalog_sections，active section accent

app.rs
  help_open: bool  (+ help_scroll: u16/usize)
  open/close_help；close 不 resume_following

main.rs
  Normal 分发：在允许条件下 Char('?') toggle help
  help 打开时：j/k/Esc/?，其余吞掉或仅滚动键生效
```

## Data contracts

### HintEntry

| Field | Role |
|-------|------|
| `key` | 显示用键名，如 `j/k`、`Esc`、`?`、`^L` |
| `label` | 状态栏短词，如 `move`、`follow`、`help` |
| `detail`（可选） | Help Active/目录用稍长说明；缺省则用 `label` |

状态栏渲染：`Span::dim(key) + " " + Span::normal(label)`，条目之间 `"  "`（双空格或经测量的固定间距），**无** `:`。

宽度截断：在 Span 列表层按字符预算裁切（替换现 `fit_help(&str)`）；预算 `< MIN_HELP_WIDTH` 时整段隐藏。

### Catalog

固定章节顺序（建议）：

1. Navigation（j/k、g/G、Esc follow、wheel…）
2. Leader / Pickers（Space、`;` `/` `` ` ``、mm…）
3. Filter / Exclude / Highlight operators（c/C、strip h/l/dd/di…）
4. Lock / Time / Yank / Bookmark
5. Detail / Visual / Search / hdc extras
6. Help（`?` / Esc close）

Active 节：根据 `context_entries` 的来源（Focus 或 pending 名）在目录中标记；顶部另渲染 Active 块（详细 label）。

hdc：目录与 L1 省略 time 交互、加入 Ctrl-L；与现 `L1_LOGLIST_HDC` 分支一致。

## Status bar left cluster

| State | Render |
|-------|--------|
| cursor | dim `cur/total` |
| highlight hits | glyph(search) + `k/total` 或 `- /total`，无 `[]` |
| following | `GLYPH_FOLLOWING` only，语义色 |
| visual | `GLYPH_VISUAL` only |
| lock | `GLYPH_LOCK` + short value（如 `pid=1` / `tid=2`） |
| time | `GLYPH_TIME` + short bound text |
| file progress | glyph or bare short percent/label |
| pending | dim/warning text `c…` 等（保持键位提示，非单词徽章） |
| flash | accent/warning 非反色文字，英文 |

`status_badge` 可扩展为 `status_icon(glyph, fg)` 与 `status_icon_value(glyph, value, fg)`；禁止再拼 FOLLOWING 等单词。

## Key dispatch

允许打开 Help 当且仅当：

- `help` 未涉及 Insert 编辑路径；
- `picker.is_none()` 且无 time_panel、非 detail、非 highlight_box.editing；
- 全部 `pending_*` / `pending_leader` 为 false；
- `focus` ∈ {LogList, ChipStrip, ExcludeStrip, HighlightStrip}。

Help 打开时事件优先：`Esc`/`?` close；`j`/`k`/`Down`/`Up` 调整 `help_scroll`；不修改 `following`。

## Compatibility

- `/` → Highlight New：不变。
- 无旧 `?` → Highlight 的运行时行为可迁移（源码本无此绑定）。
- `CLAUDE.md` / `AGENTS.md` 键位描述同步，避免文档回归。

## Trade-offs

| Choice | Why |
|--------|-----|
| 结构化 HintEntry 而非纯字符串 | 支撑 dim/normal 双样式与 Help 复用 |
| Help 只读 | 不与 Picker/Leader 抢执行入口 |
| 开关态纯图标 | 更干净；可发现性靠 `? help` + Help 图例 |
| Esc 不 resume | 对齐 Detail，避免「查帮助却跳回底部」 |

## Risks

- 窄终端：图标化后左侧更短，右侧英文 hint 更长 → 依赖截断；AC 要求关键时 L1 含 `? help` 在够宽时可见。
- 无 Nerd Font：已知 YAGNI（与既有任务一致）。
- Flash 英文化面广：需全量 grep `set_flash`，单测若断言中文需同步。
