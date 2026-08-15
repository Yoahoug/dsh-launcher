# DSH Launcher 下一阶段开发调研

> 调研日期：2026-08-15
> 项目路径：`/Users/yoahoug/Desktop/dsh-launcher`
> 调研范围：内置安装系统、国内镜像、仓库克隆、提权、DeepSeek Harness 内嵌窗口、窗口拖动、图标、启动与运行性能、任务阻塞与防重复操作
> 本次交付性质：只做调研和方案设计，没有修改任何业务代码、前端代码、Tauri 配置或构建配置。

## 1. 结论先行

建议把下一阶段定义为 **Windows 优先的一体化交付能力**，而不是继续在现有“检查环境 + 调命令”的基础上零散增加按钮。

核心方案由三部分组成：

1. **用户级托管工具链**：Node.js、Git、pnpm 均安装到应用自己的数据目录，默认不改系统 PATH、不写全局配置、不要求管理员权限。国内镜像只作为下载通道，版本和 SHA-256 由随 Launcher 发布的可信清单固定。
2. **单一任务协调器**：检查、下载、校验、解压、克隆、依赖安装、构建、启动都进入同一套有任务 ID、有进度、有终态、有取消边界的状态机。界面按动作矩阵禁用冲突操作，但日志、最小化、安全取消等功能保持可用。
3. **独立的 DSH WebView 窗口**：Launcher 主窗口继续承担可信控制面；DeepSeek Harness 的本地 Web UI 通过单独的 Tauri WebView 窗口访问 `http://127.0.0.1:<port>`。该窗口不授予 Tauri IPC 权限，避免网页获得本地应用能力。

这条路线能同时解决当前的几个根问题：安装不完整、国内网络不稳定、权限范围过大、操作状态误判、网页只能外部打开、窗口拖不动，以及启动阶段工作过多。

## 2. 本次调研边界与术语

### 2.1 “DeepSeek 对话窗口”的解释

根据当前项目调用方式和本地 `deepseek-harness` 仓库，本方案把“DeepSeek 对话窗口”解释为 **DeepSeek Harness 自带、运行在本机 3080 端口的 Web UI**，不是直接嵌入官方 `chat.deepseek.com`。

这是兼容性最好、风险最低的主路径：DSH 的 HTTP 与 WebSocket 都保持同源，本地服务的浏览器信任检查也无需绕过。

如果产品目标其实是官方 DeepSeek 网页，应另开技术验证，不应与本期主方案混做。官方站点的登录、风控、WAF、跨域和页面结构可能变化，而且远程页面绝不能获得 Launcher 的 Tauri 权限。

### 2.2 Windows 优先

当前仓库可以在 macOS 开发和运行，但用户目标包含 EXE、UAC 和 WebView2，因此下一阶段应以 Windows 10/11 x64 为首个完整验收平台；macOS 保持现有能力并在后续补齐托管 Git 等平台差异。

## 3. 已验证的项目现状

### 3.1 技术与交付基线

- 桌面壳为 Tauri 2，核心为 Rust，界面为 React 18、TypeScript、Vite 和 Tailwind。
- 当前版本号为 `0.3.1`。
- 当前 Tauri bundle 只配置了 `app` 和 `dmg`，Release 工作流也只构建 macOS；还没有 Windows NSIS/EXE 正式交付链路。
- 已启用 single-instance、window-state、dialog、opener、log、autostart 和 updater 等插件。
- 主窗口配置为可缩放，使用 Overlay 标题栏和隐藏标题。

### 3.2 已有能力可以保留

- 已有 Node 24 托管安装雏形：下载、SHA-256 校验、解压、进度上报。
- 已有 Node、Git、pnpm、仓库与构建产物的环境快照。
- 已有 Git fetch、stash、pull/rebase/autostash 和冲突报告，且没有使用破坏性的 `reset --hard`。
- 已有安装、构建、启动、停止等运行状态，以及防止流程重复进入的原子标记。
- 启动时的仓库刷新已经放到后台线程，没有直接阻塞首帧。

### 3.3 当前关键缺口

