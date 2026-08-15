# dsh-launcher 桌面版全量完成与上线方案

> 方案版本：2.0
>
> 编写日期：2026-08-15
>
> 当前基线：M0、M1、M2 已完成且未提交
>
> 本方案终点：在一个持续会话内完成 M3–M6，推送 GitHub，Actions 全绿并发布 `v0.3.0`

## 1. 最终决定

这次不再按“先做一部分、下次继续”的方式推进。后续开发必须作为一个完整交付闭环执行：

1. 接管当前未提交的 M0–M2 工作区；
2. 完成 Logs、Settings、First-run、托盘、关闭隐藏、自启、静默启动和最终视觉走查；
3. 把当前 Node daemon 的全部核心职责迁移到 Rust；
4. 删除 bridge、HTTP/SSE 控制面、3090、浏览器控制台、C launcher 和旧更新器；
5. 实现 macOS/Windows 正确的进程树管理、配置迁移和应用自更新；
6. 完成跨平台 CI、安装包和 updater artifacts；
7. 建分支、分阶段提交、推送、创建 PR；
8. 等待并修复 GitHub Actions，直到全绿；
9. 合并到 `main`；
10. 发布 `v0.3.0` GitHub Release，并核对下载资产。

除必须由账号持有人提供的平台证书外，不允许因为任务较大、构建耗时、CI 第一次失败或会话较长而停在本地。若 macOS Developer ID / notarization 或 Windows Authenticode 证书不可用，仍需完成无平台签名的 Preview Release，并在 Release Notes 明确系统提示；Tauri updater 自身的签名 key 必须完成配置，不能一起跳过。

## 2. 当前仓库事实

### 2.1 GitHub 状态

- 仓库：`https://github.com/Yoahoug/dsh-launcher`
- 可见性：Public
- 默认分支：`main`
- 本地状态：`main` 比 `origin/main` ahead 1，且 M0–M2 全部未提交
- 最新 Release：`v0.2.1`
- 旧发布 workflow：`.github/workflows/release.yml`
- 新桌面构建 workflow：`.github/workflows/desktop-build.yml`，尚未推送验证
- 本次目标版本：`v0.3.0`

### 2.2 已完成基线

| 里程碑 | 状态 | 已有交付 |
|---|---|---|
| M0 行为安全网 | 完成 | Node 单元/集成测试 40/40，fake git/pnpm/node，真实 server E2E |
| M1 桌面壳 | 完成 | Tauri 2 + React/TS/Vite/Tailwind，窗口、single-instance、window-state、mock UI |
| M2 bridge | 完成 | Rust bridge、token/Origin、状态日志轮询、动作转发、detach、安全接管 |

当前已验证：

- `pnpm check`：通过；
- `pnpm test`：40/40；
- `pnpm typecheck`：通过；
- `pnpm build:renderer`：通过；
- `cargo fmt --check`：通过；
- `cargo clippy -- -D warnings`：通过；
- `cargo test`：7/7；
- Tauri debug build：通过；
- macOS 独立窗口、单实例、bridge、detach：手工通过。

### 2.3 工作区保护规则

- 当前旧 `doc/DEVELOPMENT-PLAN.md`、`doc/NEW-SESSION-PROMPT.md`、`doc/ui/mockup.html` 保持删除，不得恢复；
- 当前新增的两版方案文档属于重构交付资料，需要保留；
- 不改 `/Users/yoahoug/Desktop/deepseek-harness`；
- 不使用 `git reset --hard`、`git checkout --` 或其它丢改动操作；
- 开发开始时直接从当前脏工作区创建 `codex/desktop-app-complete`，不 stash、不清空现有修改。

## 3. 当前代码缺口审计

### 3.1 UI 与产品体验

- 设置齿轮目前错误地打开日志目录；
- 日志按钮只打开日志目录，没有应用内 Logs 页面；
- 分段切换器和三卡片已有骨架，但维护模式、动作反馈和失败恢复未完成；
- `cancel` 当前无实现；
- `get_logs` Rust command 仍返回空占位，晚打开日志页会丢历史；
- 缺少 toast、确认框、空状态、首次运行引导和字段级错误；
- theme 只有 CSS token，没有完整持久化与 system theme 监听；
- 缺少 renderer component tests 和视觉截图门禁。

