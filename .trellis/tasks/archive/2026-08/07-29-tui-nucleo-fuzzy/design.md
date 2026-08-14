# Design: TUI nucleo fuzzy search

## Architecture (MVP — shipped)

```
                    ┌─────────────────────────────────────┐
                    │ App                                 │
                    │  groups / excludes / highlight_…    │
                    │  row_passes_filters / rebuild visible│
                    └───────────┬─────────────────────────┘
                                │
           ┌────────────────────┼────────────────────┐
           ▼                    ▼                    ▼
   nucleo-matcher          nucleo-matcher        exact preds
   (per-row in File        (picker/time/msgchip) pid/tid/level
    Filter/Highlight scans)
           │                    │                    │
           └────────────────────┴────────────────────┘
                                │
                         Visible / matched
                         + paint positions
```

**MVP decision:** Use **`nucleo-matcher` only** inside existing File FilterBatch / Highlight Inc scans and Stream `row_passes_filters`. A high-level `nucleo` `FuzzyIndex` async corpus worker is **deferred** (see `.trellis/spec/alnav/backend/fuzzy-matching.md`).

### New module

- `alnav/src/fuzzy.rs` — haystack、chip 求值、positions 映射、`fuzzy_label_indices`。
- 依赖：`nucleo-matcher`（无 `nucleo` crate）。

### Match text contracts

| 用途 | 文本 |
|------|------|
| Search / Highlight 行是否命中 | `join_tag_msg(tag, msg)`；若 tag 与 msg 皆空 → `raw` |
| Search / Highlight 上色 | 在上述拼接串上取 nucleo indices，按固定分隔符映射回字段字节范围 |
| Filter chip `tag`/`msg`/`pkg` | 仅该字段字符串；**字段空 → 该 chip 不命中**（不用 raw 冒充字段） |
| 未解析且无字段的 Filter 文本 chip | 不命中（与上条一致）；用户可用 Search 搜 raw |

拼接分隔符：建议单字节 `\t`（不可见于正常 tag 尾/msg 头的概率高；实现常量 `TAG_MSG_SEP`）。映射时 `tag` 区间 `[0, tag.len())`，sep 不绘制，`msg` 区间从 `tag.len()+sep.len()` 起。

### Index lifecycle

**File**

1. 行索引/惰性解析推进时，将 `(line_index, tag, msg, raw)` 注入 nucleo（可双列：`tag`/`msg`，或 Search 用拼接列 + 字段列——以实现简单为先：维护 **两套 pattern 目标**：
   - column/haystack A：`join_tag_msg` 或 raw（Search/Highlight）
   - 按字段求值时对单字段 `Matcher::fuzzy_match` / 独立 Nucleo 列）。
2. 务实方案（推荐 MVP）：
   - **一个** `Nucleo` worker，item 携带 `line_index`（或 row_id）+ 预计算的 `search_haystack`（tag+msg|raw）以及可选的字段缓存；
   - Filter 字段约束：对 snapshot 命中后再用字段级 `nucleo_matcher::Matcher` 二次确认，**或** 为 tag/msg/pkg 分列注入（nucleo 多 column）。优先 **分列注入**（与 Q3=4 / Q4=B 对齐，避免二次扫描大 snapshot）。
3. status：`indexed/total`；`tick` 融入现有事件循环（与 async scan 同节奏）。
4. Filter/Search 变更 → 对当前 snapshot 求匹配行号 → File：`Visible::Subset`；完成后若索引仍增长则增量合并或全量重算（设计取：pattern 不变时增量 append 匹配；pattern 变则重算）。

**Stream**

- `drain` 注入新行；淘汰时删除对应 item（若 nucleo API 不支持按 id 删除：记录 generation / 重建 worker，或匹配后 `row_id` 存在性校验——**MVP 用匹配后存在性校验 + 周期性紧凑**，避免复杂删除 API 依赖）。

### Evaluation order（保持）

`groups (fuzzy AND/OR) → excludes (NOT fuzzy) → lock → time_bound → view_focus`

TUI 文本路径 **不再** 为 chip 编译 `Expr`/`Regex`。`Group` 可保留 `chips` 为真源；`expr: Option<Expr>` 删除或仅用于过渡期测试迁移后删除。

`pid`/`tid`/`level` chip：仍走精确谓词（可留在 Group 内非 fuzzy 分支）。

### Highlight / paint

- `HighlightGroup`：去掉 `re: Regex`；存 `pattern: String` + enabled；命中与 positions 由 `FuzzyIndex` / matcher 提供。
- `ui::render_entry_lines`：消费 `Vec<(field, ranges, color_idx, is_active)>` 而非 `Regex::find_iter`。
- 多 Highlight 组：各自 fuzzy，positions 合并绘制（重叠时既有优先级规则可沿用「后者覆盖/或严重优先」——保持与现多 pattern 绘制相近：按 `paint_patterns` 顺序叠加）。

### Small lists

```rust
fn fuzzy_indices(labels: &[str], query: &str) -> Vec<usize>
```

替换 `PickerSession::filtered_indices`；空 query → 全量；ignore-case config。

### Export (`yc`)

- 仍把 pattern 当字面写入 `-e` / flags。
- 成功 yank 后 `set_flash` 含 `approx` 或 `not fuzzy` 短提示（英文 dim 风格与现 flash 一致）。

### Compatibility

| 层 | 策略 |
|----|------|
| CLI | 不变 |
| TUI 启动 flags | 语义改为 fuzzy（破坏性，文档/Help） |
| theme/logcolor | 不改色值；只改命中区间来源 |

### Risks

| 风险 | 缓解 |
|------|------|
| 大文件内存（nucleo 存 haystack 副本） | 与「可全文件浏览」一致接受；监控；必要时后续加上限任务 |
| nucleo API 未 1.0 | 锁版本；封装在 `fuzzy.rs` |
| Stream 无删除 API | 存在性校验 + 重建 |
| 测试大量依赖 Regex/子串 | 批量改断言为 fuzzy 样例（`abc`⊂`aXbYc`） |

### Rollback

- 单一引擎切换，无 feature flag（产品决定）。回滚 = git revert 本任务提交。
