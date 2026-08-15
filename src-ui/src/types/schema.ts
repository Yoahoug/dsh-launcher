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

/** 托管工具链安装快照。 */
export interface InstallationSnapshot {
  catalogVersion: number
  node: InstalledComponent | null
  git: InstalledComponent | null
  pnpm: InstalledComponent | null
  installedAt: number | null
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

export interface EnvironmentNode {
  current: string
  inRange: boolean
  used: string | null
  usedVersion: string | null
  usedSource: string | null
}

export interface EnvironmentSnapshot {
  repoPath: string
  repoUsable: { ok: boolean; reason?: string }
  distBuilt: boolean | null
  node: EnvironmentNode
  pnpm: string | null
  git: string | null
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

/** 页面名(托盘 app://open-page 事件取值;repo/env 为 UI 内部子界面)。 */
export type PageName = 'dashboard' | 'repo' | 'env' | 'logs' | 'settings' | 'first-run'

/** 事件 payload 名(与 Rust emit 的 event 名对齐)。 */
export const EVENTS = {
  STATE_CHANGED: 'app://state-changed',
  LOG_APPENDED: 'app://log-appended',
  OPEN_PAGE: 'app://open-page',
  PREFERENCES_CHANGED: 'app://preferences-changed',
  PERF_METRICS: 'app://perf-metrics',
  CHAT_STATE: 'app://chat-state',
} as const
