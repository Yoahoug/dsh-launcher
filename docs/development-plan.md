# dsh-launcher 后续开发方案：插件管理子界面 + 技能管理子界面（联动 dsh-plugins）

> 状态:方案草案(v0.1)→ **已实施(M1–M4 完成,2026 实现会话回写)** · 调研基于 2026-08-15 的 `deepseek-harness`(master)与 `dsh-plugins` 快照
> 本文只描述"做什么、为什么、怎么做、怎么验收",不约束实现细节;实现时以主仓库 AGENTS.md 与 dsh-plugins AGENTS.md 为准。
> **实现偏差与补充记录见 §11(回写)。**

---

## 1. 背景与目标

### 1.1 现状摘要

`dsh-launcher`（当前 v0.6.0）是 Tauri 2 桌面启动器：纯 Rust 原生核心 + React 控制台，负责 dsh 仓库的
启动 / 更新构建 / 托管工具链 / 日志 / 托盘，并在主窗口内以零权限子 WebView 承载 `dsh web`。

DeepSeek Harness 的“一切皆插件”由 Cordis 插件系统承载。官方 Web UI 里的插件管理存在明显短板：

- **`ui-settings-plugin-inventory`（插件清单）**：只读投影 `ctx.loader.entries()`，无来源、无启用/停用/增删能力；
- **`ui-settings-plugins`（插件配置）**：只有“由用户拥有的 Host 插件”注册 settings 命名空间才渲染卡片，
  且受 `api-proxy` 白名单限制。当前**只有 3 张卡可自行配置**（`bash` 执行器、`agent-loop` 工具并行度、
  `web-search-deepseek` 搜索提供方），其余插件只能看不能配。

同时 dsh 官方没有“独立管理技能”的入口（技能只能写文件/靠 preset 组合），也没有“发现本机其他 agent
（codex / opencode / cursor / .agent / claude）的既有技能”的能力。

### 1.2 本方案目标

在启动器内新增两个**子界面**（左侧导航新页）：

1. **插件子界面**：把“全部插件”（官方内置 bundle 行 + 用户补丁行 + dsh-plugins 外部包）以卡片形式**具象化**，
   支持启停、配置（不止 3 张卡）、从 **dsh-plugins** 仓库一键安装/移除——官方插件管理的增强版；
2. **技能子界面**：**独立管理**技能的添加/删除/导入/编辑，并**自动扫描发现**本机 codex、opencode、cursor、
   `.agent`、claude 等工具目录里的既有技能，一键导入到 dsh 可用；
3. **配套新插件**：在 dsh-plugins 新建一个插件（`@dsh-plugins/skill-external-roots`），把外部技能根目录
   挂进运行中 dsh 的 `ctx.skills` 注册表，让模型真正能用上这些技能。该插件的开发方案单独成文
   （`dsh-plugins/packages/skill-external-roots/docs/development-plan.md`），本文只引用其契约。

设计总原则：**怎么简单怎么来**——尽量复用 dsh 既有机制（补丁层 + HMR、skill-filesystem 根目录约定），
启动器只做“文件层读写 + CLI 子进程 + 展示”，不侵入 dsh 主仓库、不改造官方 UI。

---

## 2. 关键调研结论（只读期）

### 2.1 插件机制（dsh 侧）

- 插件 = Cordis 插件；组合包（bundle）在 `package.json` 声明 `dsh.bundle.patch`（一个 `cordis.patch.yml`）；
  profile 在 `$DSH_HOME/profiles/<name>/` 下，`package.json` 声明 `dsh.profile.bundles`（有序）。
- **生效配置分层叠加**（后层按行胜出，patch 按 `id` **替换目标行整个 `config`，不做深合并**）：
  1. profile bundles（按列表顺序，先是 `@deepseek-ai/dsh-base`）
  2. profile 自己的 `cordis.patch.yml`
  3. `$DSH_HOME/cordis.patch.yml`（home 级）
  4. `--patch <path>` overlay（argv 级，不持久）
- 安装/移除：`dsh plugin --profile <name> add|remove <spec>`（profile 目录内转发 pnpm，并按已装依赖
  reconcile `bundles` 列表）。`dsh web` 是 `--profile web` 的硬编码别名。`file:` 安装直接链接 checkout，
  需包内先 `pnpm install && pnpm run build` 产出 `lib/`；git 安装才需要 `allowBuilds` 授权（本方案不用）。
- **运行中生效**：`packages/boot/app-boot` 的 `watchUserPatches` 用 Cordis HMR 精确监听 profile 的
  `cordis.patch.yml`——改补丁 → 运行中的 dsh web 热重载（插件启停/配置变更无需重启）。
