# 新会话提示词：内置安装系统、DSH 内嵌与 Windows 体验

下面内容可以直接复制到新的 Codex 任务中使用。

---
你现在接手项目：

- 项目路径：`/Users/yoahoug/Desktop/dsh-launcher`
- 当前基线版本：先从仓库和 Git 标签确认，不要只相信文档中的 `0.3.1`
- 目标平台：Windows 10/11 x64 优先，macOS 现有能力不能被破坏

请先完整阅读：

- `AGENTS.md`
- `doc/NEXT-PHASE-INSTALLER-EMBEDDED-DSH-RESEARCH.md`
- `doc/DESKTOP-APP-FULL-DELIVERY-PLAN.md`
- `doc/NEW-SESSION-FULL-DELIVERY-PROMPT.md`

然后检查当前工作树、现有实现、测试、CI 和最近提交。当前或后续可能存在另一条前端 UI 重构分支/未提交改动；这些都属于用户，必须保留，不能恢复旧文件、覆盖、重排或顺手重构。应在当前 UI 结构上做最小必要集成。

## 任务目标

把 DSH Launcher 实现为可在干净 Windows 机器上使用的一体化桌面程序：

1. 用户通过按钮完成环境检查、托管 Git/Node/pnpm 安装、选择 clone 地址与目录、依赖安装、构建和启动。
2. Launcher 自有下载、clone 和 npm registry 默认走明确可见、可校验的国内源；不得使用来源不明的公共 GitHub 代理，也不得静默回退境外源。
3. 默认使用用户级托管工具链，不修改系统 PATH、全局 npm/Git 配置，不要求 UAC。
4. 只有用户明确选择系统级能力或受保护路径时才触发 Windows UAC，而且只能通过签名、固定操作白名单的窄权限 helper 执行；不能把整个 Launcher 提权，也不能把任意命令交给 helper。
5. 将本机 DeepSeek Harness 的 `dsh web` 完整放进 Launcher 体系：使用独立、可复用、零 Tauri capability 的 chat WebViewWindow 顶层打开 `http://127.0.0.1:<port>`。这不是内嵌官方 `chat.deepseek.com`。
6. 修复 Launcher 的窗口拖动、边缘缩放、双击最大化、多 DPI 和多显示器体验；Windows chat 窗口第一版保留原生标题栏。
7. 优化图标、冷/热启动、后台探测、WebView 生命周期和安装/构建时的交互锁。
8. 补全 Windows NSIS/EXE、WebView2 分发、updater、CI 和干净虚拟机验收能力。

不要只给计划。先检查仓库证据，给出简短实施计划，然后持续实现、测试和修正，直到本阶段目标达到可验证状态。不要发布 Release、推送远端、创建 PR 或删除用户数据，除非我在本任务中明确授权。

## 不可偏离的架构约束

### A. 托管工具链

- 工具链放应用数据目录，采用版本化目录和 active pointer/配置，不写用户全局环境。
- Windows Git 优先使用固定版本的官方 MinGit；Node 使用与仓库 engines 兼容的固定版本；pnpm 必须从 clone 后真实 `package.json` 的 `packageManager` 解析精确版本。
- 可复用兼容系统工具，但 UI 必须显示来源，并允许切换到托管版本。
- 修复当前 Windows Node ZIP 解压占位逻辑、安装后当前进程未刷新托管 Node，以及 Git/pnpm stdout/stderr 可能因顺序读取而死锁的问题。
- 不默认安装 Python、Rust、Visual Studio Build Tools；只有明确的 node-gyp/MSVC 失败才提供可选诊断路径。

### B. 国内镜像与供应链

