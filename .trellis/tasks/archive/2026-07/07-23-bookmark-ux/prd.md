# Bookmark UX Overhaul

## Goal

优化 aloggrep-tui 的 bookmark 交互：日志行视觉突出、聚合面板与书签管理面板拆分、面板内 Enter 改为定位跳转、tip 用行内 icon 表达。

## Background

当前 bookmark 存在三类问题：
1. 被标记的日志行在 LogList 无视觉突出，与普通行无异。
2. 聚合搜索面板（`Space Space`/`5`）里 bookmark 项的 Enter 做 `toggleEnabled`，对 bookmark 无意义（bookmark 没有 enabled 语义）。
3. `mm` 本应开 Manage（注释与 CLAUDE.md 都这么写），代码却调 `open_picker_new`（New 模式）；`MM` 落 `_=>未知` 死键；bookmark 有个 `enabled` 字段但实际无消费价值，是僵尸状态。

## Requirements

### 功能需求

- **F1 — 书签行背景色**：LogList 中被标记为 bookmark 的行，设置极淡黄背景色突出。优先级 `visual > bookmark-bg > cursor-selection`，被 visual/cursor 覆盖时让位。颜色经 theme token 取得，不硬编码 RGB。
- **F2 — 聚合面板与书签面板拆分**：
  - 聚合面板（`Space Space`/`5`）只管 Filter/Highlight/Exclude，移除 Bookmark 段。
  - `mm` 开启**书签专属 Manage 面板**（非聚合），只列 bookmark，无前缀 `[Bookmark]:`。
  - 书签面板不支持 Tab 多选（整面板禁，非 per-item 校验）。
  - 书签面板不支持编辑（Ctrl-E 无效）。
  - 书签面板 Enter = 定位到该 bookmark 所在行 + 关闭面板 + focus LogList。
  - 书签面板 Ctrl-D = 删除选中（二次确认，复用现有 ConfirmKind）。
  - Manage 渲染/键路由按 `session.kind` 分派，为后续其他类型专属面板留扩展点。
- **F3 — 行内 action icon tip**：候选列表每行右侧固定显示 nerdfont action icon：
  - jump 动作（bookmark 行）→ `nf-fa-arrow_right` 类。
  - toggle 动作（filter/highlight/exclude 行）→ `nf-fa-toggle_on`/`nf-fa-toggle_off`（按 enabled 状态）。
  - label 超长时截断给 icon 让位，icon 永远贴右。
  - 不新增 tip 栏。
- **F4 — 删除 `enabled` 字段**：`Bookmark.enabled` 字段、`bookmark_disabled_style`、聚合面板的 Bookmark arm、`toggle_unified_enabled` 的 Bookmark arm、相关测试，全部删除。
- **F5 — minimap 书签标记**：minimap 加 `Bookmark` mark。优先级 `Severe > Bookmark > Highlight > Viewport > Track`。书签单独全量扫描（≤50），保证每个存活书签都显示。
- **F6 — help 文案**：`L2_BOOKMARK` 从 `m:新建` 改为 `m:管理`。

### 非功能需求

- **N1 — 主题一致**：所有新颜色经 theme token 取得（`bookmark_row_bg`、toggle/jump glyph 常量集中在 `theme.rs`）。
- **N2 — 零业务侵入**：不改 aloggrep-core；不影响 export.rs（export 本就不含 bookmark）。
- **N3 — 测试同步**：删 `enabled` 相关测试；新增书签面板 jump/Ctrl-D、行 bg、minimap mark 的测试。

## Constraints

- 不改 `Bookmark` 的 `row_id`/`label` 字段语义。
- 不改 `jump_to_bookmark` 的 `JumpResult` 枚举（只去掉 enabled 短路）。
- 不动 `Space m` 别名（文档漂移，非本任务）。
- `MM` 维持死键现状（不在本期修复）。
- 不引入新依赖。

## Acceptance Criteria

- [ ] AC1：LogList 中 bookmark 行有极淡黄背景；visual 选中段覆盖之；光标行覆盖之；优先级 visual > bookmark-bg > cursor-selection。
- [ ] AC2：聚合面板（`Space Space`/`5`）不含 Bookmark 段。
- [ ] AC3：`mm` 开启书签专属 Manage 面板，只列 bookmark，无 `[Bookmark]:` 前缀。
- [ ] AC4：书签面板内 Tab 无效（不勾选、不报错）；Ctrl-E 无效；Enter 跳转到对应行并 focus LogList；Ctrl-D 删除（二次确认）。
- [ ] AC5：聚合面板各候选行右侧显示 toggle icon（按 enabled 状态）；书签面板各行右侧显示 jump icon；label 超长截断给 icon 让位。
- [ ] AC6：`Bookmark.enabled` 字段及所有消费点已删除；相关测试已删/已改。
- [ ] AC7：minimap 显示书签标记，优先级高于 highlight；每个存活书签都显示（不被采样丢失）。
- [ ] AC8：`cargo test --workspace` 全绿。
- [ ] AC9：`help.rs` `L2_BOOKMARK` 文案为 `m:管理`。

## Out of Scope (YAGNI)

- `MM` 死键修复（落「未知」即可）。
- `Space m` 别名路由修复（文档漂移）。
- 书签面板的搜索历史 / vocab 补全。
- bookmark 动画。
- 书签行 minimap 标记的配置开关。
