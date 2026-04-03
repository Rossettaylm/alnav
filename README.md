# aloggrep

轻量级 Android logcat / xlog 日志过滤与分析 CLI 工具。

专为 **AI agent 日志分析场景** 设计——支持结构化输出 (JSON/CSV)、时间窗口聚合、去重归并、崩溃提取等，帮助 AI 高效消化大规模日志。

## 安装

```bash
cargo install aloggrep
```

安装后提供两个命令：`aloggrep`（完整名）和 `alg`（简写），功能完全相同。

或从源码安装：

```bash
cargo install --path .
```

## 快速上手

```bash
# 管道模式（配合 adb logcat）
adb logcat | aloggrep --tag "OkHttp" --level W

# 文件模式
aloggrep -f app.log --tag "MyApp" --level E

# 全局概览
aloggrep -f app.log --summary

# 崩溃提取
aloggrep -f app.log --crashes
```

## 核心功能

### 多条件过滤

```bash
aloggrep -f app.log --tag "OkHttp" --msg "timeout" --level W
aloggrep -f app.log --tag A --tag B            # OR: tag=A 或 tag=B
aloggrep -f app.log --tag A --tag B --and      # AND: tag=A 且 tag=B
aloggrep -f app.log --pid 3542 --tid 999       # 按 PID/TID 过滤
```

### 布尔表达式 (`-e`)

```bash
aloggrep -f app.log -e 'msg ~ timeout and tag ~ OkHttp'
aloggrep -f app.log -e '(tag ~ OkHttp or tag ~ Retrofit) and level >= W'
aloggrep -f app.log -e 'not tag ~ Debug'
aloggrep -f app.log -e 'pid ~ 3542 and tid ~ 999'
```

语法：`FIELD ~ VALUE` | `level >= LEVEL`，用 `and` / `or` / `not` / `()` 组合。FIELD = `tag` | `msg` | `pkg` | `pid` | `tid`。

### 时间范围

```bash
aloggrep -f app.log --since 10:30:00 --until 10:35:00
aloggrep -f app.log --since '2026-03-04 10:30:00' --until '2026-03-04 10:35:00'
```

### 输出格式与字段选择

```bash
aloggrep -f app.log --format json --limit 50          # JSON lines
aloggrep -f app.log --format csv > out.csv             # CSV
aloggrep -f app.log --fields timestamp,level,tag,msg   # 只输出指定字段
aloggrep -f app.log --count                            # 仅输出匹配数量
```

### 上下文

```bash
aloggrep -f app.log --tag crash -C 3                   # 前后各 3 行
aloggrep -f app.log --level F --time-context 5s        # 前后各 5 秒
```

### 分析工具

```bash
aloggrep -f app.log --summary                          # 级别分布、Top tags/errors、崩溃数
aloggrep -f app.log --histogram 1m                     # 每分钟级别分布 (JSON)
aloggrep -f app.log --dedupe --limit 20                # 去重归并 Top 20 模式
aloggrep -f app.log --crashes                          # 崩溃提取 (JSON)
```

### 多行合并与采样

```bash
aloggrep -f app.log -M --tag AndroidRuntime            # 合并堆栈追踪
aloggrep -f app.log --tail 50                          # 最后 50 条
aloggrep -f app.log --sample 100                       # 均匀抽样 100 条
```

### 多文件归并

```bash
aloggrep -f 'logs/*.log' --sort-time --level E         # 按时间合并排序
```

## 过滤逻辑

| 场景 | 行为 |
|------|------|
| `--tag A --tag B` | OR：匹配 A 或 B |
| `--tag A --tag B --and` | AND：同时匹配 A 和 B |
| `--tag "A\|B"` | OR（值内管道符） |
| `--tag A --msg err` | AND：跨类型始终 AND |
| `--level W` | 匹配 W / E / F（最低级别） |
| `-e EXPR1 -e EXPR2` | OR（多个 -e 之间），与其他 flag AND |

## 支持的日志格式

自动检测，可混合使用：

- **xlog**: `2026-03-04 10:23:28.872|1[3542]3831|3542|I|NTKernel|msg`
- **threadtime**: `03-04 10:23:28.872  3542  3831 I NTKernel: msg`
- **brief**: `I/NTKernel(3542): msg`

## 退出码

| 码 | 含义 |
|----|------|
| 0 | 有匹配 |
| 1 | 无匹配 |
| 2 | 参数错误 |

## 架构

```
src/
├── main.rs        # CLI 入口，输入调度，主循环
├── parser.rs      # LogEntry 解析（threadtime/xlog/brief）
├── filter.rs      # FilterChain：多条件组合过滤，支持 pid/tid
├── expr.rs        # -e 布尔表达式：tokenizer + 递归下降 parser + AST evaluator
├── multiline.rs   # 多行合并（堆栈追踪等续行）
├── crash.rs       # 崩溃识别 + CrashInfo 结构化提取
├── dedupe.rs      # 消息归一化 + 去重分组
├── sampler.rs     # 输出采样（tail / sample）
├── histogram.rs   # 时间窗口聚合（--histogram）
├── formatter.rs   # 输出格式化（text/json/csv + 字段选择）
└── summary.rs     # 聚合统计（级别分布、Top tags/errors、崩溃计数）
```

**数据流：** stdin/file → 逐行读取 → [MultilineMerger] → `LogEntry::parse()` → `FilterChain::matches()` → [CrashDetector] → `Formatter::write_entry()` / `Summary::record()`

## Claude Code Skill

本仓库附带 `loggrep-analyzer.skill`，这是一个 [Claude Code](https://claude.ai/code) skill，可以让 AI agent 自动使用 loggrep 进行系统化日志分析。

### 安装 skill

将 `.skill` 文件放入 Claude Code 的 skill 目录：

```bash
# 解压到全局 skill 目录（所有项目可用）
unzip loggrep-analyzer.skill -d ~/.claude/skills/loggrep-analyzer

# 或解压到项目级 skill 目录（仅当前项目可用）
unzip loggrep-analyzer.skill -d .claude/skills/loggrep-analyzer
```

### 使用

安装后，在 Claude Code 中直接描述分析需求即可自动触发：

```
帮我分析这个日志 /path/to/app.log
在日志中搜索所有 OkHttp 相关的 timeout 错误
这份日志有崩溃吗？Error 集中在哪个时间段？
```

Skill 会引导 agent 按 4 阶段工作流进行分析：**全局概览 → 定位问题区域 → 深入追踪 → 结构化报告**。

## License

MIT
