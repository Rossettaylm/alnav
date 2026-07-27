# TUI status bar + help panel

## Goal

优化 `aloggrep-tui` 底部状态栏的英文可读性与视觉密度，并新增只读 Help 面板（`?` / Shift+/）：状态栏继续显示情境短提示（L1/L2），Help 提供「当前 Active 详细版 + 完整键位目录」。

## Background / Confirmed Facts

- 状态栏渲染在 `aloggrep-tui/src/ui.rs` 的 `render_status_bar`；右侧短提示来自 `help.rs::context_help`（目前中文、`键:短名` 单字符串）。
- L1/L2 优先级已实现：modal/picker/confirm > operator-pending > Focus L1。
- 左侧徽章经 nerdfont 改造后已是「图标 + 单词、无反色块」（`theme::status_badge`）；本次进一步去掉开关态单词，改为纯图标（有参数则图标+短数值）。
- 当前代码：`;` / `/` / `` ` `` 分别 `open_picker_new` Filter / Highlight / Exclude；`Space Space` 开 Unified Manage；**源码中无 `?` 绑定**（文档里「`?` = Highlight 强制 New」已过时）。本次把 `?` 分配给 Help，与现有 `/` = Highlight New 不冲突。
- Flash 文案大量中文（`main.rs` / `app.rs` 的 `set_flash(...)`）。
- 弹出层惯例：靠上 `top_modal_rect` + `render_modal_shell`；Detail 的 Esc 只关浮层、不 `resume_following`。

## Requirements

### R1 — 状态栏全英文

- 状态栏一切可见文案英文化：右侧 L1/L2 短提示、左侧数值/进度、pending 短标、flash toast。
- 确认框标题等非状态栏中文不在本任务强制范围（除非实现时顺手触达且无额外风险）。

### R2 — 右侧 L1/L2 视觉统一

- 键名 dim、说明正常字重；条目间靠空格间距分组；不使用 `:` / `|` / `·` 等隔断符。
- 保留现有二级逻辑：无 pending → 焦点 L1；operator-pending → 对应 L2；modal 打开时仍覆盖为该 modal 的 L1。
- LogList L1 必须包含 `? help`（窄宽度时由现有截断逻辑自然裁掉）。
- `--hdc` 与 file 模式差异保留（如 hdc 有 Ctrl-L clear、无 time 窗提示）。

### R3 — 左侧状态簇图标化

- follow / visual：纯 Nerd Font 图标，不显示 FOLLOWING / VISUAL 单词。
- lock / time / 文件进度（及类似有参数状态）：图标 + 短数值/短文本，无 LOCK/TIME 单词前缀。
- 高亮命中计数：去掉方括号，改为无括号短形式（可附搜索图标）。
- pending（`c…` / `f…` 等）与 flash：非反色文字；与右侧风格协调。
- 新增/调整字形只进 `theme.rs`；`ui.rs` 不硬编码码点。

### R4 — 只读 Help 面板（`?`）

- `?`（Shift+/）打开只读 Help；不执行命令、不提供搜索过滤、不替代 Leader/Picker。
- 可用：`Focus` 为 ChipStrip / ExcludeStrip / HighlightStrip / LogList，且无 Picker / Time / Detail / Confirm，且无任何 operator-pending。
- 不可用时：不抢第二键；静默忽略或极短英文 flash（实现任选，优先静默）。
- UI：靠上 modal；`j/k`（及等效上下）滚动；`Esc` / 再次 `?` 关闭；关闭不 `resume_following`。
- 内容结构：
  1. 顶部 **Active** 摘要 = 当前状态栏情境的详细版（与 L1/L2 同源）；
  2. 下方 **固定完整键位目录**（分类整齐）；当前情境在目录中可辨（标题 accent 或等价强调）。
- Help 与状态栏共用同一套 key/label 语义数据，避免两套文案漂移。

### R5 — 键位与文档

- 绑定 `?` → Help；保持 `/` → Highlight New（现状）。
- 更新 `help` 文案、相关单测断言；若 `CLAUDE.md` / `AGENTS.md` 仍写「`?` = Highlight New」，在本任务内改为「`?` = Help」。

## Acceptance Criteria

- [x] AC1：LogList Normal 按 `?` 打开 Help；再次 `?` 或 `Esc` 关闭，且 `following` 不因关闭而恢复。
- [x] AC2：Chip / Exclude / Highlight strip Normal 可开 Help；Picker / Time / Detail / Confirm 打开时，或任意 operator-pending 时，`?` 不开 Help。
- [x] AC3：Help 顶部显示与当前 L1/L2 对应的 Active 详细摘要；下方为完整分类键位表，当前情境段落可辨。
- [x] AC4：状态栏右侧短提示为英文，键 dim、说明正常，无冒号/竖线隔断；L1↔L2 切换视觉一致；LogList L1 含 `? help`。
- [x] AC5：follow/visual 仅图标；lock/time/进度为图标+短值；无 FOLLOWING/LOCK/TIME/VISUAL 单词；命中计数无 `[]`。
- [x] AC6：所有经 `set_flash` / 状态栏展示的用户可见反馈为英文。
- [x] AC7：`/` 仍打开 Highlight New；`;` / `` ` `` / `Space Space` / `mm` 行为不变。
- [x] AC8：`cargo test -p aloggrep-tui help:: ui::`（及本任务触及模块测试）通过；`cargo build -p aloggrep-tui` 通过。

## Out of Scope

- 可搜索 / 可执行命令面板（VS Code 式 palette）。
- 改 Leader / ActionList / Picker 的执行模型。
- Insert 草稿输入中绑定 `?`；stdin 管道、Windows 专项。
- 非状态栏 UI 的全面英文化（Confirm 标题、strip 中文等）除非为实现所必需。
- 无 Nerd Font 时的 ASCII fallback。

## Open Questions

（无阻塞项；grill 已收敛。）
