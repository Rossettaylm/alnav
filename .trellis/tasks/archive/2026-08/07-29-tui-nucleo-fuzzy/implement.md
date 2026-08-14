# Implement: TUI nucleo fuzzy search

## Checklist

### 1. Dependencies & facade

- [ ] `alnav/Cargo.toml` 增加 `nucleo` / `nucleo-matcher`（版本以 crates.io 稳定版为准）
- [ ] 新增 `alnav/src/fuzzy.rs`：`join_tag_msg`、positions 映射、`fuzzy_label_indices`、`FuzzyIndex` 骨架
- [ ] `main.rs` / `lib` 模块注册

### 2. Small-list path（低风险先行）

- [ ] 替换 `PickerSession::filtered_indices` → `fuzzy::fuzzy_label_indices`
- [ ] MsgChip / Time 日期候选同步替换
- [ ] 更新 `picker` / `time_panel` / `main` 相关单测（fuzzy 样例）

### 3. HighlightGroup 去 Regex

- [ ] `HighlightGroup` 去掉 `re`；`from_pattern` / `matches_row` / `paint_patterns` 改为 pattern + fuzzy 服务
- [ ] `ui.rs` 渲染改为 positions 映射上色
- [ ] `n`/`N`、minimap、preview Search 淡高亮改走 fuzzy 命中
- [ ] 迁移 `highlight_model` / `ui` / `app` 中 Regex 断言测试

### 4. Filter/Exclude 去 Expr（TUI 文本路径）

- [ ] `Group`：文本 chip 不再编译 `Expr`（或删除 `expr` 字段）；求值改为 FuzzyIndex + 精确谓词
- [ ] `build_group` / `initial_group`（`main.rs` `Expr::from_filters`）改为构造 chips + fuzzy 就绪状态
- [ ] `GroupList::matches` / `row_passes_filters` / File Subset 扫描 / Stream matched 双写接入 fuzzy
- [ ] Exclude AND NOT 覆盖测试

### 5. FuzzyIndex File / Stream

- [ ] File：后台注入 + status 进度 + 完成后刷新
- [ ] Stream：drain 注入 + 淘汰后存在性校验
- [ ] 与现有 async scan / `Visible` 协调，避免双扫打架（优先 fuzzy 结果驱动 Subset）

### 6. Export / Help / 文案

- [ ] `yc` flash 近似提示
- [ ] Help 中检索描述改为 fuzzy（若有子串/regex 措辞）

### 7. Validation

```bash
cargo test -p alnav --bin alnav
cargo test --workspace
cargo build -p alnav
```

手动（可选）：大文件 `-f` 看 `idx` 进度；`--adb`/`--hdc` 淘汰后无幽灵跳转；`yc` 见 approx 提示。

## Risky files

- `alnav/src/app.rs` — 过滤/可见性中枢
- `alnav/src/highlight_model.rs` / `filter_model.rs` — 模型语义
- `alnav/src/ui.rs` — 高亮绘制
- `alnav/src/main.rs` — 初始组、Picker 过滤调用点
- `alnav/src/store.rs` / ingest — 索引注入时机

## Rollback

整任务 revert；无配置开关。

## Before `task.py start`

- [x] `prd.md` / `design.md` / `implement.md` 齐备
- [ ] 用户明确批准本最终规划摘要
- [ ] （若走 sub-agent）`implement.jsonl` / `check.jsonl` 写入真实 spec 条目（非仅 `_example`）
