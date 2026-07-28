# Implement: rename to alnav

按阶段推进；每阶段末跑通所列验证。不在未批准规划前执行本清单。

## Phase 0 — 分支与基线

1. 从 `master` 建分支 `feat/rename-to-alnav`。
2. 记录基线：`cargo test --workspace` 全绿。

## Phase 1 — 目录与 package 更名（编译可红）

1. `git mv aloggrep-core alnav-core`；`git mv aloggrep-tui alnav`。
2. 改 workspace `Cargo.toml` members。
3. `alnav-core/Cargo.toml`：package `alnav-core`，lib name `alnav`，去掉或停用 `[[bin]]`（逻辑迁出），`repository` 先写最终 URL，version `0.2.0`。
4. `alnav/Cargo.toml`：package `alnav`，依赖 `alnav-core`，version `0.2.0`；准备多 `[[bin]]`。
5. 全局替换 `use aloggrep::` → `use alnav::`；`-p aloggrep-*` 脚本/文档暂可后置。
6. Gate：尽量 `cargo build --workspace`（允许主入口未接好时仅 lib 通过）。

## Phase 2 — 抽出 CLI runner + 统一入口

1. 将原 `alnav-core` bin `main` 核心路径提成 `pub fn run_cli(cli: Cli) -> …`。
2. 实现 `alnav/src/main.rs`：argv0 分发 + `alnav` / `alnav grep` clap。
3. 挂兼容 bin：`aloggrep`、`alg`、`aloggrep-tui`（同 path 或薄包装）。
4. 更新 CLI/TUI 内所有 `command(name=…)`、错误前缀、测试里的 `parse_from(["aloggrep", …])` 等。
5. Gate：`cargo test -p alnav-core`；`cargo test -p alnav`。

## Phase 3 — 配置硬切与导出

1. `config.rs`：仅 `ALNAV_HOME` / `~/.config/alnav`；测例同步。
2. `export.rs` + 测试：`alnav grep` 前缀。
3. Gate：`cargo test -p alnav config`；`cargo test -p alnav export`。

## Phase 4 — 文档与 Trellis spec 路径

1. README、AGENTS.md、CLAUDE.md、examples、Help 文案。
2. `.trellis/spec/aloggrep-core` → `alnav-core`；`aloggrep-tui` → `alnav`（或约定名），更新 `get_context.py`/索引引用若有硬编码。
3. Gate：文档自检；`cargo test --workspace`。

## Phase 5 — 全量验证

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets
cargo run -p alnav --bin alnav -- -f <sample.log>    # TTY 冒烟（可记跳过）
cargo run -p alnav --bin alnav -- grep -f <sample.log> --help
```

## Phase 6 — GitHub 仓库重命名与推送

1. 确认 `repository` / README clone URL 为 `https://github.com/Rossettaylm/alnav`。
2. **人工**：GitHub Settings → Rename repository → `alnav`。
3. `git remote set-url github git@github.com:Rossettaylm/alnav.git`（及 fetch 验证）。
4. 推送分支；开 PR 或按维护者习惯合并 `master`。
5. gongfeng：列出 follow-up，不阻塞 Phase 7。

## Phase 7 — crates.io 发布

1. 登录/token 确认（`cargo login` 已具备）。
2. `cargo publish -p alnav-core`。
3. `cargo publish -p alnav`。
4. 旧包：发布 `aloggrep` 迁移版（README DEPRECATED + 仍可用的 CLI 或明确 stub；与 design 一致）。
5. 验证：`cargo install alnav --version 0.2.0`（或 yank 前干跑 `cargo publish --dry-run`）。

## Rollback

- Phase 1–5：分支丢弃或 revert；未 publish 则无 crates 副作用。
- Phase 6：GitHub 改名一般可再改回，但应避免反复。
- Phase 7：已 publish 版本不可覆盖；只能发新版本修复；误发用 yank（慎用）。

## Review gates（start 前自检）

- [ ] prd 决策均已落入 design/implement
- [ ] 兼容 bin 与硬切配置无遗漏
- [ ] publish 顺序：core → alnav → 旧 aloggrep 迁移说明
- [ ] 无 `lognav` 命名残留意图