- `dsh --profile <name> --dump-config`：**不启动实例**地按层渲染组合配置（带 `# == <层>` 注释），
  且**不求值 `!!js` 表达式**——可作为“生效配置”权威来源与补丁校验手段。

### 2.2 技能机制（dsh 侧）

- `ctx.skills` 是宿主 + 按 scope 分层的技能注册表；提供方实现 `SkillProvider`（`name` / `list()` / `get()`），
  经 `ctx.skills.registerProvider(create)` 同步注册（同名在同层内会抛错）。
- 内置 `skill-filesystem` 提供方按 rank 扫描根目录：

  | rank | source | 根目录 |
  |---|---|---|
  | 100 | project-dsh | `<project>/.dsh/skills` |
  | 200 | project-agents | `<project>/.agents/skills` |
  | 300 | custom | `Config.customSkillDirs` |
  | 400 | user-dsh | `$DSH_HOME/skills`（跳过 `.system`） |
  | 500 | user-agents | `$DSH_AGENTS_HOME/skills`（默认 `~/.agents/skills`） |
  | 600 | bundled | `Config.bundledSkillDir` / `$DSH_BUNDLED_SKILL_DIR` |

- skill 身份：kebab-case（`^[a-z0-9]+(?:-[a-z0-9]+)*$`）；接受目录包 `<name>/SKILL.md` 与扁平 `<name>.md`；
  frontmatter 必须含 `name` + `description`；可选 `whenToUse`、`disable-model-invocation`、`user-invocable`。
- 监控：chokidar 监听既有根目录增删改 → `skills/change` 失效事件 → 模型侧目录自动刷新。

### 2.3 外部工具技能目录实测（本机）

| 工具 | 目录 | 格式 | 与 dsh 兼容性 |
|---|---|---|---|
| OpenAI Codex | `~/.codex/skills/<name>/SKILL.md` | frontmatter `name/description` | ✅ 直接兼容 |
| Claude Code | `~/.claude/skills/<name>/SKILL.md` | frontmatter `name/description` | ✅ 直接兼容 |
| Cursor | 项目 `.cursor/skills/`；本机另有 `~/.cursor/skills-cursor/` | `<name>/SKILL.md`（另有 `environments` 等键） | ✅ 多余键被忽略 |
| OpenCode | `~/.config/opencode/`（本机只有 `opencode.json`，无 skills 目录） | 候选 `~/.config/opencode/skills` | 需按同约定 |
| Agents（dsh 亲缘） | `~/.agents/skills/`、`<project>/.agents/skills/` | 同 dsh 约定 | ✅ dsh 已原生扫描 |

> “复用 dsh 已有插件”：`.agents` 系列已被内置 `skill-filesystem` 覆盖；“外部技能进 dsh”最简路径是把
> `~/.codex/skills` 等目录加进 `skill-filesystem` 的 `customSkillDirs`（零代码补丁）；更完整的“独立管理 +
> 一键开关”则由新插件承担（见 §5）。

### 2.4 与运行中 dsh 通信的可行性

- 启动器 React 渲染层**不直接 fetch dsh**（现有铁律：一切数据经 Tauri IPC）。
- 启动器 Rust 核心已掌握：repo 路径、`DSH_HOME`、pnpm/node 解析、日志中心（log_hub）、supervisor 进程树。
  新增能力全部走 **Rust 侧**：读 profile/bundle/patch 文件、跑 `pnpm dsh plugin --profile …` /
  `pnpm dsh --profile … --dump-config` 子进程（复用 supervisor 的 pnpm 解析 + PATH 注入）、扫描技能目录。
- dsh 侧 `webServer` 服务允许插件注册 HTTP 路由（`ctx.webServer.register`）——作为**可选增强**，新插件可
  暴露 `/api/…` 供启动器拿“运行中实时清单/状态”；v1 不依赖它（静态组合视图 + dump-config 已足够）。

---

## 3. 总体架构决策