### 3.2 桌面生命周期

- 缺少系统托盘；
- 点击窗口关闭会退出应用，而不是隐藏；
- 缺少 macOS Dock 隐藏/恢复策略；
- 缺少 Windows taskbar 隐藏/恢复策略；
- 缺少 Tauri autostart 和 silent startup；
- 桌面偏好和 dsh engine 设置混在同一个 `SettingsSnapshot`；
- 当前 `autostart` 保存到 Node 配置会调用旧 LaunchAgent，不适合 Tauri App；
- 普通退出、停止并退出、自更新重启三条路径尚未分开。

### 3.3 bridge 与打包

- `server_path()` 只支持仓库 dev 路径，打包 `.app/.exe` 后找不到 `src/server.mjs`；
- Windows `is_alive`、`port_holder`、`ps_command` 是占位；
- Windows token 生成不是密码学强随机；
- daemon 依赖系统 Node 才能启动桌面核心；
- 轮询是每秒三次 HTTP 请求；
- 3090 是全局固定资源，Node integration 和 Cargo E2E 并行会冲突；
- bridge 存在自愈重启与退出竞态的长期维护成本。
- `cargo test` 当前会报告 `bridge.rs` 测试模块未使用 import；`cargo clippy --all-targets -- -D warnings` 还会发现对非 `Drop` 类型显式 `drop(handle)`。现有 clippy 未加 `--all-targets`，因此这两项测试目标问题未进入原门禁。

这些 bridge 问题不单独建设长期解决方案。因为本轮必须完成 Rust 原生核心，最终发布产物不会包含 Node daemon，所以 `server_path()` 打包问题、Windows bridge 占位、token 和 3090 冲突都以删除 bridge 的方式根治。只允许为 M3 开发补最小日志读取等必要适配，不为 legacy daemon 新建另一套发布体系。

### 3.4 Windows 与发布

- dsh/dev 进程树在 Windows 上需要 Job Object，当前 Node/bridge 路径不可靠；
- `tauri.conf.json` 还没有 updater、resources、平台 bundle 配置；
- 三处版本号尚未建立同步检查；
- 没有 Tauri updater public key 和 GitHub signing secrets；
- 没有 macOS notarization / Windows Authenticode 证书；
- `desktop-build.yml` 尚未在 GitHub 运行；
- 旧 `release.yml` 仍发布 C launcher + Node 资源包，必须被桌面发布流程替代。

## 4. 最终产品与架构

### 4.1 用户可见形态

- Finder / 开始菜单中的独立桌面应用；
- 原生窗口，视觉和操作节奏对齐 CC Switch；
- 关闭窗口默认隐藏到托盘；
- 托盘可直接启动、打开、停止、查看日志和退出；
- 应用可开机静默启动；
- dsh web 继续运行在 3080，并通过外部浏览器打开；
- launcher 不打开浏览器控制台、不监听 3090；
- Node 缺失时桌面 App 仍能打开并提供运行时安装入口；
- GitHub Release 提供 macOS 和 Windows 下载资产。

### 4.2 最终技术架构

```text
┌───────────────────────────────────────────────────────────┐
│ dsh-launcher · Tauri 2                                    │
│                                                           │
│ React 18 / TypeScript / Vite / Tailwind                   │
│ ├─ Dashboard                                               │
│ ├─ Logs                                                    │
│ ├─ Settings / First-run                                    │
│ └─ DesktopApi (invoke + event only)                        │
│                       │                                    │
│ Rust Native Core      ▼                                    │
│ ├─ lifecycle / tray / single-instance / autostart          │
│ ├─ AppState + action coordinator                           │
│ ├─ LogHub + config + migration                             │
│ ├─ Supervisor (Unix process group / Windows Job Object)    │
│ ├─ RepoService / BuildService                              │
│ ├─ RuntimeService / managed Node                           │
│ └─ Tauri signed updater                                    │
└───────────────┬──────────────────────┬────────────────────┘
                │                      │
        pnpm dsh web            pnpm run dev:web
        127.0.0.1:3080          development mode only
```

