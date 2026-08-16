// dsh-launcher · 前后端共享 schema(Rust ↔ TypeScript payload contract)
// 变更本文件时必须同步更新 src-tauri/src/contract.rs 与对应 contract tests。

/** launcher 状态机(与 src/state.mjs STATES 对齐)。 */
export type LauncherState =
  | 'idle'
  | 'syncing'
  | 'installing'
  | 'building'
  | 'starting'
  | 'running'
  | 'stopping'
  | 'failed'

/** 运行模式:none=未运行 / normal=普通 / dev=开发模式。 */
export type LauncherMode = 'none' | 'normal' | 'dev'

export type LogLevel = 'info' | 'ok' | 'warn' | 'err'

export interface ErrorSummary {
  summary: string
  detail: string
}

export interface RepoSnapshot {
  branch: string
  head: string
  behind: number
  ahead: number
  dirty: boolean
  dirtyFiles: number
  syncAt: number | null
  remoteUpToDate: boolean
}

export interface UpdateSnapshot {
  mode: string | null
  checking: boolean
  available: boolean
  version: string | null
  url: string | null
  size: number | null
  notes: string | null
  message: string | null
  error: string | null
  installing: boolean
  progress: number | null
}

/** 与 src/state.mjs `state` 对象对齐的完整快照。 */
export interface AppSnapshot {
  version: string
  state: LauncherState
  mode: LauncherMode
  phase: string
  error: ErrorSummary | null
  url: string | null
  webPid: number | null
  devPid: number | null
  startedAt: number | null
  readyAt: number | null
  hmrActive: boolean
  repo: RepoSnapshot
  busy: boolean
  launcherPid: number
  update: UpdateSnapshot
  /** 当前长任务(无任务时 null)。UI 只在 status === 'success' 时显示成功。 */
  operation: OperationSnapshot | null
  /** 动作矩阵:被禁用的动作与原因(按钮禁用时展示具体原因)。 */
  disabledActions: DisabledAction[]
}

/** 长任务种类(exclusive-write 分组,同一时间只能运行一个)。 */
export type OperationKind =
  | 'install_node'
  | 'install_git'
  | 'install_pnpm'
  | 'install_toolchain'
  | 'clone_repo'
  | 'full_setup'
  | 'install_deps'
  | 'build'
  | 'update_rebuild'
  | 'rebuild_restart'
  | 'start_web'
  | 'start_dev'
  | 'stop_all'
  | 'self_update'
  | 'plugin_install'
  | 'plugin_remove'

/** 操作状态:只有 success 才是终态成功。 */
export type OperationStatus =
  | 'queued'
  | 'running'
  | 'success'
  | 'failed'
  | 'cancelled'
  | 'interrupted'

export interface OperationSnapshot {
  operationId: number
  kind: OperationKind
  status: OperationStatus
  stage: string
  progress: number | null
  error: string | null
  startedAt: number | null
  finishedAt: number | null
  cancellable: boolean
}

/** 被动作矩阵禁用的动作与原因。 */
export interface DisabledAction {
  action: string
  reason: string
}

/** Clone 弹窗请求(UI → 后端)。 */
export interface CloneRequest {
  url: string
  targetDir: string
  source: string
  branch: string | null
}

/** Clone 弹窗初始数据。 */
export interface CloneDialogData {
  lastGoodUrl: string | null
  defaultTarget: string
  officialUrl: string
}

/** Clone 状态(上次成功地址)。 */
export interface CloneState {
  lastGoodUrl: string | null
}

/** 已安装工具链组件。 */
export interface InstalledComponent {
  version: string
  path: string
  verified: boolean
  source: string
}

/** catalog 当前可安装的托管版本(「可选托管工具链」展示;Windows 才有 MinGit)。 */
export interface OfferedVersions {
  node: string
  git: string | null
  pnpm: string
}

/** 托管工具链安装快照。 */
export interface InstallationSnapshot {
  catalogVersion: number
  node: InstalledComponent | null
  git: InstalledComponent | null
  pnpm: InstalledComponent | null
  installedAt: number | null
  /** catalog 当前提供的托管版本(读取时按当前 catalog 补齐)。 */
  offered: OfferedVersions
}

