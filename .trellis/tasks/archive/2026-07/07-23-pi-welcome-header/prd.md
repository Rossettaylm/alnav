# pi Pet Welcome Header Extension

## Goal

为 pi TUI 制作一个启动期头部展示：像素风 pet 形象 + 项目信息（pi 版本 / 安装路径 / cwd / git 分支），风格与 pi 原生主题统一，作为 pi 全局 extension 安装。

## Background

- pi 启动时原生已在某处展示 context/skills/prompts/extensions 四类信息。本 extension **不替换、不重复、不干扰** 这四类原生展示，仅新增一个 header 区。
- 参考实现：pi 官方示例 `custom-header.ts` 已用 `ctx.ui.setHeader()` 把 pi block 画 mascot + `VERSION` 画进 header，本任务沿用同一机制并扩展为「pet + 结构化项目信息」。

## Requirements

### 功能需求

- **F1 — pet 形象**：在 header 左侧渲染像素风 pi mascot（复用 `custom-header.ts` 的 block 字符画 `█`/`▌`）。静态、无动画。
- **F2 — pet 可替换**：pet 抽象为可替换接口，默认实现为 pi mascot；后续换形象只需替换一个实现，不改渲染主逻辑。
- **F3 — 项目信息**：header 右侧纵向展示以下 meta（顺序固定）：
  - `pi` → `v{VERSION}`（从 `@earendil-works/pi-coding-agent` 导入 `VERSION`）
  - `install` → pi 安装路径（npm 包根目录）
  - `cwd` → 当前工作目录（`ctx.cwd`，`~` 折叠家目录）
  - `git` → `{branch} *{dirty}`（分支名 + 未提交变更数；非 git 仓则此行省略）
- **F4 — label + 颜色**：每行 meta 同时具备固定宽 label 列与颜色区分（label 用 muted/dim 色，值按语义着色），二者都做。
- **F5 — 底部分割线**：header 最底一条 `─` 撑满当前终端宽度，与下方对话区分隔。
- **F6 — 自适应宽度**：每帧根据 `render(width)` 重算布局；值列超长时尾部 `…` 截断；极端窄宽（低于下限）时优雅降级（至少保留 pet + 版本），不溢出、不换行错乱。
- **F7 — 主题一致**：所有颜色经 `theme` 的 token（如 `accent`/`muted`/`dim`/`success`）取得，不硬编码 RGB / `Color::*`，与 pi 原生 header 风格一致。
- **F8 — 作用域**：仅在 `ctx.mode === "tui"`（且 `ctx.hasUI`）时启用 header；print / json / rpc 模式静默不启用。
- **F9 — 可关闭 / 可恢复**：提供命令恢复 pi 原生 header（参考 `custom-header.ts` 的 `/builtin-header`），便于排障与回退。

### 非功能需求

- **N1 — 放置位置**：全局 `~/.pi/agent/extensions/pi-welcome-header/`（跨项目通用工具）。
- **N2 — 性能**：header 数据采集（git 状态等）在 `session_start` 一次性完成并缓存；`render()` 为纯函数读缓存，不产生 IO，不阻塞每帧渲染。
- **N3 — 零业务侵入**：不依赖 aloggrep 仓任何代码；与 aloggrep 的 `.pi/extensions/trellis` 等共存。
- **N4 — 类型安全**：TypeScript，遵循 pi extension 类型（`ExtensionAPI` / `Theme`）。

## Constraints

- 不修改、不替换 pi 原生四类信息（context/skills/prompts/extensions）的展示。
- 不引入额外 npm 运行时依赖（仅用 pi 已提供的 `@earendil-works/pi-coding-agent` / `typebox` / `@earendil-works/pi-tui` 及 Node 内置）。
- 不做动画（v1 范围外，避免 `invalidate` 定时器复杂度）。
- 不支持非 TUI 模式的渲染。
- Windows 终端兼容性不在 v1 验收范围（block 字符在主流 macOS/Linux 终端正常即可）。

## Acceptance Criteria

- [ ] AC1：全局安装后，`pi`（无参）在 TUI 模式启动时，顶部出现自定义 header：左侧像素 pet，右侧纵向 4 行 meta（pi/install/cwd/git，git 行仅在 git 仓出现），底部 `─` 分割线。
- [ ] AC2：终端宽度变化（拉宽/缩窄）时，header 每帧重排：pet 不变，meta 值列按宽度截断（尾部 `…`），不溢出、不错位。
- [ ] AC3：label 列等宽对齐，label 与值用不同 theme 颜色区分，视觉上与 pi 原生 header 配色风格一致。
- [ ] AC4：pet 为可替换接口；默认实现独立成文件，渲染主逻辑不直接硬编码 mascot 行。
- [ ] AC5：`pi -p "hi"`（print 模式）/ json 模式启动时，不渲染自定义 header、不报错。
- [ ] AC6：执行恢复命令（如 `/builtin-header`）后，自定义 header 消失、pi 原生 header 恢复。
- [ ] AC7：pi 原生四类信息（context/skills/prompts/extensions）的展示位置/内容与本 extension 安装前后一致（未受干扰）。
- [ ] AC8：在 aloggrep 仓（有 git）与一个非 git 目录分别启动，git 行分别正确显示 / 正确省略。
- [ ] AC9：代码位于 `~/.pi/agent/extensions/pi-welcome-header/`，`/reload` 可热重载。

## Out of Scope (YAGNI)

- pet 动画（眨眼/呼吸）。
- 右侧 context/skills/prompts/extensions 表格（用户已明确去除；pi 原生已有展示）。
- pet 形象自定义配置文件 / 主题选择器（v1 仅留接口，默认一个实现）。
- 跨终端 block 字符 fallback 策略（假定主流终端支持）。
- 非 TUI 模式输出。