| 领域 | 仓库现状 | 影响 |
|---|---|---|
| 工具链 | 只有 Node 托管安装；Git 主要依赖 PATH；pnpm 没有完整托管安装 | 干净 Windows 机器无法真正一键完成 |
| 国内源 | Node 仍从 `nodejs.org` 获取；Git 与 clone 没有稳定镜像策略 | 国内网络下安装成功率不可控 |
| 操作语义 | 后端接受任务后立即返回 `ok`，部分 UI 把“已接受”显示成“已安装成功” | 会出现假成功和状态错乱 |
| 取消 | 当前 epoch 能阻止后续阶段继续，但正在运行的下载、Git、pnpm 子进程未必立即终止 | 用户点击停止后仍可能长时间运行 |
| 交互锁 | busy 时主要按钮整体禁用，连安全取消也可能不可达 | 卡住时用户无法自救 |
| 内嵌网页 | `open_dsh` 只用系统浏览器打开本地 URL | 无法实现软件内对话体验 |
| 窗口拖动 | 前端虽标记 drag region，但能力清单没有 `allow-start-dragging` 和 resize drag 权限 | 自定义标题栏拖动无法可靠生效 |
| Windows 发布 | 未配置 NSIS、WebView2 安装模式和 Windows updater 产物 | 还不能交付目标 EXE |

进一步的代码级只读检查还发现三项实施前必须处理的问题：

- Windows 的 Node ZIP 解压路径目前仍返回占位错误，因此现有“一键安装 Node”不能在目标 Windows 环境完成。
- Node 安装结束后没有立即刷新当前进程采用的托管 Node 路径，界面可能显示完成但必须重启后才真正生效。
- Git 与 pnpm 子进程按顺序读取 stdout、stderr，输出量大时存在一侧管道写满而互相等待的风险；实施时必须并发消费两路输出。

## 4. 推荐总体架构

建议将桌面端划分成四个边界清晰的组件：

| 组件 | 职责 | 权限原则 |
|---|---|---|
| Launcher 主窗口 | 展示环境、仓库、任务、日志、设置 | 只保留所需 Tauri 权限 |
| Operation Coordinator | 排队、互斥、进度、取消、恢复、终态 | 一个前台变更任务，禁止并发破坏 |
| Managed Toolchain | Node、MinGit、pnpm、缓存和版本清单 | 只写应用数据目录，不改全局环境 |
| DSH WebView 窗口 | 显示本地 DSH Web UI | 不授予 Tauri IPC；限制导航和新窗口 |

安装依赖关系建议为：

`系统与 WebView2 检查 → Node / Git / pnpm 可并行准备 → clone → pnpm install → build → start → 打开内嵌 DSH`

这里的“阻塞”是业务状态上的互斥，不是阻塞 UI 线程。安装或构建时仍应能滚动日志、最小化窗口、复制错误信息，以及在允许取消的阶段发起取消。

## 5. 内置安装系统方案

### 5.1 托管工具链优先，不做全局安装

建议默认把所有工具放在应用数据目录，例如逻辑结构为：

- `toolchains/node/<version>`
- `toolchains/git/<version>`
- `toolchains/pnpm/<version>`
- `downloads`、`cache`、`operations` 和 `logs`

启动子进程时由 Launcher 显式组装 PATH 和镜像环境变量。不要修改用户的系统 PATH、全局 npm 配置、Git 全局配置，也不要自动写 PowerShell profile。

这样有四个直接收益：

- 绝大多数安装不需要 UAC。
- 版本可复现，不会被用户机器上的旧环境污染。
- 卸载与修复范围明确。
- 可以并存多个版本并安全回滚。

### 5.2 固定兼容版本

DeepSeek Harness 当前声明：

- Node：`^22.19 || >=24`
- 包管理器：`pnpm@11.7.0`

建议 Launcher 的一个正式版本固定一组经过验证的工具链版本，而不是每次启动动态寻找“最新 LTS”。版本升级通过 Launcher 新版本中的受信清单完成。

Windows Git 建议采用 Git for Windows 的 **MinGit**。它是官方面向第三方应用嵌入的最小发行形态，足以承担 clone、fetch 和 pull。正常 DSH 安装不应默认安装 Python、Rust 或 Visual Studio Build Tools；只有依赖确实退回 node-gyp 源码构建时，才给出针对性诊断和可选方案。

### 5.3 国内镜像与供应链完整性

推荐下载策略：

| 资源 | 国内首选 | 回退 | 完整性规则 |
|---|---|---|---|
| Node | npmmirror 的 Node 镜像或自有国内 CDN | 官方 Node 下载站，需用户同意 | 版本与 SHA-256 来自 Launcher 内置签名/受信清单 |
| pnpm 包及项目依赖 | `https://registry.npmmirror.com` | 官方 npm registry，需用户同意 | 锁文件完整性 + 固定 pnpm 版本 |
| MinGit | 自有国内 CDN 缓存的固定官方资产 | Git for Windows 官方发布 | 固定版本、长度和 SHA-256；可增加发布签名验证 |
| DSH 仓库 | 用户选择并保存的镜像 URL | 官方 GitHub 地址，需用户同意 | clone 后验证仓库身份、默认分支和期望文件 |

