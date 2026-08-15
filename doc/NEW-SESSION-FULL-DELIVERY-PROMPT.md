# 新会话提示词：一次完成桌面重构并发布 GitHub Release

复制下面分隔线内的全部内容到一个新的 Codex 会话。

---

你要在一个持续会话内接管 `/Users/yoahoug/Desktop/dsh-launcher` 当前工作区，完成剩余全部桌面重构，直到代码合并到 GitHub `main`、macOS/Windows Actions 全绿并发布 `v0.3.0` GitHub Release。不要只规划，不要只完成 M3，不要在 PR 创建后停止，也不要把 M4/M5 留给下一会话。

本提示词明确授权你在 `Yoahoug/dsh-launcher` 范围内执行完成交付所必需的外部操作：创建分支、选择性 stage、commit、push、创建/更新/合并 PR、触发并等待 Actions、读取失败日志并修复、生成 Tauri updater signing key、把私钥安全写入该仓库 GitHub Secrets、创建并推送 `v0.3.0` tag、创建/更新 GitHub Release。不得修改其它仓库、删除已有 Release/tag、绕过失败测试、泄露私钥或改写无关历史。

## 1. 权威方案与必读文件

先完整阅读：

1. `/Users/yoahoug/Desktop/dsh-launcher/doc/DESKTOP-APP-FULL-DELIVERY-PLAN.md` —— 本次唯一执行方案；
2. `/Users/yoahoug/Desktop/dsh-launcher/doc/DESKTOP-APP-REFACTOR-PLAN.md` —— 第一版架构背景；
3. `/Users/yoahoug/Desktop/dsh-launcher/README.md`；
4. 当前 Node 核心 `src/*.mjs`；
5. 当前 Tauri：`src-tauri/src/{lib,bridge,state,commands,contract}.rs`、`Cargo.toml`、`tauri.conf.json`、capabilities；
6. 当前 React：`src-ui/src/`；
7. `tests/`；
8. `.github/workflows/{desktop-build,release}.yml`；
9. `package.json`、`pnpm-workspace.yaml`、lockfiles。

参考 UI：`https://github.com/farion1231/cc-switch`，研究基线提交 `40d747c009bff6a6097d5094e57d205420d9b24c`。重点参考它的主界面截图、Tauri 窗口/托盘/close-to-tray/single-instance/updater 实现。只复刻桌面视觉语言和运行方式，不复制名称、Logo、Provider 资产或无关业务；若直接复用 MIT 代码片段，保留必要声明。

## 2. 当前进度是事实，不要重做

M0 已完成：

- `tests/helpers/env.mjs` 沙箱 + fake git/pnpm/node；
- config/state/nodeenv/repo/build 单元测试；
- 真实 `server.mjs` 集成测试；
- 40/40 通过；
- 覆盖 readiness、超时、早退、epoch、进程组、端口、recall、dev、detach、token/Origin。

M1 已完成：

- Tauri 2 + React 18 + TypeScript + Vite 6 + Tailwind v4；
- 1000×650/min 900×600/overlay；
- single-instance、Dock Reopen、window-state；
- DesktopApi 唯一 IPC；
- idle/running/dev/failed mock；
- CC Switch 风格 Dashboard 骨架。

M2 已完成：

- Rust bridge spawn/takeover Node daemon；
- token 0600、Origin 防护；
- state/log/config 轮询与事件；
- 全动作转发；
- detach 后 dsh 存活；
- legacy UI 仅 `DSH_LAUNCHER_LEGACY_UI=1`；
- Cargo tests 7/7；
- macOS debug 和手工链路已通过。

当前所有改动仍未提交；本地 `main` ahead origin 1。旧 `doc/DEVELOPMENT-PLAN.md`、`doc/NEW-SESSION-PROMPT.md`、`doc/ui/mockup.html` 是用户已有删除，不得恢复。新版方案文档需要保留并上线。

## 3. 唯一终点

必须连续完成：

