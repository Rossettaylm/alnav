<p align="center">
  <img src="./assets/readme/hero-zh.svg" width="100%" alt="alnav — App / Android Log Navigator：默认 vim 风 TUI，alnav grep 提供 CLI">
</p>

<p align="center">
  <a href="./README.md">English</a> · <strong>中文</strong>
</p>

<p align="center">
  <a href="https://crates.io/crates/alnav"><img src="https://img.shields.io/crates/v/alnav.svg" alt="crates.io version"></a>
  <a href="https://crates.io/crates/alnav"><img src="https://img.shields.io/crates/d/alnav.svg" alt="downloads"></a>
  <a href="https://github.com/Rossettaylm/alnav/blob/master/LICENSE"><img src="https://img.shields.io/crates/l/alnav.svg" alt="license"></a>
  <a href="https://github.com/Rossettaylm/alnav"><img src="https://img.shields.io/github/stars/Rossettaylm/alnav?style=flat" alt="GitHub stars"></a>
</p>

**alnav**（App / Android Log Navigator）面向 Android logcat、xlog、HarmonyOS hilog：默认打开 vim 风格 TUI；脚本与 AI agent 用 `alnav grep` 做结构化过滤、崩溃与聚合。

<table>
  <tr>
    <td align="center" valign="top" width="50%">
      <img src="./assets/dashboard.png" width="100%" alt="启动 Dashboard：主题色 ALNAV 字标、快捷操作与最近文件">
      <br>
      <sub>Dashboard</sub>
    </td>
    <td align="center" valign="top" width="50%">
      <img src="./assets/loglist.png" width="100%" alt="日志列表：主题色级别徽标、tag 与光标选中">
      <br>
      <sub>Log list</sub>
    </td>
  </tr>
</table>

## 安装

```bash
cargo install alnav

alnav -f app.log                 # TUI
alnav grep -f app.log --level E  # CLI
```

<details>
<summary>从源码安装</summary>

```bash
git clone https://github.com/Rossettaylm/alnav
cd alnav
cargo install --path alnav
```
</details>

配置目录：`--config-path` > `$ALNAV_HOME` > `~/.config/alnav/`。

## 主题

九套 TUI 配色，写在 `config.toml` 后**重启**生效。Dashboard 字标、边框、日志色都跟主题走；`alnav grep` CLI 颜色不变。

```toml
# ~/.config/alnav/config.toml
theme = "kanagawa"
```

| `theme` | 签名色 | 别名 |
|:--------|:-------|:-----|
| `default` | 青，不涂画布 | `builtin` |
| `onedark` | 蓝 | `one-dark` |
| `dracula` | 品红 | |
| `everforest` | 绿 | |
| `tokyo-night` | 蓝 | `TokyoNight` |
| `catppuccin-mocha` | 品红 | `catppuccin` / `mocha` |
| `gruvbox-dark` | 黄 | `gruvbox` |
| `nord` | 青 | |
| `kanagawa` | 蓝 | `kanagawa-wave` |

可选 overlay：复制 [`alnav/examples/theme.toml`](alnav/examples/theme.toml) 到 `~/.config/alnav/theme.toml`。`[palette]` 先合并 18 槽 ANSI 再映射；其余键覆盖语义色（`accent`、选中底、8 档 `highlight`）。`highlight` 必须正好 8 项，否则整份丢弃。

## 使用

```bash
alnav -f app.log
alnav --adb
alnav --hdc --device <serial>

adb logcat | alnav grep --tag "OkHttp" --level W
alnav grep -f app.log --summary
alnav grep -f app.log --crashes
alnav grep --help
```

TUI 内 `?` 打开 Help，`C-p` 打开命令面板，`yc` 将当前过滤导出为一行 `alnav grep …`。

## 为什么不只用 grep

一条 `alnav grep` 通常能替代多轮 `grep` / `rg` 加自行统计：

| | alnav | grep / rg |
|:--|:------|:----------|
| 格式 | hilog / xlog / threadtime / brief | 纯文本 |
| 级别 | `--level W` → W/E/F | 手写正则 |
| 布尔 | `-e '(tag ~ A or tag ~ B) and level >= W'` | 多管道 |
| 聚合 | `--summary` / `--histogram` / `--dedupe` | 自行算 |
| 崩溃 | `--crashes` 结构化输出 | 手写识别 |
| 输出 | `--fields` / `--limit` / JSON · CSV | 整行文本 |

仅限 Android / HarmonyOS 设备日志；通用文本请用 `rg`。

## CLI

```bash
alnav grep -f app.log --tag "OkHttp" --msg "timeout" --level W
alnav grep -f app.log -e '(tag ~ OkHttp or tag ~ Retrofit) and level >= W'
alnav grep -f app.log --since 10:30:00 --until 10:35:00
alnav grep -f app.log --format json --fields timestamp,level,tag,msg --limit 50
alnav grep --adb --device <serial> --level E
```

同字段多值 OR，加 `--and` 改 AND；跨字段 AND。`--level W` 匹配 W/E/F。多个 `-e` 之间 OR。

`--adb` 与 `--hdc` 互斥，且不可与 `-f`、`--time-context`、`--follow-pid`/`--follow-tid`、`--sort-time` 同用。

## 日志格式

| 格式 | 示例 |
|:-----|:-----|
| **hilog** | `04-16 11:52:56.297 11114 11114 I A00201/com.tencent.mqq/QRouter: msg` |
| **xlog** | `2026-03-04 10:23:28.872\|1[3542]3831\|3542\|I\|NTKernel\|msg` |
| **threadtime** | `03-04 10:23:28.872  3542  3831 I NTKernel: msg` |
| **brief** | `I/NTKernel(3542): msg` |

[MIT](LICENSE)