不要内置来源不明的公共 GitHub 代理，也不要把“从同一个镜像下载压缩包和校验文件”当成安全校验。镜像被篡改时，两者可能同时被替换。

对于官方 DeepSeek Harness，目前没有足够依据把某个国内 Git 托管镜像认定为官方长期镜像。因此产品应允许用户选择并持久化镜像 clone URL，并把官方 GitHub 地址作为明确显示的备选，而不是悄悄切换。

所有镜像配置应只作用于当前 Launcher 子进程；不得写入用户全局 npm、pnpm 或 Git 配置。

需要把“全走国内源”的验收口径写清楚：Launcher 自有制品、npm registry 和 clone 地址可以强制使用国内节点，但任意第三方依赖的 `postinstall` 仍可能把境外 URL 硬编码在脚本中。若要求网络层 100% 不出境，还需要自建制品代理/完整缓存和出站域名 allowlist；仅切换 registry 不能诚实保证这一点。

### 5.4 Clone 交互与安全流程

点击“克隆仓库”后弹出专用窗口，至少包含：

- clone URL：默认填充“上一次成功使用的地址”，首次使用填官方地址或经过产品确认的镜像地址。
- 目标目录：默认应用管理的工作区，也允许用户选择。
- 分支：默认自动发现远端 HEAD；高级选项才允许指定。
- 网络源：国内镜像、官方源或自定义；清楚显示是否允许失败后回退。

推荐流程：

1. 校验 URL 格式并执行只读的远端可达性检查。
2. 检查目标目录；只允许不存在或为空的目录。
3. 克隆到同级临时目录，而不是直接写最终目录。
4. 验证 `.git`、`package.json`、包管理器和 Node engines 等仓库特征。
5. 如果用户只执行 Clone，则验证通过后原子重命名到最终目录；如果执行“一键全套安装”，依赖安装、构建与自检也应在 staging 中完成，最后才提交最终目录。
6. 原子提交成功后再保存 repoPath、resolved commit 和“上次成功 URL”。
7. 失败或取消时只清理由 Launcher 创建的临时目录，绝不删除既有目录。

URL 中如果含 token 或密码，日志、设置和历史记录都必须脱敏；产品最好阻止把凭证直接写进 URL，并引导使用 Git Credential Manager 或 SSH。

不建议默认 shallow clone。Launcher 后续要支持 rebase、更新和冲突诊断，完整历史更稳定；可以使用单分支降低体积，但必须动态识别默认分支，不能硬编码 `main`，因为当前官方仓库默认分支是 `master`。

### 5.5 下载、安装与恢复能力

每个资源安装都应遵循统一流水线：

`检查 → 下载到 .part → 校验长度与 SHA-256 → 解压到临时目录 → 自检 → 原子切换版本 → 记录结果`

同时需要：

- 磁盘空间预检、合理的文件大小上限和连接/读取超时。
- 支持 HTTP Range 时断点续传；不支持时安全重试。
- 指数退避和有限次数重试，切源前取得用户许可。
- 写入操作日志，但对 URL、环境变量和命令参数做凭证脱敏。
- 操作日志持久化；应用异常退出后能识别“可恢复、需重试、需清理”的步骤。
- 所有步骤幂等：重复执行检查或修复不会破坏已完成环境。
- ZIP/TAR 解压防止路径穿越、绝对路径、`..`、符号链接/reparse point 逃逸，并限制条目数与解压后总体积。

### 5.6 提权模型

Launcher 主进程和默认安装器都应以 **asInvoker / currentUser** 运行。Tauri 的 NSIS `currentUser` 模式可安装到用户目录，不需要管理员权限。

仅在用户明确选择以下可选能力时考虑提权：

- 安装到所有用户可见的系统目录。
- 修改系统级 PATH。
- 安装系统级组件或服务。
- 修改需要管理员权限的安全软件规则。

需要提权时，先在应用内展示“原因、目标路径、将执行的固定动作和影响”，用户确认后再由 Windows UAC 弹窗。提权应交给一个体积小、签名、操作白名单固定的辅助程序；不得让高权限辅助程序接受任意 shell 命令或任意参数。用户拒绝 UAC 后应回到用户级方案，而不是把整个安装判为不可用。

### 5.7 在线版与离线完整版

建议最终提供两种 Windows 安装包：

- **在线轻量版**：内嵌 WebView2 Bootstrapper，工具链从国内源下载；体积小。
- **离线完整版**：包含 WebView2 Offline Installer、固定 Node、MinGit 和 pnpm 缓存；体积大但适合受限网络。