最终不存在：

- Node launcher daemon；
- `127.0.0.1:3090`；
- HTTP/SSE 控制 API；
- renderer 中的 fetch/EventSource；
- `public/` 浏览器控制台；
- `native/launcher.c`；
- 自制 zip 指针 updater；
- LaunchAgent 安装/卸载脚本。

## 5. 单会话执行原则

1. 里程碑只是内部检查点，不是停止点；
2. 每个里程碑结束立即跑门禁、提交，再继续下一个；
3. CI 失败后读取日志、修复、重新推送并继续等待；
4. 不因 token/context/耗时主动结束，允许任务自动续接；
5. 不要求用户重复确认已经明确授权的分支、commit、push、PR、merge、tag 和 Release；
6. 只有凭据确实不存在且无法自行安全生成时才降级，不得把可解决问题报成 blocker；
7. 平台证书缺失只影响 OS 信任提示，不阻止 Preview Release；
8. Tauri updater key 可以自行生成并安全写入 GitHub Secrets，私钥不得进入 git、日志或最终回复；
9. 版本发布失败不能跳号，仍修复并发布 `v0.3.0`；
10. 最终回复必须给出仓库、PR/merge、Actions 和 Release 下载页面链接。

## 6. 实施阶段 A：接管工作区与可重复门禁

### A1. 建分支并记录基线

- [ ] 确认 git remote、GitHub 登录和工作区状态；
- [ ] 从当前工作区执行 `git switch -c codex/desktop-app-complete`；
- [ ] 不清理、不 stash、不恢复旧文档；
- [ ] 保存当前测试输出和手工验收事实；
- [ ] 核对 M0–M2 变更全在任务范围内。

### A2. 提交已完成的 M0–M2

按逻辑选择性 stage，不把全部内容塞进一个 commit：

1. `test: add launcher regression safety net`
2. `feat: add tauri desktop shell and daemon bridge`
3. `docs: add desktop refactor plans`

旧文档三项删除可在 docs commit 中一并提交，因为它们已被新版方案替代；不得恢复后再删除。

### A3. 统一验证入口

新增跨平台的 `scripts/verify-desktop.mjs`，用 `spawnSync` 串行执行：

1. `pnpm check`（legacy 未移除前）；
2. `pnpm test`（legacy 未移除前）；
3. `pnpm typecheck`；
4. `pnpm test:ui`；
5. `pnpm build:renderer`；
6. `cargo fmt --check`；
7. `cargo clippy --all-targets -- -D warnings`；
8. `cargo test`；
9. `pnpm tauri build --debug --no-bundle`。

在 M4 删除 3090 后，Node/Cargo 端口碰撞自然消失；迁移完成前验证脚本必须串行。

退出条件：当前基线有可重复的一键门禁，M0–M2 已形成可回退 commits。

## 7. 实施阶段 B：M3 完整桌面体验

### B1. 设置模型拆分

把设置拆成两个清晰 contract：

```text
EngineSettings
  repoPath, port, host, dshHome, openDshOnReady,
  autoUpdateCheck, buildArgs, readyTimeoutMs, startTimeoutMs

DesktopPreferences
  theme: system | light | dark
  closeBehavior: tray | quit
  launchOnStartup: boolean
  silentStartup: boolean
  showTrayIcon: boolean
  confirmStopAndQuit: boolean
```

- DesktopPreferences 由 Rust 原子 JSON 或 `tauri-plugin-store` 持久化；
- autostart 只由 Tauri 插件管理，不再转发给 Node LaunchAgent；
- 旧配置中的 `autostart` 一次性迁移到 `launchOnStartup`；
- 保存失败要回滚 UI optimistic state 并显示可读错误；
- schema 在 Rust/TypeScript 两端同步并覆盖 contract tests。

### B2. Logs 页面