```
┌────────────────────────── dsh-launcher (Tauri) ──────────────────────────┐
│ src-ui (React)                                                          │
│   SideNav: 服务 | 仓库与构建 | 工具链 | 运行日志 | 【插件】|【技能】| 设置 │
│      └─ plugins-page / skills-page  ── 全部经 desktop-api (Tauri IPC)   │
├──────────────────────────────────────────────────────────────────────────┤
│ src-tauri (Rust)                                                        │
│   commands.rs       新增命令: plugins_* / skills_* / dshctl_*           │
│   services/dshctl.rs  新模块: dsh CLI 子进程封装(复用 runtime/supervisor)│
│   services/plugins.rs 新模块: profile/bundle/patch 组合视图 + 补丁读写   │
│   services/skills.rs  新模块: 技能根目录扫描 + managed 技能 CRUD + 导入   │
│   contract.rs / schema.ts  新增快照与事件类型(两侧同步)                  │
└──────────────┬───────────────────────────────┬──────────────────────────┘
       文件层(读/写)                     CLI 层(跑 dsh 子进程)
   ~/.dsh/profiles/<p>/package.json      pnpm dsh plugin --profile <p> add/remove …
   ~/.dsh/profiles/<p>/cordis.patch.yml   pnpm dsh --profile <p> --dump-config
   $DSH_HOME/cordis.patch.yml             (校验 + 取生效配置)
   ~/.dsh/skills / ~/.agents/skills
   ~/.codex/skills ~/.claude/skills ~/.cursor/skills* ~/.config/opencode
   <dsh-plugins>/packages/*/package.json  ← 联动源(dsh-plugins 仓库)
                        │
                运行中的 dsh web（补丁 HMR 自动生效 / skill 目录 watcher 自动刷新）
```

**写路径收敛到两个可信位置**（运行中的 dsh 都会自动接收，无需重启）：

1. 插件启停/配置 → 写 profile 的 `cordis.patch.yml`（HMR 热重载）；
2. 技能增删改/导入 → 写 `~/.dsh/skills/<name>/SKILL.md`（user-dsh 根，watcher 自动刷新）。

**联动 dsh-plugins**：启动器不修改 dsh-plugins 代码；只是把 `packages/*` 里的 bundle 包“安装/移除”
到 profile（`dsh plugin add file:<abs>`），安装前在包目录跑 `pnpm install && pnpm run build`。

---

## 4. 子界面 A：插件管理（官方增强版）

### 4.1 页面结构

```
插件（新导航页）
├── 顶部工具条: 当前 profile 选择器 | dsh-plugins 仓库路径(可改) | 刷新 | 搜索
├── 分组标签: 全部 | 官方内置 | dsh-plugins | 用户补丁 | 已停用
├── 插件卡片（每行一个 loader 行 id）
│   ├── 头部: 名称(id) · 来源层徽标(内置/外部包/用户补丁) · 启用状态开关
│   ├── 正文: 说明(来自包 README 或 dump-config 注释) · 管辖范围
│   └── 展开: 配置表单(见 4.4) + 原始 YAML 高级模式 + 危险操作(停用/移除)
└── 侧栏/抽屉: dsh-plugins 可安装包列表(联动) + 最近变更日志
```

### 4.2 数据模型（Rust 侧契约，camelCase 序列化）

```ts
interface ProfileSummary { name: string; bundles: string[]; deps: Record<string,string>; patchOk: boolean }
interface PluginRow {                       // 组合后的一个 loader 行
  id: string; module: string;               // name 字段(包导出名)
  layer: 'bundle' | 'profile-patch' | 'home-patch';
  layerLabel: string;                       // 如 "@deepseek-ai/dsh-base" / "~/.dsh/profiles/web/cordis.patch.yml"
  enabled: boolean;                         // 有无 disabled 生效
  config: Record<string, unknown>;          // 组合后的 config(来自 dump-config；!!js 原样透出)
  configSource: 'dump' | 'raw-yaml';        // !!js 存在时为 raw-yaml
  editable: boolean;                        // 用户补丁层可编辑;bundle 层可经覆盖编辑(整行重述)
}
interface DshPluginPackage {                // dsh-plugins 仓库里的包
  dir: string; absDir: string; name: string; version: string;
  description: string; isBundle: boolean; patchFile?: string;
  installedIn: string[];                    // 已安装到的 profile
}
interface PluginsSnapshot { profiles: ProfileSummary[]; rows: PluginRow[]; packages: DshPluginPackage[] }
```

### 4.3 新增 Rust 命令（commands.rs + services/）

