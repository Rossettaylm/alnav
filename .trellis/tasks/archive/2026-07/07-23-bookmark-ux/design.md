# Design — Bookmark UX Overhaul

## 1. 数据模型变更

### 1.1 `bookmark.rs`

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookmark {
    pub row_id: u64,
    pub label: String,
    // `enabled` 字段删除。
}
```

- `BookmarkList` 不变（`try_add`/`remove_id`/`update_label`/`delete_at`/`display_recent`/`contains_id` 均不涉及 enabled）。
- `jump_to_bookmark` 去掉 `!b.enabled → Filtered` 短路，只按 `row_alive` 判定。
- 测试：`bm()` helper 去掉 `enabled: true`；`try_add_dedup_and_cap` 不变。

### 1.2 `picker.rs`

```rust
pub enum UnifiedKind {
    Filter,
    Highlight,
    Exclude,
    // `Bookmark` 变体删除。
}
```

- `UnifiedKind::tag`/`as_picker_kind` 去 Bookmark arm。
- `UnifiedId`/`UnifiedItem` 不变（`UnifiedItem.enabled` 字段保留，由 Filter/Highlight/Exclude 消费）。
- `PickerKind::Bookmark` **保留**（书签专属面板用）。
- `PickerSession` 不变；书签面板复用 `PickerMode::Manage` + `query` + `selected` + `confirm`，不用 `checked`（Tab 多选禁用）。

## 2. 面板拆分与路由

### 2.1 `mm` 路由

```rust
// main.rs handle_normal_key，pending_m 分支
KeyCode::Char('m') => {
    app.open_picker(crate::picker::PickerKind::Bookmark);
}
```

- `open_picker`（非 `open_picker_new`）开 Manage 模式。
- `MM` 第二键本就落 `_ => app.set_flash("未知")`，维持现状。

### 2.2 聚合面板去 Bookmark

```rust
// main.rs unified_picker_items
// 删除末段：
// for (displayed, bookmark) in app.bookmarks.items.iter().enumerate().rev() { ... }
```

聚合面板只剩 Filter+Highlight+Exclude 三段。

### 2.3 书签专属面板（Manage 按 kind 分派）

`picker_render_data`（`main.rs:639`）的 `PickerMode::Manage` 分支改为按 `session.kind` 分派：

```rust
PickerMode::Manage => {
    let (all, empty_msg) = match session.kind {
        PickerKind::Unified => (unified_picker_items(app), "无项目"),
        PickerKind::Bookmark => (bookmark_picker_items(app), "无书签"),
        _ => (unified_picker_items(app), "无项目"), // 其他 kind 不走 Manage
    };
    // ... 原有 visible/styles/checked 逻辑不变
}
```

新增 `bookmark_picker_items(app) -> Vec<UnifiedItem>`：

```rust
fn bookmark_picker_items(app: &App) -> Vec<crate::picker::UnifiedItem> {
    use crate::picker::{UnifiedId, UnifiedItem, UnifiedKind};
    // 新书签面板不复用 UnifiedKind::Bookmark（已删），用 UnifiedId.source_index 指向
    // app.bookmarks.items 下标。kind 用一个固定标记——但 UnifiedKind 已无 Bookmark，
    // 所以书签面板的 item.id.kind 需要新枚举值，或者书签面板不走 UnifiedId 体系。
}
```

**决策**：书签面板不走 `UnifiedId` 体系（它本来是为聚合面板多 kind 共存设计的）。书签面板用独立的 `Vec<Bookmark>` 投影，`session.selected` 直接索引 `app.bookmarks.items`。`unified_visible_ids`/`unified_selected_id` 在 `session.kind == Bookmark` 时返回空（书签面板不进聚合路径）。

具体：新增 `fn bookmark_visible_indices(app) -> Vec<usize>`（按 `session.query` 过滤 `app.bookmarks.items` 的 label，返回下标），`fn bookmark_selected_index(app) -> Option<usize>`。

### 2.4 键路由（`handle_picker_key` Manage 分支）

`handle_picker_key` 的 Manage 分支（`main.rs:1176`）改为按 `session.kind` 分派：

```rust
if matches!(mode, PickerMode::Manage) {
    match session.kind {
        PickerKind::Unified => { /* 原有聚合逻辑，但 bookmark 相关 arm 删 */ }
        PickerKind::Bookmark => { /* 书签专属键路由 */ }
        _ => { /* 其他 kind 不该进 Manage，no-op */ }
    }
    return;
}
```

书签面板键路由：
- `Up`/`Down`/`Backspace`/`Char` → 复用 query/selected 操作（与聚合同构）。
- `Tab` → no-op（禁多选）。
- `Ctrl-E` → no-op 或 flash「书签不支持编辑」。
- `Ctrl-D` → `request_delete_many`，但书签面板单选删除走 `delete_bookmark_at(selected)`。Confirm 复用 `ConfirmKind::DeleteMany`。
- `Enter` → `jump_to_bookmark(row_id)`，按 `JumpResult` flash，成功后 `close_picker` + `focus_loglist`。

### 2.5 `submit_bookmark_picker` 退化

`submit_bookmark_picker`（`main.rs:1109`）整体删除——书签面板 Enter 在 Manage 分支内直接处理，不再走 New/Edit submit 路径。`PickerKind::Bookmark` 的 New/Edit 渲染分支（`main.rs:803`、`1461`）删。

## 3. 渲染变更

### 3.1 LogList 行背景（`ui.rs` render_log_list）

```rust
// 优先级：visual > bookmark-bg > cursor-selection
if let Some((lo, hi)) = selection {
    if abs_i >= lo && abs_i <= hi {
        item = item.style(theme::log_visual_style());
    } else if is_bookmark_row {
        item = item.style(theme::bookmark_row_style());
    }
} else if is_bookmark_row {
    item = item.style(theme::bookmark_row_style());
} else if active && abs_i == app.cursor {
    item = item.style(theme::log_selection_style());
}
```

- `is_bookmark_row`：需要判断 `app.bookmarks.contains_id(row.row_id)`。为避免每帧 O(n*50)，在 `App` 加一个 `bookmark_row_ids: HashSet<u64>` 缓存，`try_add`/`remove_id`/`clear` 时同步维护。`render_log_list` 查 HashSet O(1)。

### 3.2 `theme.rs` 新增

```rust
pub const GLYPH_ACTION_JUMP: &str = "\u{f061}";   // nf-fa-arrow_right
pub const GLYPH_ACTION_TOGGLE_ON: &str = "\u{f205}";  // nf-fa-toggle_on
pub const GLYPH_ACTION_TOGGLE_OFF: &str = "\u{f204}"; // nf-fa-toggle_off

