<p align="center">
  <h1 align="center">aloggrep</h1>
  <p align="center">
    轻量级 Android logcat / xlog / HarmonyOS hilog 日志过滤与分析 CLI 工具，专为 <strong>AI agent 日志分析场景</strong> 设计
  </p>
  <p align="center">
    <a href="https://crates.io/crates/aloggrep"><img src="https://img.shields.io/crates/v/aloggrep.svg" alt="crates.io version"></a>
    <a href="https://crates.io/crates/aloggrep"><img src="https://img.shields.io/crates/d/aloggrep.svg" alt="downloads"></a>
    <a href="#license"><img src="https://img.shields.io/crates/l/aloggrep.svg" alt="license"></a>
  </p>
</p>

---

支持结构化输出（JSON/CSV）、时间窗口聚合、去重归并、崩溃提取等，用一条命令完成 agent 需要多轮 grep + 自行推理的工作。

## 安装

```bash
cargo install aloggrep
```

安装后提供两个命令：**`aloggrep`**（完整名）和 **`alg`**（简写），功能完全相同。

<details>
<summary>从源码安装</summary>

```bash
git clone https://github.com/Rossettaylm/loggrep
cd loggrep
cargo install --path .
```
</details>

## 快速上手

```bash
# 管道模式（配合 adb logcat）
adb logcat | alg --tag "OkHttp" --level W

# 文件模式
alg -f app.log --tag "MyApp" --level E

# 全局概览（JSON 统计）
alg -f app.log --summary

# 崩溃提取
alg -f app.log --crashes
```

## 功能

### 多条件过滤

```bash
alg -f app.log --tag "OkHttp" --msg "timeout" --level W

# 多值默认 OR，加 --and 改为 AND
alg -f app.log --tag A --tag B            # tag=A OR tag=B
alg -f app.log --tag A --tag B --and      # tag=A AND tag=B

# 按 PID / TID 过滤
alg -f app.log --pid 3542 --tid 999
```

### 布尔表达式（`-e`）

对跨字段复杂逻辑使用 `-e` 表达式，语法更直观：

```bash
alg -f app.log -e 'msg ~ timeout and tag ~ OkHttp'
alg -f app.log -e '(tag ~ OkHttp or tag ~ Retrofit) and level >= W'
alg -f app.log -e 'not tag ~ Debug'
```

> **语法**：`FIELD ~ VALUE` | `level >= LEVEL`，用 `and` / `or` / `not` / `()` 组合。
>
> FIELD = `tag` | `msg` | `pkg` | `pid` | `tid`

多个 `-e` 之间为 OR，与其他 flag 之间为 AND。

### 时间范围

```bash
alg -f app.log --since 10:30:00 --until 10:35:00
alg -f app.log --since '2026-03-04 10:30:00' --until '2026-03-04 10:35:00'
```

### 输出格式与字段选择

```bash
alg -f app.log --format json --limit 50          # JSON lines
alg -f app.log --format csv > out.csv             # CSV 导出
alg -f app.log --fields timestamp,level,tag,msg   # 只输出指定字段
alg -f app.log --count                            # 仅输出匹配数量
```

### 上下文行

```bash
alg -f app.log --tag crash -C 3           # 前后各 3 行
alg -f app.log --level F --time-context 5s  # 前后各 5 秒内的所有日志
```

### 分析工具

```bash
alg -f app.log --summary                  # 级别分布、Top tags/errors、崩溃数
alg -f app.log --histogram 1m             # 每分钟级别分布（含异常检测）
alg -f app.log --dedupe --limit 20        # 去重归并，输出 Top 20 模式
alg -f app.log --crashes                  # 崩溃提取（JSON）
```

> [!TIP]
> `--histogram` 输出自带基于均值+2σ 的异常检测，无需手动计算。

### 多行合并与采样

```bash
alg -f app.log -M --tag AndroidRuntime    # 合并堆栈追踪为单条记录
alg -f app.log --tail 50                  # 最后 50 条匹配
alg -f app.log --sample 100               # 水塘抽样 100 条
```

### 多文件归并

```bash
alg -f 'logs/*.log' --sort-time --level E  # 多文件按时间归并排序
```

## 过滤逻辑速查

| 用法 | 行为 |
|:-----|:-----|
| `--tag A --tag B` | OR — 匹配 A 或 B |
| `--tag A --tag B --and` | AND — 同时匹配 A 和 B |
| `--tag "A\|B"` | OR — 值内管道符 |
| `--tag A --msg err` | AND — 跨字段始终 AND |
| `--level W` | 匹配 W / E / F（最低级别） |
| `-e EXPR1 -e EXPR2` | OR（多个 `-e` 之间），与其他 flag AND |

## 支持的日志格式

自动检测，可混合使用：