- 应用内完整日志页，不是打开 Finder；
- 首次进入能读取历史 ring buffer；
- 实时订阅 `app://log-appended`；
- 默认最多保留 2,000 条，避免长期后台内存增长；
- 来源筛选：launcher、dsh web、dev:web、git、pnpm；
- 级别筛选：info、ok、warn、err；
- 自动滚动可暂停；用户向上滚动自动暂停；
- 搜索、复制选中/全部、清空、打开日志目录；
- 错误日志可从 Dashboard 一键跳转并带筛选；
- bridge 过渡期补最小 `get_logs(since)` 实现，M4 切到 Rust LogHub。

### B3. Settings 页面

- 基础：仓库路径、端口、host、DSH_HOME；
- 行为：关闭到托盘、开机启动、静默启动、就绪后打开 dsh；
- 外观：system/light/dark；
- 运行时：Node/pnpm/git 状态、重新检测、安装托管 Node；
- 更新：当前版本、自动检查、手动检查；
- 关于：项目地址、版本、许可证；
- 原生目录选择器；
- 端口和路径字段级校验；
- 运行中修改关键项时提示“重启后生效”；
- 设置页返回时不丢失未保存输入，离开前提示。

### B4. First-run

首次启动条件：没有有效 repoPath 或迁移状态缺失。

步骤：

1. 欢迎与产品职责说明；
2. 自动探测 `~/Desktop/deepseek-harness`；
3. 选择仓库；
4. 检测 dist、Node、pnpm、git；
5. 可安装托管 Node；
6. 保存并进入 Dashboard。

First-run 失败不能让 App 白屏；用户始终可进入 Settings 修复。

### B5. Dashboard 与交互补完

- 维护模式真正对应 update/rebuild 操作；
- `cancel` 映射为后端取消/stop，不再 no-op；
- 所有动作有 pending、成功、失败 toast；
- busy 时只禁用冲突动作，仍允许日志和取消；
- 停止、重建、停止并退出使用原生确认框；
- 错误卡可复制详情、跳日志、重试；
- 更新横幅可展示进度；
- 根据真实 `EnvironmentSnapshot` 渲染环境卡；
- 运行时间每秒刷新只在前端计算，不增加后端轮询；
- 主题监听系统变化并持久化用户选择；
- 适配 `900×600`、`1000×650`、`1280×800`；
- 支持 reduced motion、键盘焦点和基本可访问性。

### B6. 托盘与生命周期

新增 `tray.rs`、`lifecycle.rs`、`preferences.rs`：

托盘菜单：

```text
DSH Launcher · 当前状态
打开主窗口
打开 DeepSeek Harness
────────────
普通启动
开发模式
更新并构建
重建并重启
停止
────────────
查看日志
设置
检查更新
────────────
退出 Launcher
停止服务并退出…
```

行为：

- 菜单项随状态启用/禁用；
- 左键托盘图标召回窗口；
- CloseRequested 默认 prevent close + hide；
- macOS 隐藏后切 Accessory/隐藏 Dock，召回恢复 Regular；
- Windows 隐藏后 `set_skip_taskbar(true)`，召回恢复；
- 普通退出：持久化状态、detach dsh、退出 App；
- 停止并退出：确认、停止进程树、退出；
- updater restart：不走普通退出清理死锁路径，不停止 dsh；
- runtime 自动 ExitRequested：阻止退出，保持托盘；
- autostart + silent startup 正确联动；
- 退出前保存窗口状态、移除托盘图标、释放 single-instance。

### B7. UI 测试与视觉门禁

新增 Vitest、Testing Library、jsdom：

- idle/running/dev/failed/busy；
- mode → action mapping；
- Logs 筛选、暂停、清空；
- Settings 校验/保存失败；
- theme/system change；
- First-run 各步骤；
- close/tray/menu 的纯逻辑映射。

浏览器 mock 页面生成 `idle/running/dev/failed` 截图，与 CC Switch `main-zh.png` 并排检查：

- 标题栏留白；
- 分段控制位置；
- 卡片圆角、边框、阴影；
- 操作密度；
- 颜色层级；
- 900×600 不溢出；
- dark mode 对比度。

