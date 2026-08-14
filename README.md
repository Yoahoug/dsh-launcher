# dsh-launcher

针对 **DeepSeek Harness(dsh)开发者**的纯启动器:源码启动 dsh web、一键更新构建、热重载开发模式、后台常驻、亮色单页控制台。

> **定位铁律:启动器只是一个启动器。** 它不承载任何 dsh 界面;主界面永远是 `http://127.0.0.1:3080/`(dsh web),启动器就绪后自动打开它,之后只负责更新 · 构建 · 进程托管 · 日志。

## 快速开始(macOS)

```sh
git clone <本仓库> ~/Desktop/dsh-launcher
chmod +x ~/Desktop/dsh-launcher/bin/start.command   # 若权限未保留
```

**双击 `bin/start.command`** → 本地服务(`http://127.0.0.1:3090/`)自动起、浏览器自动打开亮色控制台(<0.5s);重复双击只召回已有实例,绝不重复起服务。

### 控制台三个主操作

| 按钮 | 做什么 |
|---|---|
| **启动** | 源码启动 `pnpm dsh web --port 3080`(等价 `node --import tsx/esm apps/cli/src/bin.ts web`),**不用 npx、不装发布包**;就绪行 `dsh web: http://…` 命中 → 自动打开主界面 → 状态 running |
| **开发模式** | 同跑 `dsh web` + `pnpm run dev:web`(HMR watcher)。提示热更边界:客户端插件/前端改动**免刷新热更**,`lib/` 产物改动需「重建并重启」 |
| **更新并构建** | `git fetch` → dirty 自动 stash → `git pull --rebase --autostash`(冲突**只报告、绝不 reset --hard**)→ `pnpm-lock.yaml` 变化才 `pnpm install` → `pnpm run build`(阶段进度)→ 重启服务 |

次级操作:**停止**(进程组 SIGTERM → 5s → SIGKILL)、**重建并重启**(停 → 构建 → 按原模式启动)、**清空日志**。

### 状态条与日志

- 状态条:服务 URL / PID / 端口 / 运行时长 / HMR 活跃;仓库分支 / HEAD / 落后数 / 工作区状态。
- 日志区:按来源(dsh web · dev:web · git · pnpm · launcher)着色、可过滤、可暂停滚动;SSE 实时推送。

### 日志位置

```
~/.local/state/dsh-launcher/logs/YYYY-MM-DD.log   # 所有子进程输出,按日期轮转
~/.local/state/dsh-launcher/logs/launcher.out.log # 服务自身 stdout(双击启动时)
```

### 停止 / 重启

- 控制台点「停止」:停止 dsh web / dev:web / 构建子进程,launcher 服务本身继续常驻(可随时召回看日志、再启动)。
- 关掉浏览器/控制台标签页**不影响服务**;服务崩溃或 launcher 被强杀后,重启 launcher 会自动**召回**仍在运行的 dsh web(pid 文件 + 端口 + 命令行校验)。
- 彻底停掉 launcher 服务本身:`pkill -f src/server.mjs`(会连带优雅停止托管的 dsh web)。

### 开机自启(可选)

控制台「设置 → 开机自启」开关,或手动:

```sh
scripts/install-launch-agent.sh     # 安装 LaunchAgent(登录后起 launcher 服务,不开浏览器)
scripts/uninstall-launch-agent.sh   # 卸载
```

### 设置项

仓库路径(默认 `~/Desktop/deepseek-harness`)、dsh web 端口(默认 3080)、host(默认 `127.0.0.1`;dsh 拒绝 `0.0.0.0`)、`DSH_HOME`、构建参数透传、就绪后自动打开主界面、开机自启。端口被占用时给出占用进程诊断,改端口后重试即可。

## 架构与文档

- `src/server.mjs` — HTTP 服务 + SSE + 动作编排(状态机 idle → syncing → installing → building → starting → running / failed)
- `src/process.mjs` — 进程托管:detached 进程组、就绪行正则 `/dsh web: (http:\/\/[^\s]+)/`、SIGTERM→SIGKILL
- `src/repo.mjs` / `src/build.mjs` — git 同步 / lockfile 比对 + 构建阶段识别
- `src/log.mjs` — 环形缓冲 + 落盘 + SSE 广播
- `src/config.mjs` / `src/state.mjs` — 设置读写 / 状态机
- `public/` — 亮色单页控制台(纯 HTML/CSS/JS,零依赖)
- 完整方案:[`doc/DEVELOPMENT-PLAN.md`](doc/DEVELOPMENT-PLAN.md);UI 原型:[`doc/ui/mockup.html`](doc/ui/mockup.html)

## 技术约束

Node `^22.19 || >=24`(本机 23 可跑服务与 dsh web,但 `dev:web` 的 tsx/tsdown 与 23 不兼容,控制台会给出明确提示,建议切 Node 22/24)、ESM、**零 npm 运行时依赖**(仅 Node 内置模块)、前端纯 HTML/CSS/JS 单页。

## 使用注意

- dsh web 启动需前端 dist 已构建(首次请先「更新并构建」;仓库代码事实:`packages/bundle/web-app` 未构建 dist 会直接报错)。
- 与 dsh 的关系:launcher 是**外挂进程管家**,不改 dsh 任何源码、不进插件树、不代理 3080。