| 格式 | 示例 |
|:-----|:-----|
| **hilog** | `04-16 11:52:56.297 11114 11114 I A00201/com.tencent.mqq/QRouter: msg` |
| **xlog** | `2026-03-04 10:23:28.872\|1[3542]3831\|3542\|I\|NTKernel\|msg` |
| **threadtime** | `03-04 10:23:28.872  3542  3831 I NTKernel: msg` |
| **brief** | `I/NTKernel(3542): msg` |

> hilog 格式会自动分离 domain / package / tag，`--package` 和 `-e 'pkg ~ ...'` 可精确匹配 package 字段。

## 与 grep 对比（AI agent 场景）

aloggrep 核心优势在于**用一条命令完成 agent 需要 3–5 轮 grep + 自行推理的工作**，大幅降低 token 消耗。

<details>
<summary>展开完整对比表</summary>

| 维度 | aloggrep | grep / rg |
|:-----|:---------|:----------|
| **结构化解析** | 自动识别四种格式，提取 timestamp/pid/tid/level/tag/pkg/msg | 纯文本正则，需自写复杂 regex |
| **语义过滤** | `--level W` 即匹配 W/E/F | 需 `grep -E "[WEF]/"` 并处理格式差异 |
| **多条件组合** | `--tag X --msg Y --level E` 一行完成 | 需多管道 `grep \| grep \| grep` |
| **布尔表达式** | `-e '(tag ~ A or tag ~ B) and level >= W'` | 无法单命令表达 |
| **聚合分析** | `--summary` / `--histogram` / `--dedupe` 直接输出 JSON | agent 需把原始行读入 context 自行统计 |
| **崩溃提取** | `--crashes` 结构化输出 type/exception/stack | 需手写正则识别 FATAL EXCEPTION/ANR/SIGSEGV |
| **多行合并** | `-M` 自动合并 stack trace | grep 逐行输出，stack trace 被打散 |
| **Token 效率** | `--fields --limit` 精确控制输出量 | 输出整行，无法选字段 |
| **时间窗口** | `--since` / `--until` / `--time-context` 原生支持 | 需先 grep 时间戳再手动过滤 |
| **多文件排序** | `--sort-time` glob 多文件归并排序 | 需手动 `sort -m`，不理解时间格式 |
| **通用性** | 仅适用于 Android logcat/xlog | 任意文本文件 |

</details>

> [!NOTE]
> 聚合分析场景（summary / histogram / dedupe）可节省 **80%+** token 消耗。主要劣势是适用范围仅限 Android 日志，且需额外安装。

## Claude Code Skill

本仓库附带 `loggrep-analyzer.skill`，可在 [Claude Code](https://claude.ai/code) 中让 AI agent 自动完成系统化日志分析。

**安装：**

```bash
# 全局安装（所有项目可用）
unzip loggrep-analyzer.skill -d ~/.claude/skills/loggrep-analyzer

# 项目级安装（仅当前项目）
unzip loggrep-analyzer.skill -d .claude/skills/loggrep-analyzer
```

**使用：** 安装后在 Claude Code 中直接描述需求即可触发：

```
帮我分析这个日志 /path/to/app.log
在日志中搜索所有 OkHttp 相关的 timeout 错误
这份日志有崩溃吗？Error 集中在哪个时间段？
```

Skill 引导 agent 按 **全局概览 → 定位问题区域 → 深入追踪 → 结构化报告** 四阶段工作流进行分析。

## 架构

```
src/
├── main.rs        # CLI 入口（clap derive），输入调度，主循环
├── parser.rs      # LogEntry 解析（hilog / threadtime / xlog / brief）
├── filter.rs      # FilterChain：多条件组合过滤，支持 pid/tid
├── expr.rs        # -e 布尔表达式：tokenizer + 递归下降 parser + AST evaluator
├── multiline.rs   # 多行合并（堆栈追踪等续行）
├── crash.rs       # 崩溃识别 + CrashInfo 结构化提取
├── dedupe.rs      # 消息归一化 + 去重分组
├── sampler.rs     # 输出采样（tail / sample）
├── histogram.rs   # 时间窗口聚合（--histogram）
├── formatter.rs   # 输出格式化（text / json / csv + 字段选择）
└── summary.rs     # 聚合统计（级别分布、Top tags/errors、崩溃计数）
```

**数据流：**

```
stdin/file → 逐行读取 → [MultilineMerger] → LogEntry::parse()
  → FilterChain::matches() → [CrashDetector]
  → Formatter::write_entry() / Summary::record()
```

关键设计决策：

- **`LogEntry<'a>` 零拷贝解析**：所有字段均为 `&'a str`，直接引用原始行，避免堆分配。
- **`FilterChain::from_cli`** 是唯一过滤器构建入口，将所有 CLI 参数统一转换为内部过滤链。
- **`dispatch_lines!` 宏**：根据 `--multiline` / `--crashes` 标志决定是否用 `MultilineMerger` 包裹迭代器，避免运行时分支开销。

## 退出码

| 码 | 含义 |
|:---|:-----|
| `0` | 有匹配 |
| `1` | 无匹配 |
| `2` | 参数错误 |

## License

[MIT](LICENSE)
