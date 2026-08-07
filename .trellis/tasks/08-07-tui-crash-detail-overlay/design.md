# Design: TUI 崩溃/ANR 结构化详情浮层

## 落点

`app.detail == DetailView::Pretty` 分支的渲染逻辑（`ui.rs::detail_pretty_lines` / `detail_content_lines`），不新增 `DetailView` 变体、不新增按键、不新增 `App` 状态字段。`detail_content_lines(app: &App, inner_width)` 已经持有 `&App`，可以直接访问 `app.store.row_at_source(...)` 做 File 模式续行扫描，不用改函数签名。

## 数据流

```
P 键 (main.rs, 已有 toggle_detail_pretty) 不变
  → app.detail = DetailView::Pretty（状态机不变）
渲染时 (ui.rs::detail_content_lines, DetailView::Pretty 分支):
  1. crash_context_for_row(app, row) -> Option<CrashInfo>
       a. CrashDetector::detect(&row.as_log_entry())
          None  → 返回 None（调用方走原 JSON/raw 判定链）
          Some(ty) →
            - store.is_file(): 从当前行的**物理行号**（`source_idx_for_visible(app.cursor)`）开始，逐行 `store.row_at_source(idx, false)`，按 `parsed == false` 连续向下拼接（最多 500 行，命中即停），拼出 merged msg String
            - store 非 file（Stream）: merged msg = row.msg.clone()（单行）
            - 用 row 的 timestamp/pid/tid/tag + merged msg 构造一个临时
              LogEntry，调用 CrashDetector::parse_crash(&entry, ty) 得到
              CrashInfo（owned，merged String 生命周期只需覆盖这一次调用）
  2. Some(info) → render_crash_detail_lines(&info, truncated: bool, width)
     None       → 现有 pretty_json_for_row 判定链（不变）
```

## 新增函数（均落在 `ui.rs`，`CrashDetector`/`CrashInfo` 来自 `alnav-core::crash`，无需改 core）

- `fn crash_context_for_row(app: &App, row: &EntryRow) -> Option<(CrashInfo, bool)>`
  （`bool` = 是否命中 500 行截断，供渲染提示用）
  - `CrashDetector` 用 `OnceLock` 单例（跟 `model.rs::crash_detector()` 同款写法，避免每帧重建正则）。
  - **关键点（易踩坑）**：`app.row_at(vis_i)` 是按 **`Visible`（过滤后）索引** 取行，续行在文本过滤下大概率不在 `visible` 里（未解析行 tag/msg 是原始整行，很难匹配用户的过滤条件），如果用 `row_at(cursor_vis_i + 1)` 往下走会跳过被过滤掉的物理行，扫描逻辑是错的。
  - 正确做法：先用 `app.source_idx_for_visible(app.cursor)` 拿到当前行的**物理行号**（不是 visible 索引），后续用 `app.store.row_at_source(idx, false)`（`RowStore::row_at_source`，File 分支本就忽略 `filter_active`、直接按物理行号取）逐行 `+1` 前进，绕开 `Visible` 过滤层，保证拿到的是文件里真正相邻的物理行。
  - 检查每行 `.parsed`，`false` 则拼接、`true` 或越界则停止；上限 500 次迭代。
- `fn render_crash_detail_lines(info: &CrashInfo, truncated: bool, width: usize) -> Vec<Line<'static>>`
  - 头部：类型徽标（`theme::GLYPH_CRASH` + `theme::warning()`）+ headline。
  - `exception`（若 `Some`）单独一行。
  - `pid`/`tid`/`tag`/`timestamp` 一行，`theme::muted()`。
  - `stack`：逐行 `Line`，为空时按 store 类型显示占位——
    - Stream: "stream 模式无堆栈"
    - File 且确实为空: "无堆栈"（理论上少见，签名行本身可能就是唯一行）
  - `truncated == true`: 末尾追加一行 "…(已截断)"（`theme::preview_placeholder_style()`）。

## 判定优先级（写入 `pretty_json_for_row` 调用点之前）

```rust
DetailView::Pretty => {
    if let Some(row) = app.current_row() {
        if let Some((info, truncated)) = crash_context_for_row(app, &row) {
            return render_crash_detail_lines(&info, truncated, width);
        }
    }
    detail_pretty_lines(app.current_row().as_deref(), inner_width) // 原逻辑不变
}
```

## 性能

- `CrashDetector::detect` 只在当前光标行跑一次（3 个正则，O(msg 长度)），每次渲染都重算无压力（跟现有 `pretty_json_for_row` 每帧重算 JSON 解析的成本量级一致）。
- File 模式续行扫描上限 500 次 `row_at` 惰性解析调用，单次开销与 minimap 扫描（预算约 4000/帧）同量级，不需要 async/gen-cancel。**不复用候选面板的 async worker**——这个扫描只在 `P` 键触发的当前光标行上做，不是全量/大词表扫描，同步做完全在预算内。

## 边界

- 不改 `alnav-core::crash`（`CrashInfo`/`CrashDetector` 原样复用）。
- 不改 `DetailView` 枚举、不改 `toggle_detail_pretty`/`toggle_detail`。
- File 模式 `row_at` 惰性解析已有实现（mmap 场景），无需新增缓存。
