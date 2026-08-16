<div align="center">

# ⚡ dsh-launcher

**为 [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)(dsh)开发者打造的桌面启动器**

Tauri 2 原生核心(纯 Rust,无 Node daemon)+ React 控制台 · 一键启动 / 构建 / 更新 dsh web · 后台常驻与托盘

</div>

---

## ✨ 功能

| 操作 | 说明 |
|---|---|
| **首次运行向导** | 检测不到有效仓库时进入全屏引导:填写已有仓库路径,或一键克隆 `deepseek-harness` |
| **克隆仓库** | git clone 语义:选择「放置位置」(桌面等非空目录也行),自动生成 `<位置>/deepseek-harness`;克隆各阶段带进度条,失败/取消只清理本次 staging,**绝不覆盖已有目录** |
| **启动** | 源码启动 dsh web(优先 node 直连仓库声明的 dsh 入口,Windows 上单进程、无多余 cmd/conhost);启动前若缺少构建产物(web dist / 客户端 bundle)会自动先构建;就绪行(`dsh web: http://…`)命中后自动打开主界面 |
| **开发模式** | 同跑 `dsh web` + `pnpm run dev:web`(HMR);前端改动免刷新热更 |
| **更新并构建** | `git pull --rebase --autostash`(冲突**只报告、绝不 reset --hard**)→ lockfile 变化才 `pnpm install` → 构建 → 重启服务 |
| **重建并重启 / 停止** | 进程组停止(SIGTERM → 5s → SIGKILL;Windows 为 CTRL_BREAK → Job Object),零残留;危险动作有确认弹窗 |
| **托管工具链** | 签名 catalog(全部国内镜像)一键安装 Node 24 LTS / pnpm(Windows 另有托管 MinGit)到托管目录;自动解析 dsh 兼容 Node(`^22.19 \|\| >=24`) |
| **环境检查** | 仓库可用性、前端 dist 是否已构建、Node 版本是否在 dsh 范围内,逐项给出可执行诊断;**检测结果文件缓存**(24h 内秒开,安装/克隆/设置变更自动失效,「重新检测」强制刷新) |
| **主窗口 DeepSeek 工作区** | 标题栏可在「启动器 / DeepSeek」间切换；DeepSeek 由同一原生窗口内的零权限子 WebView 承载，不弹独立窗口、不使用 iframe、不跳浏览器 |
| **插件管理** | 官方插件管理增强版:全部 loader 行卡片化(来源层徽标/启停开关/自动生成配置表单/原始 YAML 高级模式),配置写入 profile 补丁前自动备份 + `--dump-config` 校验,失败自动回滚;运行中的 dsh web 无需重启即热重载;联动 dsh-plugins 仓库一键构建安装/移除 |
| **技能管理** | 独立管理技能的增删改/导入(`$DSH_HOME/skills`),自动扫描发现本机 Codex / Claude Code / Cursor / OpenCode / Agents 等工具目录的既有技能;「一键启用」把外部根写入 `skill-filesystem.customSkillDirs`,模型侧经 HMR 直接可调用 |
| **成功后自动进入** | 启动、开发、更新构建或重建只有到达真实成功终态，并通过服务、健康检查、端口持有者和页面就绪校验后才进入 DeepSeek；失败、取消和超时不会提前显示成功 |
| **会话保持与重连** | 返回启动器仅隐藏子 WebView，再进入时保留登录态、会话和页面状态；服务重启时显示断线状态并自动重连 |
| **后台常驻与退出** | 关窗默认最小化到托盘,服务不受影响;重启启动器后自动**召回**运行中的 dsh web(进程存活 + 命令行 + 端口三重校验);托盘「退出」= 先停止 dsh 进程树再完全退出,**无残留后台进程** |
| **托盘 / 单实例** | 托盘动态状态菜单 + 左键召回主窗口;重复启动只召回,不重复起 |
| **日志** | 实时推送 + 按来源着色;落盘 `~/.local/state/dsh-launcher/logs/` 可回溯 |
| **自动更新** | Tauri updater(minisign 签名),启动时自动检查或手动检查,下载安装后自动重启 |
| **设置** | 仓库路径、端口、host、`DSH_HOME`、构建参数透传、超时、开机自启、主题(亮色/深色/跟随系统)、关窗行为、插件与技能(目标 profile / dsh-plugins 路径 / managed 技能根 / 外部技能根) |

