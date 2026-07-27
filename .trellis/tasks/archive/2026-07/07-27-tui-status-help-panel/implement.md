# Implement: TUI status bar + help panel

## Checklist

1. **数据层 `help.rs`**
   - 引入 `HintEntry` / section 结构；英文 copy 覆盖全部原 L1/L2。
   - `context_entries(app)` 替代（或包装）`context_help` 的优先级逻辑。
   - `catalog_sections(app)` 输出完整目录 + active 标记。
   - `entries_to_spans` + 宽度 fit；更新/重写 `help` 模块单测（英文、`? help`、hdc 分支、L2 优先）。

2. **App 状态**
   - `help_open: bool`、`help_scroll: usize`；`open_help` / `close_help`（不 `resume_following`）。
   - `detail_open` 类查询：Help 打开时视作 modal，阻止其它 Normal 动作按需短路。

3. **按键 `main.rs`**
   - 允许条件下 `?` toggle Help。
   - Help 打开时独立 handler：`j/k/Esc/?`。
   - operator-pending / picker / time / detail 路径不打开 Help。
   - 单测：开关、Esc 不改 following、pending 时忽略、`/` 仍 New Highlight。

4. **渲染 `ui.rs` + `theme.rs`**
   - `render_status_bar`：左侧图标化；右侧用 spans；去掉 `[]` 与单词徽章。
   - `theme`：`status_icon` / 调整 `status_badge`；Help 面板标题/Active/section 样式（禁止 `ui` 硬编码 Color）。
   - `render_help_panel`：靠上 modal + 可滚内容；主循环接入。

5. **Flash 英文化**
   - grep `set_flash`（`main.rs` / `app.rs` / 其它）；统一短英文。
   - 修正依赖中文 flash 的测试断言。

6. **文档**
   - 更新 `CLAUDE.md` / `AGENTS.md` 中 `?` / 状态栏 / Help 描述，与实现一致。

7. **验证**
   - `cargo test -p aloggrep-tui help::`
   - `cargo test -p aloggrep-tui`（或至少 `ui::` + `app::` + `dispatch` 相关）
   - `cargo build -p aloggrep-tui`

## Validation commands

```bash
cargo test -p aloggrep-tui help::
cargo test -p aloggrep-tui ui::
cargo test -p aloggrep-tui
cargo build -p aloggrep-tui
```

## Risky files

| File | Risk |
|------|------|
| `aloggrep-tui/src/help.rs` | 文案与 API 重写，测试面大 |
| `aloggrep-tui/src/ui.rs` | status bar / 新 panel 渲染 |
| `aloggrep-tui/src/main.rs` | 键位分发顺序易回归 |
| `aloggrep-tui/src/app.rs` | flash 文案 + help 状态 |
| `aloggrep-tui/src/theme.rs` | 图标/样式 API |

## Rollback

单 commit 或逻辑清晰的连续 commit；回滚即恢复上述文件。无数据迁移。

## Before `task.py start`

- [x] `prd.md` / `design.md` / `implement.md` 齐备
- [ ] 用户批准本规划摘要
- [ ] （若走 sub-agent）为 `implement.jsonl` / `check.jsonl` 填入真实 research/spec 条目
