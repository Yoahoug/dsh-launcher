// dsh-launcher · 浏览器预览用的 mock 实现(仅开发/回归对照,不进 Tauri 运行时)
// 用法:pnpm dev:renderer 后在浏览器打开,用 ?mock=idle|running|dev|failed 切换快照。
import type {
  ActionAccepted,
  ActionName,
  AppSnapshot,
  EnvironmentSnapshot,
  LogPage,
  SettingsSnapshot,
} from '@/types/schema'
import type { DesktopApi } from '@/lib/desktop-api'
import { EVENTS } from '@/types/schema'

const VERSION = '0.2.1'

const baseRepo = {
  branch: 'main',
  head: 'abc1234',
  behind: 0,
  ahead: 0,
  dirty: false,
  dirtyFiles: 0,
  syncAt: Date.now() - 3600_000,
  remoteUpToDate: true,
}

function snapshot(partial: Partial<AppSnapshot>): AppSnapshot {
  return {
    version: VERSION,
    state: 'idle',
    mode: 'none',
    phase: '',
    error: null,
    url: null,
    webPid: null,
    devPid: null,
    startedAt: null,
    readyAt: null,
    hmrActive: false,
    repo: baseRepo,
    busy: false,
    launcherPid: 4242,
    update: { mode: 'git', checking: false, available: false, version: null, url: null, size: null, notes: null, message: null, error: null, installing: false, progress: null },
    ...partial,
  }
}

const SNAPSHOTS: Record<string, AppSnapshot> = {
  idle: snapshot({}),
  running: snapshot({
    state: 'running', mode: 'normal', url: 'http://127.0.0.1:3080/',
    webPid: 88321, startedAt: Date.now() - 600_000, readyAt: Date.now() - 590_000,
  }),
  dev: snapshot({
    state: 'running', mode: 'dev', url: 'http://127.0.0.1:3080/',
    webPid: 88321, devPid: 88345, hmrActive: true,
    startedAt: Date.now() - 300_000, readyAt: Date.now() - 290_000,
  }),
  failed: snapshot({
    state: 'failed',
    error: { summary: 'git 冲突:已报告,未破坏工作区', detail: '冲突文件:src/foo.js、apps/web/src/lib/api.ts' },
  }),
}

function mockSnapshot(): AppSnapshot {
  const q = new URLSearchParams(window.location.search)
  return SNAPSHOTS[q.get('mock') ?? 'idle'] ?? SNAPSHOTS.idle!
}

let current = mockSnapshot()

function patch(next: Partial<AppSnapshot>) {
  current = { ...current, ...next }
  window.dispatchEvent(new CustomEvent('mock:state', { detail: current }))
}

let seq = 0
const logs = [
  { id: ++seq, ts: Date.now(), src: 'launcher', level: 'ok' as const, text: 'dsh-launcher 启动(mock)' },
  { id: ++seq, ts: Date.now() - 1000, src: 'git', level: 'info' as const, text: 'git pull --rebase --autostash → abc1234' },
  { id: ++seq, ts: Date.now() - 2000, src: 'pnpm', level: 'info' as const, text: 'pnpm run build 完成 ✓' },
]

export const mockApi: DesktopApi = {
  getAppSnapshot: async () => current,
  runAction: async (action: ActionName): Promise<ActionAccepted> => {
    if (action === 'start') {
      patch({ state: 'starting', busy: true, mode: 'normal', phase: '启动 dsh web…' })
      setTimeout(() => patch({ state: 'running', busy: false, url: 'http://127.0.0.1:3080/', webPid: 88321, startedAt: Date.now(), readyAt: Date.now() }), 800)
      return { ok: true }
    }
    if (action === 'stop') {
      patch({ state: 'stopping', busy: true })
      setTimeout(() => patch({ state: 'idle', busy: false, mode: 'none', url: null, webPid: null, devPid: null }), 500)
      return { ok: true }
    }
    if (action === 'dev') {
      patch({ state: 'starting', busy: true, mode: 'dev', phase: '启动开发模式…' })
      setTimeout(() => patch({ state: 'running', busy: false, url: 'http://127.0.0.1:3080/', webPid: 88321, devPid: 88345, startedAt: Date.now(), readyAt: Date.now() }), 800)
      return { ok: true }
    }
    if (action === 'update') {
      patch({ state: 'syncing', busy: true, phase: '同步远端…' })
      setTimeout(() => {
        patch({ state: 'building', phase: '构建 web 前端…' })
        setTimeout(() => {
          patch({ state: 'starting', phase: '启动 dsh web…' })
          setTimeout(() => patch({ state: 'running', busy: false, url: 'http://127.0.0.1:3080/', webPid: 88321 }), 800)
        }, 900)
      }, 600)
      return { ok: true }
    }
    if (action === 'rebuild') {
      patch({ state: 'building', busy: true, phase: '构建中…' })
      setTimeout(() => patch({ state: 'running', busy: false, url: 'http://127.0.0.1:3080/', webPid: 88321 }), 1500)
      return { ok: true }
    }
    if (action === 'clear') return { ok: true }
    return { ok: true }
  },
  getLogs: async (sinceId = 0): Promise<LogPage> => ({
    logs: logs.filter((l) => l.id > sinceId),
    sources: ['launcher', 'dsh web', 'dev:web', 'git', 'pnpm'],
  }),
  getSettings: async (): Promise<SettingsSnapshot> => ({
    repoPath: '/Users/yoahoug/Desktop/deepseek-harness',
    port: 3080, host: '127.0.0.1', dshHome: '', autostart: false,
    openBrowser: true, autoUpdateCheck: true, buildArgs: '',
    readyTimeoutMs: 120_000, startTimeoutMs: 120_000,
  }),
  saveSettings: async (patch) => ({ ...(await mockApi.getSettings()), ...patch }),
  inspectEnvironment: async (): Promise<EnvironmentSnapshot> => ({
    repoPath: '/Users/yoahoug/Desktop/deepseek-harness',
    repoUsable: { ok: true },
    distBuilt: true,
    node: { current: 'v24.19.0', inRange: true, used: null, usedVersion: null, usedSource: null },
    pnpm: '10.0.0', git: 'git version 2.47.0', warnings: [],
  }),
  checkForUpdate: async () => current.update,
  applyUpdate: async () => ({ ok: true }),
  openDsh: async () => {},
  openRepoDirectory: async () => {},
  openLogDirectory: async () => {},
  onStateChanged: async (cb) => {
    const h = (e: Event) => cb((e as CustomEvent<AppSnapshot>).detail)
    window.addEventListener('mock:state', h)
    return () => window.removeEventListener('mock:state', h)
  },
  onLogAppended: async (cb) => {
    const h = (e: Event) => cb((e as CustomEvent).detail as LogPage['logs'][number])
    window.addEventListener('mock:log', h)
    return () => window.removeEventListener('mock:log', h)
  },
}

export { EVENTS }
