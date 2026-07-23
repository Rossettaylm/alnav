# Design: TUI nerdfont glyphs and reduced-border panel redesign

## Architecture Constraints

- `theme.rs` 是颜色/样式的唯一入口（AGENTS.md 硬约束），本次扩展为**字形唯一入口**。
- `ui.rs` 禁止硬编码 `Color::*` 或字形字面量。
- `focus_style()` 保留（log selection 功能性反色）。
- 跨 crate 日志颜色统一（`logcolor.rs`）不动。

## 数据流不变

本次改动是纯渲染层，不触碰 App 状态机、ingest 管线、filter/highlight model 逻辑。唯一的数据流影响是 pill 文本宽度变化导致 strip 内排版重算，但这走现有的 `filter_strip_lines` / `inner.width` 路径，不需要新参数。

## 改动设计

### D1 — `theme.rs` glyphs 常量区块

新增 `pub mod glyphs`（或 `pub const` 区块，看现有风格——现有是 flat pub fn，用 flat pub const 一致）：

```rust
// Nerdfont semantic glyphs (hard dependency — no fallback).
pub const GLYPH_MODE_MANAGE: &str = "\u{f0b7}";  // 
pub const GLYPH_MODE_NEW: &str = "\u{f0fe}";     // 
pub const GLYPH_MODE_EDIT: &str = "\u{f044}";    // 
pub const GLYPH_CARET_SEL: &str = "\u{f0da}";    // 
pub const GLYPH_TITLE_PICKER: &str = "\u{f002}"; // 
pub const GLYPH_TITLE_LOG: &str = "\u{f0c5}";    // 
pub const GLYPH_TITLE_FILTER: &str = "\u{f0b0}"; // 
pub const GLYPH_TITLE_EXCLUDE: &str = "\u{f056}";// 
pub const GLYPH_TITLE_HIGHLIGHT: &str = "\u{f0e0}"; // 
pub const GLYPH_GROUP_ON: &str = "\u{f192}";     // 
pub const GLYPH_GROUP_OFF: &str = "\u{f10c}";    // 
pub const GLYPH_BOOKMARK: &str = "\u{f02e}";     // 
pub const GLYPH_LOCK: &str = "\u{f023}";         // 
pub const GLYPH_FOLLOWING: &str = "\u{f062}";    // 
pub const GLYPH_VISUAL: &str = "\u{f245}";     //  (i-cursor / selection)
pub const GLYPH_SEARCH: &str = "\u{f002}";       // 
pub const GLYPH_CRASH: &str = "\u{f071}";        // 
pub const GLYPH_SEP: &str = "\u{e0bf}";          // 
pub const GLYPH_FIELD_TAG: &str = "\u{f02b}";    // 
pub const GLYPH_FIELD_MSG: &str = "\u{f075}";    // 
pub const GLYPH_FIELD_PKG: &str = "\u{f187}";    // 
pub const GLYPH_FIELD_PID: &str = "\u{f292}";    // 
pub const GLYPH_FIELD_TID: &str = "\u{f2bd}";    // 
pub const GLYPH_FIELD_LEVEL: &str = "\u{f0d0}";  // 
pub const GLYPH_PILL_LEFT: &str = "\u{e0b6}";    // 
pub const GLYPH_PILL_RIGHT: &str = "\u{e0b2}";   // 
pub const GLYPH_HR: &str = "\u{2500}";           // ─
```

### D2 — `border_style` 降级

```rust
pub fn border_style(active: bool) -> Style {
    if active {
        Style::default().fg(accent()).add_modifier(Modifier::DIM) // +DIM
    } else {
        Style::default().fg(t().border_inactive).add_modifier(Modifier::DIM) // 不变
    }
}
```

### D3 — `numbered_title` / `plain_title` 去 `focus_style`

- `numbered_title`：徽标从 `focus_style()` 反色块改为 `{GLYPH} {number} {label}`，全部 accent fg。
- `plain_title`：前缀加字形图标。
- 数字保留（Tab 循环 1/2/3/5 语义）。

### D4 — `status_badge` 降级

现有 `status_badge(label, bg)` 返回反色块。改造为 `status_badge(glyph, label, fg)` 返回 `Span::styled("{glyph} {label}", Style::fg(fg))`，无 bg。

调用点 `render_status_bar` 逐个传字形：
- FOLLOWING → `GLYPH_FOLLOWING` + `success()`
- LOCK → `GLYPH_LOCK` + `lock()`
- VISUAL → `GLYPH_VISUAL` (`\u{f245}`, ) + `accent()` — 选中确认。
- pending `c…/C…/f…/m…/y…/d…` → 无字形，保持文字 + `warning()` fg（无 bg）。
- flash toast → 无字形，accent/warning fg。

### D5 — `picker_mode_prefix` / `candidate_prefix` 换字形

