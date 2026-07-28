# Design: rename to alnav

## Target shape

```text
workspace
├── alnav-core/          # package alnav-core, lib name = alnav
│   └── (parser/filter/expr/… + pub fn run_cli(cli) )
└── alnav/               # package alnav — crates.io 主发布物
    └── bin targets:
        alnav            # argv0 路由：默认 TUI；子命令 grep → CLI
        aloggrep, alg    # 兼容：直接走 CLI（≡ alnav grep）
        aloggrep-tui     # 兼容：直接走 TUI（≡ alnav）
```

```text
alnav [tui-args]           → TUI
alnav grep [cli-args]      → CLI (现 aloggrep::Cli)
aloggrep|alg [cli-args]    → CLI
aloggrep-tui [tui-args]    → TUI
```

## Binary dispatch

单一 `alnav/src/main.rs`（各 `[[bin]]` 可共用同一 path）：

1. 读 `argv0` 基名。
2. `aloggrep` | `alg` → 按 **CLI** 解析（clap `name` 可用 `aloggrep`，或静默接受）并 `alnav::run_cli`。
3. `aloggrep-tui` → 按 **TUI** 解析并跑现有 TUI 主循环。
4. 其他（含 `alnav`）→ 顶层 clap：
   - 子命令 `grep` → 其余参数解析为现 CLI `Cli`，`run_cli`。
   - 无子命令 → 解析为现 TUI CLI，进 TUI。

从 `alnav-core` 的 `main.rs` **抽出** `pub fn run_cli(cli: Cli) -> ExitCode`（或等价），供统一二进制调用；**core 包不再发布独立 bin**（避免与 `alnav` 双入口混淆）。现有 core 单测若依赖 bin，改为测 `run_cli` / lib API。

CLI 结构体仍定义在 `alnav-core`（现 `lib.rs` 的 `Cli`）；`command(name = …)` 在作为 `grep` 子命令挂载时用 clap `#[command(subcommand)]` / flatten，使 `alnav grep --help` 展示原 CLI 帮助。

## Package / lib rename

| 旧 | 新 |
|----|-----|
| dir `aloggrep-core` | `alnav-core` |
| package `aloggrep-core` | `alnav-core` |
| lib `aloggrep` | `alnav` |
| dir `aloggrep-tui` | `alnav`（发布包；源码自 tui 迁入） |
| package `aloggrep-tui` | `alnav` |
| `use aloggrep::` | `use alnav::` |

依赖：`alnav` → `alnav-core = { path = "../alnav-core", version = "…" }`（publish 时 path+version 按 crates 惯例）。

版本建议：产品更名升 **0.2.0**（`alnav` 与 `alnav-core` 对齐）。

## 字符串与导出

- `export.rs` / 测试：命令前缀 `alnav grep`。
- Help、flash、eprintln、README、AGENTS.md、CLAUDE.md、examples 注释中的产品名更新。
- `.trellis/spec/aloggrep-*` → `.trellis/spec/alnav-core` / `alnav`（或 `alnav-tui` 若仍想按层命名；与目录 `alnav` 对齐即可）。

## 配置硬切

`config.rs`：

- `--config-path` > `$ALNAV_HOME` > `~/.config/alnav`
- 删除对 `ALOGGREP_HOME` / `~/.config/aloggrep` 的读取；测试改用新变量/路径。

## crates.io

1. **主发布**：`cargo publish -p alnav-core`（若 `alnav` 依赖需要 crates 上的 core）然后 `cargo publish -p alnav`。  
   - 因 `alnav` 依赖 path 的 core：应对 **`alnav-core` 也 publish**（version 0.2.0），`alnav` 的 Cargo.toml 写 `alnav-core = "0.2"`。  
   - 用户只需 `cargo install alnav`（会拉取依赖库）。
2. **旧包 `aloggrep`**：再发一版（如 0.2.0 或 0.1.8）——README 显著 DEPRECATED，指向 `cargo install alnav`；二进制策略二选一（实现时取较简单者）：
   - **推荐**：该版本仅保留 CLI bin，依赖已发布的 `alnav-core`，行为与 `alnav grep` 相同，并在 `--help`/README 提示迁移；或
   - 极简 stub：打印迁移说明后 exit 2（体验较差，不优先）。
3. 不单独发布旧名 `aloggrep-tui` crate（本就未作为主发布物）。

## GitHub

- 维护者在 GitHub UI 将 `loggrep` **Rename** 为 `alnav`（旧 URL 自动 redirect）。
- 本地 `git remote set-url`；所有 `repository = "https://github.com/Rossettaylm/alnav"`。
- gongfeng：文档记为 follow-up，不阻塞。

## 风险与回滚

| 风险 | 缓解 |
|------|------|
| clap 子命令与 TUI 全局 flag 冲突（如 `-f`） | TUI flags 仅在无 `grep` 时解析；`grep` 后只用 CLI `Cli` |
| argv0 在 `cargo run` 下可能是 `alnav` 而非兼容名 | 兼容名靠 `[[bin]]` 与安装后 PATH；单测用 `parse_from` 显式前缀 |
| 双 publish 顺序 | 先 `alnav-core` 后 `alnav`；失败则不要 yank 已成功的 core，修后重发 |
| 硬切配置 | README 写明需移动 `~/.config/aloggrep` → `~/.config/alnav` |
| 仓库改名时机 | 先改代码与 URL，再 Rename；或先 Rename 再推送——implement 固定一种顺序 |

## 非目标

- 不改 live/filter/TUI 行为。
- 不在本任务删除兼容 bin（只预告）。
