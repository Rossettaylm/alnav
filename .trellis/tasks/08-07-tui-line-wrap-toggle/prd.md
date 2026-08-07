# TUI 单行/多行折叠切换

## Goal

`w` 键在 LogList 语境下切换日志区展示密度：默认多行 wrap（现状） vs 单行截断折叠（超宽用"…"截断），会话级开关、不持久化到 `config.toml`。

## Background

- 现状渲染函数 `ui.rs::render_entry_lines`（多行，贪婪按空白断行/超宽硬切）是 `render_log_list` 唯一调用路径（`ui.rs:1149`）。
- Preview 面板已有等价的单行截断渲染 `ui.rs::render_entry_line_single`（无行号前缀、超宽 "…" 截断），折叠模式直接复用其截断算法，只是要给 LogList 补上行号前缀（Preview 没有行号列）。
- 全局会话开关，不是逐行展开/折叠；折叠模式定位是"临时快速扫读"，不是持久偏好，重启回到默认多行。
- 折叠模式下若高亮/搜索命中恰好落在被截断的部分，**接受视觉丢失**，不做"命中优先居中裁剪"的特殊逻辑——真要看清楚就切回多行或走 Fields/Pretty 浮层看全文。

## Requirements

### R1 — 按键与状态

- `keymap.rs` 新增 `ActionId::LogListWrapToggle`：`context: KeyContext::LogList`、`toml_key: "wrap_toggle"`、`default: Binding::parse_str("w")`、`kind: ActionKind::Leaf`。
- `App` 新增会话级 `bool` 字段（如 `collapsed_view: bool`，默认 `false`），`main.rs` 分发时原地翻转，不影响 `following`/`cursor`/`list_offset` 等其他状态。

### R2 — 渲染分支

- `render_log_list`（`ui.rs:1149` 调用点）按 `app.collapsed_view` 二选一：
  - `false`（默认）：现状 `render_entry_lines`（多行）。
  - `true`：新函数（如 `render_entry_line_collapsed`，参照 `render_entry_line_single` 的截断算法 + 补上行号前缀列，跟 `render_entry_lines` 的 header 拼装方式对齐，只是不做多行 wrap、超宽直接 "…" 截断）。
- `ListItem` 高度因此从"多行"变"单行"，翻页/滚动逻辑（`PAGE_SIZE`、`move_cursor_manual`）不需要感知，因为它们本就不关心 item 内部行数（见既有设计原则）。

### R3 — 持久化边界

- 不写入 `config.toml`，不新增配置项；重启会话恢复默认多行。

### R4 — 明确不做

- 不做"命中位置优先裁剪"，折叠模式下超宽 msg 一律从头截断加 "…"。
- 不做逐行展开/折叠（不是 tree-view 式交互），是全局显示模式二选一。

## Acceptance Criteria

- [ ] LogList 聚焦时按 `w`：多行 ↔ 单行折叠 来回切换，渲染立即生效
- [ ] 折叠模式下超宽 msg 显示"…"截断，行号/时间戳/level/tag 前缀保持不变
- [ ] 折叠模式下 `j`/`k`/`Shift+J`/`Shift+K`/翻页/鼠标滚轮行为不受影响（一行 = 一个可滚动单位）
- [ ] 折叠模式下高亮/搜索命中若被截断，不 panic、不做特殊裁剪（按 R4 验收，不视为 bug）
- [ ] 重启会话（新进程）默认回到多行模式
- [ ] Help 面板能查到 `w` 键提示
- [ ] `cargo test -p alnav` 全绿

## Notes

- 轻量任务，PRD-only；核心是复用 `render_entry_line_single` 的截断算法 + 补行号前缀，不需要 `design.md`/`implement.md`。