- 引入可签名验证的 runtime catalog，固定 component、version、platform、URL、size 和 SHA-256；Launcher 内置公钥，国内 CDN 只负责字节分发。
- 下载流程必须是 part 文件、可取消流式下载、长度/哈希校验、安全解压、工具自检、原子切换。
- 解压必须防 Zip Slip、绝对路径、`..`、symlink/reparse point 逃逸，并限制条目和总体积。
- npm/pnpm registry 只通过当前子进程环境或受控临时配置注入 `https://registry.npmmirror.com/`，不改全局配置。
- 不得声称任意第三方包的 postinstall 一定不会访问境外地址。若验收要求网络层完全只走国内，必须通过受控代理/缓存与域名 allowlist 实现并测试。
- 校验或签名失败属于安全失败，不得通过忽略哈希、force 或 update-checksums 绕过。

### C. Clone 与事务性

- Clone 按钮打开应用内弹窗，包含 URL、目标目录、网络源和高级分支选项；目录使用原生选择器。
- 默认填“上一次远端验证通过或 clone 成功”的地址；非法/失败输入不得覆盖好地址。
- 默认只允许 HTTPS 与受控 SSH；禁止任意 remote helper、shell 拼接和凭证落盘。所有命令使用参数数组，日志必须脱敏。
- 先用禁交互、带超时的只读远端检查验证 URL，再 clone 到最终目录同卷的 runId staging。
- 一键全套流程应在 staging 中完成 clone、依赖安装、构建和 post-check，最后才原子提交最终目录与配置；目标非空绝不覆盖。
- 失败、取消只清理本 operation 创建的 staging，绝不删除用户已有目录。
- 默认不 shallow clone，默认分支从远端 HEAD 动态发现，不能硬编码 `main`。

### D. Operation Coordinator

- 不要继续只依赖粗粒度 `busy`。新增统一操作协调器和持久化 InstallationSnapshot/operation journal。
- 每个长任务有 operationId；命令返回 accepted 只代表已接受，UI 只有收到 terminal success 才显示成功。
- 环境安装、clone、依赖安装、build、update/rebuild、自更新属于 exclusive-write，同一时间只能运行一个。
- start/dev 与 exclusive-write 互斥；影响 repo/runtime/network 的设置在任务期间禁用。
- stop/cancel、查看与复制日志、最小化必须保持可用。按钮被禁用时显示具体原因。
- 取消令牌传到下载器、解压器和所有子进程；Windows 用 Job Object 终止整棵进程树。原子提交的短临界区可以标记为不可取消。
- journal 每次状态变化原子写入。崩溃重启后先探测事实，再允许清理、重试或继续安全步骤，不能盲目续跑外部安装器。
- 下载、Git 和 pnpm 的 stdout/stderr 必须并发消费并持续上报日志，不能等一侧完全结束后再读另一侧。

### E. UAC

- 主程序与默认 NSIS 使用 asInvoker/currentUser。
- UAC 前先在应用内展示动作、发布者、版本、目标、体积和影响，再由系统弹窗确认。
- helper 固定、签名、验证哈希与发布者，只接受 enum 化白名单动作；禁止任意 EXE、命令行、PowerShell 或目标路径。
- UAC 被拒绝归类 cancelled，并提供用户级回退方案。

### F. DSH chat WebView

- 第一阶段采用独立 `chat` WebviewWindow，不使用 iframe，不把主 React WebView 导航到 DSH，不复制重写 DSH 前端。
- 服务未运行时先启动；健康检查必须确认是预期 DSH 实例，而不只是端口开放。
- WebView 按需创建，后续 hide/show 复用；固定本地 UDF，保持 origin 和端口稳定。
- chat WebView 不授予任何 Tauri API/capability；不要配置 localhost remote capability。
- 只允许精确 loopback origin 内部导航；外链用系统浏览器；拦截未知协议与任意 window.open。
- release 默认关闭 chat DevTools，默认拒绝不需要的网页权限。
- 验证 HTTP、WebSocket、中文输入法、复制粘贴、图片附件、HTML5 拖放、文件选择、下载、会话恢复、DSH 重启重连和白屏/崩溃回退。
- 错误页至少提供：重试页面、重启 DSH、系统浏览器打开、查看日志、返回控制台。

### G. 窗口、图标与性能

