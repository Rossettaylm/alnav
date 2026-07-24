# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

aloggrep — 轻量级 Android logcat 日志过滤与分析工具（Rust）。Cargo workspace，两个 member crate：

- **`aloggrep-core`**（binary 名 `aloggrep`、`alg`）— 原有 CLI，逻辑未变。package 名为 `aloggrep-core`，但 `[lib] name = "aloggrep"`（有意保持不变，`use aloggrep::...` 全仓库统一，不因 package 改名而变化）。发布于 crates.io。
- **`aloggrep-tui`**（binary 名 `aloggrep-tui`）— 交互式 vim 风格 TUI，基于 ratatui/crossterm，依赖 `aloggrep-core`。支持 `-f` 静态文件浏览与 `--hdc` 实时设备流。

## Build & Test

```bash
cargo build --workspace                       # 构建全部 workspace 成员
cargo build -p aloggrep-core                   # 仅构建 CLI
cargo build -p aloggrep-tui                    # 仅构建 TUI
cargo test --workspace                         # 运行全部测试
cargo test -p aloggrep-core --test parser      # 仅运行 parser 集成测试
cargo test -p aloggrep-tui app::                # 仅运行 aloggrep-tui 的 app 模块测试
cargo run -p aloggrep-core --bin aloggrep -- -f app.log --tag "MyApp"   # 运行 CLI
cargo run -p aloggrep-tui -- -f app.log         # 运行 TUI（需要真实 TTY）
cargo run -p aloggrep-tui -- --hdc              # TUI 实时抓取模式
```

测试分布（`aloggrep-core`，位于 `aloggrep-core/`）：
- `tests/*.rs`（10 个文件）— 集成测试，通过 pub API 测试，占绝大多数
- `src/histogram.rs` — 3 个单元测试（访问私有 `snap_secs()` 和 `buckets` 字段）
- `src/main.rs` — 4 个单元测试（访问私有 `run_follow()` 函数）
- `src/hdc.rs` — 3 个单元测试（`HdcLiveFilter` 历史缓冲跳过逻辑）
- `src/expr.rs` — `Expr::from_filters` 相关测试

测试分布（`aloggrep-tui`，位于 `aloggrep-tui/`）：全部为源文件内 `#[cfg(test)]` 单元测试，覆盖 `model`/`filter_model`/`highlight_model`/`ingest`/`app`/`ui`/`input`/`main`（`dispatch_tests`）各模块。

## Architecture