1. M3 完整 UI、Logs、Settings、First-run、托盘和生命周期；
2. M4 将 Node daemon 核心全部迁到 Rust；
3. 删除 bridge、3090、HTTP/SSE、public UI、C launcher、旧 updater/LaunchAgent；
4. 实现 Unix process group 与 Windows Job Object；
5. M5 Tauri updater、跨平台打包、版本统一、CI/CD；
6. 本地全门禁和 smoke/视觉验证；
7. 分阶段 commits；
8. push、PR、等待/修复 Actions、merge main；
9. release dry run；
10. 发布并核验 `v0.3.0` Release。

里程碑是检查点，不是停止点。上下文压缩、耗时或首次 CI 失败都不是结束理由；继续执行直到达到终点或遇到方案中定义的真正外部权限 blocker。

## 4. 第一动作：todo 与分支

先检查 git status、remote、gh auth、可用工具链，然后建立不超过 8 项的 todo。todo 必须覆盖：

1. 接管/提交 M0–M2；
2. M3 UI/设置/日志；
3. 托盘/生命周期；
4. M4 Rust 核心；
5. legacy 删除与迁移；
6. M5 updater/打包/CI；
7. 全量验证；
8. PR/merge/v0.3.0 Release。

立即把第一项标为 `in_progress`，每完成一项立即更新，始终只有一个 `in_progress`。从当前脏工作区创建：

```text
codex/desktop-app-complete
```

不要 stash、reset 或清理当前改动。

## 5. 先固化当前成果

在继续开发前：

1. 串行重跑当前 baseline；
2. 选择性 stage 并提交：
   - `test: add launcher regression safety net`
   - `feat: add tauri desktop shell and daemon bridge`
   - `docs: add desktop refactor plans`
3. 添加 `scripts/verify-desktop.mjs`，跨平台串行执行全部门禁，避免 Node integration 与 Cargo E2E 同时争用 3090；
4. 把 clippy 门禁改为 `cargo clippy --all-targets -- -D warnings`，并清掉当前 `bridge.rs` 测试模块的 unused import 与 `drop_non_drop` 两项问题；
5. 不要把现有所有改动粗暴塞进一个 commit。

## 6. M3 必须实现

### 6.1 页面

- Dashboard：完成所有模式和动作反馈；
- Logs：历史 + 实时、来源/级别筛选、搜索、暂停滚动、复制、清空、打开目录，ring 上限 2,000；
- Settings：基础、行为、外观、运行时、更新、关于；
- First-run：仓库探测/选择、环境检测、托管 Node 安装；
- 所有占位按钮必须消失；
- toast、确认框、错误详情、loading/empty 状态完整。

### 6.2 设置拆分

不要继续把 Tauri 桌面行为写进 Node 配置：

```text
EngineSettings:
repoPath/port/host/dshHome/openDshOnReady/autoUpdateCheck/
buildArgs/readyTimeoutMs/startTimeoutMs

DesktopPreferences:
theme/closeBehavior/launchOnStartup/silentStartup/
showTrayIcon/confirmStopAndQuit
```

DesktopPreferences 由 Rust 持久化；旧 `autostart` 只迁移一次，不能再调用旧 LaunchAgent。

### 6.3 托盘/生命周期

- 动态托盘：状态、打开窗口、打开 dsh、普通/开发、更新构建、重建、停止、日志、设置、检查更新、退出、停止并退出；
- CloseRequested 默认隐藏；
- macOS Accessory/Regular Dock 策略；
- Windows taskbar skip/restore；
- autostart + silent startup；
- runtime 自动退出请求必须被阻止以保持托盘；
- 普通退出只 detach，不停止 dsh；
- 停止并退出二次确认；
- updater restart 单独处理，不停止 dsh、不与普通退出清理死锁；
- 保存窗口状态、清理托盘、释放单实例锁。

### 6.4 UI 验证

- 加 Vitest + Testing Library + jsdom；
- 覆盖状态、动作映射、日志、设置、主题、First-run；
- 用 mock URL 生成 idle/running/dev/failed、light/dark、900×600/1000×650 截图；
- 与 CC Switch `main-zh.png` 并排检查间距、圆角、阴影、层级和操作密度；
- 使用项目 Logo，不使用 CC Switch 品牌资产。

## 7. M4 必须完成 Rust 原生核心

