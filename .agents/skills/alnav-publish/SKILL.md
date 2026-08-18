---
name: alnav-publish
description: >
  Cut an alnav release: bump crate versions, write CHANGELOG, review README,
  tag vX.Y.Z, and push to the github remote so CI publishes crates.io and
  creates the GitHub Release. Use when the user asks to 发版, release, publish,
  ship a version, push to crates.io, or create a GitHub Release.
---

# alnav-publish

本仓库发版的唯一流程。Agent **不**在本机 `cargo publish`，**不**装 git hook。

推送 `v*` tag 到 remote **`github`** 后，[`.github/workflows/publish.yml`](../../../.github/workflows/publish.yml) 会：测全 workspace → 先发 `alnav-core` 再发 `alnav` → 用 CHANGELOG 建 GitHub Release。

## 仓库事实（不要猜）

- 远程：**`github`**（`git@github.com:Rossettaylm/alnav.git`）。没有 `origin`。不要推 `gongfeng`。
- 版本必须三处一致：`alnav-core/Cargo.toml` `[package].version`、`alnav/Cargo.toml` `[package].version`、`alnav` 对 `alnav-core` 的 `version =`。
- Tag：`vX.Y.Z`（已有 `v0.2.2` / `v0.2.3`）。
- Changelog 标题：`## 0.2.3 — 2026-08-13`（英文条目、em dash、日期）。
- README 不写死版本号（crates.io badge 会自己变）。中英两份一起看：`README.md`、`README.zh.md`。
- 不可逆：crates.io 版本不能覆盖，只能再发新版本或 yank。

## 一次性准备

仓库 Settings → Secrets and variables → Actions → `CARGO_REGISTRY_TOKEN` = crates.io API token。没有这个 secret，workflow 必须失败，禁止跳过 publish。

## 流程

复制并勾选：

```
Release Progress:
- [ ] 1. 状态检查
- [ ] 2. 版本 + Changelog 草稿（等用户确认）
- [ ] 3. 改文件 + README 审阅
- [ ] 4. preflight + 测试
- [ ] 5. 提交 + 附注 tag（等用户确认）
- [ ] 6. push github（commit + tag）
- [ ] 7. 盯 Actions，核对 crates.io / Release
```

### 1. 状态检查

```bash
git status -sb
git rev-parse --abbrev-ref HEAD   # 应是 master
git describe --tags --always
git log "$(git describe --tags --abbrev=0)"..HEAD --oneline
```

工作区应干净，或只含用户已知的发版改动。不在 `master` 时先停下来问。

### 2. 版本 + Changelog 草稿

- **patch**：修 bug / 文档 / 抛光。
- **minor**：用户可见的 TUI/CLI 能力。
- **major**：破坏性（例如去掉兼容 bin）。

从上次 tag 的 commit 写英文 bullet，风格对齐现有 `CHANGELOG.md`。**先把建议版本号和条目发给用户，等确认再改文件。**

### 3. 改文件

1. Bump 两个 `Cargo.toml` 和 `alnav` 里的 `alnav-core = { ..., version = "X.Y.Z" }`。
2. `cargo check --workspace`，把 `Cargo.lock` 里的包版本一起提交。
3. `CHANGELOG.md` **顶部**插入 `## X.Y.Z — YYYY-MM-DD`（今天的日期）。
4. 审 README 双份：过时功能描述、截图是否还能代表当前 UI。徽章不用改。UI 大变再问要不要重截图。

### 4. preflight + 测试

```bash
bash .agents/skills/alnav-publish/scripts/preflight.sh --tag X.Y.Z
cargo test --workspace
cargo fmt --all --check
```

`preflight.sh` 检查双 crate 版本、core 依赖、CHANGELOG 对应节、本地/`github` 上还没有 `vX.Y.Z`。

### 5. 提交 + tag

把拟议的 commit message 和 `git diff` 给用户看，**确认后再** commit / tag。

```bash
git add alnav/Cargo.toml alnav-core/Cargo.toml Cargo.lock CHANGELOG.md README.md README.zh.md
# 只 add 实际改过的路径
git commit -m "$(cat <<'EOF'
chore: release vX.Y.Z

EOF
)"
git tag -a "vX.Y.Z" -m "vX.Y.Z <one-line summary>"
```

提交信息跟仓库习惯：英文、`chore:` / `docs:` / `feat:`。发版 commit 用 `chore: release vX.Y.Z`。

### 6. 推送

```bash
git push github master
git push github vX.Y.Z
```

先推 commit 再推 tag。不要 `--force`。不要推 `gongfeng`。

### 7. 验证

- Actions：`publish` workflow 必须绿。
- `cargo search alnav --limit 1` 应显示新版本。
- GitHub Releases 出现 `vX.Y.Z`，notes = CHANGELOG 该节正文。

```bash
gh run list --repo Rossettaylm/alnav --branch vX.Y.Z --limit 5
```

## 禁止

- 本机 `cargo publish`（除非 CI 挂了且用户明确要求补发；仍须 core → 等索引 → alnav）。
- 添加 git hook 来 publish 或 push tag。
- 无用户确认就 bump / tag / push。
- 用 `origin` 当远程名。
- `--no-verify` publish、yank（除非用户明确要求 yank）。

## 失败恢复

| 情况 | 做法 |
|------|------|
| preflight / 测试失败 | 修，不要 tag |
| 已 tag 未 push | `git tag -d vX.Y.Z` 后重来（仅限未 push） |
| tag 已 push，CI 因缺 secret 失败 | 补 `CARGO_REGISTRY_TOKEN` 后 **Re-run jobs**（脚本会跳过已上架的 crate） |
| crates 已上架、Release 失败 | 不要重发 crate；`gh release create vX.Y.Z --notes-file <(bash .agents/skills/alnav-publish/scripts/changelog-notes.sh X.Y.Z)` |
| crates 上架后发现 bug | 发 **新版本**；不要覆盖 |

## 脚本

在仓库根执行：

- `bash .agents/skills/alnav-publish/scripts/preflight.sh [--tag] [X.Y.Z]`
- `bash .agents/skills/alnav-publish/scripts/changelog-notes.sh X.Y.Z`
