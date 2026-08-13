# Implement: TUI open-file log_dirs nucleo search

## Checklist

- [x] **Config**：`AppConfig` + `ConfigToml` + `--init` 注释默认；单测加载/默认 extensions
- [x] **`log_corpus.rs`**：展开 `~`、递归遍历、后缀/点项/symlink 规则、batch channel、cancel/refresh gen、进程内缓存 API
- [x] **`OpenFilePanel`**：去掉 path_complete 分支；空/非空 query 语义；接入 corpus；进度文案；`Ctrl-r`
- [x] **`App`/`main`**：持有 corpus；开/关面板生命周期；打开文件仍 `recent.record`
- [x] **UI/Help/Dashboard**：文案与扫描状态；keymap/Help 条目
- [x] **清理**：删除 `path_complete.rs`（仅 Open file 使用）
- [x] **测试**：corpus 单测 + panel 选择逻辑单测；`cargo test -p alnav --bin alnav` 545 passed

## Validation

```bash
cargo test -p alnav --bin alnav config::
cargo test -p alnav --bin alnav source_panel::
cargo test -p alnav --bin alnav log_corpus::
cargo test -p alnav --bin alnav
```

手动（可选）：配 `log_dirs`，`alnav` Dashboard → Open file → 空态 recent → 键入 fuzzy → Enter 打开；`Ctrl-r` 重扫；无配置时 flash。

## Notes

- 先落地 corpus + 配置，再改面板，最后清 path_complete / 文案。
- 不在本任务改 CLI / alnav-core。
