# aloggrep 待办与方向

本文档分两块：

1. **CLI P1（历史）** — 面向 AI agent 日志分析的 CLI 能力，多数已完成，保留作背景。
2. **TUI 体验增强（现行）** — 借鉴 [lnav](https://lnav.org) 的**交互与架构思想**（非功能移植），后续开发以本节为准，**不必再阅读 lnav 源码**。

---

## 产品边界（读此节即可，无需读 lnav）

| | lnav（对照物） | aloggrep（本项目） |
|--|----------------|-------------------|
| 定位 | 通用日志「IDE」：多格式、多文件、SQL/脚本 | Android/鸿蒙 logcat「手术刀」：轻量、快、场景专一 |
| 核心抽象 | 时间有序的**消息索引**；UI/SQL 都是索引的投影 | `rows`（环形缓冲）+ `visible` + chip/`Expr` 过滤 |
| 过滤 UX | regex in/out、SQL 表达式、输入时预览效果 | Filter chip 组内 AND / 组间 OR；Search chip 高亮 OR |
| 明确不做 | — | 内嵌 SQLite/PRQL、通用 70+ 格式生态、SSH/URL 远程协议、完整多 View 栈（LOG/DB/TIMELINE…） |

**三条可落地的设计原则（从 lnav 提炼，已本地化）：**

1. **索引是中心，视图是投影** — 今日 `rows`/`visible` 已是雏形；直方图跳转、书签、多文件合并时，应把「消息索引」从渲染列表中抽离，避免逻辑堆在 `ui.rs`。
2. **先预览再提交** — 与现有 Input/Search 居中模态、chip Enter 两段式契合；draft 阶段应能看见「将隐藏 / 将命中」，而不只是改内部状态。
3. **自动化能省则省，扩展能声明则声明** — 不追功能广度；把主题/键位/会话等用户可配置面与解析/过滤引擎分开。

**建议落点（模块）：** `aloggrep-tui` 的 `app.rs` / `ui.rs` / `input.rs` / `search_model.rs` / `theme.rs`；分析能力可复用 `aloggrep-core` 已有 histogram/crash/`Expr`。

---

# TUI 体验增强（现行待办）

状态约定：`[ ]` 未做 · `[~]` 进行中 · `[x]` 已完成

---

## 高价值（优先）

与现有 vim-TUI + chip 模型增量兼容，体感提升大、不必引入命令语言或 SQL。

### H1. 操作预览（filter / search draft） `[x]`

**用户价值：** 改过滤或搜索时，确认前就能看到「提交后长什么样」，减少反复 Enter → 重建 visible → 不满意再改。

**已定决议（头脑风暴 2026-07-20）：**

| 决策点 | 决议 |
|--------|------|
| 预览载体 | Input/Search 弹层下另缀 **Preview 窗**（非主 Log dim/划掉） |
| 模态位置 | Input / Search **靠上**（不再垂直居中） |
| 垂直栈 | 模态正文 → 字段/历史候选（有则显示）→ **Preview** |
| 展示哲学 | 尽量展示 **最终结果**：Filter=过滤后仍可见行；Search=将命中行+淡高亮 |
| 采样 | 以当前选中行为锚的上下文，约 **10** 条结果；扫描设上限防卡顿 |
| Filter 条件叠 | 已生效 Filter∧Exclude∧H8 lock ∧ 本模态 pills ∧ **当前 draft 估算** |
| 空 Search 草稿 | Preview 折叠或占位「输入以预览」 |
| Following | 预览期不改 `following`；主 Log 的 `visible` 不因 draft 重建 |

**本项目语境：**

- 提交后行为保持现状：全量重建 / `jump_first_match`。
- Search 淡高亮与正式 `USER_HIGHLIGHT` 色阶区分，走 `theme`（如 `preview_highlight_style`）。
- 排除模式（H9）：预览按将提交的 excludes 叠加计算。
- UI 风格与现有 modal/popup 壳一致（圆角 + `plain_title`）。

**非目标：** 主列表 dim/删除线预览；lnav 式命令语言；每键全量 `rebuild_visible`。

**实现提示：**

- 抽出与 `rebuild_visible` 同构的「给定临时条件 → 行是否可见/是否命中」纯函数，供 Preview 复用。
- 窄终端：候选与 Preview 高度 clamp，优先保证模态正文可读。
- 布局改动触及 `centered_modal_rect` → 需新增靠上变体（如 `top_modal_rect`），Search/Input 共用。

**验收：**

- Filter：改 draft/pill 时 Preview 显示过滤后行；Esc 后 Preview 消失，主列表不变。
- Search：draft 时 Preview 显示淡高亮命中；Enter 后正式 chip + `jump_first_match`。
- 模态靠上；候选在上、Preview 在下，无重叠错位。

---

### H2. 语义级快速跳转（Error / Fatal / Crash） `[x]`

**用户价值：** 排障时高频动作是「下一条错误」，不必手搓 `/` 或 level chip。

**本项目语境：**

- LogList Normal 模式增加跳转（建议键位：`e` / `E` = 下一条 / 上一条「严重」行）。
- 「严重」默认：`Level::E` / `Level::F`；若行已被 crash 检测标记，一并视为可跳目标。
- 仅在 **当前 `visible`** 内跳转（尊重过滤结果）；无目标时 status 提示，不报错退出。
- 与 `n`/`N`（search）独立：search 有启用 pattern 时 `n`/`N` 仍只走 search；`e`/`E` 始终走 level/crash 语义。

**非目标：** 不做可配置的「任意 SQLite 条件跳转」。

**实现提示：**

- 任意手动跳转 → `following = false`（与 `j/k/n/N` 一致）。
- 可后续扩展：仅 Fatal、或「仅 crash 块首行」；v1 用 E|F|crash 即可。

**验收：**

- 有 E/F 可见行时，`e`/`E` 循环跳转正确；过滤后只在可见集内跳。
- 与 search `n`/`N` 互不干扰。

---

### H3. 滚动条 / 边栏位置提示 `[x]`

**用户价值：** 大缓冲里一眼看出「错误/搜索命中在文件的什么相对位置」，减少盲目翻页。

**已定决议（头脑风暴 2026-07-20）：**

| 决策点 | 决议 |
|--------|------|
| 比例基准 | 当前 **`visible`**（非全量 `rows`） |
| 标记 | 严重(E/F/crash) · 启用 search 命中 · **当前视口区间** |
| 轨 | 日志边框 **内侧 1 列** |
| 重叠 | **严重色优先**；视口用更淡背景段 |
| 空轨 | `visible` 非空即画极淡轨（无标记也占位，避免布局跳动） |
| 交互 | v1 **只读**；跳转仍靠 `e`/`E`、`n`/`N` |

**非目标：** GUI 拖拽滚动条；像素级与 List 完美对齐（近似比例即可）。

**实现提示：**

- 密度过高合并/采样；每帧标记预算上限。
- 样式进 `theme.rs`（dim 轨 + 错误色/高亮色点 + 视口淡段）。
- M2 书签可后续加第三种标记色。

**验收：**

- 有 E/F/crash 或 search 命中时轨上有对应点；视口段随滚动移动。
- 大 `visible` 不显著拖慢每帧渲染。

---

### H4. 字段详情 overlay `[x]`

**用户价值：** 选中行快速看清 pid/tid/tag/pkg/level/timestamp/msg，无需在 wrap 后的长行里肉眼拆字段。

**已定决议（头脑风暴 2026-07-20；与 H5 共用浮层）：**

| 决策点 | 决议 |
|--------|------|
| 与 H5 | **同一浮层**两模式：字段 / Pretty |
| 开关键 | `p` toggle；默认进字段模式 |
| 形态 | **靠上** modal 壳（对齐 H1，非居中、非贴行） |
| Esc | **只关浮层**，不 `resume_following` |
| 光标 | 浮层开着时 `j`/`k` 等仍可移动，内容跟随选中行 |
| 同构键 | 浮层内 `c`/`C`+字段（H7/H9）与 LogList 相同 |

**本项目语境：**

- 内容来自当前选中 `EntryRow`：timestamp / level / pid / tid / tag / pkg / msg。
- 未解析行：raw + `unparsed`。
- 只读；颜色/边框走 `theme`。

**非目标：** 可编辑字段。从字段一键生成 chip 见 **H7**。

**验收：**

- 选中不同行时内容跟随；Esc 关闭后 following 不变、焦点回 LogList。
- 颜色/边框符合 theme；与 H1 靠上壳视觉一致。

---

### H5. 结构化消息 Pretty-Print `[x]`

**用户价值：** msg 里常见 JSON；pretty 后比单行 wrap 更易读。

**已定决议（头脑风暴 2026-07-20；挂在 H4 同一浮层）：**

| 决策点 | 决议 |
|--------|------|
| 入口 | 浮层内 **`P`** 在字段↔Pretty 间切换；浮层未开时 `P` = 打开并进 Pretty |
| 数据 | 对 msg（失败可回退试 raw）做 JSON pretty；不改 `EntryRow` |
| 失败 | 显示原文 + 窗内或 status「非 JSON」，不崩溃 |

**非目标：** v1 不做 XML/HTML 美化；不做整页批量 pretty；独立第二套浮层。

**验收：**

- 合法 JSON 缩进可读；非法 JSON 可退回原文。
- 与 H4 切换不打架；Esc 一次关闭整个浮层。

---

### H6. 上下文帮助（按 Focus 的键位提示） `[x]`

**用户价值：** 降低「五个 Focus + 模态」的记忆负担，接近「光标处有相关帮助」。

**本项目语境：**

- status_bar 右侧按**二级**切换短提示（统一 `键:短名`）：无 operator 时 L1（单键/前缀）；Leader `Space` 与 `m`/`f`/`c`/`C`/`y`/`d` pending 时显示 L2。文案集中在 `help.rs`。
- LogList L1 以 `Space:面板` 为统一入口；Leader L2 为 `/:列表 f:滤 s:搜 m:签 x:排除`；书签 `m` 后仅保留 `a:新增 d:删除`。
- 临时反馈（`YANKED` / `NO ERROR` / `已收藏` 等）走 flash toast，**3s 后自动隐藏**；`FOLLOWING` / `LOCK` / `VISUAL` / pending 徽标为持久状态。

**非目标：** 全屏 HELP 视图、或上下文 SQL 文档。

**验收：**

- 切换 Focus / pending / 模态时提示同步变化；终端很窄时截断或隐藏帮助；flash 到期消失。

---

### H7. 光标 → Chip（以行为源的过滤） `[x]`

**用户价值：** 纯键盘收窄的主路径——看到可疑行后 1–2 次按键生成 filter chip，不必再开 Input 手打 tag/pid。

**已定决议（头脑风暴 2026-07-20）：**

| 决策点 | 决议 |
|--------|------|
| 交互形态 | **operator-pending**：`c` + 字段字母 |
| 字段字母表 | 对齐 `YankField::from_char`：`t` tag / `m` msg / `g` pkg / `p` pid / `T` tid / `l` level |
| 不支持 | `c`+`r`/`y`（raw）、`c`+`s`（timestamp）→ status 提示；非法第二键 →「未知字段」并清 pending |
| level 语义 | **最低级别 `>=`**，与 CLI `--level` / 现有 level chip 一致 |
| 取消 | Esc 只清 pending / 关 msg 面板，**不** `resume_following`（与 Search 取消一致） |
| 成功后 | 留 LogList；`following = false`；不自动钉底 |

**本项目语境：**

- LogList（及 H4 overlay 内同构键）对当前选中行取字段 → 单 chip 组 → `push_filter_group` → 重建 visible。
- 空字段（无 tag/pid 等）时 status 提示，不 push；与现有组 **ignore-case 去重**（`same_as`）一致，重复则不 push 并提示。

**msg 特殊路径（`c`+`m`）：**

1. **切词**：连续 `[A-Za-z0-9]+` 为 token；非字母数字为分隔。
2. **候选边界**：丢弃长度 &lt; 2；去重；最多 **8** 条；**不**附带「整段 msg」项。
3. **面板**：复用现有候选 popup（`render_popup` / `candidate_popup_rect` + Up/Down/Enter|Tab）。
4. **打开时** draft 为空，先展示全部 token（≤8）；键入做 **ignore-case contains** 过滤。
5. **有候选**：Enter/Tab → 选中 token 作为 `msg` chip 并 push。
6. **无候选**（检索无命中）：Enter → **草稿全文**作为 `msg` chip 并 push。
7. 无有效 token 且草稿空：status「无可选片段」，不 push。

**与 H4 / H9 关系：**

- H4 负责「看字段」；H7 负责「从字段行动」；overlay 打开时同一套字段键生效。
- H9 是「排除」对称操作；建议 `C` 或 `!`+字段，字母表与 H7 共用（含 msg 切词面板）。

**非目标：** 多字段一次组合成复杂 AND 向导；用户仍可用 Input 模态做多 pill 组。

**实现提示：**

- 复用 `input::build_group` / `Chip` + `Expr::from_filters(..., SameFieldOp::And)`。
- msg 切词纯函数可单测（分隔、最短长、上限、去重）。
- pending `c` 状态机对齐现有 `y` / `d` operator-pending。

**验收：**

- 有 tag 的行上 `c` `t` → Filter strip 出现 pill，visible 立即收窄。
- `c` `m` → 候选面板；过滤无命中时 Enter 用草稿 push msg chip。
- `c` `l` 在 level=W 的行上 → 可见集为 W/E/F（最低级别语义）。
- 重复触发同一字段值 → 不 push，status 说明已存在。
- Esc 取消后列表与 following 不变。
- H6 提示在 LogList 下含 `c+字段` 短说明。

---

### H8. 光标 Follow pid / tid（锁定进程/线程） `[x]`

**用户价值：** 移动端排障主路径——从崩溃/错误行锁定同 pid 或 tid，沿因果链往下读；对齐 CLI `--follow-pid` / `--follow-tid` 意图。

**已定决议（头脑风暴 2026-07-20）：**

| 决策点 | 决议 |
|--------|------|
| 实现形态 | **会话级 lock**（非 Filter chip 组） |
| 状态 | `App.lock_pid` / `lock_tid: Option<String>`，**互斥**（同时最多一个有值） |
| 匹配 | `rebuild_visible` / `drain` 在 chip 过滤后再 **AND** lock |
| 键位 | operator `f`：`f` `p` 锁 pid · `f` `t` 锁 tid · `f` `u` 清除 |
| toggle | 再按同字段且当前行**同值** → 清除；不同值 → 切换目标 |
| 展示 | **仅 status_bar** 反色徽标：`LOCK pid=…` / `LOCK tid=…`（走 `theme`） |
| Esc | **不**清除 lock；`f` pending 时 Esc 只清 pending、不 resume following |

**本项目语境：**

- 文案用「锁定 / LOCK」，避免与钉底 live **following** 混淆；二者可同时存在（`FOLLOWING` + `LOCK pid=…`）。
- 空 pid/tid：status 提示，不设 lock；`f`+非法键 → status「未知」并清 pending。
- 锁定期间 live 追加行仍按 lock 过滤。

**与 H7 / H10 关系：**

- H7 的 `c` `p`/`c` `T` 仍是普通 filter 组；H8 lock 是会话约束，两者可叠加（AND）。
- H10 导出：lock → CLI `--pid` / `--tid`。

**非目标：** 多 pid OR 锁定；跨文件进程树；用 chip `dd` 清除 lock。

**实现提示：**

- pending `f` 状态机对齐 `c` / `y` / `d`。
- status 徽标与 `FOLLOWING` 并列，窄终端时截断策略与 H6 一致。

**验收：**

- `f` `p` 后 visible 仅该 pid；`f` `t` 后仅该 tid且 pid lock 已清；`f` `u` 后恢复仅受 Filter chip 约束。
- 同值再 `f` `p` → toggle 清除；异值 → 切换。
- 与钉底 `following` 可同时存在，status 可区分。
- Esc resume following 后 lock 仍在。
- H6 含 `fp/ft 锁定  fu 解锁`。

---

### H9. 排除当前（NOT / filter-out） `[x]`

**用户价值：** triage 的另一半——刷屏噪声 tag/pid 一键排除，而不只靠「再 inclusive 出想看的」。

**已定决议（头脑风暴 2026-07-20）：**

| 决策点 | 决议 |
|--------|------|
| 语义 | **全局 AND NOT**：`(G1∨G2∨…) ∧ ¬e1 ∧ ¬e2 ∧ …`（无 include 组时 = 全量再减排除） |
| 存储 | `GroupList.excludes: Vec<Chip>`（含 `enabled`，支持 **`di`**） |
| 多 exclude | 彼此 AND（各独立 `NOT`）；**不做** `NOT (a AND b)` 整组否定 |
| level | 与 H7 对齐：对「最低级别 `>=`」取反 → `NOT (level >= X)` |
| Normal 键 | **`C`+字段**（字母表同 H7）；`C` `m` 复用 msg 切词/候选/草稿回退 |
| Input | **`!` toggle 排除模式**；仅 chips+draft **全空**时可切；开启后本模态 pill 全为排除 |
| 布局 | **独立 Exclude strip**（空则折叠）；pill 带 `!` 前缀，样式走 `theme` |
| Focus | Filter(1) → **Exclude(2)** → Search(3) → Log(4) → Input(5) |

**本项目语境：**

- 引擎：`aloggrep-core::expr` 已有 `not`；匹配在 include 组 OR 之后、H8 lock 之前（或之后，AND 可交换）施加 excludes。
- Exclude strip：`h`/`l`/`dd`/`di` 与 Filter 同构；`dd` 删光 → 回 LogList。
- Input 排除模式：模态标题/提示区分（如含 `!`）；Esc/Ctrl+C 重置 Input（含模式 flag）。
- 去重 ignore-case；空字段 / 重复 → status，不 push。

**与 H7 / H1 / H10：**

- H7 = 包含；H9 = 排除；字母表与 msg 面板共用。
- H1：排除 draft/模式预览时 dim「将被隐藏」行。
- H10：excludes → `-e` 中的 `not …`。

**非目标：** 任意嵌套布尔编辑器；完整 SQL `filter-expr`；单次 Input 混提普通+排除 pill。

**实现提示：**

- 单测：`include(tag=A) + exclude(tag=Spam)`；仅 exclude；`di` 禁用单条排除后恢复可见；Focus 编号与空 strip 折叠。
- 数字键 `1`–`5` 与 H6 文案随本项一并更新（打破原「四分区」）。

**验收：**

- `C` `t` / Input `!` 模式提交后，Exclude strip 出现 `!` pill，对应行从 visible 消失。
- `dd` / `di` 行为正确；删光回 LogList。
- Tab 顺序含 Exclude；空 strip 高度为 0。
- H6 写清「排除 = 全局 AND NOT」与新编号。

---

### H10. 导出当前过滤为 CLI 一行 `[x]`

**用户价值：** TUI 试错完成后一键带走可复现命令，交给 CLI / AI agent / 同事；本仓库「人机协作分析」叙事的具体切片（亦为 M5 的可先做部分）。

**已定决议（头脑风暴 2026-07-20）：**

| 决策点 | 决议 |
|--------|------|
| 键位 | **`y` `c`**（yank command；并入现有 `y` operator，`c` 非 `YankField`） |
| 过滤编码 | **统一 `-e`**（组内 AND、组间 OR；excludes 为 `not …`） |
| H8 lock | **`--pid` / `--tid` flag**（贴近 CLI follow） |
| 空过滤 | 仍复制骨架：`aloggrep -f …` 或 `aloggrep --hdc` |
| 纳入 | 启用中的 Filter 组 + Excludes + lock；**不含** Search；**不含** `di` 禁用项 |
| 二进制名 | `aloggrep` |
| 剪贴板 | 复用 `copy_to_clipboard` / `record_yank` + status |

**本项目语境：**

- `-f` 启动带同一路径；`--hdc` 带 `--hdc`（实时流无文件）。
- 与 TUI visible「近似一致」即可（环形缓冲截断 / following 差异可接受，文档一句说明）。

**非目标：** `.lnav` 脚本、会话文件、复杂 pipeline；把 search 当 filter 导出。

**实现提示：**

- 从 `Expr`/chip 生成可解析的 `-e` 字符串（注意 shell 引号转义）。
- 单测：给定 `GroupList` + excludes + lock，快照期望命令字符串。
- `y` pending 时 `c` 走导出，勿落入「未知字段清 status」。

**验收：**

- `y` `c` 复制命令；CLI 同文件结果与 TUI 过滤语义近似一致。
- M5 对照表「TUI→CLI 桥接」过滤导出打勾。
- H6 含 `yc 导出命令`。

---

## 中价值（分期）

增强分析深度；可在高价值项稳定后做。部分与 CLI 已有能力对齐，重点是 **TUI 联动**。

### M1. Histogram 视图 ↔ 日志跳转 `[~]` 暂缓

**状态：** **先不做**（2026-07-20 头脑风暴确认搁置）。CLI `--histogram` 已覆盖分析侧；TUI 联动待高价值项与书签等稳定后再开。

**用户价值（保留）：** TUI 内看桶并跳到对应时间，排障闭环更短。

**待恢复时议题（未决议）：**

- UI 形态：可折叠 panel vs 靠上模态 vs 暂存全屏表
- 数据：对 `visible` 还是 `rows` 做与 CLI 同构桶
- 选中桶 → LogList 跳转 + `following = false`；无时间戳则禁用

**非目标：** 完整 TIMELINE/opid 视图；面包屑 View 历史栈。

---

### M2. 书签 / 锚点 `[x]`

**用户价值：** 标记可疑行，稍后跳回；标注存在「会话侧」，不改原日志文件。

**已定决议（头脑风暴 2026-07-20）：**

| 决策点 | 决议 |
|--------|------|
| UI | LogList **顶部内嵌**书签区（非独立 Focus、不占 Tab）；背景与正常 log 略区分（`theme`） |
| 展示 | 最多 **N=3**（最近添加）；无书签时 **折叠**；N/软上限 v1 为常量（默认软上限 50） |
| `m` `a` | 收藏当前行并上屏；已存在则提示；达软上限拒绝 |
| `Space` `m` | 打开统一 **Bookmark fzf Picker**：过滤并选中 → Enter 跳转（`following=false`） |
| `m` `d` | 删除 **当前 Log 行**对应书签（区同步移除）；未收藏则 status 提示 |
| 锚定 | ingest 单调 **`row_id`**；缓冲淘汰后在 Picker 标失效、不可跳 |
| 持久化 | v1 进程退出丢弃 |

**与 H3：** 边轨第三色书签点为可选后续，非本项必做。

**非目标：** 独立书签 Focus；区条内选中；注释长文；导出会话脚本；v1 配置文件（留给 M4）。

**实现提示：**

- pending `m` 仅处理 `ma`/`md`；书签列表与 CRUD 统一由 Leader `Space m` 的 Picker 承载。
- Bookmark Picker 支持 Manage/New/Edit、检索、Enter 跳转及带确认的 Ctrl-D/Ctrl-U 删除。

**验收：**

- `ma` 上屏、`md` 下屏、`Space m` 检索跳转正确；淘汰后不崩溃。
- 书签区不抢 LogList 焦点；H6 含 Leader 与 `ma/md`。

---

### M3. 多文件按时间合并（远期） `[~]` 暂缓

**状态：** **先不做**（2026-07-20 头脑风暴确认搁置）。CLI `--sort-time` 已覆盖；TUI 多文件需先设计「消息索引」层，并改 `CLAUDE.md` YAGNI 后再开。

**用户价值（保留）：** 多段日志统一时间线。

**待恢复前提：** 产品确认 + 索引层设计 + 同步 YAGNI；禁止多个 ingest 硬拼 `VecDeque` 再排序。

**非目标：** 目录监视、自动发现轮转文件（除非单独立项）。

---

### M4. 主题 / 键位声明式外置 `[x]`

**用户价值：** 用户可改配色/键位而无需重编译；代码面仍保持「语义 → 样式」单入口。

**已定决议（头脑风暴 2026-07-20）：**

| 决策点 | 决议 |
|--------|------|
| v1 范围 | **仅主题**；键位外置后续（可先文档化默认键位表） |
| 配置目录 | `$ALOGGREP_HOME`，未设置时默认 **`~/.config/aloggrep`** |
| CLI | **`--config-path <dir>`** 覆盖配置目录（不是单文件路径） |
| 主题文件 | `$ALOGGREP_HOME/theme.toml` |
| 面板配置 | `$ALOGGREP_HOME/config.toml`；读取 `picker_left_ratio`（默认 0.4，clamp 0.2–0.8） |
| 扩展位 | 同目录预留 `keymap.toml`（v1 **不读**） |
| 覆盖内容 | UI token（ACCENT、pill、border、selection、preview…）；渲染仍只调 `theme::*` |
| 日志色 | **不得**外置覆盖；仍以 `aloggrep-core::logcolor` 为唯一源 |
| 失败 / 热更新 | 缺失或坏文件 → 回退内置 + status；v1 **仅启动时**加载 |

**非目标：** v1 键位 toml；用户 JSON 自定义 log 格式；`.lnav` 脚本语言。

**验收：**

- 仓库提供示例 `theme.toml` 与 `config.toml`；坏文件回退可感知；`--config-path` 与默认 home 行为有单测。

---

### M5. 无头 / 脚本化分析对齐（CLI 向） `[x]` 收束为文档

**状态：** **不另开大功能**（2026-07-20）。主切片 **H10（`yc` 导出 CLI）** 已落地；对照表「过滤导出」已打勾。

**用户价值：** AI/CI 批量分析继续走 CLI，而不是把 TUI 变成脚本宿主。

**本项目语境：**

- 已有：`--summary`、`--histogram`、`-e`、`--format json/csv`、`--fields` 等。
- 不引入嵌入式脚本引擎。
- 书签等「仅 TUI」能力在对照表标明即可，不必强行补 CLI。

**验收：**

- H10 完成后对照表「过滤导出」打勾；表随实现更新。

---

## 明确不做（防止范围膨胀）

| 想法 | 原因 |
|------|------|
| 内嵌 SQLite / PRQL | 体量与「轻量」定位冲突；复杂聚合用 CLI/`-e` |
| 照搬 lnav 多 View 栈 + breadcrumb | 现有 Focus 四分区已清晰；HIST 用 panel 即可 |
| Timeline / opid 视图 | 依赖通用 opid；若做应为 pid/tid/tag 分组，且单独立项 |
| SSH remote / docker URL / HTTP External Access | 已有 `--hdc` 与外部 skill；不做通用远程协议 |
| 70+ 通用日志格式 JSON 生态 | 非目标；保持四格式 + 零拷贝 `LogEntry` |

---

## 建议实施顺序

| 顺序 | 项 | 理由 |
|------|-----|------|
| 1 | H6 上下文帮助 ✅ | 成本低，给后续键位改动垫文档与 UI |
| 2 | H2 语义跳转 `e`/`E` ✅ | 纯 LogList 逻辑，独立可测 |
| 3 | **H7 光标 → Chip** ✅ | 键盘收窄主路径；为 H8/H9 定字段键约定 |
| 4 | **H8 Follow pid/tid** ✅ | 因果链；可复用 H7 字段提取，建议会话 lock |
| 5 | **H9 排除当前** ✅ | 补齐 triage；需先定全局 AND NOT 模型 |
| 6 | H1 操作预览 ✅ | 对 H7/H9 的 draft/生效都有加成 |
| 7 | H4 字段 overlay ✅ → H5 Pretty ✅ | 与 H7 同构键；先 H7 再 overlay 避免键位返工 |
| 8 | **H10 导出 CLI** ✅ | 过滤模型稳定后编码更准；兑现 M5 切片 |
| 9 | H3 边轨位置提示 ✅ | 可复用 H2/search 的「可跳转行」扫描 |
| 10 | M2 书签 ✅ | Log 顶书签区 + `ma`/`md` + `Space m` Picker |
| 11 | M4 配置外置 ✅ | `theme.toml` + `config.toml` + `--config-path` |
| — | M1 Histogram / M3 多文件 | **暂缓**（先不做） |
| — | M5 脚本化对齐 ✅ | **收束为文档**：以 H10 为主切片 + 对照表 |

---

## TUI vs CLI 能力对照（随实现更新）

| 能力 | CLI | TUI |
|------|-----|-----|
| 表达式过滤 `-e` / chip | ✅ | ✅（chip → `Expr`） |
| 光标生成 / 排除过滤 | — | ✅ H7 / ✅ H9 |
| follow pid/tid | ✅ `--follow-pid/tid` | ✅ H8 会话 lock |
| 过滤状态导出为命令 | —（本身即 CLI） | ✅ H10 `yc`（= M5 主切片） |
| histogram | ✅ `--histogram` | ⏸ M1 暂缓 |
| 错误跳转 | —（可用 `--level`） | ✅ H2（`e`/`E`） |
| 边轨位置提示 | — | ✅ H3 Log 内侧 minimap |
| 过滤/搜索预览 | — | ✅ H1 靠上 Preview 窗 |
| 字段详情 / pretty | `--fields` / 外部工具 | ✅ H4 / ✅ H5 Pretty（同浮层） |
| 书签 | — | ✅ M2 Log 顶区 + `ma/md` + Bookmark Picker |
| 主题/面板配置外置 | — | ✅ M4 `theme.toml` + `config.toml` + `--config-path` |
| 多文件时间归并 | ✅ `--sort-time`（P1-5） | ⏸ M3 暂缓 |

---

# CLI P1（历史清单）

面向 AI agent 日志分析场景。下列项大多已完成，保留需求背景。

## AI agent 分析日志的典型流程与痛点

```
阶段 1: 全局概览
  → "这份日志有多大？时间跨度？有多少 Error？有崩溃吗？"
  → 工具: --summary, --count

阶段 2: 聚焦问题区域
  → "Error 集中在哪个时间段？是瞬间爆发还是持续发生？"
  → 工具: --summary（缺少时间分布）, --dedupe（有时间范围但无分布）

阶段 3: 缩小范围
  → "只看 10:32~10:33 这段的 Error"
  → 工具: --since/--until + --level

阶段 4: 追踪因果链
  → "这个崩溃之前发生了什么？同一个线程在做什么？"
  → 工具: --crashes + -C（上下文）, --pid/--tid

阶段 5: 模式识别
  → "这些 Error 是同一类问题吗？哪个组件最不稳定？"
  → 工具: --dedupe, --summary top_errors

阶段 6: 精确提取
  → "把所有 OkHttp 的 timeout 日志导出为 JSON 给我"
  → 工具: -e + --format json + --limit

阶段 7: 上下文关联
  → "出错前后 5 秒的所有日志（不限 tag）"
  → 工具: -C（按行数）, --time-context（按时间窗口）
```

## P1 条目

### P1-1: `--since`/`--until` 支持完整日期 ✅

**阶段**: 3（缩小范围）

**痛点**: 当前只支持 `HH:MM:SS`，xlog 格式时间戳是 `YYYY-MM-DD HH:MM:SS`，跨天日志无法按日期筛选。

**方案**: 兼容两种写法：
- `--since 10:30:00` （仅时间，现有行为）
- `--since "2026-03-04 10:30:00"` （完整日期时间）

### P1-2: `--pid`/`--tid` 过滤 ✅

**阶段**: 4（追踪因果链）

**痛点**: AI 追踪某个线程的执行流时，只能用 `--msg` 间接匹配，无法按 PID/TID 精确过滤。

**方案**: 新增 `--pid` 和 `--tid` 参数，支持精确数值或正则。同时在 `-e` 表达式中支持 `pid` 和 `tid` 字段。

### P1-3: 时间窗口聚合 `--histogram` ✅

**阶段**: 2（聚焦问题区域）

**痛点**: `--summary` 给出了全局 top_errors，但 AI 无法判断错误是集中爆发还是均匀分布。

**方案**: `--histogram <INTERVAL>` 按时间桶输出 level 分布（JSON）。

### P1-4: `--fields` 字段选择 ✅

**阶段**: 6（精确提取）

**痛点**: AI context window 有限；裁剪字段可节省 token。

**方案**: `--fields timestamp,level,tag,msg` 只输出指定字段。

### P1-5: 多文件时间线合并排序 ✅

**阶段**: 7（上下文关联）

**痛点**: 多文件 glob 顺序拼接而非按时间交叉排序。

**方案**: `-f '*.log' --sort-time` 按 timestamp 归并输出。

### P1-6: 时间窗口上下文 `--time-context` ✅

**阶段**: 7（上下文关联）

**痛点**: `-C 5` 按行数给上下文，密度不均时无法表达「出错前 5 秒」。

**方案**: `--time-context 5s` 按时间窗口输出匹配行前后上下文。