Tauri 官方文档显示，WebView2 的离线安装模式会显著增加安装包体积。因此两种包应分别命名并在下载页解释差异，不应让一个包同时承担所有场景。

## 6. 任务状态机、阻塞与防重复点击

### 6.1 单一真实状态源

所有长任务都返回唯一 operationId。调用响应只表示“已接受”或“拒绝”，不能表示安装已经成功。后端随后按 operationId 发出阶段、百分比、日志和最终结果；只有收到 terminal success 才显示成功。

单个步骤建议具有这些状态：

`not_checked → checking → ready / needs_action → queued → downloading → verifying → installing → validating → ready / failed / cancelled`

整个引导流程是步骤 DAG；任一步失败后，用户可以从失败点重试，不必清空前面已验证的结果。

### 6.2 动作矩阵

| 当前任务 | 必须禁用 | 仍应允许 |
|---|---|---|
| 下载/安装工具 | 再次安装、切换工具目录、构建、启动、更新应用 | 查看/复制日志、最小化、在安全阶段取消 |
| Clone | 再次 Clone、切换仓库目录、构建、启动 | 日志、打开父目录、安全取消 |
| 依赖安装/构建 | 仓库更新、切换分支/路径、启动、应用更新 | 日志、安全取消、最小化 |
| 服务启动/停止 | 重复启动/停止、端口变更 | 日志和取消（若阶段支持） |
| 等待 UAC | 同一流程的其他变更动作 | 返回、在辅助程序启动前取消 |

关闭应用时如果有任务运行，弹出三选项：“后台继续”“取消并关闭”“返回”。只有任务确实支持后台继续时才展示第一项。不要把同步耗时工作放到渲染线程或 Tauri 主线程；“该阻塞”指业务操作互斥，而不是窗口失去响应。

### 6.3 真正的取消

取消令牌要传到下载器和每个子进程。取消后应终止对应进程树、等待句柄回收、清理本次创建的临时文件，再发出 cancelled 终态。只在两个阶段之间检查 epoch 不足以提供可靠的“停止”。

## 7. DeepSeek Harness 内嵌方案

### 7.1 为什么使用独立 WebView 窗口

建议新建并复用一个标签固定的 DSH WebView 窗口，而不是把 DSH 页面 iframe 到 Launcher，也不建议让主窗口在本地控制页和 DSH 页面之间反复导航。

原因：

- 顶层访问 `http://127.0.0.1:<port>`，HTTP 和 WebSocket 保持同源。
- Launcher 控制页和 DSH 网页的安全权限可以完全隔离。
- DSH 窗口可以独立记忆大小、位置、最大化状态，并支持系统任务栏和多显示器。
- 打开一次后采用 hide/show 复用，可避免反复创建 WebView 和重新登录/恢复页面状态。

### 7.2 生命周期与界面状态

推荐生命周期：

1. 用户点击“打开 DSH”时检查服务状态。
2. 服务未启动则进入启动任务，窗口先展示本地加载壳和日志入口。
3. 健康检查成功后创建或显示 DSH WebView，并导航到回环地址。
4. DSH 服务崩溃、端口改变或页面加载失败时，切回可恢复的错误页，提供重启、重试和查看日志。
5. 用户关闭 DSH 窗口默认只隐藏窗口，不停止服务；托盘可再次唤回。
6. 真正退出 Launcher 时再按设置决定停止 DSH 服务。

不要在应用启动时无条件创建 DSH WebView。首次需要时再创建，之后保持复用，是启动速度与二次打开速度之间较好的平衡。

### 7.3 安全边界

- DSH WebView 不配置任何 Tauri capability，不允许它调用本地命令。
- 不要为 localhost 增加宽泛的 remote URL capability。
- 内部导航只允许配置的 `127.0.0.1:<port>` 与必要的同源路径。
- 外部 HTTPS 链接交给系统浏览器；阻止 `file:`、`javascript:` 和未知自定义协议。
- 新窗口、下载、文件选择、剪贴板、通知和摄像头/麦克风都应逐项验证并设置最小权限。
- 端口占用时不仅检测“端口已开”，还要验证响应确实来自预期 DSH 实例。
- 为聊天窗口使用固定且可写的本地 WebView2 User Data Folder，以保存 localStorage、缓存和页面状态；不要放在临时目录、仓库或网络盘。

### 7.4 文件、拖放和下载完整性

“完整迁移”不能只验证页面能打开，还要验证 DSH 的附件与导出能力：

