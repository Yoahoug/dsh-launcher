# Windows 验收清单(干净虚拟机 / 干净机器)

> 本清单覆盖 v0.4.0+ 桌面版的 Windows 目标验收项。全部为**人工/脚本化验收步骤**,
> 由维护者在干净 Windows 10/11 x64 虚拟机(无预装 Git/Node/pnpm,无 WebView2 亦可)执行;
> 无法在本机完成的项已落入 `.github/workflows/ci.yml`(windows-latest 矩阵)与
> `release.yml`(windows-latest 构建 NSIS + 签名)自动检查编译与打包链路。
> 记录格式:`[ ] 未验证 / [x] 已验证(环境、日期)`。

## 0. 前置准备

- [ ] 虚拟机:Windows 10 22H2 或 Windows 11 23H2 x64,全新镜像,未装任何 dev 工具
- [ ] 安装包:`dsh-launcher_<ver>_x64-setup.exe`(currentUser NSIS)
- [ ] 干净网络:仅国内网络(可访问 npmmirror.com / registry.npmmirror.com),GitHub 不可达
- [ ] 无 UAC 弹窗的普通用户账户

## 1. 安装与 WebView2 分发

- [ ] 双击 setup.exe → 无 UAC 提示(installMode=currentUser),安装到 `%LOCALAPPDATA%\...`
- [ ] 无 WebView2 机器:bootstrapper 静默安装 WebView2 Runtime;离线机器使用
      `webviewInstallMode: offlineInstaller`(构建时切换,见 doc/ 说明)
- [ ] 开始菜单/桌面快捷方式出现;卸载程序可移除
- [ ] 中文安装界面(SimpChinese)

## 2. 无预装工具一键全流程(核心验收)

1. [ ] 启动 Launcher(首次运行引导出现)
2. [ ] 环境页显示 git/pnpm/node 未找到,提供「一键安装工具链」
3. [ ] 点击「一键安装工具链」:
   - [ ] Node: 从 `registry.npmmirror.com/-/binary/node/v24.9.0/…` 下载 win-x64.zip
   - [ ] 长度 + SHA-256 校验通过(与签名 catalog 一致)
   - [ ] 安全解压到 `%LOCALAPPDATA%\…\toolchains\node\v24.9.0`(无 Zip Slip)
   - [ ] MinGit: 从 `…/git-for-windows/v2.55.0.windows.4/MinGit-2.55.0.4-64-bit.zip` 下载,
         校验 + 解压到 `toolchains\git\2.55.0.4\cmd\git.exe`,自检 `git --version`
   - [ ] pnpm: 从 `registry.npmmirror.com/pnpm/-/pnpm-11.7.0.tgz` 下载,校验 + shim 生成
   - [ ] 进度条/日志实时刷新;按钮禁用且有原因;全程无 UAC
4. [ ] 仓库页「克隆仓库」弹窗:默认填充上次成功地址;填
      `https://github.com/deepseek-ai/deepseek-harness.git` + 空目录
5. [ ] 「一键安装并启动」:
   - [ ] staging 克隆(不 shallow,默认分支从远端 HEAD 动态发现)
   - [ ] pnpm 版本从 clone 后 package.json 的 packageManager 解析(11.7.0)并校验
   - [ ] `pnpm install` 走 `NPM_CONFIG_REGISTRY=https://registry.npmmirror.com/`(仅子进程环境)
   - [ ] `pnpm run build` 通过;post-check 检测 `apps/web/dist/index.html`
   - [ ] 原子提交到目标目录;`dsh web` 启动,主界面 running
6. [ ] 中文/空格路径(如 `D:\我的 代码\repo`)全流程正常

## 3. 取消与进程树

- [ ] 下载/克隆/构建中点击「取消」:日志、最小化、取消按钮始终可用
- [ ] Job Object 终止整棵进程树(pnpm → node → 子进程均退出,无残留)
- [ ] 取消后 journal 记录为 cancelled;重试时已完成步骤跳过

## 4. 崩溃恢复

- [ ] 安装/克隆/构建过程中强杀 Launcher 进程 → 重启:
- [ ] journal 中 running 记录被标记 interrupted(不续跑外部安装器)
- [ ] staging 目录提示清理;用户已有目录未被触碰

## 5. DSH chat WebView(Windows)

- [ ] 服务未运行时点「打开 dsh」→ 先启动服务,再创建 chat 窗口
- [ ] 健康检查确认是预期 DSH 实例(标题/清单 + 端口持有者身份)
- [ ] 窗口保留原生标题栏;拖动、缩放、双击最大化、Snap 正常
- [ ] 中文输入法、复制粘贴、图片附件、HTML5 拖放、文件选择、下载(不覆盖已有文件)
- [ ] 外链在系统浏览器打开;chat 窗口内无法调用任何 Tauri IPC(零 capability)
- [ ] 会话恢复(重开窗口内容保持);DSH 重启后自动重连
- [ ] 100%/125%/150%/200% DPI 与多显示器不模糊、不漂移
- [ ] release 版无 DevTools(devtools 仅 debug)

## 6. 主窗口体验

- [ ] 自绘标题栏可拖动;边缘缩放是 resize 不是 move;双击最大化
- [ ] 关闭窗口 → 隐藏到托盘;托盘召回;退出 Launcher 不停止 dsh
- [ ] 窗口位置/大小重启后恢复(window-state)

## 7. 性能(固定基准机器,记录 P50/P95)

- [ ] 冷启动:process_start → tauri_ready → 主窗口可见 → react_interactive(ms)
- [ ] 热启动(托盘隐藏后)耗时
- [ ] 首次打开 chat:启动服务 → 健康检查 → chat load finished
- [ ] chat 打开/隐藏后的内存变化(任务管理器)

## 8. 网络(仅国内)

- [ ] 断网/镜像 5xx:下载失败给出明确错误,不静默回退境外源
- [ ] 哈希错误:安全失败,拒绝安装
- [ ] (可选)受控代理 + 域名 allowlist 下验证 npm postinstall 不触境外

## 9. 签名与供应链(发布前)

- [ ] runtime catalog Ed25519 验签自检通过;篡改 catalog 拒绝启动(单测覆盖)
- [ ] Tauri updater:Windows 端 `windows-x86_64` 条目签名校验通过
- [ ] Authenticode(可选):setup.exe 签名后 SmartScreen 不告警