> 主 React WebView 只负责原生窗口外壳、标题栏和启动器控制台；DeepSeek 页面仍由本机 `dsh web` 提供，并在标题栏以下的独立零权限子 WebView 中显示。远程页面不获得 Tauri IPC，也不注入本地密钥或状态。

## 🚀 快速开始

### 方式一:下载安装包(推荐,支持自动更新)

从 [Releases](https://github.com/Yoahoug/dsh-launcher/releases) 下载:

- **macOS(Apple Silicon)**:`dsh-launcher_<版本>_aarch64.dmg` → 打开后把 `dsh-launcher.app` 拖入「应用程序」
- **Windows 10/11 x64**:`dsh-launcher_<版本>_x64-setup.exe`(currentUser 安装,默认无需管理员;Windows 10/11 自带 WebView2 运行时,系统确实缺失时才联网补齐)

> 需要系统已安装 Node.js(`^22.19 || >=24`,dsh 开发本来就有的环境);没有时可在应用内「环境 → 安装托管 Node 24 LTS」一键安装。未配置开发者签名时系统会提示未知开发者:macOS 右键 → 打开,Windows 点「更多信息 → 仍要运行」。

**Windows 绿色版(免安装,自测用)**:`dsh-launcher_<版本>_win-x64-portable.zip` 解压后双击
`dsh-launcher.exe` 即可运行(`WebView2Loader.dll` 必须与 exe 同目录;Windows 10/11 自带 WebView2
运行时,无需额外安装)。绿色版为本地/CI 构建的测试产物,不内置自动更新,正式使用请装 NSIS 安装包。

### 方式二:源码运行(开发者,适合改启动器本身)

```sh
git clone https://github.com/Yoahoug/dsh-launcher.git
cd dsh-launcher
pnpm install
pnpm dev:desktop   # tauri dev:起 Rust 原生核心 + React 渲染器
```

需要本机具备 Rust 工具链(rustup)与 Node `^22.19 || >=24`。其他常用命令:`pnpm test:ui`(前端测试)、`pnpm typecheck`、`pnpm build:desktop`(打安装包)。
在 macOS 上交叉编译 Windows 包:`rustup target add x86_64-pc-windows-gnu`(需 Homebrew `mingw-w64`)后
`pnpm tauri build --target x86_64-pc-windows-gnu --bundles nsis`。

## 🔄 自动更新

打包安装的版本**内置自动更新**:启动时自动检查(可在设置中关闭),或点「设置 → 更新 → 立即检查更新」,查询 GitHub Releases:

- 新版本经 **minisign 签名校验**后下载安装,完成后启动器自动重启;
- 正在运行的 dsh web 服务不受影响——新实例启动后直接**召回**接管;
- 源码运行时请用「更新并构建」拉取 dsh 代码,不走应用自更新。

## 🗂️ 结构

```
src-tauri/               Tauri 2 原生核心(纯 Rust,无 Node daemon)
  src/lib.rs             应用入口:插件、命令注册、启动 / 召回 / 迁移流程
  src/commands.rs        IPC 命令(run_action / get_logs / check_for_update / …)
  src/contract.rs        前后端共享契约(状态机 / 长任务 / 事件)
  src/clone.rs           克隆与事务性安装(自动建目录 / 进度 / staging 原子提交)
  src/lifecycle.rs       退出语义(托盘退出=停 dsh;关窗=最小化托盘保活)
  src/services/          进程托管(supervisor)、运行时解析(runtime)、git 同步(repo)、构建(build)、dsh CLI(dshctl)、插件组合视图与补丁读写(plugins)、技能扫描与 CRUD(skills)
  src/ops.rs             长任务编排(journal / 取消 / 崩溃恢复)
  src/toolchain.rs       托管工具链(签名 catalog + 国内镜像)
  src/state.rs           状态机与动作协调(环境缓存 / 启动自动构建 / 快照)
  src/dsh_view.rs        主窗口内 DeepSeek 子 WebView(零权限、状态机、重连与布局)
  src/chat.rs            旧独立 WebView 回退路径(普通 UI/托盘不再调用)
  src/tray.rs            托盘(动态状态菜单 + 召回)
  src/log_hub.rs         日志中心(落盘 + 事件广播)
src-ui/                  React + TypeScript + Vite 控制台
  src/App.tsx            页面路由 + 动作分发
  src/components/        dashboard / repo / env / plugins / skills / logs / settings / first-run
.github/workflows/       ci.yml + release.yml(v* tag → win+mac 资产 + 签名 latest.json)
scripts/                 构建 / 校验脚本
assets/                  应用图标
```

## 📄 License

[MIT](LICENSE)