- 普通文件选择应使用 WebView2 原生选择器。
- Windows HTML5 文件拖放需要避免被 Tauri/Wry 自己的拖放处理器截获，只对 chat WebView 调整该行为。
- 下载必须监听开始与完成状态；默认让用户选择保存位置，禁止静默覆盖已有文件。
- 默认拒绝摄像头、麦克风、定位等意外网页权限；未来确有语音需求时按 origin、权限类型和用户动作逐项放开。
- 加载失败至少提供“重试页面、重启 DSH、系统浏览器打开、查看日志、返回控制台”。

## 8. 窗口拖动与缩放修复方向

当前自定义顶栏已经使用 `data-tauri-drag-region`，但 Tauri capability 没有授予 `core:window:allow-start-dragging`；`core:window:default` 并不包含这个权限。这是当前“按着窗口边边无法拖动”的首要可验证原因。

还需要区分平台：当前 `titleBarStyle: Overlay` 主要影响 macOS；Windows 如果保留原生 decorations 和 `resizable: true`，应天然支持标题栏移动、边缘缩放与 Snap。第一步要在打包版确认实际 decorations、最大化/全屏和 window-state 恢复结果，不能把窗口边缘的“缩放”误改成“移动”。聊天窗口第一版建议保留 Windows 原生标题栏。

实施时应同时处理：

1. 为 Launcher 自定义标题栏授予最小的 start-dragging 权限；需要自绘缩放区时再授予对应方向的 start-resize-dragging 权限。
2. drag region 属性必须直接标记在实际接收鼠标事件的元素上，不能只依赖父元素继承。
3. 按钮、输入框、菜单和可选择文本必须排除拖动区域。
4. Windows 下补充 `app-region: drag` 以改善触控和触控笔；必要时用显式 `startDragging()` 作为可靠后备。
5. 窗口边缘优先使用原生 resizable hit-test；只有无边框样式导致原生区域缺失时，才增加窄的自绘 resize handles。
6. 验证双击标题栏最大化/还原、最大化后拖出、多显示器、不同 DPI、任务栏位置和窗口状态恢复。

拖动能力只应授予本地 Launcher 窗口；DSH 远程内容区域不要获得窗口控制能力。DSH 窗口如果使用原生标题栏，能减少大量自绘窗口边界问题。

## 9. 图标优化方案

当前图标是蓝色圆角方块加白色小写 `dsh`，在 16–24 px 下文字辨识度有限，源 SVG 还依赖系统字体，跨平台输出可能不一致。

建议改为不依赖文字的原创几何标志：**对话气泡 / 终端光标 / D 与 S 的负形组合**。保留深海蓝到电光蓝的产品气质，但不要复刻 DeepSeek 官方鲸鱼标志。

设计要求：

- 先制作 1024×1024 的确定性矢量主稿，再导出各平台资产。
- 主体轮廓在 16 px 仍可识别，留 12%–15% 安全边距。
- 小尺寸版本减少渐变、细线和阴影，必要时做光学校正。
- Windows ICO 至少检查 16、24、32、48、64、256 px；任务栏、开始菜单、安装器和托盘分别实机检查。
- 使用 Tauri 官方 icon 生成流程产出 PNG、ICO 和 ICNS，但生成后仍需逐层检查。
- AI 生图适合探索概念，不建议把带模糊边缘或生成文字的位图直接当生产主稿；选定方向后应人工矢量化。

可用于下一会话或设计工具的图标提示词：

> 为 DSH Launcher 设计一个原创桌面应用图标。核心符号是“对话气泡与终端光标融合”，通过负空间轻微暗示字母 D 和 S，但不出现可读文字，不使用 DeepSeek 官方鲸鱼或任何现有品牌标志。深海蓝到明亮钴蓝配色，简洁几何、强轮廓、现代桌面开发工具气质，1024×1024，透明背景，中心构图，四周保留 15% 安全区。必须在 16×16 像素仍清楚，避免细线、复杂纹理、拟物、发光文字和过度阴影。输出一版扁平主标志，以及针对小尺寸减少细节的变体。

## 10. 启动与运行性能方案

### 10.1 先建立测量点

下一阶段先记录这些时间点，再决定优化优先级：

- 进程启动、Tauri ready、主窗口 visible、React mounted。
- 缓存快照可用、环境检查完成、仓库扫描完成。
- DSH 服务健康、DSH WebView 首次可交互。

建议初始性能目标：

- 基准 Windows 机器上，Launcher 热启动可交互小于 1 秒，冷启动小于 1.5 秒。
- 所有按钮点击在 100 ms 内出现状态反馈。
- 首屏不等待网络、Git、版本更新或完整环境探测。
- DSH 窗口立即展示加载状态，服务就绪后自动切换，不显示长时间空白页。

目标值需要在确定基准机器后校准，不能只凭开发机主观感受宣布达标。

