# 新会话提示词:实现 dsh-launcher

把下面整段(含分隔线内的内容)直接粘贴给一个新会话,即可开始实现。

---

你将在本会话中实现「dsh-launcher」——一个针对 **DeepSeek Harness(dsh)开发者**的纯启动器,用于源码启动 dsh web、一键更新构建、热重载开发模式、后台常驻,带亮色单页控制台。

## 0. 先读这些文件(权威依据,以实现它们为准)

1. `~/Desktop/dsh-launcher/doc/DEVELOPMENT-PLAN.md` —— 完整开发方案 v0.2:背景、职责边界、需求清单(F1–F11)、技术选型、架构、核心流程、UI 设计、里程碑(M0–M5)、验收标准。全部以实现它为准。
2. `~/Desktop/dsh-launcher/doc/ui/mockup.html` —— 亮色单页控制台高保真原型(浏览器打开预览),是 UI 的视觉基准,照它的配色 token 与布局实现。
3. dsh 仓库 `~/Desktop/deepseek-harness` 内的代码事实(读代码核实,不要凭印象):
   - `apps/cli/src/bin.ts` 与 `apps/cli/src/args.ts` —— `dsh web` 是 `--profile web` 的别名;源码启动等价于 `node --import tsx/esm apps/cli/src/bin.ts web`
   - `packages/bundle/web-app/src/index.ts` —— 就绪行 `dsh web: http://…` 是给 supervisor 的 readiness signal;前端 dist 未构建会抛错(要求先 `pnpm run build`)
   - `scripts/dev-web.ts` —— `pnpm run dev:web` 是 Vite HMR watcher,向 dsh web 广播 rebuilt 帧
   - `apps/web/tests/support.ts` —— 就绪行检测正则先例:`/dsh web: (http:\/\/[^\s]+)/`

## 1. 项目定位(铁律,违反即返工)

- 这是一个**纯启动器**:只负责 git 同步、依赖安装、构建、源码启动 dsh web、进程托管、日志、停止/重启。
- **不承载任何 dsh 界面**;主界面永远是 `http://127.0.0.1:3080/`(dsh web),启动器就绪后自动打开它。
- **不做桌面壳**(不引入 Tauri/Electron)、不做 dsh 插件、不侵入 dsh 源码、不打包发布。
- 用户后续始终通过浏览器访问 3080 使用 dsh;控制台只是控制面板。

## 2. 核心需求

1. **双击启动**:`bin/start.command`(macOS,chmod +x)双击 → 起本地服务(默认 `http://127.0.0.1:3090/`)→ 自动开浏览器显示亮色单页控制台;重复双击只召回已有实例(pid 文件 + 端口探测,单实例)。
2. **控制台三大主操作**:启动 / 开发模式 / 更新并构建;次级操作:停止、重建并重启、清空日志;另有状态条(服务 URL、PID、分支、HEAD、落后数、工作区状态)与日志区。
3. **源码启动**(关键):`spawn pnpm dsh web [--port <port>]`,等价 `node --import tsx/esm apps/cli/src/bin.ts web`;**不用 npx、不装发布包**,保证本地改动源码生效。
4. **就绪检测**:`node:readline` 逐行解析子进程 stdout,匹配 `/dsh web: (http:\/\/[^\s]+)/` → 自动打开 `http://127.0.0.1:3080/` → 状态 running;超时(120s)或退出码非 0 → failed + 尾部日志诊断。
5. **开发模式**:额外 `spawn pnpm run dev:web`(Vite HMR watcher),与 dsh web 同跑;UI 明确提示热更边界——客户端插件/前端改动免刷新热更,`lib/` 产物改动需「重建并重启」(停 → `pnpm run build` → 再启动)。
6. **更新并构建**:`git fetch` → dirty 检测(有改动默认 `git stash push -u`)→ `git pull --rebase --autostash` → rebase 冲突时**只报告冲突文件、绝不 `reset --hard`** → `pnpm-lock.yaml` 变化才 `pnpm install` → `pnpm run build`(阶段进度:build:lib:host → build:lib:client → build:web)。
7. **后台常驻**:子进程 `detached: true` + `unref()`,stdout/stderr 落盘 `~/.local/state/dsh-launcher/logs/`(按日期轮转);关掉浏览器/控制台不影响服务;服务常驻,可随时召回控制台。
8. **停止/重启**:进程组 SIGTERM → 宽限 5s → SIGKILL,清理 pid 文件,不残留僵尸。
9. **亮色单页控制台 UI**:严格按 `mockup.html` 视觉实现(配色 token、状态色圆点带脉冲、渐变主按钮、日志按来源着色 dsh web/dev:web/git/pnpm/launcher、SSE 实时推送日志与状态、设置折叠面板:仓库路径/端口/host/DSH_HOME/开机自启)。UI 文案用中文。

## 3. 技术约束

- Node `^22.19 || >=24`,ESM(`"type": "module"`)。
- **零 npm 运行时依赖**:只用 Node 内置模块 `node:http`、`node:child_process`、`node:fs`、`node:readline`;package.json 不写 dependencies。
- 前端纯 HTML/CSS/JS 单页,无框架;SSE(`text/event-stream`)推状态与日志。
- 外部命令 `git`、`pnpm` 通过 PATH 调用,启动时校验版本是否符合 dsh engines。
- 按方案 §4.3 的目标目录结构组织代码(src/server.mjs、state.mjs、process.mjs、repo.mjs、build.mjs、log.mjs、config.mjs;public/ 前端;bin/start.command;scripts/install-launch-agent.sh 可选)。

## 4. 实现顺序(按里程碑推进,每个里程碑完成即对照方案 §8 自查)

- M0 脚手架:git 已初始化;package.json、HTTP 静态服务 + 亮色空壳控制台、状态机骨架
- M1 最小闭环:源码启动 dsh web、就绪检测、自动打开 3080、日志落盘 + SSE
- M2 更新构建:git 同步、依赖比对、构建、失败诊断
- M3 开发模式:dev:web 同跑、HMR 热更提示、重建并重启
- M4 后台与自启:detached 常驻、LaunchAgent 脚本、单实例强化
- M5 打磨:状态机视觉完善、设置完整、错误诊断文案

## 5. 验收标准(全部满足才算完成)

- 双击 `bin/start.command` → 亮色控制台 <2s 出现
- 点「启动」→ ≤3s 就绪 → 自动打开 `http://127.0.0.1:3080/`
- 源码改动生效(改 dsh 的 `apps/cli/src/args.ts` help 文本后启动可见,证明非 npx 发布包)
- 开发模式:改 `packages/client/**` 免刷新;改 `lib/` 产物后点「重建并重启」生效
- 「更新并构建」:HEAD 前进、服务重启、日志完整;本地有未提交改动不丢代码;git 冲突只报告不破坏
- 关浏览器后服务继续跑;二次双击不重复起服务
- 构建失败 / git 冲突 / 端口占用(3080、3090)都有明确 UI 诊断,不闪退
- 控制台页面不含任何 dsh 会话/Agent 功能(纯启动器定位)

## 6. 收尾

- 完成并在 `~/Desktop/dsh-launcher` 提交代码(规范 commit message)。
- 最后给出使用说明:如何双击启动、如何进入开发模式、日志在哪里、如何停止/重启。

---

注意:这是纯实现会话,不要改变 dsh 源码,不要引入新的运行时依赖,不要扩大范围(不做桌面壳、不做安装包)。