不要发布 bridge 版，也不要花时间给 `server_path()` 做长期资源打包。M3 开发结束后直接迁移原生核心，发布前 bridge 必须删除。

实现：

- `config.rs`：旧配置兼容、原子写、校验；
- `preferences.rs`：桌面设置；
- `state.rs` + ActionCoordinator：唯一状态机、busy、epoch/cancel；
- `log_hub.rs`：ring、文件、事件、脱敏；
- `services/supervisor.rs`：dsh/dev、readiness、timeout、stop、detach、recall；
- `services/repo.rs`：fetch/status/stash/rebase，冲突只报告；
- `services/build.rs`：lockfile、install、分阶段 build、取消/失败；
- `services/runtime.rs`：PATH/Homebrew/nvm/volta/fnm/托管 Node/Windows 路径、Node 24 下载 SHA256；
- `migration.rs`：旧 daemon/PID/autostart/config 幂等接管；
- Tauri commands/events：不经 HTTP，不轮询。

硬性行为：

- `pnpm dsh web` 和 `pnpm run dev:web` 参数数组启动；
- readiness `/dsh web: (http:\/\/[^\s]+)/` + 端口确认；
- SIGTERM → 5 秒 → SIGKILL；
- stop/cancel 后旧任务不能回写；
- dirty/stash/rebase 不丢用户修改；
- 退出 App 默认不停止 dsh；
- PID + 命令行 + 端口三重 recall；
- App 在 Node 缺失时仍能打开。

Windows 必须实现真实 Job Object，不允许保留占位：

- `windows-sys`；
- `CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW`；
- Job Object 管理子进程树；
- close job handle 不自动杀 dsh；
- stop 时优雅信号可用则先用，最终 `TerminateJobObject`；
- Windows CI 条件测试进程树和不误杀。

把现有 M0 场景等价迁入 Rust integration tests。只有 Rust tests 覆盖并全绿后，才删除：

- `src/*.mjs`
- `public/`
- `native/launcher.c`
- `bin/`
- 旧 LaunchAgent/package scripts
- `bridge.rs`、ureq、token/Origin
- 自制 updater/zip
- 已被 Rust 测试替代的 Node server tests

删除后验证启动 App 时 3090 无监听。

## 8. M5 打包与 updater

版本统一为 `0.3.0`：