### 10.2 推荐优化顺序

1. 首帧只读本地缓存，环境探测和仓库刷新继续后台执行。
2. 更新检查延后 3–5 秒，且不与首次引导、安装或构建抢网络。
3. 根据 PATH、工具路径和文件时间戳缓存环境结果，避免每次页面切换重复拉起子进程。
4. 用有界后台执行器管理磁盘、网络和子进程任务，避免无限并发。
5. DSH WebView 按需创建、后续复用；不要每次打开都销毁重建。
6. 对前端页面按路由/功能懒加载；动画只保留用户能感知价值的部分。
7. 将冷/热启动、安装、构建和内嵌页面内存占用纳入 Windows CI 或发布前基准。

当前 renderer 产物规模并不异常，不能为了追求体积先做大范围前端重构。应先修复重复探测、子进程与 WebView 生命周期等更可能影响体验的问题。

## 11. Windows 打包与更新

下一阶段需要补全：

- Tauri NSIS `currentUser` 安装包，作为默认 EXE 交付。
- WebView2 安装模式选择，以及在线/离线包的清晰命名。
- Windows x64 Release CI、签名、产物校验和 smoke test。
- updater 的 Windows 平台条目、签名资产和失败回滚验证。
- 安装、升级和卸载对托管工具链、用户仓库、日志和设置的保留策略。

卸载器默认不应删除用户 clone 的仓库或对话数据。若提供“同时删除本地数据”，必须列出精确目录并二次确认。

## 12. 推荐实施里程碑

### M0：契约与测量

- 定义 operationId、事件、终态、取消和动作矩阵。
- 加入启动/任务性能测量点。
- 建立假的下载、Git 和进程测试接口，避免测试依赖真实网络。

### M1：安装器核心

- 受信版本清单、国内镜像、断点下载、校验、原子安装。
- 托管 Node、MinGit、pnpm 的发现、安装、自检和修复。
- Clone 弹窗、临时目录 clone、仓库身份验证和成功地址持久化。

### M2：全流程编排

- 环境检查、clone、依赖安装、构建、启动串成可恢复的 DAG。
- 统一进度、日志、错误、重试、取消和关闭窗口行为。
- 修复“accepted 被显示为 success”的语义问题。

### M3：内嵌 DSH

- 独立低权限 WebView 窗口、导航策略、加载/错误/重连。
- 服务健康验证、窗口复用、托盘唤回和退出策略。
- 验证 DSH HTTP、WebSocket、文件交互和外链。

### M4：桌面体验与性能

- 修复拖动/缩放/最大化和 DPI 行为。
- 落地图标资产并完成小尺寸实机检查。
- 按基准结果优化启动、探测、动画和 WebView 生命周期。

### M5：Windows 正式交付

- NSIS 在线版与可选离线完整版。
- Windows updater、签名、CI、干净虚拟机端到端测试。
- 安装/升级/卸载/恢复文档。

大功能从 `0.3.1` 进入新版本时，建议按语义版本评估为 `0.4.0`，但实际版本必须依据仓库已有标签顺序决定，构建失败也不能跳号。

## 13. 验收清单

### 13.1 干净 Windows 机器

- 没有预装 Node、Git、pnpm 时，普通用户不提权也能完成全套环境准备。
- 国内源可用时，全流程不访问被配置为禁用的海外源。
- 镜像损坏、被截断或哈希不符时拒绝安装，且旧版本仍可用。
- 网络中断、应用重启后可续传或从安全步骤重试。

### 13.2 Clone 与构建

- 弹窗默认显示上一次成功地址，而不是上一次输入但失败的地址。
- 非空目标目录不会被覆盖；失败只清理 Launcher 自建临时目录。
- URL 凭证不出现在日志、设置或错误报告。
- clone、install、build 运行时，冲突按钮不可点击；取消按钮在安全阶段保持可用。
- 后端只在收到真实终态后显示成功。

### 13.3 内嵌 DSH

- 在 Launcher 内完成 DSH Web UI 加载、HTTP 请求和 WebSocket 对话。
- DSH 页面不能调用任何 Tauri 本地命令。
- 外链按策略打开系统浏览器，未知协议被拦截。
- 服务重启后自动重连；关闭再打开窗口不重复创建无用 WebView。

### 13.4 桌面与性能

- 标题栏拖动、边缘缩放、双击最大化、从最大化拖出均工作正常。
- 100%、125%、150%、200% DPI 和多显示器下命中区域正确。
- 图标在安装器、任务栏、开始菜单、托盘和窗口标题中清晰。
- 启动性能有可重复记录，并达到经过确认的冷/热启动目标。

### 13.5 自动化测试建议

