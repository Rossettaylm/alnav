# Implement — TUI dialog border + compact picker

## 执行清单

### Step 1 — Modal / 候选壳改圆角全边
- [x] `ui.rs`：`render_modal_shell` 改用 `rounded_block(..., true)`（`popup_block`）
- [x] `ui.rs`：standalone `render_candidate_list(..., bordered=true)` 同步圆角全边；Picker 内 `bordered=false` 避免嵌套双框
- [x] 更新过时注释
- [x] 验证：`cargo build -p aloggrep-tui`

### Step 2 — 相邻 1 格空隙
- [x] 增加 `POPUP_GAP = 1`
- [x] 垂直：`stack_below_rect_gapped`（空间够才留缝，否则 flush）
- [x] 水平：`split_picker_lr_gapped`
- [x] 几何单测更新并绿

### Step 3 — Compact picker + 确认锚点
- [x] `picker_frame_rect(frame, show_preview)`：`false` → 宽 ≈ 全宽/2
- [x] `render_picker` 使用新签名
- [x] `render_confirm_dialog` 接收实际 `picker_area`；`main.rs` 传入同一外框
- [x] 验证：`cargo test -p aloggrep-tui`

### Step 4 — 测试与文档锚点
- [x] modal shell 圆角角断言；strip 仍仅 `─`
- [x] `show_preview=false` 半宽测试
- [x] confirm 锚在 compact 内
- [x] `CLAUDE.md` / `AGENTS.md` UI 指导与实现对齐
- [x] 验证：`cargo test -p aloggrep-tui`（381 passed）

### Step 5 — 收尾
- [ ] 手动：`-f` Filter Picker / `mm` Bookmark / 删除确认 / Input 字段候选
- [ ] commit（用户要求时）/ `task.py finish`

## 验证命令

```bash
cargo build -p aloggrep-tui
cargo test -p aloggrep-tui
# 手动：cargo run -p aloggrep-tui -- -f <log>
```

## 风险文件

- `aloggrep-tui/src/ui.rs` — 主改动
- `aloggrep-tui/src/main.rs` — confirm 传 `picker_area`
- `CLAUDE.md` / `AGENTS.md` — UI 指导同步