```
aloggrep-core/src/
├── main.rs        # CLI 入口（clap derive），输入调度（stdin/文件），主循环
├── clearkey.rs    # --hdc 模式下 Ctrl-L 清屏：cbreak 模式读键盘 + KeypressGate 包装行迭代器
├── hdc.rs         # --hdc 子进程 spawn + 历史缓冲跳过（HdcLiveFilter/HdcSession/spawn_hilog），供 CLI 与 TUI 共用
├── parser.rs      # LogEntry 结构体 + 解析器（支持 hilog/threadtime/xlog/brief 四种格式）
├── filter.rs      # FilterChain：多条件组合过滤（同类 OR，跨类 AND），支持 pid/tid
├── expr.rs        # -e 布尔表达式：tokenizer + 递归下降 parser + AST evaluator；Expr::from_filters 将 CLI 标量过滤条件编译为 AST，供 TUI 复用
├── multiline.rs   # MultilineMerger：多行合并迭代器适配器（合并续行如栈追踪）
├── crash.rs       # CrashDetector：崩溃识别 + CrashInfo 结构化提取
├── dedupe.rs      # Normalizer（消息归一化）+ Deduper（去重分组）
├── sampler.rs     # Sampler：输出采样（--tail 尾部 / --sample 均匀抽样）
├── histogram.rs   # Histogram：时间窗口聚合（--histogram 10s/1m/5m），JSON 输出
├── formatter.rs   # 输出格式化：text（彩色）/ json / csv，支持 --fields 字段选择
├── logcolor.rs    # 颜色语义数据（RGB 常量 + Badge 映射），不依赖 colored/ratatui，供 formatter.rs 与 aloggrep-tui::theme 共用
└── summary.rs     # 聚合统计：级别分布、Top tags、Top errors、崩溃计数

aloggrep-tui/src/
├── main.rs         # CLI 入口、终端生命周期、事件循环、Leader/Picker/普通按键分发
├── model.rs        # EntryRow：拥有所有权的行模型，from_line() 解析，as_log_entry() 借出给 core 匹配逻辑复用
├── filter_model.rs # Group（chips + AND Expr + enabled；same_as 去重）+ GroupList（组间 OR；全禁用≡空列表≡全可见）+ TimeBound（全局时间窗匹配）
├── time_panel.rs   # `-f` 全局时间窗面板（`ts`）：日期候选自 `rows` + HH:MM:SS 键入/夹紧
├── highlight_model.rs # HighlightGroup（单 pattern）/HighlightGroupList（组间高亮 OR）+ Highlight compose 逻辑
├── input.rs        # ChipField/Chip/InputBox（Enter 两段式：收 pill / 提交组）/Popup + build_group（复用 Expr::from_filters）
├── app.rs          # App 状态机：rows/matched/visible/groups/highlight_groups/time_bound/Focus/following/pending_leader/picker
├── picker.rs       # fzf PickerSession：ActionList/Manage/New/Edit/删除确认
├── ui.rs           # 渲染：log/strip + fzf 左右面板/确认框/Preview/Time 面板 + status_bar
├── help.rs         # H6 二级键位短提示：L1 前缀键 / L2 operator-pending；`键:短名`；供 status_bar 右侧截断展示；业务反馈走 App::set_flash（3s）
├── export.rs       # H10：当前 Filter/Exclude/lock/time_bound → 一行 `aloggrep` CLI（`yc`）
├── bookmark.rs     # M2：会话书签（row_id 锚定；ma/md；顶区展示 + Picker 管理）
├── config.rs       # 配置目录解析 + theme.toml/config.toml 加载（坏文件回退）
├── theme.rs        # UI 颜色映射唯一入口：运行时 UiTokens（可 theme.toml 覆盖）+ 日志色派生自 aloggrep::logcolor
├── store.rs         # RowStore：FileStore（mmap+行索引+惰性解析）/ StreamStore（--hdc VecDeque）/ RowRef
└── ingest.rs        # spawn_hdc_ingest（DropOldestRing）；spawn_file_ingest 仅供测试（生产 -f 走 FileStore）
```

**CLI 数据流：** stdin/file → 逐行读取 → [MultilineMerger] → `LogEntry::parse()` → `FilterChain::matches()` → [CrashDetector] → `Formatter::write_entry()` / `Summary::record()`

**TUI 数据流：**
- **`-f`：** `FileStore::open` → mmap + 后台行索引 → `Visible::All { len }`；filter 时后台扫描 → `Visible::Subset(命中行号)`；渲染经 `App::row_at` 惰性解析（无全文件 owned `EntryRow`，无 `max_lines` 淘汰）。
- **`--hdc`：** 后台线程 → `EntryRow::from_line()` → `DropOldestRing` → `App::drain()` → `StreamStore`；filter 未激活时 `Visible::All` 索引 `rows`；激活时命中双写 `matched`、`Visible::All` 索引 `matched`。

### Key Design Decisions

