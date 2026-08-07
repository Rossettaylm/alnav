# TUI 设备断线提示

## Goal

在 Stream 模式（`--hdc`/`--adb`）下，设备/子进程断开连接时，status bar 展示一个常驻图标提示，替代目前"完全无反馈、用户以为卡住"的现状。

## Background

- `App.ingest_done: bool` 已存在，Stream 模式下子进程 stdout EOF（设备拔线/进程退出/被杀等任何导致读取结束的情况）时会被置 `true`（`ingest.rs::spawn_live_ingest` 在 `session.lines` 迭代器耗尽时调用 `producer.mark_disconnected()` → `drain()` 收到 `TryRecvKind::Disconnected` → `self.ingest_done = true`）。
- 该 flag 当前**只用于 P4 画面节流**（`main.rs` 的 `should_draw` 判断），完全没有在 UI 上展示。
- File 模式下"读完文件"也会走到同一个 `ingest_done = true`（正常终止，不是断线），因此**不能不加区分地展示**——必须限定 `!app.store.is_file()` 才展示。
- 不采集/区分断线具体原因（子进程 exit code、设备拔线 vs 命令找不到等）——`ingest.rs` 现在没有这类信息可用，做区分需要新增管线，v1 不做。
- 不做自动重连——这是后续可能的独立需求，本任务只解决"提示"这一层。

## Requirements

### R1 — 展示条件

仅 Stream 模式（`!app.store.is_file()`）且 `app.ingest_done == true` 时展示；File 模式永不展示（即便 `ingest_done` 为 true）。

### R2 — 图标与颜色

- 新增 `theme::GLYPH_DISCONNECT`（建议 `\u{f127}`，nf-fa-chain-broken，跟现有 `GLYPH_LOCK`/`GLYPH_TIME` 同属 Font Awesome PUA 码位族）。
- 颜色用 `theme::warning()`（Yellow），不新增颜色常量。
- 纯图标、不带附加文字值（跟 `following`/`visual` 一样是布尔状态展示，用 `theme::status_icon`，不用 `status_icon_value`）。

### R3 — 位置

`render_status_bar`（`ui.rs`）中，插入在 `following` 图标之后、`lock` 徽标之前（同属"直播状态"分组）。

## Acceptance Criteria

- [ ] Stream 模式下子进程退出（模拟：`--hdc`/`--adb` 子进程结束或测试里直接 `mark_disconnected`）后，status bar 出现断线图标
- [ ] File 模式下读完整个文件（`ingest_done` 可能被其他路径置 true）不展示断线图标
- [ ] 图标颜色为 `theme::warning()`，位置在 `following` 与 `lock` 徽标之间
- [ ] 不影响现有 `ingest_done` 用于画面节流的行为
- [ ] `cargo test -p alnav` 全绿，新增单测覆盖 R1 的展示条件分支

## Notes

- 轻量任务，PRD-only；实现落点明确（`theme.rs` 新增常量、`ui.rs::render_status_bar` 插入一段 `if` 分支），不需要 `design.md`/`implement.md`。