| 命令 | 输入 | 输出 | 说明 |
|---|---|---|---|
| `plugins_get_snapshot(profile?)` | profile 名(缺省取设置) | `PluginsSnapshot` | 组合视图：profile bundles + profile patch + home patch + dump-config 交叉；扫描 dsh-plugins 包 |
| `plugins_set_enabled(profile, id, enabled)` | 行 id | 补丁摘要 | 写 `- id: <id>\n  disabled: true/false` 到 profile patch（`false` 时移除该字段） |
| `plugins_save_config(profile, id, config)` | 整行 config | 补丁摘要 | 按“整行替换”语义重写该行 `config`（UI 负责给出全量键，见 4.4） |
| `plugins_reset_row(profile, id)` | 行 id | 补丁摘要 | 删除 profile patch 中该 id 的条目，回落到 bundle 默认 |
| `plugins_validate_patch(profile)` | — | ok/错误详情 | 写入后跑 `dsh --profile <p> --dump-config` 校验（失败提示回滚按钮） |
| `plugins_install_package(profile, absDir)` | 包绝对路径 | 长任务日志 | 包目录 `pnpm install && pnpm run build` → `dsh plugin --profile <p> add file:<abs>` |
| `plugins_remove_package(profile, packageName)` | 包名 | 长任务日志 | `dsh plugin --profile <p> remove <name>`（同时移除 bundle 层与依赖） |
| `plugins_open_in_explorer(absDir)` | — | — | 打开 dsh-plugins 包目录 |
| `dshctl_dump_config(profile)` | — | 原始文本 | 供“预览补丁效果”与校验用 |

实现要点：

- 新增 `services/dshctl.rs`：封装 `pnpm dsh …` 子进程（复用 `services/runtime.rs` 的 pnpm/node 解析与
  PATH 注入，cwd = repo_path），stdout/stderr 接进 log_hub，支持取消（复用 ops.rs 的 CancellationToken）；
- 新增 `services/plugins.rs`：读 `~/.dsh/profiles/<p>/package.json`（bundles/deps）、`cordis.patch.yml`、
  `$DSH_HOME/cordis.patch.yml`；写补丁时**先备份**（`cordis.patch.yml.bak-<ts>`）再原子写；
- dsh-plugins 扫描：`<dshPluginsPath>/packages/*/package.json`，筛 `dsh.bundle` 声明；路径来源：
  设置项 → 自动探测（解析各 profile deps 里 `file:*/dsh-plugins/packages/*`）→ 手工指定；
- bundle 层可编辑性：bundle 行可“覆盖”（在 profile patch 里整行重述 config），卡上明确提示
  “覆盖会替换该行全部配置，且上游更新不改变你的覆盖”。

### 4.4 卡片配置表单

- **优先：由生效配置生成的通用表单**。对 `dump-config` 给出的行 `config` 按标量类型渲染：
  字符串/数字/布尔输入、字符串数组（多行）、嵌套对象（折叠）；未知结构回退“原始 YAML 编辑器”。
- 因为 patch 是**整行替换**语义，UI 在用户改任一字段后，把该行**全量** config（含未改字段）写入补丁，
  并在卡片内提示“已固化整行配置（非深合并）”。
- `!!js` 表达式（如 `port: !!js ctx.… ?? 8080`）：dump 不求值，原样展示；该行标记为“高级 YAML”，
  不做表单化，只提供 YAML 编辑器，避免误伤表达式。
- 保存流程：写补丁 → 备份 → `plugins_validate_patch` → 成功提示“运行中的 dsh web 已热重载”；
  失败则回滚备份并报错。密钥字段永不回显明文（dsh 侧 secret 角色字段本来就不进 config）。

### 4.5 与 dsh-plugins 联动流程（端到端示例）

```text
用户点「从 dsh-plugins 安装」→ 选择 packages/web-search-tavily
  1) 包目录 pnpm install && pnpm run build        (产物 lib/)
  2) pnpm dsh plugin --profile web add file:<abs>  (pnpm add + reconcile bundles)
  3) 刷新插件列表 → 新卡片出现于「dsh-plugins」分组,来源层 = 包名
  4) 用户按需在卡上补 config(写 profile patch)→ dsh web HMR 生效
  5) 点「移除」→ dsh plugin --profile web remove @dsh-plugins/web-search-tavily
```

### 4.6 里程碑（插件子界面）

- **P-A1**：`services/dshctl.rs` + `plugins_get_snapshot`（组合视图）+ 只读卡片列表（含来源徽标）。
- **P-A2**：启停 + 配置保存/重置 + dump-config 校验 + 备份回滚；验证运行中 dsh web HMR 生效。
- **P-A3**：dsh-plugins 联动（扫描、安装、移除）+ 设置项；Windows/macOS 双端验证 `file:` 安装。
- **P-A4**（可选增强）：新插件暴露 HTTP 后，轮询“运行中实时行状态（active/failed/unloading）”叠加到卡片。

---

## 5. 子界面 B：技能管理

### 5.1 页面结构

