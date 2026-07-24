# Design — pi Pet Welcome Header Extension

## 1. 机制选型

- 渲染入口：`ctx.ui.setHeader((tui, theme) => HeaderComponent)`，组件返回 `{ render(width: number): string[]; invalidate(): void }`。
  - `render(width)` 每帧由 pi 调用，返回 header 行数组；本组件据此做宽度自适应与截断。
  - `invalidate()` 留空实现（v1 无动画；header 内容静态，由 `setHeader` 时一次性绘制，pi 自身重绘时复用）。
- 注册时机：`pi.on("session_start", ...)` 内，`ctx.mode === "tui"` 且 `ctx.hasUI` 守卫；其余模式直接 `return`。
- 恢复原生 header：`pi.registerCommand("builtin-header", { handler: ctx => ctx.ui.setHeader(undefined) })`，复用 `custom-header.ts` 既有模式。

> 选型依据：`setHeader` 是 pi 官方「替换 logo + keybinding hints header」的稳定入口（见 `examples/extensions/custom-header.ts`），每行返回可控、`render(width)` 天然支持自适应，且不侵入对话区或原生资源展示。`setWidget` 无宽度参数与 theming；`ctx.ui.custom()` 为全屏交互组件，过度延伸，均不采用。

## 2. 文件结构

```
~/.pi/agent/extensions/pi-welcome-header/
├── index.ts            # 入口：session_start 注册 setHeader + /builtin-header 命令
├── header.ts           # HeaderComponent：render(width) 主逻辑（布局 + 截断 + theme）
├── meta.ts             # 项目信息采集：version/install/cwd/git → MetaInfo
├── truncate.ts         # 纯函数：尾部 … 截断、家目录折叠、label 对齐
└── mascots/
    ├── types.ts        # Mascot 接口
    └── pi.ts           # 默认实现：复用 custom-header.ts 的 pi block 画
```

单层目录、多文件，便于后续替换 mascot（只动 `mascots/`）。

## 3. Mascot 接口（可替换）

```ts
// mascots/types.ts
export interface Mascot {
  /** 像素画的原始行（未着色），不含尾部 padding；render 负责 pad 到列宽。 */
  lines(): string[];
  /** 对每一行的着色函数：theme → 着色后的字符串。 */
  colorize: (line: string, theme: Theme) => string;
  /** mascot 的固定显示列宽（render 据此分配左右两栏）。 */
  width: number;
}
```

- 默认 `mascots/pi.ts` 直接移植 `custom-header.ts` 的 `getPiMascot`：block `█` + pupil `▌`，`colorize` 用 `theme.fg("accent", ...)`（pi blue）+ `theme.fg("dim", ...)`（pupil）。
- `index.ts` 通过 `const mascot: Mascot = piMascot;` 静态导入默认实现；后续替换仅改这一行（或加配置选择，v1 不做）。

## 4. 布局算法

每帧 `render(width)`：

1. 计算各区域列宽：
   - `mascotCol = mascot.width`（固定，含左侧 2 格留白）。
   - `labelCol = maxLen(["pi","install","cwd","git"])`（label 列固定，等宽对齐）。
   - `valueCol = width - mascotCol - labelCol - sep(2) - rightPad(1)`。
   - 若 `valueCol < MIN_VALUE(8)`：进入降级（见 §6）。
2. 组装行：左 mascot 行（按 mascot.lines() 着色 + 右侧 pad）+ 右 meta 行（label + 值）。两侧按各自行数纵向居中（pet 行数 ≥ meta 行数时，meta 行从 pet 中线起排）。
3. 底部追加一行 `─` * width（用 `theme.fg("dim", "─".repeat(width))`）。

行数预算：mascot 6 行 + 分割线 1 行 = 7 行 header（与 `custom-header.ts` 的 9 行量级一致，pi header 可接受）。

## 5. 截断与格式化（纯函数，放 `truncate.ts`）

- `truncateTail(s, max)`: 超长则 `s.slice(0, max-1) + "…"`。
- `homefold(p)`: 以 `os.homedir()` 前缀折叠为 `~/...`。
- `padLabel(label, col)`: `label.padEnd(col)` + 一个空格分隔。
- meta 值示例：
  - pi → `v0.80.10`
  - install → `homefold(installPath)` 后 `truncateTail`
  - cwd → `homefold(ctx.cwd)` 后 `truncateTail`
  - git → `${branch}` +（dirty>0 ? ` *${dirty}` : ``），整串再 `truncateTail`