退出条件：所有当前占位按钮都有真实功能，关闭窗口后托盘可召回，UI 达到可日用完成度。

## 8. 实施阶段 C：M4 Rust 原生核心

### C1. 迁移策略

当前 M0 的行为 contract 是迁移的权威依据。按 service 逐个实现并验证；在所有 Rust 集成测试覆盖之前不能删除 legacy 模块。

最终模块：

```text
src-tauri/src/
├── app.rs
├── commands.rs
├── contract.rs
├── state.rs
├── config.rs
├── preferences.rs
├── log_hub.rs
├── tray.rs
├── lifecycle.rs
├── migration.rs
└── services/
    ├── supervisor.rs
    ├── repo.rs
    ├── build.rs
    ├── runtime.rs
    └── update.rs
```

### C2. AppState 与 ActionCoordinator

- 单一 AppState；
- 状态转换函数集中管理；
- 一次只允许一个破坏性/长流程；
- 每个流程持有 epoch/cancellation token；
- stop/cancel 递增 epoch；旧任务不得回写 running/failed；
- 状态变化直接 emit Tauri event，不轮询；
- 快照与事件 payload 使用同一 contract；
- App 启动、托盘和 UI 调用同一 ActionCoordinator，禁止出现三套动作逻辑。

### C3. Config 与迁移

- 继续兼容 `~/.config/dsh-launcher.json`；
- 状态和日志继续兼容 `~/.local/state/dsh-launcher/`；
- 写入采用 temp + fsync + rename；
- 迁移旧 autostart、state、PID 和日志位置；
- 记录 `migration-version`，保证幂等；
- 检测旧 3090 daemon：验证 PID、命令行、端口后只 detach/终止 daemon，绝不停止 dsh；
- 发现非 launcher 3090 占用时不再关心，因为新 App 不使用该端口；
- 成功迁移后删除 token 文件；
- 不静默删除用户 LaunchAgent，先取消新旧重复注册，再记录迁移结果。

### C4. LogHub

- 内存 ring 2,000 条；
- 单调递增 id；
- 来源与级别枚举；
- 文件按日期写入并限制保留周期/总大小；
- append 同时广播 event；
- 启动时可加载最近尾部用于 Logs 首屏；
- 日志写失败不能导致主流程失败；
- 对 URL query、Authorization、token、已知密钥做脱敏。

### C5. Supervisor

共同要求：

- 通过参数数组启动 `pnpm dsh web` / `pnpm run dev:web`；
- stdout/stderr 逐行读取并写 LogHub；
- readiness 使用 `/dsh web: (http:\/\/[^\s]+)/`，再做端口确认；
- ready timeout、早退和 spawn error 给出明确诊断；
- normal/dev 模式、PID、startedAt、readyAt 持久化；
- 启动前诊断端口占用，不误杀占用者；
- stop：SIGTERM/等效优雅结束 → 5 秒 → 强杀；
- App 普通退出时子进程继续运行；
- 重启后通过 PID、命令行、端口三重校验接管；
- PID 复用不能误杀。

Unix：

- `setsid`/process group；
- stop 对进程组发送 SIGTERM/SIGKILL；
- 使用 `libc` 或可靠 crate，不拼 shell 命令。

Windows：

- 使用 `windows-sys`；
- 创建进程时 `CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW`；
- 使用 Job Object 管理 dsh/dev 子进程树；
- Job 不设置 close-handle 自动杀进程，保证 launcher 退出后 dsh 存活；
- stop 时 `GenerateConsoleCtrlEvent` 可用则先优雅退出，否则 `TerminateJobObject`；
- 重启 launcher 后可验证并重新接管现有 PID；
- 测试 Job Object 不误杀其它进程。

### C6. RepoService

- `git fetch origin`；
- status/branch/head/ahead/behind；
- dirty 时保留现有安全策略；
- stash 失败中止；
- pull/rebase 冲突只报告；
- 禁止 reset hard；
- 用户 stash 必须保留/恢复规则明确；
- 支持 cancellation；
- stdout/stderr 进入 LogHub；
- 命令均使用绝对可执行路径 + 参数数组。