- 单元测试：URL 校验/脱敏、版本清单、哈希、状态迁移、动作矩阵、取消、原子 clone。
- 集成测试：假 HTTP/Git/pnpm、断点下载、镜像回退、UAC 拒绝、目标目录非空、子进程树取消。
- Windows E2E：干净 VM 安装、全套环境、构建、启动、内嵌对话、升级、卸载和重启恢复。
- 安全测试：远程 WebView 无 IPC、导航限制、凭证脱敏、恶意压缩包路径穿越和端口冒充。

## 14. 实施前需要产品确认的少量决策

这些问题不阻碍开始 M0/M1，但应在发布前确认：

1. 是否正式提供“离线完整版”，并接受约百 MB 以上的额外体积。
2. 官方 GitHub 不可达时，是否允许自动回退；建议默认询问，不静默切换。
3. 用户关闭 DSH 窗口时，默认“仅隐藏”还是“同时停止服务”；建议仅隐藏。
4. 托管工具链升级是随 Launcher 发布，还是允许独立更新；建议第一版随 Launcher 发布。
5. 用户仓库默认放应用管理目录还是 Documents；建议应用管理目录，并允许用户选择。

## 15. 明确不建议做的事

- 不把 Launcher 主进程长期以管理员权限运行。
- 不自动修改系统 PATH、全局 npm registry 或 Git 全局配置。
- 不使用未经维护和校验的公共 GitHub 代理。
- 不把官方 DeepSeek 网站当成本地 DSH Web UI 的无差别替代。
- 不授予 DSH WebView 通用 shell、文件系统或 Tauri IPC 权限。
- 不在 busy 时把所有 UI 一刀切禁用，也不允许并发 clone/install/build/start。
- 不把“命令已提交”当成“命令已完成”。
- 不在没有基准数据时进行大范围性能重构。

## 16. 主要参考资料