/** 性能测量点。 */
export interface PerfMark {
  name: string
  /** 相对 process_start 的毫秒数。 */
  ms: number
}

/** chat WebView 状态(app://chat-state 事件 + get_chat_state)。 */
export type ChatStatus = 'closed' | 'starting' | 'checking' | 'loading' | 'ready' | 'error'

export interface ChatStateSnapshot {
  status: ChatStatus
  url: string | null
  error: string | null
}

/** 主窗口内工作区:launcher=启动器 / dsh=DeepSeek 完整工作区(子 WebView)。 */
export type Workspace = 'launcher' | 'dsh'

/** dsh-content 子 WebView 生命周期状态;只有 ready 才可展示工作区。 */
export type DshViewStatus =
  | 'not_created'
  | 'creating'
  | 'loading'
  | 'ready'
  | 'disconnected'
  | 'failed'

/** DeepSeek 工作区/子 WebView 全量快照(app://dsh-view-state + get_dsh_view_state)。 */
export interface DshViewSnapshot {
  workspace: Workspace
  status: DshViewStatus
  url: string | null
  error: string | null
  /** 是否存在「成功后自动进入 DeepSeek」的 pending 意图(accepted ≠ success)。 */
  pendingEnter: boolean
  canBackToLauncher: boolean
  canRetry: boolean
  canReconnect: boolean
}

export interface LogEntry {
  id: number
  ts: number
  src: string
  level: LogLevel
  text: string
}

export interface LogPage {
  logs: LogEntry[]
  sources: string[]
}

/** EngineSettings:引擎行为,由 Node daemon 持久化。 */
export interface SettingsSnapshot {
  repoPath: string
  port: number
  host: string
  dshHome: string
  autostart: boolean
  openBrowser: boolean
  autoUpdateCheck: boolean
  buildArgs: string
  readyTimeoutMs: number
  startTimeoutMs: number
  /** 首次运行是否已处理(跳过或完成);true 时不再展示首次运行向导。 */
  firstRunSkipped: boolean
  /** M5:插件/技能子界面的目标 profile(默认 'web',对齐 dsh web 别名)。 */
  profileName: string
  /** M5:dsh-plugins 仓库根;空 = 自动探测 profile deps 里的 file: 链接。 */
  dshPluginsPath: string
  /** M5:技能扫描的自定义根目录(内置映射之外追加)。 */
  externalSkillRoots: string[]
  /** M5:managed 技能根;空 = $DSH_HOME/skills。 */
  skillManagedRoot: string
}

/** 主题偏好。 */
export type Theme = 'system' | 'light' | 'dark'

/** 关闭窗口行为。 */
export type CloseBehavior = 'tray' | 'quit'

/** DesktopPreferences:桌面行为,由 Rust 持久化(不再写入 Node 配置)。 */
export interface DesktopPreferences {
  theme: Theme
  closeBehavior: CloseBehavior
  launchOnStartup: boolean
  silentStartup: boolean
  showTrayIcon: boolean
  confirmStopAndQuit: boolean
}

/** 桌面信息:偏好 + 首次运行状态。 */
export interface DesktopSnapshot {
  preferences: DesktopPreferences
  firstRunDone: boolean
  version: string
}

/** 工具链来源:系统安装 / Launcher 托管 / 项目本地·Corepack。 */
export type ToolSource = 'system' | 'managed' | 'corepack'

/** 工具链组件检测状态。 */
export type ToolCheck = 'detected' | 'incompatible' | 'missing'

/** 单个工具链组件的运行时快照(当前实际生效;版本/来源/路径/检测状态)。 */
export interface ToolRuntime {
  /** 实际生效版本(v24.9.0 / 11.21.0 / 2.47.0);null = 未安装。 */
  version: string | null
  /** 来源;null = 未安装。 */
  source: ToolSource | null
  /** 实际生效可执行文件绝对路径;null = 未安装。 */
  path: string | null
  /** 检测状态。 */
  status: ToolCheck
  /** 是否经签名 catalog 下载并 SHA-256 校验。仅托管工具可为 true;系统工具恒 false。 */
  verified: boolean
  /** 明确提示/推荐(不兼容或缺失时给出可执行建议)。 */
  hint: string | null
  /** 是否存在可切换的托管版本(catalog 有当前平台条目)。 */
  managedAvailable: boolean
}

