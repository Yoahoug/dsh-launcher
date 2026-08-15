// dsh-launcher · 状态机
// idle → syncing → installing → building → starting → running → (stopping) → idle
// 任何阶段失败 → failed(带诊断),用户可重试。
import { writeFileSync } from 'node:fs'
import { STATE_FILE } from './config.mjs'
import { LAUNCHER_VERSION } from './config.mjs'
import { log } from './log.mjs'

export const STATES = Object.freeze({
  IDLE: 'idle',
  SYNCING: 'syncing',
  INSTALLING: 'installing',
  BUILDING: 'building',
  STARTING: 'starting',
  RUNNING: 'running',
  STOPPING: 'stopping',
  FAILED: 'failed',
})

/** 中文展示名(控制台文案)。 */
export const STATE_LABEL = Object.freeze({
  idle: '空闲',
  syncing: '同步中',
  installing: '安装依赖',
  building: '构建中',
  starting: '启动中',
  running: '运行中',
  stopping: '停止中',
  failed: '失败',
})

const subscribers = new Set()

export const state = {
  version: LAUNCHER_VERSION,
  state: STATES.IDLE,
  mode: 'none',          // none | normal | dev
  phase: '',             // 进度文案(构建/同步阶段)
  error: null,           // { summary, detail }
  url: null,             // dsh web 就绪 URL
  webPid: null,          // dsh web 进程组 pid
  devPid: null,          // dev:web 进程组 pid
  startedAt: null,       // 本次服务启动时间(ms)
  readyAt: null,
  hmrActive: false,
  repo: {                // 仓库状态快照
    branch: '', head: '', behind: -1, ahead: -1,
    dirty: false, dirtyFiles: 0, syncAt: null, remoteUpToDate: true,
  },
  busy: false,           // 是否有流程进行中
  launcherPid: process.pid,
  update: {              // 内置更新状态
    mode: null, checking: false, available: false,
    version: null, url: null, size: null, notes: null,
    message: null, error: null, installing: false, progress: null,
  },
}

/** 更新状态并广播。partial 直接浅合并。 */
export function setState(partial) {
  Object.assign(state, partial)
  emit()
  return state
}

/** 持久化最小状态(供 launcher 重启后召回「运行中」)。 */
export function persist() {
  try {
    writeFileSync(STATE_FILE, `${JSON.stringify({
      mode: state.mode,
      url: state.url,
      port: state.url ? new URL(state.url).port : null,
      startedAt: state.startedAt,
      readyAt: state.readyAt,
      hmrActive: state.hmrActive,
      state: state.state,
    }, null, 2)}\n`, 'utf8')
  } catch { /* 持久化失败不致命 */ }
}

export function subscribe(fn) {
  subscribers.add(fn)
  return () => subscribers.delete(fn)
}

function emit() {
  for (const fn of subscribers) {
    try { fn(state) } catch { /* 忽略 */ }
  }
}

/** 进入失败态并给出诊断。 */
export function fail(summary, detail = '') {
  log('launcher', `失败:${summary}${detail ? ` — ${detail}` : ''}`, 'err')
  setState({ state: STATES.FAILED, error: { summary, detail }, busy: false, phase: '' })
  persist()
}

/** 阶段推进(写日志 + 更新 phase + 广播)。 */
export function phase(p, note, level = 'info') {
  log('launcher', `${p}${note ? ` · ${note}` : ''}`, level)
  setState({ phase: p })
}
