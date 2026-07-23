# TUI nerdfont glyphs and reduced-border panel redesign

## Goal

将 aloggrep-tui 的 chrome 层（Picker + 三条 strip + status bar）从"圆角整圈 Cyan 强边框 + ASCII 前缀"改为"顶/底 box-drawing 分隔 + nerdfont 语义字形"，视觉上减弱强主题色边框，用 nerdfont 字形承担面板分隔与状态语义。

## Requirements

### R1 — Nerdfont 硬依赖

- 全量上 nerdfont 字形，不做运行时 fallback。
- 所有新增字形集中在 `theme.rs` 的 glyphs 常量区块，`ui.rs` 只读不硬编码字形。

### R2 — 范围

- **Picker**：`render_picker` / `render_modal_shell` / `render_candidate_list` / `render_preview` / `render_picker_search_line`。
- **三条 strip**：`render_chip_strip` / `render_exclude_chip_strip` / `render_highlight_chip_strip`。
- **status bar**：`render_status_bar` 的 `status_badge` 调用。
- **不含**：LogList 边框、minimap 轨（`│`/`•`）、help.rs 键位短名。

### R3 — 边框减弱（路径 B）

- Picker / strip / modal 不再用整圈 `BorderType::Rounded` 描边。
- 改为顶 + 底 `─`（box-drawing U+2500）横向分隔条，左右不描边。
- `border_style(active)` 聚焦从 `fg(accent)` 降为 `fg(accent)+DIM`，失焦不变。
- `numbered_title` / `plain_title` 徽标从 `focus_style()` 反色块降为 nerdfont 图标 + accent fg 文字。
- `status_badge` 从反色块降为 nerdfont 图标 + accent/语义 fg 文字（无 bg）。
- `focus_style()` 定义保留（log selection 仍用）。

### R4 — 字形密度（中量，约 20 个语义字形）

| 语义 | 码点 | 字形 |
|---|---|---|
| Manage 前缀 | `\u{f0b7}` |  |
| New 前缀 | `\u{f0fe}` |  |
| Edit 前缀 | `\u{f044}` |  |
| 选中行前缀 | `\u{f0da}` |  |
| Picker 标题 | `\u{f002}` |  |
| LogList 标题 | `\u{f0c5}` |  |
| Filter strip 标题 | `\u{f0b0}` |  |
| Exclude strip 标题 | `\u{f056}` |  |
| Highlight strip 标题 | `\u{f0e0}` |  |
| strip 组启用 | `\u{f192}` |  |
| strip 组禁用 | `\u{f10c}` |  |
| bookmark | `\u{f02e}` |  |
| lock | `\u{f023}` |  |
| following | `\u{f062}` |  |
| visual | `\u{f245}` |  |
| search | `\u{f002}` |  |
| crash | `\u{f071}` |  |
| status 分隔 | `\u{e0bf}` |  |
| Tag chip 图标 | `\u{f02b}` |  |
| Msg chip 图标 | `\u{f075}` |  |
| Pkg chip 图标 | `\u{f187}` |  |
| Pid chip 图标 | `\u{f292}` |  |
| Tid chip 图标 | `\u{f2bd}` |  |
| Level chip 图标 | `\u{f0d0}` |  |
| pill 左端 | `\u{e0b6}` |  |
| pill 右端 | `\u{e0b2}` |  |
| 横向分隔条 | `\u{2500}` | ─ |

### R5 — Pill 改造

- chip pill / exclude pill / highlight pill 用 powerline 端 ``/`` 包裹替代空格填充。
- pill 内字段图标前缀（Tag/Msg/Pkg/Pid/Tid/Level）。
- pill 文本宽度变化需同步 `PILL_GAP` 等宽度计算。

### R6 — 测试策略

- 只修正现有字形断言（`candidate_prefix` / `picker_mode_prefix` 等）。
- 修正因 inner rect 变化（少左右边框）导致的几何断言。
- 不新增字形表测试或渲染快照测试。

## Acceptance Criteria

- [ ] `cargo build -p aloggrep-tui` 编译通过。
- [ ] `cargo test -p aloggrep-tui` 全量通过（含修正后的断言）。
- [ ] Picker 面板：顶/底 `─` 分隔，无左右边框；标题为 nerdfont 图标 + 文字；mode 前缀为 nerdfont 字形。
- [ ] 三条 strip：顶/底 `─` 分隔；numbered_title 为 nerdfont 图标 + 数字 + 文字；状态点为 ``/``。
- [ ] status bar：`FOLLOWING`/`LOCK`/`VISUAL` 等为 nerdfont 图标 + accent fg 文字，无反色块。
- [ ] chip pill：powerline ``/`` 端 + 字段图标前缀。
- [ ] `theme.rs` 新增 glyphs 常量区块，所有字形不散落在 `ui.rs`。
- [ ] `ui.rs` 无新增硬编码 `Color::*` 或字形字面量。
- [ ] `focus_style()` 保留，log selection 行为不变。

## Notes

- Grilling 会话已达成共享理解，7 个决策全部锁定。
- 不动 LogList 边框 / minimap 轨 / help 键位短名（Q2 排除）。
- 横向分隔用 box-drawing `─` 而非 nerdfont 字形（Q5 决策）。
