# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

loggrep — 轻量级 Android logcat 日志过滤与分析 CLI 工具（Rust）。

## Build & Test

```bash
cargo build              # 开发构建
cargo build --release    # 发布构建
cargo test               # 运行所有测试
cargo test parser::      # 仅运行 parser 模块测试
cargo test filter::      # 仅运行 filter 模块测试
```

## Usage

```bash
# 管道模式（配合 adb logcat）
adb logcat | loggrep --tag "OkHttp" --msg "error" --level W

# 文件模式
loggrep --file app.log --tag "MyApp" --level E

# AI 友好输出
loggrep --file app.log --format json --limit 50
loggrep --file app.log --summary
loggrep --file app.log --tag "crash" --count
```

## Architecture

```
src/
├── main.rs        # CLI 入口（clap derive），输入调度（stdin/文件），主循环
├── parser.rs      # LogEntry 结构体 + 解析器（支持 threadtime/xlog/brief 三种格式）
├── filter.rs      # FilterChain：多条件组合过滤（同类 OR，跨类 AND），支持 pid/tid
├── expr.rs        # -e 布尔表达式：tokenizer + 递归下降 parser + AST evaluator（支持 pid/tid 字段）
├── multiline.rs   # MultilineMerger：多行合并迭代器适配器（合并续行如栈追踪）
├── crash.rs       # CrashDetector：崩溃识别 + CrashInfo 结构化提取
├── dedupe.rs      # Normalizer（消息归一化）+ Deduper（去重分组）
├── sampler.rs     # Sampler：输出采样（--tail 尾部 / --sample 均匀抽样）
├── histogram.rs   # Histogram：时间窗口聚合（--histogram 10s/1m/5m），JSON 输出
├── formatter.rs   # 输出格式化：text（彩色）/ json / csv，支持 --fields 字段选择
└── summary.rs     # 聚合统计：级别分布、Top tags、Top errors、崩溃计数
```

**数据流：** stdin/file → 逐行读取 → [MultilineMerger] → `LogEntry::parse()` → `FilterChain::matches()` → [CrashDetector] → `Formatter::write_entry()` / `Summary::record()`

## Filter Logic

- 同类型多值 = OR：`--tag "A" --tag "B"` → tag=A OR tag=B
- 同类型 AND：`--tag "A" --tag "B" --and` → tag=A AND tag=B
- 跨类型 = AND：`--tag "A" --msg "err"` → tag=A AND msg~err
- 值内 `|` 也是 OR：`--tag "A|B"`
- `--level W` 匹配 W/E/F（最低级别）
- `-e` 布尔表达式：支持 `and`/`or`/`not`/括号的任意组合
  - 语法：`FIELD ~ VALUE`、`level >= LEVEL`，用 `and`/`or`/`not`/`()` 组合
  - FIELD = `tag` | `msg` | `pkg` | `pid` | `tid`；VALUE = 裸词或 `"引号字符串"`
  - 多个 `-e` 之间 OR（与 grep `-e` 一致），与其他 flag AND

```bash
# 表达式过滤
loggrep -e 'msg ~ mobile_msf and msg ~ 0x9293'
loggrep -e '(tag ~ OkHttp or tag ~ Retrofit) and level >= W'
loggrep -e 'not tag ~ Debug' --level I
# 多个 -e 之间 OR，与其他 flag AND
loggrep -e 'tag ~ OkHttp' -e 'tag ~ Retrofit' --level W
```

## Exit Codes

- `0` — 有匹配
- `1` — 无匹配
- `2` — 参数错误
