# loggrep P1 需求清单

面向 AI agent 日志分析场景的增强功能。

---

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
  → 工具: --crashes + -C（上下文）, --pid/--tid（❌ 缺失）

阶段 5: 模式识别
  → "这些 Error 是同一类问题吗？哪个组件最不稳定？"
  → 工具: --dedupe, --summary top_errors

阶段 6: 精确提取
  → "把所有 OkHttp 的 timeout 日志导出为 JSON 给我"
  → 工具: -e + --format json + --limit

阶段 7: 上下文关联
  → "出错前后 5 秒的所有日志（不限 tag）"
  → 工具: -C（按行数）, 缺少按时间窗口的上下文
```

---

## P1-1: `--since`/`--until` 支持完整日期 ✅

**阶段**: 3（缩小范围）

**痛点**: 当前只支持 `HH:MM:SS`，xlog 格式时间戳是 `YYYY-MM-DD HH:MM:SS`，跨天日志无法按日期筛选。

**方案**: 兼容两种写法：
- `--since 10:30:00` （仅时间，现有行为）
- `--since "2026-03-04 10:30:00"` （完整日期时间）

## P1-2: `--pid`/`--tid` 过滤 ⬜

**阶段**: 4（追踪因果链）

**痛点**: AI 追踪某个线程的执行流时，只能用 `--msg` 间接匹配，无法按 PID/TID 精确过滤。崩溃报告里有 PID/TID，AI 需要用它追溯同线程之前的操作。

**方案**: 新增 `--pid` 和 `--tid` 参数，支持精确数值或正则。同时在 `-e` 表达式中支持 `pid` 和 `tid` 字段。

**示例**:
```bash
loggrep -f app.log --tid 5678 --level W          # 追踪特定线程的警告
loggrep -f app.log -e 'pid ~ 3542 and level >= E' # 表达式中使用
```

## P1-3: 时间窗口聚合 `--histogram` ⬜

**阶段**: 2（聚焦问题区域）

**痛点**: `--summary` 给出了全局 top_errors，但 AI 无法判断错误是集中爆发（如启动瞬间）还是均匀分布（如持续泄漏）。这是 AI 做根因分析的关键判断依据。

**方案**: 新增 `--histogram <INTERVAL>` 模式，按时间桶输出 level 分布（JSON）。

**示例**:
```bash
loggrep -f app.log --histogram 1m                 # 每分钟的 level 分布
loggrep -f app.log --histogram 10s --level E      # 每10秒 Error 数量
```

**输出** (JSON):
```json
[
  {"bucket": "10:32:00", "V": 0, "D": 120, "I": 45, "W": 3, "E": 15, "F": 0},
  {"bucket": "10:32:10", "V": 0, "D": 80, "I": 30, "W": 1, "E": 2, "F": 0}
]
```

## P1-4: `--fields` 字段选择 ⬜

**阶段**: 6（精确提取）

**痛点**: AI 的 context window 有限。一条 xlog 行约 200 字符，其中 PID/TID/进程信息约 50 字符是噪音。10000 行日志浪费约 500K token。裁剪输出可节省 25%+ token。

**方案**: `--fields timestamp,level,tag,msg` 只输出指定字段。

**示例**:
```bash
loggrep -f app.log --level E --fields level,tag,msg --format json  # 最精简
loggrep -f app.log --fields timestamp,msg                          # 只看时间+消息
```

## P1-5: 多文件时间线合并排序 ⬜

**阶段**: 7（上下文关联）

**痛点**: 多个日志文件按 glob 读入时是顺序拼接，不是按时间交叉排序。跨进程问题需要统一时间线。

**方案**: `-f '*.log' --sort-time` 按 timestamp 归并排序输出。

**示例**:
```bash
loggrep -f 'logs/*.log' --sort-time --level E     # 多文件按时间归并
```

## P1-6: 时间窗口上下文 `--time-context` ⬜ (新增)

**阶段**: 7（上下文关联）

**痛点**: `-C 5` 是按行数给上下文，但日志密度不均匀——高频 tag 可能 1 秒几百行，低频 tag 5 行只覆盖几毫秒。AI 分析因果关系需要的是「出错前 5 秒发生了什么」而不是「出错前 5 行」。

**方案**: `--time-context 5s` 按时间窗口输出匹配行前后的上下文。

**示例**:
```bash
loggrep -f app.log --level F --time-context 5s    # 致命错误前后5秒的所有日志
loggrep -f app.log --crashes --time-context 10s   # 崩溃前后10秒
```

## P1-7: `--around` 聚焦模式 ⬜ (新增)

**阶段**: 4（追踪因果链）+ 7（上下文关联）

**痛点**: AI 经常需要的操作模式是「先找到一个关键事件（如崩溃），然后看它前后的完整日志」。目前需要两步：先 `--crashes` 找到时间戳，再手动用 `--since/--until` 框定范围。

**方案**: `--around <EXPR>` 以匹配行为锚点，输出其前后时间窗口内的所有日志（不限原始过滤条件）。

**示例**:
```bash
loggrep -f app.log --around 'msg ~ FATAL' --time-context 10s
# 等价于: 先找到 FATAL，假设在 10:32:15，然后输出 10:32:05~10:32:25 的所有日志
```

**说明**: 此功能依赖 P1-6 的时间上下文能力，优先级排在其后。

---

## 优先级排序

| 优先级 | 编号 | 功能 | 理由 |
|--------|------|------|------|
| ★★★ | P1-2 | --pid/--tid | 成本低，追踪因果链必备 |
| ★★★ | P1-3 | --histogram | AI 判断问题模式的关键信号 |
| ★★☆ | P1-4 | --fields | 节省 token，提升 AI 处理效率 |
| ★★☆ | P1-6 | --time-context | 因果分析的核心需求 |
| ★☆☆ | P1-7 | --around | 依赖 P1-6，锦上添花 |
| ★☆☆ | P1-5 | --sort-time | 多文件场景较少，可后做 |