- 先确认 Windows/macOS 实际 decorations、resizable、maximized/fullscreen 与 window-state 恢复，再修复拖动。
- 自绘标题栏需要最小的 start-dragging/start-resize-dragging capability；交互控件排除 drag region；边缘是 resize，不是 move。
- 验证双击最大化、最大化拖出、Snap、100%/125%/150%/200% DPI 和多显示器。
- 图标使用一个确定性 master 源，去掉小尺寸文字，产出并检查 PNG/ICO/ICNS；Windows 至少覆盖 16、24、32、48、64、256 px。不要直接把模糊 AI 位图作为生产 master。
- 建立 process start、Tauri ready、主窗口 visible、React interactive、环境/仓库检查、DSH ready、chat load finished 等测量点。
- 首屏不等待网络、Git、更新或完整环境检查；更新检查延后；chat WebView 首次需要时创建、之后复用；保留 GPU 加速。
- 用 P50/P95 和固定基准机器比较冷启动、热启动、首次 chat、内存，不凭主观体感做大范围重构。

## 建议实施顺序

严格按下面里程碑推进，每完成一项就更新 todo 和验证结果：

1. M0：操作契约、状态机、动作矩阵、journal、取消模型、性能测量点和 fake test seam。
2. M1：托管 MinGit/Node/pnpm、签名 catalog、国内下载、校验、安全解压、Clone 弹窗和 staging。
3. M2：环境检查 → clone → install → build → post-check → commit → start 的完整 DAG，并修复 accepted/success 语义。
4. M3：独立 chat WebView、零权限边界、导航/新窗口/下载/拖放、UDF、服务与错误恢复。
5. M4：拖动与缩放、图标资产、启动和运行性能优化。
6. M5：Windows currentUser NSIS、在线版与可选离线完整版 WebView2、Windows updater/CI/VM 验收。

如果某个里程碑过大，允许拆成内部子步骤，但不要跨过核心安全和验证要求，也不要只完成 UI 外壳。

## 最低测试与验收

至少补齐并运行：

- 单元测试：URL 校验和脱敏、runtime catalog、哈希、版本解析、状态迁移、动作矩阵、取消、原子提交。
- 集成测试：假 HTTP/Git/pnpm、断点下载、镜像 5xx/哈希错误、非空目录、UAC 拒绝、各阶段取消、stdout/stderr 大量输出、崩溃 journal 恢复。
- Windows 检查：Node ZIP、MinGit、Job Object 取消、中文/空格路径、NSIS 安装、WebView2、无预装工具的一键全流程。
- chat E2E：同源 HTTP/WebSocket、附件、下载、外链、无 Tauri IPC、服务重启、窗口复用、DPI/多显示器。
- 性能：固定环境记录冷/热启动和首次 chat 的 P50/P95，以及 chat 打开/隐藏后的内存。

每个阶段运行与风险相称的 Rust tests、前端 tests/typecheck、构建和 Windows 验证。不能在 macOS 编译通过后声称 Windows 行为已验证。若当前机器无法完成 Windows 实机测试，要明确列出未验证项，并把对应检查落入 Windows CI；不得过度宣称完成。

## 工作方式与最终交付

- 修改前先读相关实现与调用点，保持最小正确改动。
- 使用项目已有包管理器和虚拟环境约定，不安装全局依赖。
- 不覆盖用户的 UI 重构和其他脏工作树改动。
- 不使用破坏性 Git 命令，不删除用户仓库或数据。
- 不引入通用 shell 权限，不扩大 chat WebView 权限。
- 发现调研文档与实时上游不一致时，以仓库和上游可验证事实为准，并同步更新文档。
- 版本号依据已有标签按语义版本顺序选择；失败构建不得跳版本。

最终回复必须说明：

1. 完成了哪些用户可见能力。
2. 修改了哪些关键文件。
3. 跑了哪些测试，结果是什么。
4. 哪些 Windows/UAC/WebView2 行为经过了真实 Windows 验证。
5. 仍未验证或存在的风险。
6. 没有执行哪些外部动作，例如未推送、未发布。

---
