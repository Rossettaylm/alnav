# Rename product to alnav (`alnav` + `alnav grep`)

## Goal

将产品从 aloggrep 全面更名为 **alnav**（App/Android Log Navigator）：单一安装入口 **`alnav`** 默认进入 TUI，**`alnav grep`** 提供纯 CLI；完成目录与 Cargo 包更名、文档、GitHub 仓库重命名，以及 crates.io 发布。

## Background

- 现 workspace：`aloggrep-core`（lib 名 `aloggrep`，bin `aloggrep`/`alg`）+ `aloggrep-tui`（bin `aloggrep-tui`）。
- crates.io 已发布 `aloggrep`（≈0.1.7）；`alnav` 未占用；`lognav` 已占用（不用）。
- GitHub：`Rossettaylm/loggrep`（另有 gongfeng 镜像）。
- 配置：`$ALOGGREP_HOME` 或 `~/.config/aloggrep/`。

## Requirements

### 命令

- `alnav`（无子命令）→ TUI（现 `aloggrep-tui` 能力）。
- `alnav grep …` → CLI（现 `aloggrep`/`alg` 能力）。
- 本版本兼容二进制：`aloggrep`/`alg` ≡ `alnav grep`；`aloggrep-tui` ≡ `alnav`；文档标明下一版本移除。
- TUI `yc` 导出前缀为 `alnav grep`。

### 包与目录

- `alnav-core/`：package `alnav-core`，`[lib] name = "alnav"`。
- `alnav/`：统一二进制发布包（默认 TUI + `grep` 子命令 + 兼容 bin 名）。
- 对外主发布 crates.io 名 **`alnav`**。

### 配置

- 硬切：仅 `$ALNAV_HOME` 或 `~/.config/alnav/`（不回退旧路径/旧环境变量）。

### GitHub / 发布

- 仓库重命名为 `Rossettaylm/alnav`；Cargo `repository` 与 README 同步。
- `cargo install alnav` 可用；既有 `aloggrep` crate 发迁移说明版并引导改用 `alnav`。
- gongfeng 镜像同步不阻塞本次发布。

## Acceptance Criteria

- [ ] `cargo build --workspace` / `cargo test --workspace` 在新包名下全绿。
- [ ] `alnav -f <log>` 进 TUI；`alnav grep -f <log> --tag X` 等价现 CLI。
- [ ] 安装产物含兼容 bin：`aloggrep`、`alg`、`aloggrep-tui`；文档写明下一版移除。
- [ ] `yc` 导出为 `alnav grep …`；配置只认 `ALNAV_HOME` / `~/.config/alnav`。
- [ ] 源码主品牌为 alnav（兼容别名除外）；AGENTS.md / README / examples 已更新。
- [ ] GitHub 仓库名为 `Rossettaylm/alnav`，URL 已写入 Cargo/README。
- [ ] `alnav` 已发布到 crates.io；`aloggrep` 已发迁移说明版。

## Out of Scope

- 过滤语义、TUI 交互、解析格式变更。
- 使用 `lognav` 作为名称。
- gongfeng 镜像必须同日完成。
- 下一版本删除兼容别名的实际执行（只文档预告）。

## Decisions（已全部确认）

| # | 决策 | 选择 |
|---|------|------|
| 1 | 旧名兼容 | **B** 过渡一版（保留三别名，下版删） |
| 2 | GitHub 仓库 | **重命名**为 `Rossettaylm/alnav` |
| 3 | Workspace | **A** `alnav-core` + 发布包 `alnav` |
| 4 | 配置路径 | **硬切** `ALNAV_HOME` / `~/.config/alnav` |

## Artifact status

- `prd.md`：已收敛
- `design.md`：已就绪
- `implement.md`：Phase 0–7（含 GitHub Rename + crates.io）
