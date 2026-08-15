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

/** 设置(与 src/config.mjs DEFAULTS 对齐)。 */
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
  | 'clear'
  | 'check-update'
  | 'apply-update'
  | 'quit'

export interface ActionAccepted {
  ok: boolean
  reason?: string
  aborted?: boolean
  already?: boolean
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

/** 事件 payload 名(与 Rust emit 的 event 名对齐)。 */
export const EVENTS = {
  STATE_CHANGED: 'app://state-changed',
  LOG_APPENDED: 'app://log-appended',
} as const
