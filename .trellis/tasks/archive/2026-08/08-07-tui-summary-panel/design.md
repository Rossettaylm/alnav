# Design: TUI 日志统计面板

## Boundaries

- **In**: `alnav-core::summary`（新增结构化访问 API）、`alnav/src/app.rs`（面板状态/gen 管理）、`alnav/src/ui.rs`（渲染+柱状图）、`alnav/src/keymap.rs`（`ActionId::LeaderSummary`）、`alnav/src/main.rs`（键位分发）、`alnav/src/store.rs`（复用/新增后台扫描通道）。
- **Out**: CLI `--summary` 现有 JSON 输出格式（保持不变）、`Deduper`/`Normalizer` 内部算法（原样复用）。

## 关键前置事实（必须先改 `alnav-core`）

`Summary::to_json(self, matched: usize) -> String` 是当前**唯一**的公开输出方法——消费 `self`、直接序列化成 JSON 字符串，中间的 `SummaryOutput`/`TagEntry`/`ErrorEntry`/`TimeRange` 均为私有类型。TUI 不能拿到结构化数据来画柱状图，必须先给 `alnav-core::summary` 加一层结构化访问：

- 把 `SummaryOutput`/`TagEntry`/`ErrorEntry`/`TimeRange` 改 `pub`（或新增一套同构的 `pub` 镀出类型，二选一，倾向直接 `pub` 现有类型，避免重复定义）。
- 新增 `pub fn into_report(self, matched: usize) -> SummaryOutput`（提炼 `to_json` 里 top_tags/top_errors 排序截断的逻辑到这个方法，`to_json` 内部改为调用它再 `serde_json::to_string`）。
- CLI `cli_run.rs` 对 `to_json` 的调用点不变（签名不变、只是实现内部转发）。

## 数据流

```
Leader i (main.rs, ActionId::LeaderSummary)
  → app.open_summary_panel()
      - bump app.summary_gen (u64)
      - app.summary_view = SummaryView::Loading
      - 后台线程: 按 store 类型分支
          File: clone Arc<Mmap> + Arc<RwLock<Vec<LineSpan>>>（跟 store.rs::spawn_filter_scan
                同一套共享数据），按 app.visible 的物理行集合逐行 parse_span → LogEntry →
                Summary::record()
          Stream: clone 当前 visible 对应的 Vec<EntryRow>（Clone 派生），逐行转 LogEntry →
                Summary::record()
      - 完成后 Summary::into_report(matched) → SummaryOutput，经 channel 送回主线程
        （复用/扩展 FileEvent 枚举 或 新增一个专用 mpsc 通道，二者皆可，倾向新增专用通道
        避免 FileEvent 语义膨胀）
  → 主线程每帧轮询（poll_file_store 或新增 poll_summary_job）:
      收到结果时校验 gen == app.summary_gen 才写入 app.summary_report；
      gen 不符（面板已重新打开过）则丢弃
  → app.summary_view = SummaryView::Ready(report) 后渲染

Esc / 再按 Leader i 关闭:
  → app.summary_view = SummaryView::Closed
  → bump app.summary_gen（使在途后台结果作废）
  → 不 resume_following
```

## 新增/改动点清单

### `alnav-core/src/summary.rs`
- `pub` 化 `SummaryOutput`/`TagEntry`/`ErrorEntry`/`TimeRange`（或新增 `pub` 版本）。
- 新增 `pub fn into_report(self, matched: usize) -> SummaryOutput`。
- `to_json` 内部改为 `self.into_report(matched)` 再序列化，行为不变（现有 CLI 测试应保持通过）。

### `alnav/src/app.rs`
- 新增 `pub enum SummaryView { Closed, Loading, Ready(SummaryOutput) }`（或 `Option<SummaryJob>` 亦可，二选一由实现时定）。
- 新增 `summary_gen: u64` 字段（初值 0）。
- 新增 `open_summary_panel()` / `close_summary_panel()` 方法，负责 bump gen + spawn 后台线程 + 状态切换。
- 新增轮询方法（可并入现有 `poll_file_store`，也可独立 `poll_summary_job`，Stream 模式两者都需要在主循环里被调用到）。

### `alnav/src/keymap.rs`
- 新增 `ActionId::LeaderSummary`：`context: KeyContext::Leader`、`toml_key: "summary"`、`default: Binding::parse_str("i")`、`kind: ActionKind::Leaf`、`label: "stats"`、`detail: "open summary panel"`。
- `(KeyContext::Leader, "summary") => Some(ActionId::LeaderSummary)` 加入 lookup 表。

### `alnav/src/ui.rs`
- `fn render_summary_panel(app: &App, frame: &mut Frame, area: Rect)`：`SummaryView::Loading` 显示"计算中…"占位（复用 `theme::log_loading_style`/`numbered_title_with_loading` 的视觉语言）；`Ready(report)` 渲染分区内容。
- `fn bar_line(label: &str, count: usize, max: usize, width: usize, color: Style) -> Line<'static>`：手搓 `█` 比例柱形，级别分布/Top tags 共用，颜色由调用方传入（级别用 `theme::level_badge_style`，tags 用 `theme::accent()`）。
- Top errors 区块不调用 `bar_line`，纯文本列表。

## 性能与并发

- File 模式：mmap 是只读、`Arc` 共享，后台线程读取不影响主线程渲染；`Arc<RwLock<Vec<LineSpan>>>` 只读锁，跟现有 `spawn_filter_scan` 并发模型一致，不新增锁竞争面。
- Stream 模式：clone `Vec<EntryRow>` 快照后台处理，避免长时间持有 `StreamStore` 内部锁阻塞 ingest `drain()`。
- gen 校验放在"收到后台结果"这一步（跟 `CandidateMatchService` 的做法一致），不需要真正"取消"线程执行（线程算完自然退出，只是结果被主线程丢弃）——比强行 `AtomicBool` cancel token 简单，且计算量有限（一次性、非逐帧触发）。

## Rollout / Rollback

- `alnav-core` 的改动是纯新增 API + 内部重构（`to_json` 行为不变），风险低，CLI 现有测试可作回归网。
- TUI 侧新增状态机 + 渐进渲染，出问题可整段 revert，不影响其余浮层（Fields/Pretty/崩溃详情）。