- `package.json`
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`

增加 CI 版本一致性校验。

配置 Tauri updater：

- updater/process 插件和最小 permissions；
- GitHub Release endpoint；
- 生成 updater signing key；
- public key 入配置；
- private key/password 只通过 `gh secret set` 写入本仓库 Secrets；
- 私钥不写仓库、不打印、不在最终回复出现，临时文件安全删除；
- UI 显示检查、下载、安装、重启进度；
- 更新重启不停止 dsh。

CI/CD：

- PR/push：macOS + Windows，UI typecheck/test/build，Rust fmt/clippy/test，Tauri no-bundle；
- release workflow 支持 workflow_dispatch dry run 和 `v*` tag 正式发布；
- 产出 macOS arm64/x64（可行则 universal）DMG + updater tar/sig；
- 产出 Windows x64 NSIS EXE（可选 MSI）+ updater zip/sig；
- 生成 `latest.json`；
- 资产文件名纯英文；
- 旧 C launcher workflow 在新 dry run 成功后替换。

没有 Apple Developer ID / Authenticode 时不要停工：发布明确标注 unsigned 的 Desktop Preview，并更新 README/Release Notes 的 Gatekeeper/SmartScreen 说明。Tauri updater 签名不能省略。

## 9. 验证必须串行且真实

最终至少运行：

```text
pnpm typecheck
pnpm test:ui
pnpm build:renderer
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
pnpm tauri build --debug --no-bundle
pnpm tauri build（macOS bundle）
```

legacy 删除前继续运行 `pnpm check` 和 `pnpm test`；删除后由 Rust 等价 tests 替代并调整脚本。

手工/自动 smoke：

- 首次启动/First-run；
- idle/running/dev/failed；
- start/dev/update/rebuild/cancel/stop；
- close-to-tray/召回/单实例；
- autostart/silent；
- detach/recall；
- updater restart；
- Node 缺失；
- git conflict/端口占用/build failure；
- 3090 无监听；
- 日志 10,000 条时 ring 仍 ≤2,000；
- 隐藏/召回 50 次；
- fake service start/stop 20 次；
- 30 分钟后台观察无持续 CPU/内存增长。

不能只说“编译通过”。UI 要截图检查，安装包要实际启动，Release 资产要下载核验。

## 10. GitHub 上线流程（必须执行到底）

1. 分阶段 commit，建议：
   - `test: add launcher regression safety net`
   - `feat: add tauri desktop shell and daemon bridge`
   - `docs: add desktop refactor plans`
   - `feat: complete desktop pages and tray lifecycle`
   - `test: add desktop ui and lifecycle coverage`
   - `refactor: migrate launcher core to rust`
   - `test: port launcher integration coverage to rust`
   - `build: add tauri updater and desktop release workflow`
   - `docs: document desktop installation and migration`
2. push `codex/desktop-app-complete`；
3. 创建 PR；
4. 用 `gh pr checks --watch` / `gh run watch` 等待；
5. CI 失败就读取 job log、修复、commit、push，再等；
6. macOS/Windows 全绿后 squash merge 到 main；
7. 在 main 触发 release workflow_dispatch dry run；
8. 等待全绿，下载 artifacts 检查版本、文件名、签名和 `latest.json`；
9. dry run 通过后创建 annotated `v0.3.0` tag 并 push；
10. 等待正式 release workflow；
11. transient failure rerun，代码/workflow failure修复后仍使用 `v0.3.0`，不能跳号；
12. 发布 `dsh-launcher v0.3.0 — Desktop Preview`；
13. 核对 macOS/Windows 安装包、updater artifacts、`latest.json` 都可下载。

不要在 PR 已创建、PR 已合并、tag 已推送或 workflow 正在运行时提前结束。终点是 Release 页面可用。

## 11. 安全与范围

- 不修改 deepseek-harness；
- 不用 destructive git；
- 不删除已有 tag/Release；
- 不执行任意 renderer shell；
- 外部命令只用 Rust 参数数组和绝对路径；
- Tauri capabilities 最小化；
- 日志脱敏；
- updater/Node 下载校验；
- 保留用户工作区意图；
- 不扩大到 Linux、云同步、账户或插件市场；
- 不自动复制 CC Switch 代码/资产。

## 12. 允许降级与 blocker

允许降级但必须继续发布：

- 无 Apple 证书 → unsigned macOS Preview；
- 无 Authenticode → unsigned Windows Preview；
- 无 Windows/Intel Mac 本机 → 以对应 GitHub Actions 测试和打包为准，并注明验证范围。

只有以下情况可以在完成全部本地工作和 commits 后报告阻塞：

- GitHub auth 失效；
- 仓库权限禁止 push/Secrets/PR/tag/Release；
- GitHub 持续不可用；
- 外部提交造成无法安全自动合并的真实冲突；
- updater key 无法安全生成或 Secrets API 被拒。

如果同一 blocker 连续出现，仍要先穷尽安全替代路径；不要把普通编译错误、CI 失败、缺依赖或工作量大当 blocker。

## 13. 完成定义与最终回复

唯一完成定义：

> `Yoahoug/dsh-launcher` 的 `main` 已包含完整 Tauri 桌面版，macOS/Windows CI 全绿，`v0.3.0` Release 已上线并有可下载资产；缺平台商业证书时以 unsigned Desktop Preview 明确标注。

最终回复必须包含：

- 完成的 M3–M6 内容；
- 主要架构变化和已删除 legacy；
- 本地测试结果；
- PR、merge commit、Actions 链接；
- GitHub Release 页面链接；
- macOS/Windows 资产名；
- 签名/实机验证范围和唯一剩余风险；
- 不得输出任何私钥或 Secret。

现在开始：检查工作区并创建 todo，立即建立 `codex/desktop-app-complete`，串行重跑 baseline，然后选择性提交 M0–M2，之后持续完成到 `v0.3.0` Release 上线。

---
