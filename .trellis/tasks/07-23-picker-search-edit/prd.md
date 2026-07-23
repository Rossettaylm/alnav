# Picker search area mid-cursor editing

## Goal

Picker（及仍可达的旧输入模态）草稿行支持中间光标与基础编辑键，解决只能末尾追加/Backspace 的痛点；Manage 动作键让出 Ctrl 给编辑，改绑到不冲突的键。

## Background / Confirmed Facts

- 现状：`PickerSession.query` / `draft`、`InputBox.draft`、`HighlightBox.draft` 均为仅追加 + `pop` 末尾；UI 用末尾 `theme::caret_bar()`。
- Manage 已占用：`Ctrl-E` 编辑、`Ctrl-D` 删选中（见 [`aloggrep-tui/src/main.rs`](aloggrep-tui/src/main.rs) Manage 分支；[`help.rs`](aloggrep-tui/src/help.rs) `L1_PICKER`）。
- 文档曾提 Ctrl-A/Ctrl-U 作 Manage 动作，代码未接；本任务不恢复它们为动作键。
- 决策来源：grilling 共识（方案 1 先做；vim 模式后续）。

## Requirements

1. **统一文本缓冲**：所有 Picker 草稿面（Manage `query`；New/Edit Highlight `draft`；Filter/Exclude `InputBox.draft`；MsgChip `draft`）及仍可达的旧 HighlightBox / Input 模态，共享同一套中间光标编辑模型。
2. **编辑键（全草稿面）**：
   - `←` / `→`：按字符移动光标
   - `Home` / `End`、`Ctrl-A` / `Ctrl-E`：行首 / 行尾
   - `Backspace`：删光标左侧一字
   - `Ctrl-U`：删到行首
   - 不做 `Ctrl-D`；不做「删光标右侧一字」
3. **New/Edit 专有**：`Ctrl-Backspace` = 删前一词（空白/标点分词）。
4. **Manage 动作改绑**：
   - 编辑：`Ctrl-X`（取代 `Ctrl-E`）
   - 删选中：`Delete` 或 `Ctrl-Backspace`（取代 `Ctrl-D`；始终删条目，与 query 是否为空无关）
   - Manage `query` 不做删词
5. **光标可见性**：复用 `caret_bar` 插在光标处；超长时窗口跟随光标（贴末尾打字时观感为末尾稳定可见）。
6. **Filter `InputBox`**：光标在草稿起点且草稿空时，保留现有 Backspace 级联（清 `draft_field` → 弹出已提交 chip）。
7. **Help / 文档**：更新 `L1_PICKER` 与相关 CLAUDE 描述，去掉旧 Ctrl-E/D 动作表述。

## Acceptance Criteria

- [x] Manage / New / Edit 下可在草稿中间插入与 Backspace 删除，无需重打整段
- [x] `←`/`→`/`Home`/`End`/`Ctrl-A`/`Ctrl-E`/`Ctrl-U` 行为符合 Requirements
- [x] Manage：`Ctrl-X` 进入编辑选中项；`Delete` 与 `Ctrl-Backspace` 均触发删选中确认（与现 Ctrl-D 同等路径）
- [x] Manage：`Ctrl-E` / `Ctrl-D` 不再触发编辑/删除动作（可被编辑键消费或 noop，不得再删条目）
- [x] New/Edit：`Ctrl-Backspace` 删前一词；`Delete` 不删条目、不删光标右侧
- [x] 超长草稿时可见光标（窗口跟随光标）
- [x] `InputBox` Backspace 级联在空草稿 + 光标起点时仍成立
- [x] 单元测试覆盖 TextField/缓冲编辑与关键 Manage 改绑
- [x] `help.rs` Picker 提示反映新键位

## Out of Scope

- Vim 模态编辑（insert/normal）
- 选区、`Ctrl-K`、`Ctrl-W`、词级左右跳转
- Manage 恢复/新增 Ctrl-A「新建」、Ctrl-U「删全部」动作
- 搜索/过滤历史持久化
- Windows 专门适配

## Open Questions

（无阻塞项）
