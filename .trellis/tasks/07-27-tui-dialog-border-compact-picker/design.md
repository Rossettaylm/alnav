# Design — TUI dialog border + compact picker

## 1. 边界与职责

| 层 | 职责 |
|----|------|
| `theme::border_style(true)` | 唯一边框色源（dim accent）；不新增 dialog token |
| `rounded_block` / modal shell | 弹出浮层几何壳（Clear + Rounded 全边 + title） |
| `divider_block` | **仅**主布局 strip（及测试用 legacy `render_input_box`）；本任务不改其语义 |
| `picker_frame_rect`（扩展） | 按 `show_preview` 决定全宽 vs 半宽外框 |
| 布局 gap 常量 | 相邻弹出面 1 格空隙 |

不改过滤/匹配/ingest；纯 UI 几何与渲染。

## 2. Modal / 候选壳

### 2.1 `render_modal_shell`

现状：`divider_block`（TOP|BOTTOM）。

目标：

```text
Clear(area) → rounded_block(plain_title(...), active=true) → Clear(inner) → return inner
```

复用已有 `rounded_block`；`active` 固定 `true`（浮层即焦点）。注释从「2 cols wider than old rounded」改回「全边圆角壳」。

### 2.2 `render_candidate_list`

将内部 `divider_block` 换成与 shell 相同的 `rounded_block(..., true)`（或抽 `popup_block(title)` 一处共用，避免两处漂移）。

Preview 已走 `render_modal_shell`，随 2.1 自动升级。

### 2.3 Strip

`render_chip_strip` / Exclude / Highlight 继续 `divider_block`。现有断言「strip 有 ─、无圆角角」保持有效。

## 3. Gap 几何

新增常量，例如：

```rust
const POPUP_GAP: u16 = 1;
```

### 3.1 垂直栈

`stack_below_rect`（或仅 popup 路径的包装）在 `y = anchor.bottom + POPUP_GAP` 起算，并相应减少可用 `space`。  
调用方：`candidate_popup_rect`、`preview_popup_rect`；确认不影响非 popup 的其它 `stack_below` 用途（若有，则做 `stack_below_rect_gapped` 专用包装）。

### 3.2 Picker 左右

`show_preview == true` 时：

1. `picker_area = picker_frame_rect(frame, preview=true)`（全宽）
2. 在 `picker_area.width` 内扣掉 `POPUP_GAP`，再 `split_picker_lr`；或先 split 再把右栏 `x` 右移、宽减 1。  
   保证左框右缘与右框左缘不相贴（中间 1 空列，无 widget）。

## 4. Compact Picker（无 Preview）

```text
full_w = frame.width - PICKER_FRAME_WIDTH_MARGIN   // 与现逻辑一致
compact_w = (full_w / 2).max(PICKER_LR_MIN_WIDTH) // ≈ 一半；下限防崩
height   = 同现：max(frame*3/4, PICKER_FRAME_MIN_HEIGHT).min(frame.height)
x,y      = 水平/垂直居中（同现 picker_frame_rect 居中算法）
```

API 建议：

```rust
pub fn picker_frame_rect(frame: Rect, show_preview: bool) -> Rect
```

或保留旧签名并新增 `picker_frame_rect_for(frame, show_preview)`，统一所有调用点（`render_picker*`、`render_confirm_dialog`）。

`render_fzf_picker`（或现函数名）必须把**同一** `picker_area` 传给确认框路径；确认框不能再独立算「永远全宽」的 rect。

### 4.1 确认框

```text
picker_area = picker_frame_rect(frame_area, /* 与当前 session 相同的 show_preview */)
area = centered_modal_rect(picker_area, 34.min(...), 5.min(...))
```

`main.rs` 渲染确认时需能拿到 `show_preview`（已有 `PickerRenderData.show_preview`），或让 `render_confirm_dialog` 接收 `picker_area: Rect` / `show_preview: bool`。

推荐签名：

```rust
pub fn render_confirm_dialog(confirm: &ConfirmKind, frame: &mut Frame, picker_area: Rect)
```

由调用方传入与 Picker 相同的外框，避免重复推断。

## 5. 兼容与风险

| 风险 | 处理 |
|------|------|
| 全边框吃掉 2 列内容宽 | 可接受；更新依赖「inner 更宽」的旧注释/测试 |
| 垂直 gap 使栈更高、底部裁切 | `stack_below` 已 clamp；极矮终端可能少露 Preview——与现行为同类 |
| 半宽 + `PICKER_LR_MIN_WIDTH` | 无 Preview 单栏，下限用 `PICKER_LR_MIN_WIDTH` 或略高（如 20）即可 |
| 测试里数 `─` / 角字符 | modal 相关测试改断言圆角角/`│`；strip 测试保持 `─` |

## 6. 回滚

纯渲染变更；`git revert` 即可。无数据迁移、无配置文件格式变更。
