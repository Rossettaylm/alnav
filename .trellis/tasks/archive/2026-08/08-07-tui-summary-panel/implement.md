# Implement: TUI 日志统计面板

## Checklist

1. [ ] `alnav-core/src/summary.rs`：`SummaryOutput`/`TagEntry`/`ErrorEntry`/`TimeRange` 改 `pub`；新增 `pub fn into_report(self, matched: usize) -> SummaryOutput`；`to_json` 内部改为调用 `into_report` 再序列化
2. [ ] `cargo test -p alnav-core` 确认 `to_json` 行为未变（现有 CLI `--summary` 测试全绿）
3. [ ] `alnav/src/keymap.rs`：新增 `ActionId::LeaderSummary`（`i`，`KeyContext::Leader`）+ lookup 表条目 + `ActionMeta`
4. [ ] `alnav/src/app.rs`：
   - 新增 `SummaryView` 枚举 + `summary_view`/`summary_gen` 字段
   - `open_summary_panel()` / `close_summary_panel()`
   - 后台线程 spawn 逻辑（File: mmap+LineSpan 路径；Stream: `Vec<EntryRow>` clone 路径）
   - 轮询方法接入主循环（`poll_file_store` 扩展或新增独立轮询，`main.rs` 主循环调用点）
5. [ ] `alnav/src/main.rs`：`ActionId::LeaderSummary` 分发到 `open_summary_panel`/`close_summary_panel`（toggle 语义，跟 `LeaderManage` 同类分发方式）
6. [ ] `alnav/src/ui.rs`：
   - `render_summary_panel`（Loading 占位 + Ready 渲染）
   - `bar_line` 手搓柱状图辅助函数
   - 级别分布 / Top tags（带柱状图）/ Top errors（纯列表）/ 崩溃计数 / 时间范围 各分区渲染
7. [ ] `help.rs`：Leader 层级提示补充 `i` 键位说明
8. [ ] 单测：
   - `alnav-core`：`into_report` 输出字段与原 `to_json` JSON 内容一致（同一份 `Summary` 分别走两条路径比对）
   - `alnav`：File 模式模拟大 `visible`（数万行量级即可覆盖异步路径，不必真跑百万行）下打开面板得到 `Loading` → 轮询后 `Ready`
   - `alnav`：Stream 模式同样验证 `Ready` 内容正确（level 分布/tags/errors/crashes/time_range 与手算预期一致）
   - `alnav`：快速 关闭→重开 场景下，旧 gen 的后台结果到达时被丢弃（可用测试注入延迟到达的假结果，校验 `summary_gen` 校验生效）
9. [ ] `cargo build --workspace && cargo test --workspace`

## Validation Commands

```bash
cargo test -p alnav-core --lib summary
cargo test -p alnav
cargo build --workspace
```

## Rollback

`alnav-core` 改动为纯新增/内部重构，`to_json` 对外行为不变，CLI 侧无需跟着改；TUI 侧新状态机若有问题可整体 revert `app.rs`/`ui.rs`/`keymap.rs`/`main.rs` 里本任务新增的代码块，不影响其余功能。
