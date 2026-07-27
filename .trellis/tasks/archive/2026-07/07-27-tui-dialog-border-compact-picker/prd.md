# TUI popup dialog rounded borders + compact picker

## Goal

让 TUI 所有弹出浮层带上与主题一致的单层圆角四边框，并在无 Preview 时把 Picker 宽度收成约一半，避免「空旷大框」；相邻浮层用 1 格空隙避免双线贴边。

## Background / Confirmed Facts

- 弹出壳统一入口：[`aloggrep-tui/src/ui.rs`](aloggrep-tui/src/ui.rs) `render_modal_shell`（Input / Search / Time / Detail·Pretty / Confirm / Picker 左壳 / Preview）。
- 候选列表 [`render_candidate_list`](aloggrep-tui/src/ui.rs) 目前直接用 `divider_block`（仅上下 `─`）。
- 主布局 Filter/Exclude/Highlight strip 也用 `divider_block`（Q3 弱化边框）；**本任务不改 strip**。
- 已有 `rounded_block` + `theme::border_style(active)`（聚焦 = accent + DIM）。
- Picker 外框：`picker_frame_rect` ≈ 宽 `frame-4`、高 `max(75%, 10)`；`show_preview=false` 时左栏吞满整框（Bookmark / Unified / `picker_preview_enabled=false` 等）。
- 确认框：`render_confirm_dialog` 相对 `picker_frame_rect(frame_area)` 居中；缩宽后必须改锚到**实际** Picker 外框。
- 决策来源：grilling 共识（B / 1+2 / 接缝 B / 缩宽 A / 边框色 A / 确认锚点 A）。

## Requirements

1. **弹出壳四边圆角描边**：所有经 `render_modal_shell` 的浮层改为 `BorderType::Rounded` + 全边框 + `theme::border_style(true)`（dim accent）。单层，非双描边。
2. **候选列表同壳**：`render_candidate_list` 使用与 modal shell 相同的圆角四边框样式（不再用 divider）。
3. **相邻空隙**：Picker 左右栏之间、Input/Search 垂直栈（模态 → 候选 → Preview）之间留 **1 列/行**空隙，避免 `││` / 双横线贴边。
4. **无 Preview 压缩宽**：`show_preview == false` 时，Picker 外框宽度 ≈ 有 Preview 时全宽的 **50%**，高度规则不变（仍 ≈75%），水平居中。有 Preview 时外框尺寸保持现状（再扣左右 1 列间隙做分栏）。
5. **确认框锚点**：删除确认相对**当前实际** Picker 外框居中；确认框自身尺寸逻辑不变（约 34×5）。
6. **Strip 不动**：Filter / Exclude / Highlight / Log 主布局边框保持现有 divider/既有行为。
7. **无新配置/token**：不新增 `theme.toml` dialog 色、不新增 `config.toml` 压缩比旋钮；比例与 gap 用代码常量。

## Acceptance Criteria

- [x] Input / Search / Time / Detail·Pretty / Confirm / Picker 左壳 / Preview 可见圆角四边框，色为 dim accent（`border_style(true)`）
- [x] 字段/历史候选 popup（`render_candidate_list` bordered）同样为圆角四边框；Picker 内列表无嵌套壳
- [x] Filter/Exclude/Highlight strip 仍为上下 divider（非本任务圆角全框）
- [x] Picker 有 Preview：左右栏之间有 1 列空隙（`split_picker_lr_gapped`）
- [x] Input/Search 栈：模态与下方候选/Preview 之间有 1 行空隙（`stack_below_rect_gapped`）
- [x] Bookmark / ActionList 等无 Preview：Picker 宽度约为全宽框的一半且水平居中，高度仍约 75%
- [x] 有 Preview 的 Filter/Highlight/Exclude Manage：外框宽度与改前同级（非半宽）
- [x] 无 Preview 时打开删除确认：确认框相对窄 Picker 居中
- [x] 相关 `ui` 几何/渲染单测更新并通过；`cargo test -p aloggrep-tui` 全绿（381）

## Out of Scope

- 双层/外描边（grilling 否决 A/C）
- 主布局 strip 边框回迁为四边圆角
- 无 Preview 时缩高度或按候选数自适应高度
- 确认框额外缩放
- 新 theme token / config 旋钮
- Windows 专门适配；日志色 `logcolor` 变更

## Open Questions

（无阻塞项）