export interface EnvironmentSnapshot {
  repoPath: string
  repoUsable: { ok: boolean; reason?: string }
  distBuilt: boolean | null
  /** 当前生效平台(macos / windows / linux;据此决定 MinGit 是否展示)。 */
  platform: string
  node: ToolRuntime
  pnpm: ToolRuntime
  git: ToolRuntime
  warnings: string[]
}

export type ActionName =
  | 'start'
  | 'dev'
  | 'update'
  | 'stop'
  | 'rebuild'
  | 'install-node'
  | 'install-git'
  | 'install-pnpm'
  | 'install-toolchain'
  | 'clone-repo'
  | 'full-setup'
  | 'cancel'
  | 'clear'
  | 'check-update'
  | 'apply-update'
  | 'quit'
  | 'detach'

export interface ActionAccepted {
  ok: boolean
  reason?: string
  aborted?: boolean
  already?: boolean
}

/** 检查更新结果(Rust check_for_update 命令返回)。 */
export interface UpdateCheckResult {
  ok: boolean
  reason?: string | null
  version?: string | null
  error?: string | null
}

/** UI 侧动作(含不经过后端的纯界面动作)。 */
export type UiActionName = ActionName | 'open-dsh' | 'cancel'

export interface UpdateResult {
  ok: boolean
  reason?: string
  version?: string | null
  error?: string | null
  update?: UpdateSnapshot
}

/** 页面名(托盘 app://open-page 事件取值;repo/env/plugins/skills 为 UI 内部子界面)。 */
export type PageName =
  | 'dashboard'
  | 'repo'
  | 'env'
  | 'logs'
  | 'plugins'
  | 'skills'
  | 'settings'
  | 'first-run'

// ── M5:插件管理子界面 ─────────────────────────────────────

/** profile 摘要(插件页 profile 选择器)。 */
export interface ProfileSummary {
  name: string
  /** 有序组合包列表(dsh.profile.bundles)。 */
  bundles: string[]
  /** dependencies:包名 → spec(file:/git:/版本)。 */
  deps: Record<string, string>
  /** profile 的 cordis.patch.yml 是否可读。 */
  patchOk: boolean
}

/** 插件行来源层。 */
export type PluginLayer = 'bundle' | 'profile-patch' | 'home-patch' | 'overlay'

/** config 来源:'dump' = 可表单化;'raw-yaml' = 含 !!js 表达式,锁定原始 YAML。 */
export type ConfigSource = 'dump' | 'raw-yaml'

/** 组合后的一个 loader 行。 */
export interface PluginRow {
  id: string
  /** name 字段(包导出名,如 @deepseek-ai/dsh-llm)。 */
  module: string
  layer: PluginLayer
  /** 来源层展示文本(如 @deepseek-ai/dsh-base / patch 绝对路径)。 */
  layerLabel: string
  /** 该行是否已在用户 profile patch 中存在条目(重置按钮可用性)。 */
  inUserPatch: boolean
  /** 有无 disabled 生效。 */
  enabled: boolean
  /** 组合后的 config(dump-config 不求值 !!js;含 !!js 时为 null)。 */
  config: Record<string, unknown> | null
  configSource: ConfigSource
  /** 原始 YAML 块(整行,从 `- id:` 到该行末尾;raw-yaml 编辑/预览用)。 */
  rawBlock: string
  /** bundle/home-patch 行可经覆盖编辑(整行重述);overlay 行不可编辑。 */
  editable: boolean
  /** 包说明(dsh-plugins 包匹配时来自其 package.json;否则 null)。 */
  description: string | null
}

/** dsh-plugins 仓库里的包。 */
export interface DshPluginPackage {
  dir: string
  absDir: string
  name: string
  version: string
  description: string
  /** 是否声明 dsh.bundle(bundle 安装后自动激活其 patch 层)。 */
  isBundle: boolean
  patchFile: string | null
  /** 已安装到的 profile 列表。 */
  installedIn: string[]
}