```
技能（新导航页）
├── 顶部工具条: 重新扫描 | 搜索 | 全部展开/收起
├── 分组一「已管理 · ~/.dsh/skills」: 本启动器直接管理的技能
│   ├── 每张卡: 名称 · 描述 · whenToUse · 调用策略徽标 · 编辑/删除
│   └── 工具栏: 新建技能(名称/描述/正文) · 从外部导入
├── 分组二「发现的外部技能」(按工具分组: Codex / Claude Code / Cursor / OpenCode / Agents)
│   ├── 每张卡: 名称 · 描述 · 来源路径 · 预览正文 · 【导入到 dsh】· 标记“dsh 是否已可调用”
│   └── 未安装提示条: “安装 skill-external-roots 插件或添加 customSkillDirs 即可让模型使用”
└── 分组三「项目技能」(可选): 以 repo_path 为 cwd 的 .dsh/skills 与 .agents/skills(只读展示)
```

### 5.2 数据模型

```ts
interface SkillSummary {
  name: string; description: string; whenToUse?: string;
  modelInvocable: boolean; userInvocable: boolean;
  source: 'managed' | 'codex' | 'claude' | 'cursor' | 'opencode' | 'agents' | 'project';
  dir: string; path: string; sizeBytes: number; hasScripts: boolean; // 目录包含 scripts/references 等
}
interface SkillRoot { key: string; label: string; path: string; exists: boolean; managed: boolean }
interface SkillsSnapshot { roots: SkillRoot[]; skills: SkillSummary[]; pluginsInstalled: boolean }
```

### 5.3 新增 Rust 命令

| 命令 | 输入 | 输出 | 说明 |
|---|---|---|---|
| `skills_get_snapshot` | — | `SkillsSnapshot` | 扫描全部根目录（见 5.4），解析 frontmatter，按工具分组 |
| `skills_create(name, description, whenToUse?, body)` | — | SkillSummary | 校验 kebab-case + 名称唯一 → 写 `~/.dsh/skills/<name>/SKILL.md`（自动生成 frontmatter） |
| `skills_update(name, patch)` | 名称/描述/正文 | SkillSummary | 仅限 managed 根 |
| `skills_delete(name)` | — | — | **路径围栏**：仅允许删除 managed 根下的技能；外部技能只读，需先导入 |
| `skills_import(sourcePath, name?)` | 外部技能目录 | SkillSummary | 递归拷贝到 `~/.dsh/skills/<name>/`（SKILL.md + scripts/references 等），校验 frontmatter |
| `skills_preview(sourcePath)` | — | 正文文本 | 预览 SKILL.md 内容 |
| 事件 `skills_changed` | — | 通知前端刷新 | 每次写操作后广播 |

### 5.4 扫描根目录（默认映射，均可在设置中增删）

| key | 默认路径 | 说明 |
|---|---|---|
| managed | `$DSH_HOME/skills` | 不存在则提示可一键创建（user-dsh 根，dsh 原生扫描） |
| codex | `~/.codex/skills` | Codex 技能 |
| claude | `~/.claude/skills` | Claude Code 技能 |
| cursor | `~/.cursor/skills`、`~/.cursor/skills-*` | Cursor 无标准全局路径，做通配 + 可手工添加 |
| opencode | `~/.config/opencode/skills` | 候选目录（本机暂不存在） |
| agents | `~/.agents/skills` | dsh 原生已扫（user-agents 根），这里只做展示与导入 |
| project | `<repo_path>/.dsh/skills`、`<repo_path>/.agents/skills` | 以仓库为 cwd 的项目根（只读展示） |

扫描解析规则与 dsh `skill-filesystem` 保持一致：kebab-case 目录包 `<name>/SKILL.md` 或扁平 `<name>.md`；
frontmatter 需含 `name` + `description`；解析 `whenToUse` / `disable-model-invocation` / `user-invocable`；
外部工具的额外键（如 Cursor 的 `environments`、Codex 的 `license`/`allowed-tools`）忽略并记录日志。

### 5.5 “让模型用上”的两条生效路径（简单优先）

1. **零代码路径（推荐 v1）**：技能子界面提供“一键启用”按钮，把外部根目录写进 profile patch 的
   `skill-filesystem.customSkillDirs`（id-targeted 补丁，需整行重述该行既有键）→ 运行中 dsh HMR 生效，
   模型即可调用。完全复用内置插件，不装新东西。
2. **新插件路径（配套方案）**：安装 `@dsh-plugins/skill-external-roots` 后，外部根目录以独立 provider
   （source=`external`，rank 350）挂入 `ctx.skills`，模型侧目录即时可见；插件自身提供默认根映射与开关，
   与启动器的“发现”共用同一份根列表。插件开发方案见
   [dsh-plugins/packages/skill-external-roots/docs/development-plan.md](../../../dsh-plugins/packages/skill-external-roots/docs/development-plan.md)。