- **`LogEntry<'a>` 零拷贝解析**：所有字段（timestamp, pid, tid, tag, pkg, msg）均为 `&'a str`，直接引用原始行，避免堆分配。`parse()` 依次尝试 hilog → threadtime → xlog → brief 四种格式。
- **`FilterChain::from_cli(&Cli)`** 是 CLI 唯一的过滤器构建入口，将 CLI 参数（tag/msg/level/pid/tid/since/until/-e）统一转换为内部过滤链。TUI 不用 `FilterChain`，改用更轻量的 `Expr::from_filters` + `Group`/`GroupList`（见下）。
- **main.rs `dispatch_lines!` 宏**：根据 `--multiline`/`--crashes` 标志决定是否用 `MultilineMerger` 包裹迭代器，避免运行时分支开销。
- **输出路径分支**：`run_simple`（常规快速路径）vs `run_with_context`（-C/-A/-B 上下文行缓冲）vs `run_time_context`（--time-context 两遍扫描）vs `run_follow`（--follow-pid/tid 两遍扫描）。
- **`--hdc` Ctrl-L 清屏（CLI）**：仅在 stdin/stdout 都是 tty 时启用；用 cbreak 模式（保留 `ISIG`）而非标准 raw mode，避免破坏现有 Ctrl+C 依赖的 `SIGINT` 语义。按键上报走 channel + `KeypressGate` 迭代器分发。仅支持 Unix，Windows 上静默不可用。已知权衡：若进程被 `SIGTERM`/`SIGKILL` 直接杀死（而非 Ctrl+C），termios 不会被恢复，终端会卡在 cbreak 模式，需手动 `stty sane`/`reset`——这与 vim/less 等直接操作终端的工具在被强杀时的行为一致，未特殊处理。
- **`aloggrep-tui` 的 chip 过滤模型**：`Vec<Group>`，`Group` 内 chip 之间 AND（内部编译为一个 `Expr`），`Vec<Group>` 之间 OR。Input：`Space` 进草稿（可含空格）；有草稿时 `Enter` 收成 pill；无草稿且已有 pill 时 `Enter` 提交组。提交前按 chip 多重集（ignore-case）去重，重复则不 push。启动 CLI 过滤转为第 0 组（可 `dd`/`di`）。chip 编译走 `Expr::from_filters(..., SameFieldOp::And)`；启动 `initial_group` 仍用 `SameFieldOp::Or`。**TUI 过滤/搜索一律 ignore-case**。LogList 另有 **H7 光标→Chip**：operator `c`+字段字母（`t/m/g/p/T/l`，与 `YankField` 对齐）从当前行推单 chip 组；`c`+`m` 开 msg 切词候选面板；成功后 `following=false`，Esc 只清 pending 不 resume。**H8 会话 lock**：`App.lock_pid`/`lock_tid` 互斥，在 chip 过滤后 AND；operator `f`+`p`/`t`/`u`（toggle 同值清除）；status `LOCK pid=…` 与 FOLLOWING 可并存；Esc resume 不清除 lock。**全局时间窗（仅 `-f`）**：`App.time_bound: Option<TimeBound>` 与 Filter 组正交，在 chip/exclude/lock 之后 AND；启动 `--since`/`--until` 写入全局窗（不再挂 Group）；operator `t`+`s` 开靠上 Time 面板（日期候选自 `rows` 去重、只能选自候选；时间 `HH:MM:SS` 键入并夹到该日缓冲 min/max，保证 since≤until；允许只设一端，端内须日+时成对），`t`+`u` 清除；无日期候选时 `ts` flash 拒绝；`--hdc` 硬隐藏 `t`/`ts`/`tu`；status `TIME …` 徽标；计入 `filter_active`；`yc` 导出 `--since`/`--until`；打开/提交/`tu` → `following=false`，面板 Esc 不 resume。
- **`aloggrep-tui` 的统一 fzf Picker**：Normal `Space` 进入 Leader；`Space Space` 打开 ActionList（Filter / Highlight / Exclude / Bookmark）；裸键 Manage：`;`→Filter、`/`→Highlight、`` ` ``→Exclude、`mm`→Bookmark；Shift 对应字符强制 New：`:`/`?`/`~`/`MM`；`Space f/s/m/x` 仍为别名（有候选 Manage / 无候选 New）。Picker 为左右 4:6（可由 `config.toml` 的 `picker_left_ratio` 调整），左侧候选+底部检索（mode 前缀图标：Manage `>` / New `＋` / Edit `✎`），右侧 Preview；Manage 下键入无匹配时自动切 New（query→draft；清空草稿回退 Manage）；手动进 New 不清空也不回退；Esc / 提交成功 Enter 一律关闭面板（不回 Manage）；Manage 内 Ctrl-X 编辑、Delete/Ctrl-Backspace 删除选中（二次确认）；草稿行支持中间光标与 ←/→/Home/End/Ctrl-A/E/Ctrl-U（New/Edit 另有 Ctrl-Backspace 删词）。**Filter/Highlight/Exclude/Bookmark 统一**：候选为空时打开即 New；有候选且未强制 New 则 Manage；msg-chip 也复用 Picker 壳。
- **`aloggrep-tui` 的 RowStore**：`App.store` 为 `File`（`-f` mmap）或 `Stream`（`--hdc`）。Stream 的 `rows` 按 `max_lines`（默认 500_000，`--max-lines` 仅 hdc）淘汰；File 无淘汰、可浏览全文件。`Visible::All`（身份映射）用于 Stream 与未过滤 File；File 过滤用 `Visible::Subset(行号)`。读路径统一 `App::row_at` → `RowRef`（Stream 借出 / File 惰性 Owned）。
- **`aloggrep-tui` 的匹配行保留（Stream）**：filter active 时命中双写 `rows`+`matched`，`Visible::All` 索引 `matched`；`matched` 硬上限 `MATCHED_HARD_CAP`（1_000_000）。File 不过滤进 `matched`，只维护 `Subset`。书签：Stream 查 `rows`/`matched`；File 用稳定 `row_id = line_index+1`。preview 对 Stream 扫 `rows`、对 File 惰性 `row_at`。
- **`aloggrep-tui` 的 Following**：任意 LogList 手动操作（`j/k/J/K`、滚轮、`g/G`、`n/N`、Visual、搜索跳转）一律 `following=false`；**仅 `Esc`（及同等取消路径）** `resume_following`（钉底并恢复）。`G` 只跳底不恢复。
- **`aloggrep-tui` 的 LogList 滚动跟随**：`ui::render_log_list` 每帧用持久化的 `App.list_offset` 驱动 ratatui `List` 视口。
- **`aloggrep-tui` 的 LogList 作为行动原点**：`Esc` / Insert 取消 / 提交 Filter 组 → `Focus::LogList` 并恢复 following；HighlightBox `Enter` 上屏后跳到首命中（退出 following）；`dd` 删光 strip 后回 LogList。popup 打开时 `Esc` 只关 popup。
- **`aloggrep-tui` 的 LogList 快速移动与鼠标滚轮**：`Shift+J`/`Shift+K` 各 7 行；滚轮各 3 行，始终作用于日志列表。
- **`aloggrep-tui` 的终端生命周期**：panic hook 在 `main()` 最开始安装；`--hdc` 子进程经 `HdcChildGuard` RAII 清理。
- **`aloggrep-tui` 的 Ctrl+C 语义**：Normal 退出；Insert 有 popup 只关 popup，否则重置 Input 并回 LogList；Search 模态编辑中取消草稿并回 LogList（不 resume following）。
- **`aloggrep-tui` 的五个可聚焦分区**：`Focus` 为 `ChipStrip`/`ExcludeStrip`/`HighlightStrip`/`LogList`/`Input`（数字键 1–5）；Filter/Exclude/Highlight strip **为空则折叠**。持久化条件的新增/编辑统一走 Leader fzf Picker，不再用 `a`/`i`/`o` 打开旧 Input 面板。布局：Filter → Exclude → Highlight → Log（Fill）→ status。**H9 排除**：`GroupList.excludes` 全局 AND NOT；`C`+字段（字母表同 H7）；Exclude strip 与 Filter 同构 `h/l/dd/di`。
- **跨 crate 的日志颜色统一**：`aloggrep-core::logcolor` 是唯一的颜色数据源（纯 RGB/枚举，不依赖 `colored`/`ratatui`），CLI 的 `formatter.rs`（`colored`）和 TUI 的 `theme.rs`（`ratatui::style::Color`）各自将其转换成自己的类型。新增/调整日志相关配色只改 `logcolor.rs`，两边自动同步，禁止在 `formatter.rs`/`theme.rs` 里各自硬编码一份 RGB 数值。`USER_HIGHLIGHT` 为 8 档阅读向递进色阶，供 CLI `--highlight` 与 TUI highlight chip 共用。
- **`aloggrep-tui` 日志区默认多行展示**：`ui.rs::wrap_ranges` 是唯一的换行实现（贪婪按空白断行，单词超宽则硬切），操作字节区间而非 `Cow<str>`（为了跟 `render_entry_lines` 里已经用 `Regex::find_iter` 算好的高亮命中区间对齐——顺序是"先算高亮区间，再换行"，换行只是把同一份区间数据切成多个 `Span` 分布到多个 `Line`，不会把一个高亮命中切碎到两半）。`ListItem` 内可以放多个 `Line`，`ListState` 选中/滚动天然按整个 item 处理，翻页逻辑（`PAGE_SIZE`/`move_cursor_manual`）不需要感知 item 内部行数。
- **`aloggrep-tui` 靠上模态 + Preview（H1）**：Input / Search 用 `top_modal_rect`（靠上，非垂直居中）；垂直栈为模态正文 → 字段/历史候选 → **Preview**（`preview.rs` 采样约 10 条，不改主 `visible`/`following`）。Search 淡高亮走 `theme::preview_highlight_style`。msg-chip 面板仍居中。
- **`aloggrep-tui` 字段详情 / Pretty overlay（H4/H5）**：LogList `p` 开关浮层（开→Fields，关→Closed）；`P` 开 Pretty 或在 Fields↔Pretty 间切换；靠上 `render_modal_shell`；Pretty 对 msg（失败再试 raw）做 JSON 缩进，非法则原文 +「非 JSON」；内容随 `current_row`；Esc **只关浮层**不 `resume_following`；浮层内 `j`/`k`/`c`/`C`+字段仍可用。
- **`aloggrep-tui` 导出 CLI（H10）**：LogList `y` `c` 将当前启用 Filter 组（组内 AND、组间 OR）、启用 Excludes（`not …`）、H8 lock（`--pid`/`--tid`）、全局时间窗（`--since`/`--until`）编码为一行 `aloggrep -f…|-i` / `--hdc` 命令（统一 `-e` + `-i`；不含 Search / `di` 禁用项）；复用 yank 剪贴板与 `YANKED` status。近似一致即可（环形缓冲截断可接受）。
- **`aloggrep-tui` 边轨 minimap（H3）**：Log 边框内侧 1 列只读轨；比例基准为 `visible`；标记严重(E/F/crash)、启用 search 命中、当前视口淡段；重叠时严重优先；`visible` 非空即画极淡轨；样式走 `theme::minimap_*`；每帧扫描预算约 4000。
- **`aloggrep-tui` 书签（M2）**：ingest 单调 `EntryRow.row_id`；`ma` 收藏当前行、`md` 删除当前行书签，`mm` 以 Manage / `MM` 以 New 打开 Bookmark Picker，`Space m` 同 Manage；Log 顶内嵌最多 3 条最近书签（空则折叠，软上限 50）；行同时在 `rows`/`matched` 任一存活即可跳，两个缓冲均淘汰才失效不可跳；进程退出丢弃。
- **`aloggrep-tui` 配置外置**：启动时从配置目录读取 `theme.toml` 与 `config.toml`（默认 `~/.config/aloggrep`，`$ALOGGREP_HOME`/`--config-path DIR` 可覆盖）；`theme.toml` 仅覆盖 UI token，`config.toml` 配置 `picker_left_ratio`；**不**覆盖 `logcolor`。示例见 `aloggrep-tui/examples/`。
- **`aloggrep-tui` 边轨 minimap（H3）**：Log 边框内侧右侧 1 列；比例相对当前 `visible`；标记视口淡段 / 启用 search 命中 / 严重(E/F/crash)，重叠时严重优先；`visible` 非空即画极淡轨；样式仅走 `theme::minimap_*`；每帧采样上限约 4000。

## aloggrep-tui UI 设计指导（opencode 风格）

配色与布局遵循以下规则；改动渲染代码前先看这里。所有颜色常量与映射函数集中在 `aloggrep-tui/src/theme.rs`——**禁止在 `ui.rs` 或其他渲染代码里直接写 `Color::*`/硬编码 `Style`**，新增语义就去 `theme.rs` 加常量或函数，保证同一语义在任何地方渲染出来的颜色都一致。

- **Strip/Log 套圆角边框，未聚焦低对比度**：Filter/HighlightStrip/Log 各自一层 `BorderType::Rounded`，`theme::border_style(active)`：聚焦时 `fg(ACCENT)`，未聚焦时 `fg(DarkGray)+DIM`。边框标题用 `theme::numbered_title(1|2|3, label, active)`。Input/Search **居中模态**与字段候选浮层用 `theme::plain_title` + 同一套 `render_modal_shell`（始终 active 强调色）。
- **单一强调色 + 大量 dim**：`theme::ACCENT`（Cyan）是唯一的"焦点/强调"色，非关键信息统一 `Modifier::DIM`，避免多色混战。
- **焦点**：popup/候选 List 用 `theme` 候选行 tokens（选中/非选中背景与文字、匹配字符色、选中前缀）；Filter/Highlight strip 组选中为 Magenta（`SELECTION_FRAME`）、未选中为 dim DarkGray；`di` 禁用用 `disabled_chip_style()`。
- **日志行选中态只在 LogList 聚焦时显示，且是柔和灰底**：经 `ListItem::style(log_selection_style())` 施加（**不用** `List::highlight_style`，以免 `Style::patch` 盖掉关键词高亮底色）；失焦时无选中底。关键词 Span 的高亮色在选中行上保持可读叠加。
- **状态提示用反色徽标**：状态栏 `FOLLOWING` 等为反色块。Picker 底部用柔和 `theme::picker_mode_prefix`（`>` / `＋` / `✎`，accent+DIM、无填充）；非编辑不画居中模态。
- **Filter/Highlight chip 用填充 pill**：`theme::chip_pill`（按 `field_color` / level badge）与 `theme::highlight_pill_style`（`highlight_style`）；Input 已提交 chip 与 Filter strip 共用 pill 样式。
- **字段名颜色全局唯一映射**：`ChipField` 的颜色只由 `theme::field_color` 决定；popup 字段名与 pill 背景同源。
- **不硬编码 White/Black 当默认前景色**：如 `Level::I`（默认级别）用 `Style::default()` 继承终端主题色，不写 `Color::White`，以兼容浅色/深色终端。
- **日志相关颜色（时间戳/level 徽标/关键词高亮）一律从 `aloggrep::logcolor` 派生**，不在 `theme.rs` 里另起一套数值——保证 TUI 和 CLI 的彩色输出视觉一致，见 Key Design Decisions 里的"跨 crate 的日志颜色统一"。

语义色表（对应 `theme.rs` 常量，供扩展新 UI 元素时复用）：

| 用途 | 常量/函数 | 颜色来源 |
|------|-----------|------|
| 强调/焦点/聚焦边框 | `theme::ACCENT` | Cyan |
| 成功/Following | `theme::SUCCESS` | Green |
| 会话 lock 徽标 | `theme::LOCK` | Magenta |
| 警告（chip 字段名） | `theme::WARNING` | Yellow |
| Filter chip pill | `theme::chip_pill` | `field_color` / `level_badge_style` |
| Highlight pattern pill | `theme::highlight_pill_style` | `highlight_style(idx)` |
| Picker 候选选中/匹配 | `theme::candidate_*` / `picker_mode_prefix` | theme.toml 可覆盖 |
| Chip 组圆角边框 | `theme::chip_group_border_style` | 选中 Magenta / 未选中 dim DarkGray |
| 日志时间戳/pid/tid | `theme::muted()` | `logcolor::MUTED` |
| 日志 level 徽标 | `theme::level_badge_style()` | `logcolor::level_badge()` |
| 关键词/搜索高亮 | `theme::highlight_style(idx)` | `logcolor::USER_HIGHLIGHT[idx]`，按 search pattern 全局序号递进 |
| 禁用 chip 组 | `theme::disabled_chip_style()` | DarkGray+DIM |
| 日志行选中态（仅聚焦时） | `theme::log_selection_style()` | `Color::DarkGray`，经 ListItem.style 施加 |

## Filter Logic（`aloggrep-core` CLI）

- 同类型多值 = OR：`--tag "A" --tag "B"` → tag=A OR tag=B
- 同类型 AND：`--tag "A" --tag "B" --and` → tag=A AND tag=B
- 跨类型 = AND：`--tag "A" --msg "err"` → tag=A AND msg~err
- 值内 `|` 也是 OR：`--tag "A|B"`
- `--level W` 匹配 W/E/F（最低级别）
- `-e` 布尔表达式：支持 `and`/`or`/`not`/括号的任意组合
  - 语法：`FIELD ~ VALUE`、`level >= LEVEL`，用 `and`/`or`/`not`/`()` 组合
  - FIELD = `tag` | `msg` | `pkg` | `pid` | `tid`；VALUE = 裸词或 `"引号字符串"`
  - 多个 `-e` 之间 OR（与 grep `-e` 一致），与其他 flag AND

## aloggrep-tui 已知范围外事项（YAGNI）

- 任意 `stdin` 管道流式输入（只支持 `--hdc` 自 spawn 子进程，因子进程生命周期完全自控，避免 stdin 管道场景下 Ctrl+C/`ISIG` 语义复杂度）
- 多文件 glob、`--sort-time` 归并；`--fields` 列可配置；搜索/过滤历史；光标行派生设时间；`--hdc` 交互时间窗；相对时间（last 5m）；全文件日期索引；Windows 专门支持
- 日志区 `Ctrl-d`/`Ctrl-u` 翻页；草稿 vim 模态编辑——留作后续扩展

## Exit Codes（`aloggrep-core` CLI）

- `0` — 有匹配
- `1` — 无匹配
- `2` — 参数错误
