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

测试分布（`aloggrep-tui`，位于 `aloggrep-tui/`）：全部为源文件内 `#[cfg(test)]` 单元测试，覆盖 `model`/`filter_model`/`ingest`/`app`/`ui`/`input`/`main`（`dispatch_tests`）各模块。

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
├── main.rs         # CLI 入口（clap derive）、panic hook、终端 raw mode/alt screen 生命周期、事件循环 run()、按键分发（handle_normal_key/handle_insert_key/handle_ctrl_c/handle_search_draft_key）、popup_rect 浮层定位
├── model.rs        # EntryRow：拥有所有权的行模型，from_line() 解析，as_log_entry() 借出给 core 匹配逻辑复用
├── filter_model.rs # Group（一个 AND 组合的 Expr + 只读 TimeBound）+ GroupList（组间 OR）
├── input.rs        # ChipField/Chip/InputBox（草稿+已提交 chip）/Popup（字段候选框）+ build_group（复用 Expr::from_filters）
├── app.rs          # App 状态机：环形缓冲 rows/visible 下标列表/cursor/Focus/Mode/group_cursor/pending_dd/following/highlight/search_draft
├── ui.rs           # 渲染：render_log_list（换行+关键词高亮）/render_chip_strip/render_input_box/render_search_box/render_popup（下拉浮层）/render_status_bar，颜色一律经 theme.rs
├── theme.rs        # UI 颜色映射唯一入口：语义色常量 + border_style/numbered_title/plain_title/caret/level_badge_style/highlight_style/field_color/focus_style/mode_badge，日志相关部分派生自 aloggrep::logcolor
└── ingest.rs        # 统一摄入管线：spawn_file_ingest（-f，io::Result 立即报告打开失败）/ spawn_hdc_ingest（--hdc，复用 aloggrep-core::hdc）
```

**CLI 数据流：** stdin/file → 逐行读取 → [MultilineMerger] → `LogEntry::parse()` → `FilterChain::matches()` → [CrashDetector] → `Formatter::write_entry()` / `Summary::record()`

**TUI 数据流：** 后台线程（文件一次性读完 / `--hdc` 持续读取）→ `EntryRow::from_line()` → `mpsc::channel` → `App::drain()` 增量过滤追加可见下标（O(1)）→ `render_log_list` 渲染；过滤条件（`Vec<Group>`）变化时 `App::rebuild_visible()` 全量重扫（O(n)）。

### Key Design Decisions

- **`LogEntry<'a>` 零拷贝解析**：所有字段（timestamp, pid, tid, tag, pkg, msg）均为 `&'a str`，直接引用原始行，避免堆分配。`parse()` 依次尝试 hilog → threadtime → xlog → brief 四种格式。
- **`FilterChain::from_cli(&Cli)`** 是 CLI 唯一的过滤器构建入口，将 CLI 参数（tag/msg/level/pid/tid/since/until/-e）统一转换为内部过滤链。TUI 不用 `FilterChain`，改用更轻量的 `Expr::from_filters` + `Group`/`GroupList`（见下）。
- **main.rs `dispatch_lines!` 宏**：根据 `--multiline`/`--crashes` 标志决定是否用 `MultilineMerger` 包裹迭代器，避免运行时分支开销。
- **输出路径分支**：`run_simple`（常规快速路径）vs `run_with_context`（-C/-A/-B 上下文行缓冲）vs `run_time_context`（--time-context 两遍扫描）vs `run_follow`（--follow-pid/tid 两遍扫描）。
- **`--hdc` Ctrl-L 清屏（CLI）**：仅在 stdin/stdout 都是 tty 时启用；用 cbreak 模式（保留 `ISIG`）而非标准 raw mode，避免破坏现有 Ctrl+C 依赖的 `SIGINT` 语义。按键上报走 channel + `KeypressGate` 迭代器分发。仅支持 Unix，Windows 上静默不可用。已知权衡：若进程被 `SIGTERM`/`SIGKILL` 直接杀死（而非 Ctrl+C），termios 不会被恢复，终端会卡在 cbreak 模式，需手动 `stty sane`/`reset`——这与 vim/less 等直接操作终端的工具在被强杀时的行为一致，未特殊处理。
- **`aloggrep-tui` 的 chip 过滤模型**：`Vec<Group>`，`Group` 内 chip 之间 AND（内部编译为一个 `Expr`），`Vec<Group>` 之间 OR。启动时的 CLI 过滤参数被转换为第 0 组，与交互式新增的组同等对待（可被 `dd` 删除），不存在额外的 AND 层。chip 编译直接调用 `Expr::from_filters`（不走文本拼接再 `Expr::parse` 的回路），避免值中引号转义问题，同字段多值天然走 `compile_joined` 的 OR 合并。
- **`aloggrep-tui` 的环形缓冲与光标**：`App.rows: VecDeque<EntryRow>` 按 `max_lines` 淘汰最旧行；`App.visible: Vec<usize>` 始终保持升序，淘汰只可能发生在下标 0，故 `push_row` 无需全表扫描；`rebuild_visible`（过滤条件变化时）与 `follow_tick`（新行到达/环形缓冲淘汰时）共同维护 "following 模式下 cursor 始终指向可见集合最后一行" 的不变量。
- **`aloggrep-tui` 的终端生命周期**：panic hook 在 `main()` 最开始安装，先于任何 raw mode/alt screen 操作；`--hdc` 子进程通过 `HdcChildGuard`（RAII，`Drop` 时 kill+wait）持有，保证无论从哪个 `?` 分支提前返回，子进程都不会残留。
- **`aloggrep-tui` 的 Ctrl+C 语义**：raw mode 下 `ISIG` 被禁用，Ctrl+C 以普通 `KeyEvent` 到达而非 `SIGINT`，因此显式处理为与 `Esc` 语义一致的"就地取消"（Normal 模式下退出整个程序；Insert 模式下弹窗打开时仅关闭弹窗，否则重置输入框回到 Normal；搜索草稿输入中则取消搜索），而不是无条件退出。
- **`aloggrep-tui` 的三个可聚焦分区**：`Focus` 仍是 `ChipStrip`/`LogList`/`Input` 三态（未新增/合并），`Tab`/`BackTab` 循环切换，`1`/`2`/`3` 直达（仅 Normal 态生效，只切换键盘焦点，不触发编辑——进入编辑态唯一入口仍是 `i`/`a`/`o`）。三个区域各自套一层圆角 `Block`，边框/标题颜色由 `theme::border_style`/`numbered_title` 按 `app.focus` 是否匹配决定。`/` 搜索草稿是三分区之外独立常驻的第四个边框区域（`render_search_box`），不参与编号和 Tab 循环，边框亮暗改用 `app.search_draft.is_some()` 判断。
- **跨 crate 的日志颜色统一**：`aloggrep-core::logcolor` 是唯一的颜色数据源（纯 RGB/枚举，不依赖 `colored`/`ratatui`），CLI 的 `formatter.rs`（`colored`）和 TUI 的 `theme.rs`（`ratatui::style::Color`）各自将其转换成自己的类型。新增/调整日志相关配色只改 `logcolor.rs`，两边自动同步，禁止在 `formatter.rs`/`theme.rs` 里各自硬编码一份 RGB 数值。
- **`aloggrep-tui` 日志区默认多行展示**：`ui.rs::wrap_ranges` 是唯一的换行实现（贪婪按空白断行，单词超宽则硬切），操作字节区间而非 `Cow<str>`（为了跟 `render_entry_lines` 里已经用 `Regex::find_iter` 算好的高亮命中区间对齐——顺序是"先算高亮区间，再换行"，换行只是把同一份区间数据切成多个 `Span` 分布到多个 `Line`，不会把一个高亮命中切碎到两半）。`ListItem` 内可以放多个 `Line`，`ListState` 选中/滚动天然按整个 item 处理，翻页逻辑（`PAGE_SIZE`/`move_cursor_manual`）不需要感知 item 内部行数。
- **`aloggrep-tui` 字段候选下拉浮层**：`render_popup` 不再占用固定行高的一行，而是在 `main.rs::popup_rect` 里按 `input_area` 底边动态算一个悬浮 `Rect`（`Clear` + 圆角 `List`），高度按候选数量 clamp 到 `[1,6]+2`，并 clamp 到不超过 frame 剩余高度——浮层会盖住下方 search 区甚至日志区顶部，这是有意的（跟 Claude Code 的 `/` 命令下拉一致），不是 bug。

## aloggrep-tui UI 设计指导（opencode 风格）

配色与布局遵循以下规则；改动渲染代码前先看这里。所有颜色常量与映射函数集中在 `aloggrep-tui/src/theme.rs`——**禁止在 `ui.rs` 或其他渲染代码里直接写 `Color::*`/硬编码 `Style`**，新增语义就去 `theme.rs` 加常量或函数，保证同一语义在任何地方渲染出来的颜色都一致。

- **三个可聚焦分区套圆角边框，未聚焦低对比度**：ChipStrip/LogList/Input 各自一层 `BorderType::Rounded`，`theme::border_style(active)`：聚焦时 `fg(ACCENT)`，未聚焦时 `fg(DarkGray)+DIM`（终端没有真 alpha，用 dim 模拟"半透明未聚焦"）。边框标题（`Block::title()`，天然画在左上角）用 `theme::numbered_title(1|2|3, label, active)` 带数字徽标；不参与编号的区域（Search、字段候选浮层）用 `theme::plain_title`。
- **单一强调色 + 大量 dim**：`theme::ACCENT`（Cyan）是唯一的"焦点/强调"色，非关键信息统一 `Modifier::DIM`，避免多色混战。
- **焦点用反色块，不用边框变色**：当前选中的 chip / popup 候选项用 `theme::focus_style()`（黑字 + ACCENT 底，加粗）标出，而不是给文字加粗变色或画边框。
- **状态/模式提示用反色徽标**：输入框左侧的 `NORMAL`/`INSERT` 徽标（`theme::mode_badge`）、状态栏的 `FOLLOWING` 提示都是反色块，不是纯文字提示。搜索框/输入框的编辑态用 `theme::caret`（方块=静止/Normal，竖线=正在编辑/Insert，仿 vim 光标形状），只在该区域自己判定为"活跃"时才画，不画在非活跃区域上。
- **字段名颜色全局唯一映射**：`ChipField` 的颜色只由 `theme::field_color` 决定；chip 栏、输入框、popup 三处渲染字段名时必须复用同一函数，不允许各自维护一份颜色表。
- **不硬编码 White/Black 当默认前景色**：如 `Level::I`（默认级别）用 `Style::default()` 继承终端主题色，不写 `Color::White`，以兼容浅色/深色终端。
- **日志相关颜色（时间戳/level 徽标/关键词高亮）一律从 `aloggrep::logcolor` 派生**，不在 `theme.rs` 里另起一套数值——保证 TUI 和 CLI 的彩色输出视觉一致，见 Key Design Decisions 里的"跨 crate 的日志颜色统一"。

语义色表（对应 `theme.rs` 常量，供扩展新 UI 元素时复用）：

| 用途 | 常量/函数 | 颜色来源 |
|------|-----------|------|
| 强调/焦点/聚焦边框 | `theme::ACCENT` | Cyan |
| 成功/Following | `theme::SUCCESS` | Green |
| 警告（chip 字段名） | `theme::WARNING` | Yellow |
| 日志时间戳/pid/tid | `theme::muted()` | `logcolor::MUTED` |
| 日志 level 徽标 | `theme::level_badge_style()` | `logcolor::level_badge()` |
| 关键词/搜索高亮 | `theme::highlight_style(idx)` | `logcolor::USER_HIGHLIGHT[idx]`，`/` 搜索固定用 `idx=0` |

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
- 多文件 glob、`--sort-time` 归并；`--fields` 列可配置；搜索/过滤历史；行详情浮层；时间范围 chip 的交互式新增（依赖 `expr.rs` 语法扩展）；Windows 专门支持
- 日志区 `Ctrl-d`/`Ctrl-u` 翻页、输入框内 `h`/`l` 浏览已输入文本（`InputBox.draft` 目前是仅追加/仅尾部删除的缓冲，不支持中间光标定位）——设计文档中提及但未在 v1 实现，留作后续扩展

## Exit Codes（`aloggrep-core` CLI）

- `0` — 有匹配
- `1` — 无匹配
- `2` — 参数错误
