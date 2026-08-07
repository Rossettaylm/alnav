# Implement: Jump no-wrap + view focus + tt

## Checklist（顺序）

1. **Jump no-wrap core**
   - [ ] `scan.rs`：`HighlightScanState::find_next` 去 wrap；补单测
   - [ ] `app.rs`：`find_match` / `find_severe` 有界扫描；调整 `test_find_match_next_prev_and_wrap` → no-wrap；补 severe 边界用例
   - [ ] `main.rs`：`n/N`/`e/E` 在「有命中但到头」时 `NO MORE`

2. **View focus 状态**
   - [ ] `app.rs`：`ViewFocus` + `view_focus` 字段；`toggle_view_focus` / clear helpers
   - [ ] 扩展 `row_passes_filter_parts` + `filter_active` + file `FilterPred` clone
   - [ ] `rebuild_visible` / drain 路径验证 Highlight/Severe AND
   - [ ] theme glyph + status 渲染（`ui.rs` 只调 theme）
   - [ ] `main.rs`：`pending_lock` 分支加 `h`/`e`
   - [ ] Help L2_LOCK / catalog / 必要时 L1 `f` 说明

3. **`tt` 替换 `ts`**
   - [ ] `main.rs`：`'s'` → `'t'`；测试改 `tt`
   - [ ] `help.rs`：`L2_TIME`、`CAT_SESSION`
   - [ ] 注释：`time_panel.rs` / `app.rs` 中 `ts` → `tt`

4. **验证**
   - [ ] `cargo test -p alnav --bin alnav`
   - [ ] 手动冒烟（可选）：`-f` 下 filter → `fh`/`fe` → `n`/`e` 到头；`tt` 开面板

## Validation commands

```bash
cargo test -p alnav --bin alnav find_match
cargo test -p alnav --bin alnav find_severe
cargo test -p alnav --bin alnav highlight_scan
cargo test -p alnav --bin alnav time
cargo test -p alnav --bin alnav
```

## Risky files / rollback points

| 点 | 风险 | 回滚 |
|----|------|------|
| `row_passes_filter_parts` 签名扩展 | file FilterPred / preview 调用点漏改 | 恢复签名，view_focus 仅 stream 侧 |
| `filter_active` 含 view_focus | 误伤「全可见」语义 | 去掉该分支 |
| `f`+`e` 与裸 `e` | pending 时 `e` 走 fe，非 pending 走 jump — 已由 pending 门控，注意测序 | — |

## Before `task.py start`

- [x] `prd.md` / `design.md` / `implement.md` 齐备
- [x] `implement.jsonl` / `check.jsonl` 有真实 spec 条目
- [ ] 用户显式批准本最终规划摘要
- [ ] 批准后执行 `python3 ./.trellis/scripts/task.py start`