/** 补丁写入结果(备份 + dump-config 校验)。 */
export interface PatchWriteResult {
  /** 备份文件名(cordis.patch.yml.bak-<ts>);未发生写动作为 null。 */
  backup: string | null
  ok: boolean
  summary: string
  /** dump-config 校验是否通过(通过即运行中 dsh web 可 HMR 生效)。 */
  validated: boolean
  error: string | null
}

/** 插件组合视图快照。 */
export interface PluginsSnapshot {
  profiles: ProfileSummary[]
  rows: PluginRow[]
  packages: DshPluginPackage[]
  /** 生效的 dsh-plugins 仓库根(设置值或 profile deps 自动探测值)。 */
  pluginsPath?: string | null
  /** 当前生效 profile;null = 不存在/未指定。 */
  profile: string | null
  /** dump-config 失败诊断(此时 rows 为空;UI 展示警示条)。 */
  dumpError: string | null
}

// ── M5:技能管理子界面 ─────────────────────────────────────

/** 技能来源分组。 */
export type SkillSource =
  | 'managed'
  | 'codex'
  | 'claude'
  | 'cursor'
  | 'opencode'
  | 'agents'
  | 'project'
  | 'custom'

/** 单个技能摘要。 */
export interface SkillSummary {
  name: string
  description: string
  whenToUse: string | null
  modelInvocable: boolean
  userInvocable: boolean
  source: SkillSource
  /** 技能所在目录(目录包)或根目录(扁平 md)。 */
  dir: string
  /** SKILL.md / <name>.md 的绝对路径。 */
  path: string
  sizeBytes: number
  /** 目录包含 scripts/references 等附带资源。 */
  hasScripts: boolean
}

/** 扫描根目录描述。 */
export interface SkillRoot {
  key: string
  label: string
  path: string
  exists: boolean
  managed: boolean
  /** 该根是否已写进目标 profile 的 skill-filesystem.customSkillDirs(一键启用状态)。 */
  enabled: boolean
}

/** 技能快照。 */
export interface SkillsSnapshot {
  roots: SkillRoot[]
  skills: SkillSummary[]
  /** 目标 profile 是否已安装 skill-external-roots 插件。 */
  pluginsInstalled: boolean
  /** 被跳过条目与原因(UI 展示"N 个被跳过")。 */
  skipped: string[]
}

/** 运行中 dsh 插件回写的「实际注入」技能清单条目(skills-active.json)。 */
export interface ActiveSkill {
  name: string
  description: string
  whenToUse: string | null
  /** 来源桶(插件固定 'external')。 */
  source: string
  /** 该技能所在根目录(绝对路径)。 */
  root: string
  path: string
  modelInvocable: boolean
  userInvocable: boolean
}

/** 已启动技能清单快照(技能页「已启动」子界面)。 */
export interface SkillsActiveSnapshot {
  file: string
  writtenAt: number | null
  skills: ActiveSkill[]
  /** 读取/解析失败诊断(null = 正常)。 */
  error: string | null
  /** 目标 profile 补丁里 skill-external-roots 行配置的 skillControlFile。 */
  controlFile: string | null
  controlFileExists: boolean
}

/** 注入控制文件状态(启动器写,插件读)。 */
export interface SkillsControlState {
  file: string
  version: number
  roots: Record<string, boolean>
  skills: Record<string, boolean>
}

/** 注入开关写入结果。 */
export interface SkillToggleResult {
  ok: boolean
  summary: string
  enabled: boolean
}

/** 事件 payload 名(与 Rust emit 的 event 名对齐)。 */
export const EVENTS = {
  STATE_CHANGED: 'app://state-changed',
  LOG_APPENDED: 'app://log-appended',
  OPEN_PAGE: 'app://open-page',
  PREFERENCES_CHANGED: 'app://preferences-changed',
  PERF_METRICS: 'app://perf-metrics',
  CHAT_STATE: 'app://chat-state',
  DSH_VIEW_STATE: 'app://dsh-view-state',
  SKILLS_CHANGED: 'app://skills-changed',
} as const