### C7. BuildService

- 判断 lockfile 变化；
- 需要时 `pnpm install`；
- 按现有构建阶段执行；
- 每阶段更新 phase；
- 失败保留阶段、退出码和日志尾部；
- stop/cancel 能终止构建进程树；
- update-build 完成后按原模式重启；
- rebuild-restart 不执行 git pull；
- 所有行为由 Rust 集成测试覆盖。

### C8. RuntimeService

迁移 `nodeenv.mjs` 和 `tools.mjs` 的完整候选策略：

- PATH；
- Homebrew / usr local；
- nvm；
- volta；
- fnm；
- 托管目录；
- Windows Program Files / LocalAppData；
- Node 范围 `^22.19 || >=24`；
- pnpm/git 绝对路径；
- Finder/桌面启动的精简 PATH 修复；
- Node 24 官方包下载、平台/架构选择、SHA256 校验、解压、原子安装；
- 下载进度、取消、失败清理；
- App 自身不依赖 Node。

### C9. Rust 等价测试

把 M0 的关键场景移植到 Rust integration tests：

- config；
- state/epoch/cancel；
- Node 版本和候选；
- repo dirty/stash/fetch/rebase conflict；
- lockfile/build phase/failure；
- readiness/timeout/early exit；
- normal/dev；
- stop/process tree；
- port busy；
- recall 三重校验；
- detach；
- Windows Job Object（Windows CI 条件测试）；
- migration idempotency；
- log redaction。

继续使用 fake git/pnpm/node，但由 Rust 测试直接注入 tool paths；不得依赖真实 deepseek-harness。

### C10. 删除 legacy

仅在 Rust 等价门禁全绿后删除：

- `src/*.mjs`；
- `public/`；
- `native/launcher.c`；
- `bin/start.command` / `start.bat`；
- 旧 LaunchAgent scripts；
- 自制 updater/zip；
- bridge.rs 和 ureq；
- token/Origin 逻辑；
- Node server tests（对应场景已迁 Rust 后）；
- package scripts 中的 legacy start/check。

退出条件：启动 App 后 `lsof -iTCP:3090` 无监听；Node 不存在时 App 正常显示；所有 dsh 管理动作由 Rust 完成。

## 9. 实施阶段 D：M5 更新、打包与跨平台

### D1. 版本统一

版本 `0.3.0` 必须同时写入：

- `package.json`；
- `src-tauri/Cargo.toml`；
- `src-tauri/tauri.conf.json`。

新增脚本校验三者一致，CI 不一致直接失败。

### D2. Tauri updater

- 添加 updater/process 插件和最小 permissions；
- 配置 GitHub Release updater endpoint；
- 生成 Tauri updater signing key；
- public key 写入 `tauri.conf.json`；
- private key 和密码只写 GitHub Secrets：
  - `TAURI_SIGNING_PRIVATE_KEY`
  - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
- 私钥临时文件用 `mktemp` 创建，设置权限，写入 Secret 后安全删除；
- UI 支持 check/download/install/relaunch 进度；
- 更新重启不停止 dsh；
- 更新失败可重试且不破坏当前安装。

### D3. bundle 目标

macOS：

- arm64 和 x86_64，资源允许时产出 universal；
- `.dmg`；
- updater `.app.tar.gz` + `.sig`；
- minimum macOS 12；
- 无 Developer ID 时发布 unsigned Preview，并写清右键打开/Gatekeeper 提示。

Windows：

- x64；
- NSIS `.exe`（优先）和/或 MSI；
- updater `.nsis.zip` + `.sig`；
- GUI subsystem，无控制台闪窗；
- 无 Authenticode 时发布 unsigned Preview，并写清 SmartScreen 提示。

资产名全部英文且稳定，例如：

```text
dsh-launcher_0.3.0_aarch64.dmg
dsh-launcher_0.3.0_x64.dmg
dsh-launcher_0.3.0_x64-setup.exe
latest.json
```

### D4. CI/CD

整合为两层：