> 说明：技能子界面的“扫描/导入/管理”能力完全在启动器内（Rust 文件层），**不依赖**新插件；
> 新插件解决的是“让已发现的外部技能在模型侧可调用”的运行时问题。两条路径可并存。

### 5.6 安全与边界

- 写操作只允许落在 `~/.dsh/skills`（managed 根）内；`skills_delete` 对任何外部路径一律拒绝；
- `skills_create` 严格校验 kebab-case 与 frontmatter 必填项，避免产出 dsh 无法加载的技能文件；
- 导入前校验源 frontmatter 合法、目标名未冲突（冲突则提示改名或覆盖）；
- 扫描不跟随符号链接出目录（避免读入敏感路径）；预览正文限制大小（如 256 KB）。

### 5.7 里程碑（技能子界面）

- **P-B1**：根目录扫描 + 解析 + 分组展示（managed/外部/项目）；设置项可增删自定义根目录。
- **P-B2**：managed 技能 CRUD（新建/编辑/删除）+ frontmatter 校验 + `skills_changed` 事件。
- **P-B3**：外部技能预览 + 导入（递归拷贝）+ “一键启用”写 `customSkillDirs` 补丁并验证 HMR。
- **P-B4**（可选增强）：与 `skill-external-roots` 插件联动展示“模型侧已可调用”状态；项目根随 cwd 变化刷新。

---

## 6. 设置项变更（SettingsSnapshot）

新增（均在“设置”页有入口，写入 `config::load/save` 同机制）：

```ts
profileName: string            // 默认 'web'(对齐 dsh web 别名);插件子界面的目标 profile
dshPluginsPath: string         // dsh-plugins 仓库根;空 = 自动探测 profile deps 里的 file: 链接
externalSkillRoots: string[]   // 技能扫描的自定义根目录(默认内置映射之外追加)
skillManagedRoot: string       // 默认 $DSH_HOME/skills
```

---

## 7. 实施顺序与里程碑总览

| 里程碑 | 内容 | 依赖 | 验收口径 |
|---|---|---|---|
| **M1** | `services/dshctl.rs`（CLI 封装）+ `services/plugins.rs`（组合视图）+ 插件只读列表页 | 现有 runtime/supervisor/log_hub | 能列出 profile 全部行与来源层；dump-config 校验通过 |
| **M2** | 插件启停/配置/重置/校验/回滚 + HMR 生效验证 | M1 | 改补丁后运行中 dsh web 无重启即变；备份可回滚 |
| **M3** | dsh-plugins 联动（扫描/构建/安装/移除） | M1 | 从 dsh-plugins 安装 web-search-tavily 全流程可用 |
| **M4** | 技能子界面：扫描/展示/CRUD/导入/一键启用 | 现有 fs 能力 | 本机 codex/claude/cursor/agents 技能可见并可导入；`~/.dsh/skills` 增删后模型侧目录刷新 |
| **M5** | 新插件 `skill-external-roots`（并行，见 dsh-plugins 方案） | — | 安装后外部技能在 dsh web 技能面板可见可调用 |

建议顺序：M1 → M2（插件主流程）→ M4（技能主流程，纯文件层）→ M3 / M5（联动与新插件可并行）。

---

## 8. 风险与对策

| 风险 | 影响 | 对策 |
|---|---|---|
| patch“整行替换”语义下 UI 固化全量 config，可能覆盖其他来源的键 | 配置丢失 | 卡片展示合并后全量键；保存前 diff 预览；备份 + dump-config 校验 + 一键回滚 |
| `!!js` 表达式被 dump 原样透出、表单误写 | 配置损坏 | 检测 `!!js` 后该行锁定为 YAML 高级模式，禁止表单化 |
| `file:` 安装需要包已构建 `lib/`；未构建则加载失败 | 安装后不生效 | 安装前强制 `pnpm install && pnpm run build`；失败中断并提示 |
| profile 补丁写坏导致 `dsh web` 启动失败 | 服务不可用 | 每次写入前备份；`--dump-config` 预校验；校验失败自动回滚并报警 |
| 外部技能 frontmatter 五花八门（缺 description、非法名称） | 扫描噪音 | 严格解析 + 日志点名跳过原因；UI 显示“N 个被跳过” |
| `~/.cursor/skills-*` 通配引入无关目录 | 误扫描 | 通配仅展示候选，默认不写入任何配置；设置里可精确增删 |
| Rust 新增 YAML 解析依赖的 Windows 交叉编译 | 构建失败 | 选纯 Rust 无原生依赖的 crate（如 yaml-rust2 / serde_yml），在 CI 双端验证 |
| `dsh` CLI 在托管 Node 下行为差异（pnpm 版本等） | 子命令异常 | 复用 supervisor 已注入的 PATH 与 pnpm 解析；长任务日志落 log_hub 便于排障 |