- `picker_mode_prefix`：`> ` → `GLYPH_MODE_MANAGE` + ` `，同理 New/Edit。
- `candidate_prefix`：`▌ ` → `GLYPH_CARET_SEL` + ` `。

### D6 — `chip_pill_style` / `exclude_pill_style` / `highlight_pill_style` 改造

现有 pill 文本 `format!(" {value} ")`（空格填充）。改为：

```rust
let icon = field_icon(field); // GLYPH_FIELD_TAG / MSG / ...
let text = format!("{GLYPH_PILL_LEFT} {icon} {value} {GLYPH_PILL_RIGHT}");
```

powerline 端本身有 fg/bg 对齐问题：左端 `` 的左半透明、右半取 pill bg；右端 `` 反之。ratatui `Span` 不支持单字符双 bg，所以 powerline 端的"半圆"效果靠**字形本身在单 bg 下渲染**——端字符的透明半边会露出终端底色。这要求 pill 的 bg 只作用于文本区，端字符用单独 Span 无 bg。

**实现方案**：pill 拆成 3 个 Span：`` (无 bg, pill fg) + ` {icon} {value} ` (pill bg, pill fg) + `` (无 bg, pill fg)。`committed_chip_spans` 等调用处改为 push 3 个 Span。

### D7 — `render_modal_shell` / strip 改顶/底分隔

现有 `rounded_block`（整圈 Rounded）。改为 `Block::new().borders(Borders::TOP | Borders::BOTTOM).border_type(BorderType::Plain).border_style(border_style(active))`。

`BorderType::Plain` 用 `─`（U+2500），与 Q5 决策一致。左右无描边，inner rect 宽度比原来多 2 列（省去左右边框）。

新增 `pub fn divider_block(title: Line, active: bool) -> Block` 替代 `rounded_block` 用于 Picker/strip/modal。`rounded_block` 保留给 LogList（不在范围内）。

### D8 — LogList 标题图标

LogList 虽然边框不改（不在范围），但标题加 `GLYPH_TITLE_LOG` 图标保持视觉一致——**否则 Picker/strip 都有图标、LogList 没有会割裂**。`numbered_title(4, "Log", active)` 调用处加图标前缀。这落在 `theme::numbered_title` 的调用方，不改 `numbered_title` 签名。

## 调用点清单

| 文件 | 函数 | 改动 |
|---|---|---|
| `theme.rs` | 新增 glyphs const | 26 个常量 |
| `theme.rs` | `border_style` | +DIM |
| `theme.rs` | `numbered_title` | 去 focus_style，加字形 |
| `theme.rs` | `plain_title` | 加字形 |
| `theme.rs` | `status_badge` | 签名改 `(glyph, label, fg)`，去 bg |
| `theme.rs` | `picker_mode_prefix` | 换字形常量 |
| `theme.rs` | `candidate_prefix` | 换字形常量 |
| `theme.rs` | `chip_pill_style` | 3-Span powerline + 字段图标 |
| `theme.rs` | `exclude_pill_style` | 同上 + `!` 前缀 |
| `theme.rs` | `highlight_pill_style` | 同上 powerline 端 |
| `theme.rs` | `field_icon(field)` | 新增辅助函数 |
| `ui.rs` | `rounded_block` | 保留，新增 `divider_block` |
| `ui.rs` | `render_modal_shell` | 用 `divider_block` |
| `ui.rs` | `render_chip_strip` 等 ×3 | 用 `divider_block` + 字形标题 |
| `ui.rs` | `render_candidate_list` | 用 `divider_block` |
| `ui.rs` | `render_preview` | 用 `divider_block` |
| `ui.rs` | `render_picker_search_line` | mode 前缀已走 theme，无改动 |
| `ui.rs` | `render_status_bar` | `status_badge` 调用改签名 |
| `ui.rs` | `committed_chip_spans` | pill 改 3-Span |
| `ui.rs` | strip lines 渲染 | 状态点 ``/`` |

## 风险

1. **Pill 宽度膨胀**：powerline 端 +2 列、字段图标 +2 列，单 pill 宽度 +4。strip 在窄终端可能溢出。mitigation：`PILL_GAP` 从 1 降到 0，或 strip 换行逻辑已有（`filter_strip_lines` 按 `inner.width` 截断）。
2. **powerline 端透明半边**：若终端字体 powerline 端字形不是真透明（部分 nerdfont 字体端字符有背景填充），半圆效果消失变方块。无法运行时检测，接受。
3. **inner rect 宽度变化**：strip/modal 从整圈（左右各 -1）改顶/底分隔（左右不 -1），inner.width +2。测试断言若硬编码宽度需修正。
4. **`status_badge` 签名变更**：所有调用点（`render_status_bar` 内约 10 处）需同步改。