1. `desktop-ci.yml`
   - push/PR；
   - macOS + Windows；
   - install frozen lockfile；
   - UI typecheck/test/build；
   - Rust fmt/`clippy --all-targets`/test；
   - Tauri no-bundle build；
   - Windows 条件测试 Job Object。
2. `release.yml`
   - workflow_dispatch：完整打包但不发布，用于 tag 前 dry run；
   - `v*` tag：构建、签 updater、创建 GitHub Release；
   - 上传安装包、updater artifacts、`latest.json`；
   - 同一版本不重复创建冲突资产。

旧 C launcher release workflow 在新 dry run 成功前保留；成功后原地替换，避免仓库同时存在两个同名 release workflow。

### D5. 文档

README 必须改为桌面版事实：

- 新截图；
- macOS/Windows 安装；
- 系统托盘；
- 普通/开发/维护模式；
- Node/pnpm/git 要求；
- 日志路径；
- 开机启动；
- 退出语义；
- 自动更新；
- unsigned Preview 系统提示；
- 从 v0.2.1 升级迁移；
- 开发、测试和打包命令；
- CC Switch 仅作为设计参考，不使用其品牌资产。

退出条件：workflow_dispatch 在 macOS/Windows 全部成功并产生可下载 artifacts，才允许打 tag。

## 10. 实施阶段 E：M6 GitHub 上线与 Release

### E1. 本地最终门禁

按顺序执行：

1. formatter；
2. typecheck；
3. UI tests；
4. renderer build；
5. Rust fmt；
6. Rust clippy；
7. Rust tests；
8. Tauri debug no-bundle；
9. Tauri macOS bundle；
10. 安装包 smoke test；
11. 启动后确认 3090 无监听；
12. 普通启动/开发/停止/detach/recall；
13. close-to-tray/单实例/autostart/silent；
14. 更新重启不停止 dsh。

用自动循环替代无法在单会话合理等待的 24 小时 soak：

- 窗口隐藏/召回 50 次；
- start/stop fake service 20 次；
- daemon 时代已删除，确认无 3090；
- 日志连续写入 10,000 条后 ring 仍限制 2,000；
- updater failure 注入；
- 30 分钟后台运行观察 CPU/内存无持续增长。

### E2. 推送与 PR

- 推送 `codex/desktop-app-complete`；
- 创建 PR，正文列出 M0–M6、测试、迁移和 Release 计划；
- 等待 PR checks；
- 使用 `gh run watch` / `gh pr checks --watch`；
- 失败时读取具体 job log、修复、commit、push；
- 重复直到 macOS/Windows 全绿；
- 无未解决 review/blocker 后 squash merge 到 `main`；
- 更新本地 main。

### E3. Release dry run

- 在 main 上 workflow_dispatch release workflow；
- 等待 macOS/Windows 打包全绿；
- 下载 artifacts 到临时目录并检查：
  - 文件名；
  - 版本号；
  - updater `.sig`；
  - `latest.json` URL/平台映射；
  - macOS bundle 内容；
  - Windows installer 存在；
- dry run 失败必须先修 main 并再次全绿，不得带着已知问题打 tag。

### E4. 发布 v0.3.0

- 确认 `package/Cargo/Tauri` 都是 `0.3.0`；
- 创建 annotated tag `v0.3.0`；
- 推送 tag；
- 等待 release workflow；
- 若 transient failure 直接 rerun；若代码/workflow failure，修复后仍使用 `v0.3.0`，不能跳到 `v0.3.1` 逃避；
- Release 标题：`dsh-launcher v0.3.0 — Desktop Preview`；
- Release Notes 包含：
  - 独立 Tauri App；
  - 托盘/后台；
  - Rust 原生核心；
  - 普通/开发/维护；
  - 升级迁移；
  - 下载说明；
  - unsigned 平台提示（若适用）；
  - 已知限制；
- 核对 Release 页面资产能下载；
- 核对 `latest.json` 可访问；
- 最终只在 Release 页面完整可用后宣布上线完成。

## 11. Commit 计划

建议提交序列：

