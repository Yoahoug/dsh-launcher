<div align="center">

# ⚡ dsh-launcher

**为 [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)(dsh)开发者打造的桌面启动器**

Tauri 2 原生核心(纯 Rust,无 Node daemon)+ React 控制台 · 一键启动 / 构建 / 更新 dsh web · 后台常驻与托盘

</div>

---

## ✨ 功能

| 操作 | 说明 |
|---|---|
| **首次运行向导** | 检测不到有效仓库时进入全屏引导:填写仓库路径或一键克隆 `deepseek-harness` |
| **启动** | 在仓库内源码启动 `pnpm dsh web --port <port> [--host <host>]`;就绪行(`dsh web: http://…`)命中后自动打开主界面 |
| **开发模式** | 同跑 `dsh web` + `pnpm run dev:web`(HMR);前端改动免刷新热更 |
| **更新并构建** | `git pull --rebase --autostash`(冲突**只报告、绝不 reset --hard**)→ lockfile 变化才 `pnpm install` → 构建 → 重启服务 |
| **重建并重启 / 停止** | 进程组停止(SIGTERM → 5s → SIGKILL),零残留;危险动作有确认弹窗 |
| **托管工具链** | 签名 catalog(全部国内镜像)一键安装 Node 24 LTS / Git / pnpm 到托管目录;自动解析 dsh 兼容 Node(`^22.19 \|\| >=24`) |
| **环境检查** | 仓库可用性、前端 dist 是否已构建、Node 版本是否在 dsh 范围内,逐项给出可执行诊断 |
| **内嵌 dsh 窗口** | 「打开 dsh」在独立 WebView 窗口打开 `http://127.0.0.1:3080`(零权限、健康检查确认是预期 DSH 实例);也可用系统浏览器打开 |
| **后台常驻** | 关窗默认最小化到托盘,服务不受影响;重启启动器后自动**召回**运行中的 dsh web(进程存活 + 命令行 + 端口三重校验) |
| **托盘 / 单实例** | 托盘动态状态菜单 + 左键召回主窗口;重复启动只召回,不重复起 |
| **日志** | 实时推送 + 按来源着色;落盘 `~/.local/state/dsh-launcher/logs/` 可回溯 |
| **自动更新** | Tauri updater(minisign 签名),启动时自动检查或手动检查,下载安装后自动重启 |
| **设置** | 仓库路径、端口、host、`DSH_HOME`、构建参数透传、超时、开机自启、主题(亮色/深色/跟随系统)、关窗行为 |

> 定位铁律:**启动器只是一个启动器**。它不承载任何 dsh 界面——主界面永远是 `http://127.0.0.1:3080/`(dsh web),启动器只负责把它拉起来、托管进程、提供控制与日志。

## 🚀 快速开始

### 方式一:下载安装包(推荐,支持自动更新)

从 [Releases](https://github.com/Yoahoug/dsh-launcher/releases) 下载:

- **macOS(Apple Silicon)**:`dsh-launcher_<版本>_aarch64.dmg` → 打开后把 `dsh-launcher.app` 拖入「应用程序」
- **Windows 10/11 x64**:`dsh-launcher_<版本>_x64-setup.exe`(currentUser 安装,默认无需管理员;首次运行需联网下载 WebView2 运行时)

> 需要系统已安装 Node.js(`^22.19 || >=24`,dsh 开发本来就有的环境);没有时可在应用内「环境 → 安装托管 Node 24 LTS」一键安装。未配置开发者签名时系统会提示未知开发者:macOS 右键 → 打开,Windows 点「更多信息 → 仍要运行」。

### 方式二:源码运行(开发者,适合改启动器本身)

```sh
git clone https://github.com/Yoahoug/dsh-launcher.git
cd dsh-launcher
pnpm install
pnpm dev:desktop   # tauri dev:起 Rust 原生核心 + React 渲染器
```

需要本机具备 Rust 工具链(rustup)与 Node `^22.19 || >=24`。其他常用命令:`pnpm test:ui`(前端测试)、`pnpm typecheck`、`pnpm build:desktop`(打安装包)。

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
  src/services/          进程托管(supervisor)、git 同步(repo)、构建(build)
  src/ops.rs             长任务编排(journal / 取消 / 崩溃恢复)
  src/toolchain.rs       托管工具链(签名 catalog + 国内镜像)
  src/chat.rs            内嵌 dsh WebView(独立窗口,零权限)
  src/tray.rs            托盘(动态状态菜单 + 召回)
  src/log_hub.rs         日志中心(落盘 + 事件广播)
src-ui/                  React + TypeScript + Vite 控制台
  src/App.tsx            页面路由 + 动作分发
  src/components/        dashboard / repo / env / logs / settings / first-run
.github/workflows/       ci.yml + release.yml(v* tag → win+mac 资产 + 签名 latest.json)
scripts/                 构建 / 校验脚本
assets/                  应用图标
```

## 📄 License

[MIT](LICENSE)