## 6. 降级（极窄）

- `width < mascot.width + 30`：meta 仅保留 `pi` 行（版本），其余行省略；mascot 仍画。
- `width < mascot.width + 8`：仅画 mascot + 底部分割线，meta 全省略。
- 任何情况下不换行、不溢出 `width`。

## 7. 数据采集（`meta.ts`）

`session_start` 同步采集并缓存到闭包 `let meta: MetaInfo | null`，`render` 读缓存；未就绪时该格显示 `—`。

- `version`: `import { VERSION } from "@earendil-works/pi-coding-agent"`。
- `installPath`: 优先 `require.resolve("@earendil-works/pi-coding-agent/package.json")` 取 dirname；失败回退从 `import.meta.url` 解析（jiti 下 `import.meta.url` 可用）。取 dirname 后 `homefold`。
- `cwd`: `ctx.cwd`，`homefold`。
- `git`: 用 `pi.exec("git", ["symbolic-ref","--short","HEAD"])` 取分支；`pi.exec("git",["status","--porcelain"])` 数行数取 dirty。非零退出码（非 git 仓）→ `git: null`，render 省略该行。
  - 采集为异步；完成后重新 `setHeader` 同一组件以触发重绘（等价于「数据落地后刷新一次」）。

> 为什么不一帧一帧查 git：`render` 必须无 IO、纯函数；git 状态变化频率低，启动采集一次足够（v1 不监听后续变化）。

> ⚠️ 关键约束（实测踩坑）：pi 的 `session_start` 事件 emit 是**串行 `await` 每个 handler**（见 `dist/core/extensions/runner.js` 的 `emit()`，双层 for + `await handler`），且 `agent-session.js` 在继续启动前 `await` 整个 emit。因此 handler 内**禁止 `await` 任何 IO**（git/exec），否则会阻塞整条 session_start 链。实现必须 **fire-and-forget**：先同步 `setHeader` 画占位 header，`void collectMeta().then(apply)` 在后台完成后再 `setHeader` 刷新一次。实测 2× git spawn ≈ 23ms，阻塞即被消除。

## 8. theme token 映射

| 元素 | token | 说明 |
|------|-------|------|
| mascot 主体 | `theme.fg("accent", ...)` | pi blue（与 `custom-header.ts` 一致） |
| mascot pupil | `theme.fg("dim", ...)` | 瞳孔对比 |
| label 列 | `theme.fg("muted", ...)` | 低对比标签 |
| pi 值 | `theme.fg("accent", ...)` | 强调版本 |
| install / cwd 值 | `theme.fg("text", ...)` 或 default | 正文色 |
| git branch | `theme.fg("success", ...)` | 绿 |
| git dirty 标记 | `theme.fg("warning", ...)` | 黄 |
| 底部分割线 | `theme.fg("dim", ...)` | 分隔弱化 |

> 全部经 `theme.fg(token, str)` 取色，零硬编码。token 名以 pi 现有 theme 为准（`accent`/`muted`/`dim`/`success`/`warning`/`text` 均为 `custom-header.ts` / 官方示例已用过的稳定 token）。

## 9. 错误处理 / 回退

- `require.resolve` 失败 → install 行回退 `homefold(import.meta.url dir)`，仍失败则显示 `?`。
- `pi.exec` git 失败 → git 行省略，不抛错。
- `setHeader` 调用本身置于 `session_start` 守卫内；若 `ctx.ui.setHeader` 不存在（旧版 pi）静默跳过。
- 任何采集异常 `try/catch` 吞掉并以 `—` 占位，绝不阻断 pi 启动。

## 10. 与原生展示的关系

- pi 原生四类信息（context/skills/prompts/extensions）由 pi 自身渲染于对话区 / 资源区，本 extension 仅替换顶部 header（原 logo + keybinding hints 区）。两者区域不重叠，互不读写对方数据，满足「不干扰」约束。

## 11. 验证手段

- 主要靠手动：`pi`（无参，TUI）目视 header；`pi -p "x"`（print）确认无 header；`/builtin-header` 恢复；`/reload` 热重载。
- 无自动化单元测试（依赖真实 TTY + pi runtime，v1 不投入）。
- 代码可做纯函数部分（`truncate.ts`）的轻量断言自测（可选，非验收项）。
