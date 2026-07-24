# Implement — pi Pet Welcome Header Extension

## 执行清单

### Step 1 — 脚手架
- [ ] 创建 `~/.pi/agent/extensions/pi-welcome-header/` 目录及空文件骨架（`index.ts` / `header.ts` / `meta.ts` / `truncate.ts` / `mascots/types.ts` / `mascots/pi.ts`）。
- [ ] 验证 jiti 能解析：`pi --version` 不报错（extension 仅占位时）。

### Step 2 — Mascot 接口 + 默认实现
- [ ] `mascots/types.ts`：定义 `Mascot` 接口（`lines()` / `colorize` / `width`）。
- [ ] `mascots/pi.ts`：从 `custom-header.ts` 移植 pi block 画，实现 `Mascot`，`width` 设为实际画宽。
- [ ] 自检：`mascot.lines()` 返回 6 行，每行可见宽度一致。

### Step 3 — 纯函数：截断与格式化
- [ ] `truncate.ts`：`truncateTail` / `homefold` / `padLabel`。
- [ ] （可选）小断言：`truncateTail("abcdef",4)==="abc…"`、`homefold(homedir+"/x")==="~/x"`。

### Step 4 — 数据采集
- [ ] `meta.ts`：`collectMeta(ctx, pi): Promise<MetaInfo>`，采集 version/install/cwd/git，全 try/catch。
- [ ] install 从 `process.argv[1]` realpath 向上找 `name==="@earendil-works/pi-coding-agent"` 的 package.json 目录（require.resolve 从扩展目录不可用）。
- [ ] git 用 `pi.exec`，非 git 仓返回 `git: null`。

### Step 5 — HeaderComponent 渲染
- [ ] `header.ts`：`makeHeader(mascot, meta): { render(width), invalidate }`。
- [ ] 实现 §4 布局算法 + §6 降级 + §5 截断 + §8 theme 映射。
- [ ] 底部 `─` * width 分割线。

### Step 6 — 入口注册
- [ ] `index.ts`：`session_start` 守卫 `ctx.mode === "tui" && ctx.hasUI`；**handler 不能 async-await 任何 IO**（pi 串行 await session_start emit，会阻塞启动）——先同步 `setHeader`（占位 meta），`void collectMeta().then(apply)` 后台完成后 `setHeader` 刷新一次。
- [ ] 注册 `/builtin-header` 命令恢复原生 header。
- [ ] `session_shutdown` 无需清理（无持久资源）。

### Step 7 — 手动验证（见 §验证命令）
- [ ] V1 TUI 启动见 header。
- [ ] V2 宽度变化重排 + 截断。
- [ ] V3 label 颜色 + 主题一致。
- [ ] V4 print/json 模式无 header。
- [ ] V5 `/builtin-header` 恢复。
- [ ] V6 非仓无 git 行省略。
- [ ] V7 `/reload` 热重载。

### Step 8 — 收尾
- [ ] 更新 `.trellis/spec/`（本仓无 pi extension 相关 spec；记录于 task notes 即可）。
- [ ] commit：本仓 `.trellis/tasks/07-23-pi-welcome-header/` 三件套；extension 实体在 `~/.pi/agent/extensions/`（仓外，不纳入本仓 commit，记录路径）。
- [ ] `task.py finish` + `task.py archive`。

## 验证命令

```bash
# 安装目录即 ~/.pi/agent/extensions/pi-welcome-header/，放好后：
pi                                      # V1: TUI 启动，目视 header（pet + meta + 分割线）
pi -p "hello"                           # V4: print 模式，确认无自定义 header
pi                                      # V2: 启动后拉伸终端宽度，观察 meta 值列截断
# V5: 在 TUI 内输入 /builtin-header，确认 header 恢复原生
# V7: 改动文件后在 TUI 内输入 /reload，确认热重载
cd /tmp && pi                           # V6: 非 git 目录，git 行应省略
```

## Review Gates

- Gate-1（Step 2 后）：mascot.lines() 行数/宽度正确，可独立 sanity check。
- Gate-2（Step 5 后）：在固定 width 下 render 输出行数稳定、无溢出（手动注入 width=74 / 40 / 30 三档）。
- Gate-3（Step 7）：V1–V7 全部手动通过 = AC1–AC9 达成。

## Rollback

- 临时回退：TUI 内 `/builtin-header`（仅当前 session）。
- 永久卸载：`rm -rf ~/.pi/agent/extensions/pi-welcome-header/` 或移出该目录，重启 pi。
- 无代码改动落在本仓源码，回退零成本。
