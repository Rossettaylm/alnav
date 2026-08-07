# TUI 崩溃/ANR 结构化详情浮层

## Goal

光标停在崩溃/ANR 签名行时，复用 `P`（Pretty）键弹出结构化崩溃信息（类型/headline/exception/stack），复用 `alnav-core::crash::CrashDetector`，不新增按键。

## Background

- `CrashDetector::detect(&LogEntry)` 只用签名正则（`FATAL EXCEPTION`/`ANR in `/native signal）判断类型，当前 TUI 只用它算 `is_severe_row`（还 OR 了 level E/F），从未拿它做结构化提取。
- `CrashDetector::parse_crash` 要求传入**已合并的多行 msg** 才能提取 `stack`（`at ...` 续行）；单行 msg 只能拿到 `headline`。
- **Stream 模式**（`--hdc`/`--adb`）：`EntryRow::from_line` 对解析失败的续行直接丢弃、不入队（`model.rs` 明确注释"same as CLI's default no-multiline"）——没有堆栈原始数据可恢复。
- **File 模式**（`-f`）：`EntryRow::from_line_or_raw` 会把解析失败的续行保留为独立的 `parsed=false` 行，可在崩溃行之后连续向下扫描拼接。
- `DetailView` 现有 `Closed`/`Fields`/`Pretty` 三态（`app.rs`），`P` 键当前判定链：JSON 缩进 → 失败则原文 +「非 JSON」。
- `theme.rs` 已有 `GLYPH_CRASH`（`\u{f071}`，nf-fa-warning）定义但全代码库未使用，可直接复用做本浮层的标题/提示图标。

## Requirements

### R1 — 触发键与判定链（复用 `P`，不新增按键）

`P` 键判定顺序调整为：
1. `CrashDetector::detect()` 命中当前行 msg（**仅**看签名正则命中，不是 `is_severe_row`，避免把普通 E/F 级别但非崩溃签名的日志误判）→ 显示结构化 CrashInfo。
2. 否则维持现状：JSON 缩进 → 失败则原文 +「非 JSON」。

判定基于**光标当前行自身**的 msg；若光标停在堆栈续行（无签名关键字）上按 `P`，不做"向上找最近崩溃头"，直接走判定链第 2 步（大概率显示"非 JSON"）。

### R2 — File 模式续行合并

- 命中崩溃签名后，若 `store` 是 File 模式：从该行下一行开始，沿 `parsed=false` 的行连续向下扫描并拼接（`\n` 连接），凑成多行 msg 传给 `CrashDetector::parse_crash`。
- 遇到第一个 `parsed=true` 的行即停止扫描（视为下一条独立日志）。
- 扫描上限 **500 行**；命中上限则截断，浮层内提示"…(已截断)"。

### R3 — Stream 模式降级

- Stream 模式下没有续行原始数据，直接用当前行自身单行 msg 调 `parse_crash`：`stack` 大概率为空数组，浮层照常显示 `crash_type`/`headline`/`exception`/`timestamp`/`pid`/`tid`/`tag`，堆栈区域显示"stream 模式无堆栈"提示文案。
- 不隐藏、不禁用 Stream 模式下的这个能力（不同于全局时间窗对 `--hdc`/`--adb` 的整功能隐藏）。

### R4 — 呈现

- 复用 `render_modal_shell` 靠上模态壳（跟 Fields/Pretty 一致）。
- 内容可 `j`/`k` 滚动（堆栈可能较长）。
- `Esc` 只关浮层，不 `resume_following`（跟现有 Detail 浮层行为一致）。
- 图标可用现有 `theme::GLYPH_CRASH`。

### R5 — 范围边界（明确不做）

- 不做整体 yank/复制（跟 Fields/Pretty 现状对齐，只有 LogList 上现有的按字段 yank，本浮层无整体复制动作）。
- 不做"向上找最近崩溃头"的双向扫描。
- 不区分 Stream 模式下堆栈缺失的具体原因（本来就没有数据，不用解释为什么）。

## Acceptance Criteria

- [ ] File 模式下：光标停在含 `FATAL EXCEPTION`/`ANR in `/native signal 签名的行，按 `P`，弹出结构化浮层，含完整 headline/exception/stack（真实多行堆栈样例验证）
- [ ] File 模式下：堆栈超过 500 行时正确截断并提示
- [ ] Stream 模式下：同样按键触发浮层，`stack` 为空但其余字段正确，提示文案存在
- [ ] 光标停在普通 E 级 JSON 消息行（非崩溃签名）按 `P`：仍走原 JSON pretty 判定链，不被新逻辑抢戏
- [ ] 光标停在堆栈续行按 `P`：不报错、走原判定链第 2 步
- [ ] `Esc` 关闭浮层不影响 `following` 状态
- [ ] Help 面板能查到"崩溃详情"提示（挂在 `P` 键的 detail 文案上，说明双重语义）
- [ ] `cargo test -p alnav` 全绿，新增单测覆盖 R1-R3 的判定链分支

## Notes

- 新增判定逻辑建议放在独立函数（如 `crash_context_for_row`），供 `main.rs`/`app.rs` 的 `P` 键处理调用，避免把 File/Stream 分支逻辑硬塞进渲染函数。