---

## 9. 测试计划

- **Rust 单测/集成**（`src-tauri/tests/`）：补丁读写与备份回滚、组合视图（构造 fixture profile）、
  dump-config 校验、技能 frontmatter 解析（含 codex/claude/cursor 样例）、kebab 校验、路径围栏拒绝用例；
- **UI 测试**（`src-ui/src/test/`，vitest + testing-library）：插件卡片启停/配置表单/YAML 高级模式、
  技能列表分组/新建/导入对话框、错误与回滚提示；
- **端到端手工**：真实 profile 上完成「安装 dsh-plugins 包 → 配置 → 运行中 dsh web 热重载」、
  「导入 codex 技能 → 模型侧可见可调用」全流程；macOS + Windows 双端冒烟；
- **回归**：现有 dashboard/repo/env/logs/settings 测试保持绿；`pnpm test:ui`、`pnpm typecheck`、cargo test 全绿。

---

## 11. 实现偏差与补充记录（2026 实现会话回写）

M1–M4 已按本方案落地（新增 `src-tauri/src/services/{dshctl,plugins,skills}.rs`、命令 `plugins_*`/`skills_*`、
UI 子界面 `src-ui/src/components/{plugins,skills}/`、设置「插件与技能」页）。以下为与原稿的偏差与补充：

### 11.1 契约字段补充（§4.2 / §5.2 模型之外的实现字段）

- `PluginRow` 增加 `inUserPatch: bool`（该行是否已有用户 profile patch 条目，决定「重置」按钮可用性）与
  `rawBlock: string`（整行原始 YAML 文本，原始 YAML 编辑的起点与预览用）；`config` 在含 `!!js` 时为 `null`。
- `PluginsSnapshot` 增加 `profile: string | null`（当前生效 profile）与 `dumpError: string | null`
  （dump-config 失败诊断；此时 `rows` 为空，UI 展示警示条而非硬错误）。
- `SkillRoot` 增加 `enabled: bool`（该根是否已写进目标 profile 的 `skill-filesystem.customSkillDirs`，
  即「一键启用」状态；按 user patch + home patch 比对，不依赖 dump）。
- `SkillsSnapshot` 增加 `skipped: string[]`（被跳过条目点名，UI 提示「N 个被跳过」，悬停可看原因）。
- `SkillSummary` 来源枚举新增 `custom`（设置 `externalSkillRoots` 追加的自定义根）。

### 11.2 命令与长任务

- `OperationKind` 新增 `plugin_install` / `plugin_remove`（exclusive-write），插件安装/移除走
  `ops.rs` 取消令牌 + `log_hub` 落日志（不阻塞主线程），UI 长任务横幅实时展示阶段。
- 写操作命令为同步 IPC（`plugins_set_enabled` 等）：内部 `dump-config` 校验约 1–3 s，与既有
  `inspect_environment` 同模式，未引入异步线程。

### 11.3 YAML 库选型（§8 风险表）

选用 **`serde_yaml_ng` 0.10**(serde_yaml 的维护分支,底层 unsafe-libyaml 纯 Rust,无原生链接,
`x86_64-pc-windows-gnu` 交叉编译验证通过)替代 `yaml-rust2` / `serde_yml`——`serde_yml` 已弃用
(0.0.13 为兼容 shim);原稿"如 yaml-rust2 / serde_yml"为示例性表述。`!!js` 行不做 YAML 解析
(行级文本拆分 + `!!js` 检测后锁定 raw-yaml)。

### 11.4 补丁空文件语义（dsh 侧约束,写入器适配）

实测 `dsh --profile <p> --dump-config` 要求补丁文件顶层是 **YAML 数组**;dsh 自带模板的空补丁
(仅注释)会直接报 `must be a top-level YAML array of loader patch entries`。因此写入器:
无条目时显式输出 `[]`(拆分器识别顶层 `[]` 为 `emptyArray` 标记,加条目时自动消失)。

### 11.5 一键启用的启用语义

`skills_enable_root` 在 `skill-filesystem` 行被上层(bundle)停用时,会一并写**无 `disabled` 字段**的
整行覆盖(强制启用)——否则 `customSkillDirs` 写了也不生效(实测 web profile 中该行被 dsh-web-app
停用)。原始 YAML 编辑器与表单模式均保持「整行替换」语义。

### 11.6 重置范围

