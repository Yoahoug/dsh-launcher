# dsh-launcher

针对 **DeepSeek Harness(dsh)开发者**的纯启动器:源码启动、一键更新构建、热重载开发模式、后台常驻、亮色单页控制台。

> **定位铁律:启动器只是一个启动器。** 它不承载任何 dsh 界面;主界面永远是 `http://127.0.0.1:3080/`(dsh web),启动器只负责把它拉起来、托管进程、提供控制与日志。

## 文档

| 文件 | 说明 |
|---|---|
| [`doc/DEVELOPMENT-PLAN.md`](doc/DEVELOPMENT-PLAN.md) | 完整开发方案(v0.2):需求、架构、流程、UI 设计、里程碑、验收标准 |
| [`doc/ui/mockup.html`](doc/ui/mockup.html) | 亮色单页控制台高保真原型,浏览器直接打开预览 |
| [`doc/NEW-SESSION-PROMPT.md`](doc/NEW-SESSION-PROMPT.md) | 新会话提示词:交给一个新会话直接开始实现 |

## 项目现状

方案与原型阶段,尚未写实现代码。下一步:把 `doc/NEW-SESSION-PROMPT.md` 的提示词交给一个新会话,按 `DEVELOPMENT-PLAN.md` 的里程碑(M0→M5)实现。

## 目标形态(实现后)

- 双击 `bin/start.command` → 亮色控制台(`http://127.0.0.1:3090/`)自动打开
- 控制台操作:启动 / 开发模式(热重载)/ 更新并构建 / 停止 / 重建并重启 / 日志
- 就绪后自动打开主界面 `http://127.0.0.1:3080/`(dsh web),启动器退居后台

## 技术约束

Node `^22.19 || >=24`、ESM、**零 npm 运行时依赖**(仅 Node 内置模块)、前端纯 HTML/CSS/JS 单页。