1. `test: add launcher regression safety net`
2. `feat: add tauri desktop shell and daemon bridge`
3. `docs: add desktop refactor plans`
4. `feat: complete desktop pages and tray lifecycle`
5. `test: add desktop ui and lifecycle coverage`
6. `refactor: migrate launcher core to rust`
7. `test: port launcher integration coverage to rust`
8. `build: add tauri updater and desktop release workflow`
9. `docs: document desktop installation and migration`

每个 commit 前运行与其风险匹配的测试；M4/M5 后运行全门禁。不要为了提交数量机械拆散同一原子变更，也不要把整个项目压成一个 commit。

## 12. 最终验收清单

### 12.1 桌面体验

- [ ] 独立窗口，不打开浏览器控制台；
- [ ] CC Switch 同类视觉完成度；
- [ ] Dashboard/Logs/Settings/First-run 全可用；
- [ ] light/dark/system；
- [ ] close-to-tray；
- [ ] 动态托盘；
- [ ] single-instance；
- [ ] autostart + silent startup；
- [ ] macOS Dock / Windows taskbar 行为正确；
- [ ] 所有按钮无占位。

### 12.2 核心行为

- [ ] normal/dev；
- [ ] update-build/rebuild-restart；
- [ ] cancel/stop；
- [ ] readiness/timeout/early exit；
- [ ] git dirty/stash/conflict 安全；
- [ ] Node 自动发现/托管安装；
- [ ] 日志实时/历史/筛选/落盘；
- [ ] 端口占用不误杀；
- [ ] detach/recall；
- [ ] updater restart 不停止 dsh；
- [ ] Windows Job Object 进程树测试通过。

### 12.3 架构完成度

- [ ] 无 Node daemon；
- [ ] 无 bridge.rs；
- [ ] 无 3090；
- [ ] 无 public browser UI；
- [ ] 无 C launcher；
- [ ] 无 LaunchAgent 脚本；
- [ ] renderer 无 HTTP control API；
- [ ] App 无 Node 启动依赖；
- [ ] Tauri capabilities 最小化；
- [ ] 配置迁移幂等。

### 12.4 工程与上线

- [ ] UI/Rust 全门禁通过；
- [ ] macOS/Windows Actions 通过；
- [ ] workflow_dispatch 打包 dry run 通过；
- [ ] PR 已合并 main；
- [ ] `v0.3.0` tag；
- [ ] GitHub Release 已发布；
- [ ] macOS/Windows 资产和 updater metadata 可下载；
- [ ] README/截图/迁移/许可证完整；
- [ ] 最终回复包含仓库、Actions 和 Release 链接。

## 13. 允许的降级与真正 blocker

### 可降级但不能停工

- 没有 Apple Developer ID：发布 unsigned macOS Preview；
- 没有 Windows Authenticode：发布 unsigned Windows Preview；
- 没有 Intel Mac 实机：用 macOS CI 构建/测试 x64，注明未本地实机；
- 没有 Windows 本机：使用 Windows Actions 的条件测试和打包，Release Notes 注明验证范围；
- CC Switch 无法联网：使用已知参考提交和仓库内设计目标继续。

### 只有这些可以暂停发布

- GitHub 登录失效且无法 push；
- 仓库权限不允许设置 Secret、创建 PR/tag/Release；
- GitHub 服务持续不可用；
- 用户仓库出现新的、与当前任务冲突的外部提交且无法安全合并；
- updater 私钥无法安全生成或 Secret API 被权限阻止。

即使发生真正 blocker，也必须先完成全部本地开发、测试、commits 和可重试的 workflow 配置，然后明确给出唯一剩余动作，不能提前停在中间里程碑。

## 14. 完成定义

“代码写完”“本地构建通过”“PR 已创建”都不算完成。

本次唯一完成定义是：

> `Yoahoug/dsh-launcher` 的 `main` 已包含完整桌面重构，macOS/Windows CI 全绿，`v0.3.0` GitHub Release 已发布且下载资产可用；若缺少平台商业证书，则以明确标注的 unsigned Desktop Preview 上线。