`plugins_reset_row` 仅作用于 profile patch 的**顶层 `- id:` 条目**;`- insert:` 块内嵌行不支持
重置(编辑嵌套列表风险高),UI 对这类行隐藏重置按钮(可先「停用」再在原始 YAML 里删)。

### 11.7 说明/管辖范围来源

`PluginRow.description` 仅当行 `module` 匹配 dsh-plugins 扫描包时从其 `package.json` 填充;内置
bundle 行不扫描整个仓库读 README(避免慢),UI 展示模块名与来源层路径兜底。

### 11.8 测试落地

- Rust:`cargo test`(127 单测)含补丁拆分/整行替换/启停覆盖/raw-yaml 往返/层分类/dump 解析、
  技能 frontmatter(codex/claude/cursor 样例)/kebab/围栏/导入/预览;
- 真实流程集成测试 `src-tauri/tests/plugins_skills_flow.rs`(默认 `#[ignore]`,
  `cargo test --test plugins_skills_flow -- --ignored`):隔离临时 DSH_HOME + 真实仓库跑
  dump-config 组合视图、备份→写→校验→非法补丁自动回滚、启停/保存/重置、技能 CRUD 与围栏;
- UI:`pnpm test:ui`(54 例)新增 `plugins.test.tsx` / `skills.test.tsx`(分组/启停开关/表单保存/
  raw-yaml 锁定/新建校验/删除确认/导入/一键启用)。

### 11.9 技能注入控制(0.8.1 追加)

用户反馈:dsh 官方自动发现本机全部技能导致无法精准控制注入。新增「注入控制」闭环
(launcher 0.8.1 + skill-external-roots v0.2,双仓联动):

- **机制**:launcher 写 `$DSH_HOME/skills-control.json`(per-skill 开关),插件 `list()` 时
  mtime 缓存读取并过滤(`roots.<family>=false` 整族不探测,与 Config `enabled` 取与;
  `skills.<name>=false` 按名剔除,与 `exclude` 合并);控制文件 1.5s 轮询 watch →
  `invalidate()` → 运行中 dsh 无需重启即热更新;`list()` 后把过滤后候选原子回写
  `$DSH_HOME/state/skills-active.json`(内容去重)。
- **契约新增**(contract.rs ↔ schema.ts 双向):`ActiveSkill` / `SkillsActiveSnapshot`
  (含 `controlFile`/`controlFileExists`)/ `SkillsControlState` / `SkillToggleResult`。
- **新命令**:`skills_get_active` / `skills_get_control` / `skills_set_injected(name, enabled)`
  (kebab 校验 + 原子写 + 广播 skills-changed)/ `skills_enable_control`(整行重述
  skill-external-roots 行写入 skillControlFile/activeFile,dump-config 校验,!!js 行拒绝)。
- **UI**:技能页拆「已启动 / 外部发现」两个子界面(segmented control);已启动 = 插件回写清单
  (开关关闭立即热更新,约 1-2s 生效);外部发现与已启动**去重**(「已注入 ✓ / 未注入」徽章);
  每卡注入开关;未启用控制时引导一键启用。
- **行为说明**:active 回写是惰性的——dsh 在技能收集(模型查询/技能面板)时触发;新装 v0.2
  后需 dsh 重启或一次交互才会出现首个清单;UI 空态已引导(刷新 + 与 dsh 交互)。
- **实测**:真实 web profile 补丁整行重述 skillControlFile/activeFile + dump-config 校验通过
  (运行中 dsh HMR 拾取,备份保留 `.bak-injectctl-*`)。
- **测试**:Rust +2(控制文件原子写与解析 / active 解析与控制配置检测)、UI +3(已启动默认子界面
  开关关闭 / 外部发现去重徽章与开关 / 未启用引导);插件侧 v0.2 +4(技能/族过滤、active 回写、
  文件变化 invalidate、缺失降级 v1)。

---

## 12. 相关文档

- 配套新插件开发方案：[dsh-plugins/packages/skill-external-roots/docs/development-plan.md](../../../dsh-plugins/packages/skill-external-roots/docs/development-plan.md)
- dsh 官方插件开发：[docs/user/develop/basic/config.zh.md](../../../deepseek-harness/docs/user/develop/basic/config.zh.md)、
  [publish.zh.md](../../../deepseek-harness/docs/user/develop/basic/publish.zh.md)
- dsh 技能子系统：[docs/subsystems/skills.zh.md](../../../deepseek-harness/docs/subsystems/skills.zh.md)
- dsh-plugins 开发规范：[dsh-plugins/AGENTS.md](../../../dsh-plugins/AGENTS.md)
