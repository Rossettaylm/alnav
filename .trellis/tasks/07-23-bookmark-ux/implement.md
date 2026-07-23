# Implement — Bookmark UX Overhaul

## 执行清单

### Step 1 — 数据模型：删 `enabled` 字段
- [ ] `bookmark.rs`：`Bookmark` 删 `enabled`；测试 `bm()` helper 去 `enabled: true`。
- [ ] `app.rs`：`bookmark_add_current` 去 `enabled: true`；`jump_to_bookmark` 删 `!b.enabled` 短路。
- [ ] `app.rs`：`toggle_unified_enabled` 删 `UnifiedKind::Bookmark` arm + 测试 `toggle_unified_enabled_bookmark`。
- [ ] `app.rs`：`delete_unified_at` 删 `UnifiedKind::Bookmark` arm。
- [ ] 验证：`cargo build -p aloggrep-tui`（编译错误应集中在 picker.rs UnifiedKind::Bookmark 与 ui.rs bookmark_disabled_style 的残留引用）。

### Step 2 — picker 枚举清理
- [ ] `picker.rs`：`UnifiedKind` 删 `Bookmark`；`tag()`/`as_picker_kind()` 去 Bookmark arm。
- [ ] `main.rs`：`unified_picker_items` 删 bookmark 段（843-887 行那段 for 循环）。
- [ ] `main.rs`：`picker_render_data` 的 Manage preview 分支 `UnifiedKind::Bookmark => {}` 删。
- [ ] `main.rs`：`handle_picker_key` Ctrl-E 的 `UnifiedKind::Bookmark` arm（1248-1253）删。
- [ ] 验证：`cargo build -p aloggrep-tui` 全绿。

### Step 3 — theme.rs 新增 token + 删 disabled
- [ ] 删 `bookmark_disabled_style`。
- [ ] 新增 `GLYPH_ACTION_JUMP`/`GLYPH_ACTION_TOGGLE_ON`/`GLYPH_ACTION_TOGGLE_OFF`。
- [ ] `UiTokens` 加 `bookmark_row_bg: Color`（builtin 极淡黄）。
- [ ] 新增 `bookmark_row_style()`。
- [ ] theme.toml 解析加 `bookmark_row_bg`。
- [ ] 验证：`cargo build -p aloggrep-tui`。

### Step 4 — LogList 行背景
- [ ] `app.rs`：新增 `bookmark_row_ids: HashSet<u64>` 字段；在 `bookmark_add_current`/`bookmark_remove_current`/`BookmarkList` 删除方法里同步维护。
- [ ] `ui.rs` render_log_list：item style 优先级改 `visual > bookmark-bg > cursor-selection`。
- [ ] `ui.rs` render_bookmark_strip：删 `!bm.enabled` 分支，只留 alive/stale。
- [ ] 验证：`cargo test -p aloggrep-tui bookmark` 通过（改现有测试）。

### Step 5 — 书签专属面板（路由 + 渲染）
- [ ] `main.rs`：`mm` → `app.open_picker(crate::picker::PickerKind::Bookmark)`。
- [ ] `main.rs`：新增 `bookmark_picker_items(app) -> Vec<UnifiedItem>`（或直接用 `Vec<&Bookmark>` 投影）。
- [ ] `main.rs`：`picker_render_data` Manage 分支按 `session.kind` 分派；书签面板填 labels/styles，`show_preview=false`，empty_msg="无书签"。
- [ ] `main.rs`：`handle_picker_key` Manage 分支按 `session.kind` 分派；书签面板 Tab=no-op、Ctrl-E=no-op、Ctrl-D=delete、Enter=jump。
- [ ] `main.rs`：`submit_bookmark_picker` 删除；`PickerKind::Bookmark` 的 New/Edit 渲染分支（803-805、1461-1468）删。
- [ ] `main.rs`：`unified_visible_ids`/`unified_selected_id` 在 `session.kind==Bookmark` 时返回空。
- [ ] 验证：`cargo build -p aloggrep-tui`；手动测 `mm` 开 Manage、Enter 跳转、Ctrl-D 删除。

### Step 6 — 行内 action icon
- [ ] `ui.rs`：新增 `ActionKind` 枚举。
- [ ] `ui.rs`：`render_candidate_list` 签名加 `actions: &[ActionKind]`；`candidate_label_spans` 接 `action` + `area_width`，label 截断 + 右侧 icon + padding 补白。
- [ ] `main.rs`：`picker_render_data` 填 `actions`：聚合 Filter/Highlight/Exclude=Toggle{enabled}，书签=Jump。
- [ ] 验证：`cargo build`；手动测 icon 贴右、label 截断让位。

### Step 7 — minimap 书签标记
- [ ] `ui.rs`：`MinimapMark` 加 `Bookmark` 变体。
- [ ] `ui.rs`：`build_minimap_marks` 采样循环后单独扫书签（≤50），打 Bookmark mark，优先级 `Severe > Bookmark > Highlight > Viewport > Track`。
- [ ] `ui.rs`：`render_minimap` 加 Bookmark 分支（字形 `•`，色 = `bookmark_row_bg`）。
- [ ] 验证：`cargo build`；手动测书签标记可见。

### Step 8 — help 文案 + 测试
- [ ] `help.rs`：`L2_BOOKMARK` 改 `a:新增 d:删除 m:管理 Esc:取消`。
- [ ] 删/改现有测试：`mm` 断言 Manage、`bm()` 去 enabled、聚合面板不含 Bookmark。
- [ ] 新增测试：书签面板 Enter=jump 三态、Tab 无效、Ctrl-D 删除；LogList 行 bg 优先级；minimap Bookmark mark。
- [ ] 验证：`cargo test --workspace` 全绿。

### Step 9 — 收尾
- [ ] `.trellis/spec/` 更新（如有相关 spec）。
- [ ] commit。
- [ ] `task.py finish` + `task.py archive`。

## 验证命令

```bash
cargo build --workspace                    # 全量编译
cargo test --workspace                     # 全量测试
cargo test -p aloggrep-tui bookmark        # 书签相关测试
cargo test -p aloggrep-tui picker          # picker 测试
cargo test -p aloggrep-tui app             # app 测试
# 手动：cargo run -p aloggrep-tui -- -f <log>
#   ma 标记 → 看行 bg 黄
#   mm → 书签面板 Manage
#   Enter → 跳转 LogList
#   Ctrl-D → 删除确认
#   Tab → 无反应
#   Space Space → 聚合面板无 Bookmark
#   拉 terminal 宽度 → icon 贴右、label 截断
```

## Review Gates

- Gate-1（Step 2 后）：编译全绿，`UnifiedKind` 无 Bookmark。
- Gate-2（Step 5 后）：`mm` 开书签 Manage，Enter 跳转，Ctrl-D 删除。
- Gate-3（Step 6 后）：icon 贴右，label 截断让位。
- Gate-4（Step 8 后）：`cargo test --workspace` 全绿。

## Rollback

- `git revert` 本仓改动即可。
- 无仓外资源。
