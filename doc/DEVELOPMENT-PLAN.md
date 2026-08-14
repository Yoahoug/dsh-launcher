# dsh-launcher 开发方案

> 为 DeepSeek Harness(dsh)开发者日常工作定制的**纯启动器**:源码启动、一键更新构建、热重载开发模式、后台常驻、亮色控制台。
> **定位铁律:启动器只是一个启动器。它不承载任何 dsh 界面;主界面永远是 `http://127.0.0.1:3080/`(dsh web),启动器只负责把它拉起来、托管进程、提供控制与日志。**
> 文档版本:v0.2(按「纯启动器」定位修订)· 状态:方案评审中 · 项目根:`~/Desktop/dsh-launcher`

---

## 1. 项目概述

### 1.1 背景

DeepSeek Harness 官方**没有**任何启动器形态:

- 仓库内没有 `.command` / `.bat` / `.desktop` / `Makefile` / `start` 脚本,也没有 `dsh desktop` 命令(已核实);
- 官方"从源码运行"的流程就是 4 条手工命令(见官方 [README](https://github.com/deepseek-ai/deepseek-harness)):

  ```sh
  git clone https://github.com/deepseek-ai/deepseek-harness.git
  cd deepseek-harness
  pnpm install
  pnpm run build
  pnpm dsh web
  ```

- 官方社区已有人提出桌面壳需求,至今未实现:[Discussion #510 — Desktop GUI client](https://github.com/deepseek-ai/deepseek-harness/discussions/510)。

本项目把上述 4 条命令 + `git pull` + 热重载,包成一个双击即用的亮色控制台。**它不做 #510 那种桌面壳**——用户(dsh 开发者)后续始终通过浏览器访问 `http://127.0.0.1:3080/` 使用 dsh。

### 1.2 职责边界

| 谁 | 干什么 | 不干什么 |
|---|---|---|
| **dsh web**(`127.0.0.1:3080`) | 主界面:会话、Agent、工具、设置 | 不管更新、构建、进程 |
| **dsh-launcher**(控制台) | 拉代码、装依赖、构建、拉起 dsh web、托管进程、日志、停止/重启 | **不承载 dsh 界面,不做桌面壳,不代理 3080** |
| 浏览器 | 打开 3080 用 dsh;打开 launcher 控制台操作 | — |

### 1.3 目标

| # | 目标 |
|---|---|
| G1 | 双击启动,亮色控制台,好看好用 |
| G2 | 一键:同步远端 → 装依赖 → 构建 → 源码启动 `dsh web`,并自动打开 `http://127.0.0.1:3080/` |
| G3 | 开发模式:同跑 `dsh web`(源码)+ `pnpm run dev:web`(HMR watcher),改代码免重启热更 |
| G4 | 后台常驻:关掉控制台/浏览器不影响服务;可召回控制台看日志、停止、重启 |
| G5 | 不侵入 dsh 源码,不做 dsh 插件,不打包发布 |

### 1.4 明确不做

- 不重新实现或包装 dsh 的 Web UI;
- 不做成面向普通用户的安装包 / 原生桌面壳(那是 #510,记入演进路径 §3.4);
- 不做插件管理(`dsh plugin` 已覆盖);
- 不做多仓库/多实例管理(单机单 checkout 是唯一用例)。

---

## 2. 需求分析

### 2.1 功能需求

| 编号 | 需求 | 说明 |
|---|---|---|
| F1 | 仓库定位 | 配置 dsh checkout 路径(默认 `~/Desktop/deepseek-harness`),启动时校验目录存在且是 git 仓库 |
| F2 | 同步远端 | `git fetch` + `git pull --rebase --autostash`;本地有未提交改动时先提示,默认自动 stash;冲突时**只报告不破坏**(绝不 `reset --hard`) |
| F3 | 依赖安装 | 比较 `pnpm-lock.yaml` 是否变化,变了才跑 `pnpm install`;失败给出可读错误 |
| F4 | 构建 | `pnpm run build`(= `build:lib` + `build:web`),流式输出进度,失败定位到阶段 |
| F5 | 源码启动 | `spawn pnpm dsh web`(等价 `node --import tsx/esm apps/cli/src/bin.ts web`),**不用 npx、不装发布包**,保证本地改动的源码生效 |
| F6 | 就绪检测 | 逐行扫描子进程 stdout,匹配 `dsh web: http://…` 就绪行后**自动打开 `http://127.0.0.1:3080/`**(与仓库测试 `apps/web/tests/support.ts` 的 `waitForOutput` 同款正则) |
| F7 | 开发模式 | 额外 `spawn pnpm run dev:web`(Vite watcher,见 `scripts/dev-web.ts`);dsh web 的 HMR receiver 常驻,二者同跑实现免刷新热更 |
| F8 | 停止 / 重启 | 按进程树终止(`dsh web` / `dev:web` / 构建子进程),不残留僵尸 |
| F9 | 单实例 | pid 文件 + 端口探测,重复双击只召回已有实例 |
| F10 | 日志 | 文件持久化 + 控制台内实时 tail(环形缓冲 + SSE);按来源(dsh web / dev:web / git / pnpm / launcher)着色 |
| F11 | 设置 | 仓库路径、端口(默认 3080)、host(默认 `127.0.0.1`)、`DSH_HOME`、开机自启、构建参数透传 |

> 说明:F6 里"自动打开 3080"是本项目与"只是启动器"定位的关键衔接——启动器把用户送到主界面后即退居后台。

### 2.2 非功能需求

| 编号 | 需求 |
|---|---|
| N1 | **零新增工具链**:只依赖 dsh 开发本来就有的 Node(`^22.19 || >=24`)与 pnpm(11.7.0);启动器自身**零 npm 运行时依赖**(仅 Node 内置模块) |
| N2 | 亮色控制台,现代、清爽、紧凑(见 §6) |
| N3 | 构建完成后 ≤3s 内给出 URL 并打开浏览器 |
| N4 | 容错:git 冲突、端口占用、构建失败、网络断连,全部有明确反馈,不闪退 |
| N5 | 所有子进程日志可回溯(落盘),崩溃可诊断 |

### 2.3 关键用户场景

**场景 A · 日常开发(热重载)**:双击启动器 → 点「开发模式」→ 自动拉起 `dsh web`(源码)+ `dev:web`(HMR watcher)→ 自动打开 `http://127.0.0.1:3080/` → 我改 `packages/client/**` 或前端 → **免刷新热更**;改 `lib/` 产物 → 回控制台点「重建并重启」。

**场景 B · 跟上远端**:回控制台点「更新并构建」→ git pull(自动 stash)→ lockfile 变了则 install → 构建 → 重启服务 → 打开 3080。

**场景 C · 后台常驻**:关掉浏览器和控制台,服务照跑;再双击启动器 → 召回控制台,显示「运行中」,可看日志、可停止。

---

## 3. 技术选型

### 3.1 候选对比

| 方案 | 工具链成本 | 包体积 | UI 自由度 | 后台化 | 与 dsh 同构度 | 结论 |
|---|---|---|---|---|---|---|
| 纯脚本(`.command`/`.bat`) | 极低 | 0 | 无 GUI ✗ | 一般 | 高 | 不满足「好看的 UI」 |
| **Node 本地服务 + 单页控制台** | **零额外**(Node 已有) | ~0(纯内置模块) | **极高**(纯 HTML/CSS) | **天然**(服务常驻) | **高**(同为本地 HTTP 服务) | ✅ **推荐** |
| Electron | 低(纯 JS) | ~100MB+ | 高 | 好 | 中 | 重,杀鸡用牛刀 |
| Tauri | 高(需 Rust 工具链) | ~5MB | 高 | 好 | 中 | 本机无 Rust,留作演进 |

### 3.2 推荐:Node 本地服务 + 单页亮色控制台

理由:

1. **零新增依赖**:只用 Node 内置模块(`node:http`、`node:child_process`、`node:fs`、`node:readline`),连 `npm install` 都不需要;Node 与 pnpm 是 dsh 开发硬前置,本机必然已有。
2. **与 `pnpm dsh web` 同构**:启动器自己是一个极小的本地 HTTP 服务(默认 `http://127.0.0.1:3090/`),双击 `.command` → 起服务 → 自动开浏览器 → 亮色控制台。原理和代码形态与 `dsh web` 一致,正合「原理和代码 pnpm dsh web 启动一样」。
3. **UI 自由度最高**:纯 HTML/CSS/JS,亮色设计系统随便做,不需要 Rust 或前端框架。
4. **后台天然**:控制台只是客户端,launcher 服务进程常驻;关标签不影响 `dsh web`。
5. **可平滑演进**:同一份控制台前端,以后可塞进 Tauri WebView 变成原生小窗(对应 #510),UI 代码复用。

### 3.3 启动器自身技术栈

- 运行时:Node `^22.19 || >=24`(与 dsh engines 对齐),ESM;
- 服务:`node:http` 静态文件 + JSON API + SSE(`text/event-stream`)推送状态与日志;
- 子进程管理:`node:child_process` spawn,`detached: true` + `unref()`,日志重定向到文件;
- 就绪检测:`node:readline` 逐行解析,正则 `/dsh web: (http:\/\/[^\s]+)/`;
- 前端:原生 HTML/CSS/JS,单页,无框架;
- 外部命令:`git`、`pnpm` 通过 PATH 调用(启动时校验版本)。

### 3.4 演进路径(记入文档,不在本期做)

- **M+1 原生小窗**:`cargo tauri init` 包住本控制台前端 → 原生窗口 + 托盘 + 自启 + 通知(即 #510 的轻量形态);控制台仍不承载 dsh 界面,主界面仍走浏览器 3080;
- **M+2 发布版**:仅当有外部分发需求时再做安装包。

---

## 4. 总体架构

### 4.1 组件图

```
┌────────────────────────────────────────────────────────────┐
│  浏览器                                                      │
│  ├─ 主界面: http://127.0.0.1:3080/   (dsh web,用户日常使用)   │
│  └─ 控制台: http://127.0.0.1:3090/   (亮色单页,仅启动/停止/   │
│      更新构建/开发模式/日志/设置;开完就走,可随时召回)           │
└──────────────┬─────────────────────────────┬───────────────┘
               │ JSON API + SSE              │ (自动打开)
┌──────────────▼─────────────────────────────▼───────────────┐
│  dsh-launcher Server (Node, 零 npm 依赖, 常驻)               │
│  ┌─────────────────────────────────────────────────────┐   │
│  │ ProcessManager ── 状态机(空闲→同步→安装→构建→启动→运行) │   │
│  │   ├─ spawn/进程树终止 / 单实例锁(pid+端口)             │   │
│  │   ├─ ReadinessDetector  就绪行正则扫描                │   │
│  │   └─ LogHub  环形缓冲 + 文件落盘 + SSE 广播            │   │
│  │ RepoManager   git fetch / pull --rebase --autostash   │   │
│  │ BuildManager  lockfile 比对 / pnpm install / pnpm build│   │
│  └─────────────────────────────────────────────────────┘   │
└──────────────┬─────────────────────────────┬───────────────┘
               │ child_process               │ child_process
┌──────────────▼──────────────┐  ┌───────────▼───────────────┐
│ deepseek-harness checkout   │  │ (仅开发模式)               │
│ pnpm dsh web                │  │ pnpm run dev:web          │
│ (node --import tsx/esm ...) │  │ (Vite HMR watcher)        │
│ → 打印 dsh web: http://…    │  │ → rebuilt 帧广播           │
└─────────────────────────────┘  └───────────────────────────┘
```

### 4.2 与 dsh 的关系

launcher 是 dsh 的**外挂进程管家**:只做 git / pnpm / 进程 / 日志,不改 dsh 任何源码,不进 dsh 插件树,不代理、不包装 3080。dsh 侧唯一的事实依据是就绪行 `dsh web: http://…`——这正是 dsh 设计给 supervisor 的接口(`packages/bundle/web-app/src/index.ts` 注释明确它是 readiness signal)。

### 4.3 目标目录结构

```
dsh-launcher/
├── README.md
├── LICENSE                     # MIT
├── .gitignore
├── package.json                # type: module, engines: node >=22.19, 无依赖
├── bin/
│   ├── start.command           # macOS 双击入口(chmod +x)
│   └── start.bat               # Windows 双击入口(预留)
├── src/
│   ├── server.mjs              # HTTP 服务 + SSE + 静态文件
│   ├── state.mjs               # 状态机
│   ├── process.mjs             # ProcessManager(单实例/进程树/就绪检测)
│   ├── repo.mjs                # RepoManager(git)
│   ├── build.mjs               # BuildManager(pnpm)
│   ├── log.mjs                 # LogHub(环形缓冲+落盘+SSE)
│   └── config.mjs              # 设置读写(~/.config/dsh-launcher.json)
├── public/                     # 单页控制台(纯静态,亮色)
│   ├── index.html
│   ├── app.css                 # 设计系统 token
│   └── app.js
├── scripts/
│   └── install-launch-agent.sh # macOS LaunchAgent 自启安装(可选)
└── doc/
    ├── DEVELOPMENT-PLAN.md     # 本文档
    └── ui/mockup.html          # 亮色单页控制台原型(浏览器直接打开预览)
```

---

## 5. 核心流程设计

### 5.1 状态机

```
idle ──更新并构建──▶ syncing ──▶ installing(可选) ──▶ building ──▶ starting ──▶ running
  ▲                     │             │                 │             │
  └──── stopped ◀───────┴───── failed ◀─────────────────┴─────────────┘
       (git 冲突/构建失败/启动超时 均给出诊断,回 idle 或 failed)
开发模式:同树,starting 阶段额外拉起 dev:web,running 态标记「HMR 活跃」
```

控制台五态视觉:空闲(灰)· 同步/安装/构建(蓝,进度条)· 启动(脉冲)· 运行(绿)· 失败(红,错误摘要)。运行态下用户可关闭浏览器与控制台,服务不退出。

### 5.2 同步远端(F2)

1. `git fetch origin`(失败→网络诊断);
2. `git status --porcelain` 检测 dirty;有改动则默认 `git stash push -u`(控制台告知),冲突性改动可中止;
3. `git pull --rebase --autostash`;rebase 冲突→中止并列出冲突文件,不破坏工作区;
4. 记录「落后 N 个提交 / 已更新到 <sha>」供控制台展示。

### 5.3 构建(F4)

1. 比对 `pnpm-lock.yaml` 的 git 变化 → 需要则 `pnpm install`;
2. `pnpm run build`(内部为 `build:lib:host` → `build:lib:client` → `build:web`),逐阶段上报进度;
3. 失败:捕获退出码与尾部 stderr,定位到阶段(tsc 类型错 / tsdown 打包错 / vite 构建错);
4. 构建是**最长耗时环节**(首次 5–10 分钟):后台执行、日志可见、可取消。

### 5.4 源码启动与就绪检测(F5/F6)

```text
spawn: pnpm dsh web [--port <port>] [--host 127.0.0.1]
  │
  ├─ readline 逐行: /dsh web: (http:\/\/[^\s]+)/ → 命中 → open http://127.0.0.1:3080/ → state=running
  ├─ 超时(默认 120s) → 转储尾部日志 → state=failed
  └─ 进程退出码 ≠ 0 → 显示退出码 + 尾部日志
```

注意:dsh 的 CLI 目前**拒绝 `--host 0.0.0.0`**(见 `packages/bundle/web-app` 的 `web-startup` provider),故默认 `127.0.0.1` 单机访问;LAN 访问留作设置项并给出提示。

### 5.5 开发模式与热重载(F7)

- 同时 `spawn pnpm run dev:web`(`scripts/dev-web.ts`,Vite watcher,负责把 `rebuilt` 帧广播给 dsh web);
- dsh web 的 HMR receiver 总是挂载(**「update contract」**,见 `packages/bundle/web-app` 注释):**客户端插件 / 前端改动免刷新热更**;
- 服务端与 `lib/` 产物改动需重编译:**「重建并重启」按钮** = 停进程 → `pnpm run build` → 再启动;
- 控制台明确提示两类改动的热更边界,避免误以为全部免重启。

### 5.6 后台化(G4)

- 默认:子进程 `detached: true` + `unref()`,stdout/stderr 重定向到 `~/.local/state/dsh-launcher/logs/`(按日期轮转);launcher 服务本身常驻,浏览器标签关闭无影响;
- 可选开机自启:`scripts/install-launch-agent.sh` 安装 LaunchAgent(开机起 launcher 服务,不开浏览器);
- Windows 预留:任务计划程序 / `start /b` 等价物(本期不实现)。

### 5.7 停止 / 重启 / 单实例(F8/F9)

- 停止:对进程组发 SIGTERM → 宽限 5s → SIGKILL,顺带清理 pid 文件;
- 重启:stop + 依当前模式重新 start;
- 单实例:pid 文件 + 探测 launcher 端口;重复双击 → 直接召回控制台页面。

---

## 6. UI 设计(亮色单页控制台)

### 6.1 设计语言

| Token | 值 | 用途 |
|---|---|---|
| `--bg` | `#F6F8FB` | 页面背景 |
| `--surface` | `#FFFFFF` | 卡片/面板 |
| `--primary` | `#4D6BFE` | 主操作、链接、品牌(DeepSeek 蓝系) |
| `--success` | `#16A34A` | 运行中、成功 |
| `--warning` | `#D97706` | 构建中、注意 |
| `--danger` | `#DC2626` | 失败、停止 |
| `--text-1/2/3` | `#0F172A / #475569 / #94A3B8` | 文字层级 |
| `--radius` | `12px` | 卡片圆角 |
| `--shadow` | `0 1px 3px rgba(15,23,42,.08)` | 柔和阴影 |

- 字体:系统栈 `-apple-system, "PingFang SC", "Segoe UI", sans-serif`;日志区等宽 `ui-monospace, SFMono-Regular, Menlo`。
- 风格:白底、细分割线、柔和阴影、圆角卡片、渐变主按钮、状态色圆点(运行中带脉冲动画);图标用内联 SVG,零外部依赖。

### 6.2 单页布局(紧凑,一屏装下)

```
┌────────────────────────────────────────────────────────────┐
│ ◈ dsh-launcher           ● 运行中 · 开发模式   [打开 3080]   │  ← 顶栏
├────────────────────────────────────────────────────────────┤
│  ┌──────────────┐ ┌──────────────┐ ┌─────────────────────┐  │
│  │ ▶ 启动        │ │ ⚡ 开发模式    │ │ ↻ 更新并构建          │  │  ← 三个主操作
│  └──────────────┘ └──────────────┘ └─────────────────────┘  │
│  [■ 停止]  [↻ 重建并重启]                                    │  ← 次级操作
├────────────────────────────────────────────────────────────┤
│  服务:http://127.0.0.1:3080/ · PID 81234 · 运行 42m         │  ← 状态条
│  仓库:main @ a1b2c3d · 落后 0 · 工作区干净                    │
├────────────────────────────────────────────────────────────┤
│  日志 ▸ dsh web · dev:web · git · pnpm · launcher  [⏸] [∅] │  ← 可过滤
│  [12:01:03] dsh web: http://127.0.0.1:3080/  (launcher)     │
│  [12:01:03] Server listening on 3080          (dsh web)     │
│  …                                                        │
└────────────────────────────────────────────────────────────┘
```

- **顶栏**:品牌 + 全局状态(颜色圆点 + 模式徽标)+ 「打开 3080」快捷按钮;
- **主操作区**:启动 / 开发模式 / 更新并构建(三大按钮,构建中显示进度条);停止 / 重建并重启(次级);
- **状态条**:服务与仓库两个紧凑信息行(URL、PID、分支、HEAD、落后数、工作区状态);
- **日志区**:等宽、按来源着色、可过滤来源、可暂停自动滚动、一键清屏;日志**只在控制台里看**,不作为主界面;
- **设置**:折叠面板,仓库路径、端口、host、DSH_HOME、开机自启;
- 页脚一句提醒:**「主界面:http://127.0.0.1:3080/」**,防止误把控制台当 dsh 用。

### 6.3 原型

`doc/ui/mockup.html` 为静态高保真原型(亮色单页),浏览器直接打开即可预览最终观感与布局,开发时以其为视觉基准。原型中 3080 与 3090 的职责分工按 §1.2 呈现。

---

## 7. 里程碑与任务拆分

| 里程碑 | 内容 | 交付物 |
|---|---|---|
| **M0 脚手架** | git init、package.json、HTTP 静态服务 + 亮色空壳控制台、状态机骨架 | 双击打开亮色控制台 |
| **M1 最小闭环** | 源码启动 dsh web、就绪检测、**自动打开 3080**、日志落盘+SSE | 「双击→控制台→3080 出界面」全通 |
| **M2 更新构建** | git 同步(F2)、依赖比对(F3)、构建(F4)、失败诊断 | 「更新并构建」一键 |
| **M3 开发模式** | dev:web 同跑、HMR 热更提示、重建并重启 | 场景 A 全通 |
| **M4 后台与自启** | detached 常驻、LaunchAgent 脚本、单实例强化 | 场景 C 全通 |
| **M5 打磨** | 状态机视觉完善、设置完整、亮色细节、错误诊断文案 | 可日常使用 |

每阶段结束跑一次 §8 验收,通过再进下一阶段。

---

## 8. 验收标准(对照需求)

| 验收项 | 对应需求 | 判定 |
|---|---|---|
| 双击 `start.command` 出现亮色控制台 | G1 | 浏览器自动打开 launcher 页,加载 < 2s |
| 点「启动」后 ≤3s 出现就绪 URL 并**自动打开 3080** | F5/F6/N3 | 就绪行命中、`http://127.0.0.1:3080/` 新标签打开 |
| 源码改动生效 | F5 | 改 `apps/cli/src/args.ts` 的 help 文本 → 启动后可见,证明非 npx 发布包 |
| 开发模式下改客户端插件免刷新 | F7 | 改 `packages/client/**` → 无刷新看到变化 |
| 改 lib 后点「重建并重启」生效 | F7 | 停→build→启动,新产物生效 |
| 远端有新提交时「更新并构建」完成 pull+install+build | F2/F3/F4 | 仓库 HEAD 前进、服务重启、日志完整 |
| 本地有未提交改动时同步不丢代码 | F2 | stash 后恢复,无数据丢失 |
| git 冲突时只报告不破坏 | F2/N4 | 冲突文件列表展示,工作区可恢复 |
| 关浏览器后服务继续跑 | G4 | 重新打开控制台显示「运行中」 |
| 二次双击不重复起服务 | F9 | 聚焦已有实例,端口唯一 |
| 构建失败有明确诊断 | F4/N4 | 定位到阶段 + 尾部日志,不闪退 |
| 端口 3080 被占用时给出可读提示 | F11/N4 | 提示换端口并可一键重试 |
| 控制台不承载 dsh 界面、主界面始终为 3080 | §1.2 | 控制台页面无任何 dsh 会话/Agent 功能 |

---

## 9. 风险与对策

| 风险 | 影响 | 对策 |
|---|---|---|
| 构建耗时长(首次 5–10 分钟) | 等待焦虑、误判卡死 | 阶段化进度条、后台执行、可取消、增量构建提示 |
| git rebase 冲突 | 丢改动/工作区损坏 | 只报告不自动解决;先 stash 再 pull;绝不 `reset --hard` |
| 端口占用(3080 / launcher 3090) | 启动失败 | 启动前探测,冲突提示并给出一键换端口 |
| dev:web 与 dsh web 版本/HMR 契约不匹配 | 热更失效 | 以构建产物为准;版本不一致时提示重启而非热更 |
| 进程残留(僵尸 dsh web) | 端口占用、状态错乱 | 进程组终止 + 启动前清理 + pid 文件校验 |
| 网络抖动导致 git/pnpm 失败 | 流程中断 | 明确失败态 + 重试按钮,不自动重试破坏性步骤 |
| Node/pnpm 版本不符 | 构建/启动报错 | 启动时校验 engines,给出安装指引 |
| 误把控制台当主界面 | 使用混乱 | 控制台只做启动/停止/日志;页脚常驻「主界面:3080」提示;自动把用户送到 3080 |

---

## 10. 参考与依据

- [deepseek-harness 官方 README(Run from source)](https://github.com/deepseek-ai/deepseek-harness):`pnpm install && pnpm run build && pnpm dsh web`;
- 本仓库代码事实:`apps/cli/src/bin.ts`、`apps/cli/src/args.ts`(`dsh web` 是 `--profile web` 别名)、`packages/bundle/web-app/src/index.ts`(就绪行 readiness signal、前端 dist 前置要求)、`scripts/dev-web.ts`(HMR watcher)、`apps/web/tests/support.ts`(就绪行正则先例);
- [Discussion #510 — Desktop GUI client](https://github.com/deepseek-ai/deepseek-harness/discussions/510):演进路径参考;
- [SillyTavern 更新方式](https://sillytavern.wiki/installation/updating):`UpdateAndStart.bat` 双击更新+启动先例;
- [macOS launchd 后台任务教程](https://levelup.gitconnected.com/save-time-by-automating-your-git-pulls-498120870582):LaunchAgent 自启依据;
- [腾讯云《打造通用应用启动器》](https://cloud.tencent.com/developer/article/2654039):「起后端 → 健康检查 → 加载前端」同构模式参考。