pub fn bookmark_row_style() -> Style {
    Style::default().bg(t().bookmark_row_bg)
}

// UiTokens 新增字段：
pub bookmark_row_bg: Color,  // 极淡黄，builtin = warning() 降饱和后的色值
```

- `bookmark_disabled_style` 删除。
- theme.toml 解析新增 `bookmark_row_bg` 字段。

### 3.3 行内 action icon（`ui.rs` render_candidate_list）

`render_candidate_list` 签名扩展，接 `action: ActionKind`（`None`/`Jump`/`Toggle{enabled}`）：

```rust
pub enum ActionKind {
    None,
    Jump,
    Toggle { enabled: bool },
}

pub fn render_candidate_list(
    title: &str,
    labels: &[String],
    styles: &[Style],
    checked: &[bool],
    selected: usize,
    empty_msg: &str,
    query: &str,
    actions: &[ActionKind],  // 新增，与 labels 等长
    frame: &mut Frame,
    area: Rect,
)
```

- `candidate_label_spans` 改为接收 `action` 和 `area_width`：label 可用宽 = `area_width - prefix(2) - icon(2) - padding(1)`，超长用 `fit_label` 截断，末尾拼 icon span。
- icon 颜色：jump = `accent()`，toggle_on = `success()`，toggle_off = `muted()`。
- padding 用 `Span::raw(" ".repeat(pad))` 补白，让 icon 贴右。
- `picker_render_data` 按选中行 kind 填 `actions`：聚合面板 Filter/Highlight/Exclude 行 = `Toggle{enabled}`，书签面板行 = `Jump`。

### 3.4 `render_bookmark_strip` 去 disabled 分支

```rust
let (mark, style) = if alive {
    ("★", theme::bookmark_label_style())
} else {
    ("☆", theme::bookmark_stale_style())
};
```

### 3.5 minimap 书签标记

`MinimapMark` 新增 `Bookmark` 变体，优先级 `Severe > Bookmark > Highlight > Viewport > Track`。

`build_minimap_marks`：在采样循环后，单独扫 `app.bookmarks.items`，对每个 `row_alive` 的书签，定位其在 `visible` 中的位置（若存在），打 `Bookmark` mark。上限 50，O(50·visible_find)。`visible` 查找用 `app.bookmark_row_ids` HashSet + `visible` 线性扫一次建 `row_id → visible_idx` 映射（每帧一次，O(visible)）。

minimap 书签字形用 `•`，样式 `Style::default().fg(t().bookmark_row_bg)`（与行 bg 同色系，视觉关联）。

## 4. app.rs 变更汇总

- `bookmark_add_current`：`Bookmark { ..., enabled: true }` → `Bookmark { ... }`。
- `jump_to_bookmark`：删 `!b.enabled` 短路。
- `toggle_unified_enabled`：删 `UnifiedKind::Bookmark` arm + 测试 `toggle_unified_enabled_bookmark`。
- `delete_unified_at`：删 `UnifiedKind::Bookmark` arm。
- 新增 `bookmark_row_ids: HashSet<u64>` 字段 + 在 `try_add`/`remove_id`/`clear` 同步维护（实际维护点在 `BookmarkList` 方法里，或 App 包装方法）。
- 新增 `bookmark_visible_indices`/`bookmark_selected_index` 辅助（或放 main.rs）。

## 5. main.rs 变更汇总

- `mm` → `open_picker(Bookmark)`。
- `unified_picker_items` 删 bookmark 段。
- `picker_render_data` Manage 分支按 `session.kind` 分派；书签面板用 `bookmark_picker_items`。
- `submit_bookmark_picker` 删除。
- `handle_picker_key` Manage 分支按 `session.kind` 分派；书签面板 Enter=jump、Tab=no-op、Ctrl-E=no-op、Ctrl-D=delete。
- `handle_picker_key` 的 `PickerKind::Bookmark` 非分支删除（New/Edit 不存在）。
- `unified_visible_ids`/`unified_selected_id` 在 `session.kind == Bookmark` 时返回空。

## 6. theme.rs 变更汇总

- 删 `bookmark_disabled_style`。
- 新增 `GLYPH_ACTION_JUMP`/`GLYPH_ACTION_TOGGLE_ON`/`GLYPH_ACTION_TOGGLE_OFF`。
- 新增 `bookmark_row_bg` token + `bookmark_row_style()`。
- `UiTokens` 加 `bookmark_row_bg`，theme.toml 解析加该字段。

## 7. help.rs 变更

- `L2_BOOKMARK` = `"a:新增 d:删除 m:管理 Esc:取消"`。

## 8. 测试策略

- 删：`toggle_unified_enabled_bookmark`、`mm` 开 New 模式的断言（改为 Manage）。
- 改：`bm()` helper 去 `enabled`；`mm` 测试断言 `PickerMode::Manage`。
- 新增：
  - 书签面板 Enter=jump（命中/Evicted/Filtered 三态）。
  - 书签面板 Tab 无效。
  - 书签面板 Ctrl-D 删除。
  - LogList bookmark 行 bg（visual 覆盖/cursor 覆盖/优先级）。
  - minimap Bookmark mark 存在且优先级正确。
  - 聚合面板不含 Bookmark item。

## 9. 回退

- 代码改动全部在本仓 `aloggrep-tui/`，git revert 即回退。
- 无仓外资源。
