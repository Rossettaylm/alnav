# Implement: TUI nerdfont glyphs and reduced-border panel redesign

## 执行顺序

### 阶段 1 — `theme.rs` 字形与样式基础设施

1. **新增 glyphs 常量区块**（26 个 `pub const`）置于 `theme.rs` 顶部（`UiTokens` struct 之前）。
2. **新增 `field_icon(field: ChipField) -> &'static str`** 辅助函数，映射 ChipField → GLYPH_FIELD_*。
3. **改 `border_style`**：聚焦分支加 `Modifier::DIM`。
4. **改 `numbered_title`**：去 `focus_style()`，徽标改为 `Span::styled(format!("{glyph} {number}"), accent fg + BOLD)`；需要新增参数接收字形，或内部按 number 硬编码映射（1→Filter 字形、2→Exclude、3→Highlight、5→Input）——倾向后者，调用方不改签名。
5. **改 `plain_title`**：加字形参数 `plain_title(glyph, label, active)`。
6. **改 `picker_mode_prefix`**：Manage/New/Edit 换 `GLYPH_MODE_*`。
7. **改 `candidate_prefix`**：`▌ ` → `GLYPH_CARET_SEL` + ` `。
8. **改 `status_badge`**：签名 `status_badge(glyph: &str, label: &str, fg: Color)`，返回无 bg 的 `Span::styled("{glyph} {label}", fg + BOLD)`。
9. **改 `chip_pill_style`**：返回 `(text, body)` 改为返回 3-Span 结构——**签名变更**。改为 `chip_pill_spans(field, value, disabled) -> Vec<Span<'static>>`。同理 `exclude_pill_spans`、`highlight_pill_spans`。

**验证点**：`cargo build -p aloggrep-tui` 编译通过（此时 ui.rs 调用点会断，预期失败）。

### 阶段 2 — `ui.rs` block 与调用点改造

10. **新增 `divider_block(title, active)`**：`Block::new().borders(Borders::TOP | Borders::BOTTOM).border_type(BorderType::Plain).border_style(theme::border_style(active)).title(title)`。
11. **改 `render_modal_shell`**：`rounded_block` → `divider_block`。
12. **改 `render_chip_strip` / `render_exclude_chip_strip` / `render_highlight_chip_strip`**：`rounded_block` → `divider_block`；`numbered_title` 调用不动（字形在 theme 内部映射）。
13. **改 `render_candidate_list`**：`rounded_block` → `divider_block`。
14. **改 `render_preview`**：`render_modal_shell` 已改，内部无需再改。
15. **改 `render_input_box`**（legacy test helper）：`rounded_block` → `divider_block`。

**验证点**：`cargo build -p aloggrep-tui` 编译通过。

### 阶段 3 — Pill Span 改造

16. **改 `committed_chip_spans`**：调用 `chip_pill_spans` / `exclude_pill_spans`，push Vec<Span> 替代单个 Span。
17. **改 `input_content_spans`**：draft_field 图标前缀 `format!("{}:", field.keyword())` → `format!("{} {}:", GLYPH_FIELD_*, field.keyword())`。
18. **改 strip lines 渲染**（`filter_strip_lines` / `exclude_strip_lines` / `highlight_strip_lines`）：状态点 `●`/`○` → `GLYPH_GROUP_ON`/`GLYPH_GROUP_OFF`。

**验证点**：`cargo build -p aloggrep-tui` 编译通过。

### 阶段 4 — Status Bar 改造

19. **改 `render_status_bar`**：所有 `theme::status_badge(label, bg)` 调用改为 `theme::status_badge(glyph, label, fg)`：
    - FOLLOWING → `(GLYPH_FOLLOWING, "FOLLOWING", success())`
    - LOCK → `(GLYPH_LOCK, &lock, lock())`
    - VISUAL → `(GLYPH_TITLE_LOG, "VISUAL", accent())`（或合适字形）
    - pending `c…/C…/f…/m…/y…/d…` → `("", "c…", warning())`（无字形，纯文字 fg）
    - flash toast → `("", msg, accent/warning)`

**验证点**：`cargo build -p aloggrep-tui` 编译通过。

### 阶段 5 — 测试修正

20. **跑 `cargo test -p aloggrep-tui`**，收集所有失败：
    - `theme.rs:638` `candidate_prefix()` 断言 `▌ ` → 改为新字形。
    - `theme.rs:640-641` `picker_mode_prefix` 断言 `> `/`＋ ` → 改为新字形。
    - 几何测试（`split_picker_lr` / `picker_left_stack` 等）若 inner rect 变化导致断言失败，按新值修正。
    - `render_status_bar` 测试 `content.contains("FOLLOWING")` 仍应通过（文字保留）。
21. **跑 `cargo test --workspace`** 确认 aloggrep-core 无回归。

**验证点**：`cargo test -p aloggrep-tui` 全量通过。

### 阶段 6 — 冒烟验证

22. **`cargo run -p aloggrep-tui -- -f <测试日志>`**（需真实 TTY，若 tmux 内可跑则跑，否则跳过冒烟，依赖测试）。
23. 视觉确认：Picker 顶/底分隔、strip 字形标题、status bar 图标、pill powerline 端。

## 验证命令汇总

```bash
cargo build -p aloggrep-tui
cargo test -p aloggrep-tui
cargo test --workspace
```

## Rollback

- 所有改动在 `theme.rs` + `ui.rs` 两文件内，`git checkout -- aloggrep-tui/src/theme.rs aloggrep-tui/src/ui.rs` 即可回退。
- 无 schema/配置/数据迁移。
