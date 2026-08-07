# Implement: TUI 崩溃/ANR 结构化详情浮层

## Checklist

1. [ ] `ui.rs`：新增 `fn crash_context_for_row(app: &App, row: &EntryRow) -> Option<(CrashInfo, bool)>`
   - `OnceLock<CrashDetector>` 单例（参照 `model.rs::crash_detector()`）
   - `CrashDetector::detect` 未命中 → `None`
   - 命中后按 store 类型分支构造 merged msg（File：`source_idx_for_visible` + `store.row_at_source` 逐行拼接，上限 500 行；Stream：单行）
   - 用 row 的 timestamp/pid/tid/tag + merged msg 手工构造 `LogEntry`，调用 `CrashDetector::parse_crash`
2. [ ] `ui.rs`：新增 `fn render_crash_detail_lines(info: &CrashInfo, truncated: bool, width: usize) -> Vec<Line<'static>>`
   - 头部类型徽标（`theme::GLYPH_CRASH` + `theme::warning()`）+ headline
   - exception 行、pid/tid/tag/timestamp 行（`theme::muted()`）
   - stack 逐行；Stream 模式空 stack 显示占位文案
   - `truncated` 时追加"…(已截断)"
3. [ ] `ui.rs::detail_content_lines` 的 `DetailView::Pretty` 分支：先调用 `crash_context_for_row`，`Some` 则走新渲染，`None` 落回现有 `detail_pretty_lines`
4. [ ] `help.rs`：`P`/`detail_pretty` 的 detail 文案补充"崩溃行显示结构化详情"说明（挂在现有 Pretty 键位提示上，不新增 Help 词条）
5. [ ] 单测（`alnav/src/ui.rs` 或 `main.rs` 对应 `#[cfg(test)]` 模块）：
   - File 模式：构造含 `FATAL EXCEPTION` + 若干 `at ...` 续行的样例文件，验证 `crash_context_for_row` 返回完整 stack
   - File 模式：续行数超过 500 时验证截断标记
   - Stream 模式：单行 crash 签名验证返回空 stack + 类型/headline 正确
   - 非崩溃签名行（含普通 E 级 JSON 消息）验证返回 `None`，走原 JSON pretty 判定链不受影响
   - 光标停在纯堆栈续行（无签名）验证不 panic、返回 `None`
6. [ ] `cargo build -p alnav && cargo test -p alnav`

## Validation Commands

```bash
cargo build -p alnav
cargo test -p alnav
cargo clippy -p alnav -- -D warnings   # 若仓库 CI 有此把关，按现状执行
```

## Rollback

纯新增函数 + 一处判定链插入点，无状态迁移、无按键变更；出问题可直接 revert 本次改动的 diff，不影响 Fields/Pretty 现有行为。