- [Tauri 窗口自定义与 drag region](https://v2.tauri.app/learn/window-customization/)
- [Tauri Window JavaScript API](https://v2.tauri.app/reference/javascript/api/namespacewindow/)
- [Tauri capabilities 安全模型](https://v2.tauri.app/security/capabilities/)
- [Tauri core permissions](https://v2.tauri.app/reference/acl/core-permissions/)
- [Tauri WebviewWindow API](https://v2.tauri.app/reference/javascript/api/namespacewebviewwindow/)
- [Tauri Windows Installer 与 WebView2 模式](https://v2.tauri.app/distribute/windows-installer/)
- [Tauri 图标生成](https://v2.tauri.app/develop/icons/)
- [Microsoft WebView2 性能指南](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/performance)
- [Microsoft WebView2 User Data Folder](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/user-data-folder)
- [Microsoft：以管理员权限运行的安全建议](https://learn.microsoft.com/en-us/windows/win32/secbp/running-with-administrator-privileges)
- [Microsoft：Windows application manifest](https://learn.microsoft.com/en-us/windows/win32/sbscs/application-manifests)
- [Git for Windows MinGit](https://gitforwindows.org/mingit.html)
- [pnpm 安装文档](https://pnpm.io/installation)
- [Node.js 24 Corepack 文档](https://nodejs.org/download/release/latest-v24.x/docs/api/corepack.html)
- [npmmirror](https://npmmirror.com/)
- [DeepSeek Harness 官方仓库](https://github.com/deepseek-ai/deepseek-harness)
- [DeepSeek Harness 当前 package.json](https://github.com/deepseek-ai/deepseek-harness/blob/master/package.json)

---

## 附录 A:实施状态与上游事实核对(2026-08-15 实施时更新)

> 本附录记录按本方案实施后的落地状态,以及与调研时假设不一致、以上游/镜像可验证事实为准的更正项。

### A.1 已落地(代码 + 测试)

| 领域 | 落地模块 | 验证 |
|---|---|---|
| M0 操作契约 | `src-tauri/src/ops.rs`(OperationCoordinator / journal / 取消令牌 / InstallationSnapshot)、`contract.rs`(OperationSnapshot / DisabledAction)、`perf.rs` | cargo test 76 项含状态机/动作矩阵/journal/取消/崩溃恢复单测 |
| M1 签名 catalog | `catalog.rs`(Ed25519 内置公钥验签,安全失败)、`resources/catalog.json` + `.sig`(私钥在仓库外) | 篡改/错钥/错签名均拒绝 |
| M1 下载/解压 | `download.rs`(.part / 断点续传 / 长度+SHA-256 / 取消)、`archive.rs`(防 Zip Slip/绝对路径/`..`/符号链接,条目与体积上限) | 假 HTTP 服务器集成测试 |
| M1 托管工具链 | `toolchain.rs`(node/git/pnpm 版本化目录 + active pointer + InstallationSnapshot)、修复 Windows ZIP 占位、安装后 tools 刷新 | 单元测试 |
| M1 clone 事务 | `clone.rs`(URL 校验/脱敏、远端只读验证、动态分支、同卷 staging、非空不覆盖、原子提交、last-good URL) | 单元测试 |
| M2 全流程 DAG | `state.rs` run_action → ops.begin → 线程 → finish(Success/Cancelled/Failed),accepted ≠ success | 单元测试 |
| M3 chat WebView | `chat.rs`(零 capability、固定 UDF、导航白名单、下载不覆盖、隐藏复用、健康检查=内容标记+端口持有者、错误页 data URL、5s 自动重连) | 单元测试;窗口行为待 Windows/macOS 实机 |
| M4 拖动/图标 | capabilities 增 `allow-start-dragging`;master SVG → `pnpm tauri icon`(ICO 含 16/24/32/48/64/256;ICNS/PNG) | 图标已目检 |
| M5 Windows 打包 | `tauri.conf.json` NSIS currentUser + `bundle.windows.webviewInstallMode`;`release.yml` Windows NSIS + 签名 + latest.json(windows-x86_64);`doc/WINDOWS-ACCEPTANCE.md` 验收清单 | NSIS 配置经本地 debug build 校验;NSIS 实机构建在 Windows CI |

### A.2 与调研假设不一致、以可验证事实为准的更正

1. **MinGit 镜像路径**:npmmirror 二进制镜像实际根为 `https://registry.npmmirror.com/-/binary/…`(非 `npmmirror.com/mirrors/…`);git-for-windows 目录为 `/-/binary/git-for-windows/v2.55.0.windows.4/`。
2. **Node 固定版本**:调研时未定版;实施时锁定 `v24.9.0`(npmmirror `latest-v24.x` 最新),win-x64.zip 字节与官方 `SHASUMS256.txt` 完全一致(`6873514c…`)。
3. **MinGit 版本**:锁定 `2.55.0.4`;npmmirror 下载字节与 GitHub 官方 Release 完全一致(`4e03f94c…`)。
4. **pnpm 来源**:pnpm 不走 standalone 二进制,而是 `registry.npmmirror.com/pnpm/-/pnpm-<ver>.tgz`(npm registry 镜像,字节与官方 npm registry 一致),从 clone 后 `package.json` 的 `packageManager` 解析精确版本;不在签名 catalog 内的版本一律安全失败,不静默换版本。
5. **catalog「国内 CDN」实现**:以 npmmirror(registry.npmmirror.com)作为唯一字节来源;URL 全部固定进签名 catalog,下载失败不静默回退境外。
6. **Tauri NSIS 配置字段**:新版本 Tauri 2 中 `webviewInstallMode` 位于 `bundle.windows` 层(不在 `nsis` 内),且 `createDesktopShortcut` 等字段已移除;以 `pnpm tauri build` 实际校验为准。
7. **DSH 上游事实**:`deepseek-harness` master `packageManager: pnpm@11.7.0`,engines `node: ^22.19.0 || >=24.0.0`;`dsh web` 就绪行 `dsh web: http://127.0.0.1:<port>`;首页含 `<title>DeepSeek Harness</title>` 与 `/manifest.webmanifest`(健康检查标记);`--host 0.0.0.0` 被 CLI 拒绝。
8. **README 中的 v0.3.1**:与仓库 tag `v0.3.1` 一致,无需修正。

### A.3 未验证/遗留风险(如实声明)

- Windows/macOS **实机窗口行为**:chat WebView 零权限边界、WebView2 下载/权限、拖放、DPI/多显示器、NSIS currentUser 安装、WebView2 bootstrapper 下载、Job Object 取消——均未在本机(Mac)验证,已列入 `doc/WINDOWS-ACCEPTANCE.md` 并在 Windows CI 自动执行编译/打包/单测(engine_flow 为 Unix-only,Windows 单测覆盖 win 模块)。
- **UAC helper**:本阶段未实现窄权限 helper(当前不触发 UAC;系统级能力入口未开放),方案 §7 的设计保留为下一阶段。
- **网页权限默认拒绝**:chat DevTools 已按 debug/release 区分;WebView2 逐权限拒绝需 `webview2-com` 事件挂钩,未在本机验证,列入 Windows 验收。
- **npm postinstall 境外流量**:未做网络层封锁(仅 registry 指向 npmmirror);如需验收级「完全只走国内」需受控代理 + 域名 allowlist,见验收清单 §8。
