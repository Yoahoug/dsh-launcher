// dsh-launcher · 浏览器预览用的 mock 实现(仅开发/回归对照,不进 Tauri 运行时)
// 用法:pnpm dev:renderer 后在浏览器打开,用 ?mock=idle|running|dev|failed 切换快照;
// ?first-run=1 进入首次运行流程;?silent=1 无日志。
import type {
  ActionAccepted,
  ActionName,
  AppSnapshot,
  DesktopPreferences,
  DesktopSnapshot,
  DshViewSnapshot,
  EnvironmentSnapshot,
  LogEntry,
  LogPage,
  PageName,
  PatchWriteResult,
  PluginsSnapshot,
  SettingsSnapshot,
  SkillSummary,
  SkillsSnapshot,
  Workspace,
} from '@/types/schema'
import type { DesktopApi } from '@/lib/desktop-api'
import { EVENTS } from '@/types/schema'

const VERSION = '0.8.0'

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
    operation: null,
    disabledActions: [],
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
  // 测试环境 teardown 后 window 已销毁:仅更新内存态,不再派发事件(避免未处理异常)
  if (typeof window === 'undefined') return
  window.dispatchEvent(new CustomEvent('mock:state', { detail: current }))
}

let seq = 100
const logs: LogEntry[] = [
  { id: ++seq, ts: Date.now(), src: 'launcher', level: 'ok', text: 'dsh-launcher 启动(mock)' },
  { id: ++seq, ts: Date.now() - 1000, src: 'git', level: 'info', text: 'git pull --rebase --autostash → abc1234' },
  { id: ++seq, ts: Date.now() - 2000, src: 'pnpm', level: 'info', text: 'pnpm run build 完成 ✓' },
]

function logEntry(src: string, level: LogEntry['level'], text: string): LogEntry {
  const e: LogEntry = { id: ++seq, ts: Date.now(), src, level, text }
  window.dispatchEvent(new CustomEvent('mock:log', { detail: e }))
  return e
}

const DEFAULT_PREFS: DesktopPreferences = {
  theme: 'system', closeBehavior: 'tray', launchOnStartup: false,
  silentStartup: false, showTrayIcon: true, confirmStopAndQuit: true,
}

function loadPrefs(): DesktopPreferences {
  try {
    const raw = localStorage.getItem('mock:preferences')
    return raw ? { ...DEFAULT_PREFS, ...JSON.parse(raw) } : { ...DEFAULT_PREFS }
  } catch {
    return { ...DEFAULT_PREFS }
  }
}

let prefs = loadPrefs()

/** mock 首次运行状态:completeFirstRun 后置 true(与 Rust firstRunSkipped 对齐)。 */
let mockFirstRunDone = false

// ── M4.1:DeepSeek 工作区(dsh-content 子 WebView)mock 状态机 ──
// 与 Rust dsh_view 语义对齐:accepted ≠ success;只有服务 running + 视图 ready
// 才自动进入 DeepSeek 工作区;失败/取消留在启动器或错误状态。

// ── 定时器登记(测试用):__resetDshView 会清空未触发的定时器,避免跨测试泄漏 ──
const pendingTimers = new Set<ReturnType<typeof setTimeout>>()
function later(fn: () => void, ms: number) {
  const id = setTimeout(() => {
    pendingTimers.delete(id)
    fn()
  }, ms)
  pendingTimers.add(id)
  return id
}

let dshView: DshViewSnapshot = {
  workspace: 'launcher',
  status: 'not_created',
  url: 'http://127.0.0.1:3080/',
  error: null,
  pendingEnter: false,
  canBackToLauncher: false,
  canRetry: false,
  canReconnect: false,
}

function emitDshView(next: Partial<DshViewSnapshot>) {
  const merged = { ...dshView, ...next }
  dshView = {
    ...merged,
    canBackToLauncher: merged.workspace === 'dsh',
    canRetry: merged.status === 'failed' || merged.status === 'disconnected',
    canReconnect: merged.status === 'disconnected',
  }
  // 测试环境 teardown 后 window 已销毁:仅更新内存态,不再派发事件
  if (typeof window === 'undefined') return
  window.dispatchEvent(new CustomEvent('mock:dshview', { detail: dshView }))
}

/** 模拟服务 running 后进入 dsh 工作区(自动进入;仅在有 pending 意图时)。 */
function maybeAutoEnterDsh() {
  const running = current.state === 'running'
  if (!running || !dshView.pendingEnter) return
  // 模拟:健康检查 + 子 WebView 创建 + 页面加载 → ready
  later(() => {
    if (!dshView.pendingEnter || current.state !== 'running') return
    emitDshView({
      workspace: 'dsh',
      status: 'creating',
      pendingEnter: true,
      error: '正在启动 DSH 服务并加载 DeepSeek 界面,就绪后自动进入…',
    })
    later(() => {
      if (!dshView.pendingEnter || current.state !== 'running') return
      emitDshView({ workspace: 'dsh', status: 'ready', pendingEnter: false, error: null })
    }, 400)
  }, 150)
}

/** vitest 每测后重置工作区状态(避免 workspace/status 跨测试泄漏)。 */
export function __resetDshView() {
  for (const id of pendingTimers) clearTimeout(id)
  pendingTimers.clear()
  current = mockSnapshot()
  mockFirstRunDone = false
  dshView = {
    workspace: 'launcher',
    status: 'not_created',
    url: 'http://127.0.0.1:3080/',
    error: null,
    pendingEnter: false,
    canBackToLauncher: false,
    canRetry: false,
    canReconnect: false,
  }
  resetM5State()
}

/** 模拟启动流程(runAction start/dev/update/rebuild 共用)。 */
function simulateStart(mode: 'normal' | 'dev') {
  emitDshView({ status: 'creating', pendingEnter: true, error: '正在启动 DSH 服务并加载 DeepSeek 界面,就绪后自动进入…' })
  patch({ state: 'starting', busy: true, mode, phase: mode === 'dev' ? '启动开发模式…' : '启动 dsh web…' })
  later(() => {
    patch({
      state: 'running', busy: false, url: 'http://127.0.0.1:3080/',
      webPid: 88321, devPid: mode === 'dev' ? 88345 : null,
      startedAt: Date.now(), readyAt: Date.now(), hmrActive: mode === 'dev',
    })
    maybeAutoEnterDsh()
  }, 800)
}

export const mockApi: DesktopApi = {
  getAppSnapshot: async () => current,
  runAction: async (action: ActionName): Promise<ActionAccepted> => {
    logEntry('launcher', 'info', `动作:${action}(mock)`)
    if (action === 'start') {
      simulateStart('normal')
      return { ok: true }
    }
    if (action === 'stop') {
      patch({ state: 'stopping', busy: true })
      later(() => {
        patch({ state: 'idle', busy: false, mode: 'none', url: null, webPid: null, devPid: null })
        emitDshView({ status: 'disconnected', workspace: 'launcher', pendingEnter: false, error: 'DSH 服务已停止' })
      }, 500)
      return { ok: true }
    }
    if (action === 'dev') {
      simulateStart('dev')
      return { ok: true }
    }
    if (action === 'update') {
      emitDshView({ status: 'creating', pendingEnter: true, error: '更新并启动中…' })
      patch({ state: 'syncing', busy: true, phase: '同步远端…' })
      later(() => {
        patch({ state: 'building', phase: '构建 web 前端…' })
        later(() => {
          patch({ state: 'starting', phase: '启动 dsh web…' })
          later(() => {
            patch({ state: 'running', busy: false, url: 'http://127.0.0.1:3080/', webPid: 88321 })
            maybeAutoEnterDsh()
          }, 800)
        }, 900)
      }, 600)
      return { ok: true }
    }
    if (action === 'rebuild') {
      emitDshView({ status: 'creating', pendingEnter: true, error: '重建并启动中…' })
      patch({ state: 'building', busy: true, phase: '构建中…' })
      later(() => {
        patch({ state: 'running', busy: false, url: 'http://127.0.0.1:3080/', webPid: 88321 })
        maybeAutoEnterDsh()
      }, 1500)
      return { ok: true }
    }
    if (action === 'clear') {
      logs.length = 0
      return { ok: true }
    }
    if (action === 'install-node') {
      logEntry('launcher', 'info', '安装托管 Node 24(模拟)…')
      return { ok: true }
    }
    return { ok: true }
  },
  confirmAndRun: async (action) => {
    const ok = window.confirm(`(mock)确认执行 ${action}?`)
    return ok ? mockApi.runAction(action as ActionName) : { ok: false, aborted: true, reason: '已取消' }
  },
  getLogs: async (sinceId = 0): Promise<LogPage> => ({
    logs: logs.filter((l) => l.id > sinceId),
    sources: ['launcher', 'dsh web', 'dev:web', 'git', 'pnpm'],
  }),
  clearLogs: async () => {
    logs.length = 0
  },
  getSettings: async (): Promise<SettingsSnapshot> => ({
    repoPath: '/Users/yoahoug/Desktop/deepseek-harness',
    port: 3080, host: '127.0.0.1', dshHome: '', autostart: false,
    openBrowser: true, autoUpdateCheck: true, buildArgs: '',
    readyTimeoutMs: 120_000, startTimeoutMs: 120_000,
    firstRunSkipped: false,
    profileName: 'web',
    dshPluginsPath: '/Users/yoahoug/Desktop/dsh-plugins',
    externalSkillRoots: [],
    skillManagedRoot: '',
  }),
  saveSettings: async (patch) => ({ ...(await mockApi.getSettings()), ...patch }),
  inspectEnvironment: async (_force?: boolean): Promise<EnvironmentSnapshot> => ({
    repoPath: '/Users/yoahoug/Desktop/deepseek-harness',
    repoUsable: { ok: true },
    distBuilt: true,
    platform: 'macos',
    // 系统工具齐全(自检通过),未启用托管 → 页面必须展示系统版本而非“未安装”
    node: {
      version: 'v24.19.0', source: 'system', path: '/usr/local/bin/node',
      status: 'detected', verified: false, hint: null, managedAvailable: true,
    },
    pnpm: {
      version: '11.7.0', source: 'system', path: '/Users/you/Library/pnpm/pnpm',
      status: 'detected', verified: false, hint: null, managedAvailable: true,
    },
    git: {
      version: '2.47.0', source: 'system', path: '/usr/bin/git',
      status: 'detected', verified: false, hint: null, managedAvailable: false,
    },
    warnings: [],
  }),
  getDesktopSnapshot: async (): Promise<DesktopSnapshot> => {
    const q = new URLSearchParams(window.location.search)
    const firstRun = q.get('first-run') === '1'
    // ?theme=dark|light|system 可临时覆盖主题(浏览器预览用)
    const themeOverride = q.get('theme')
    const preferences = themeOverride ? { ...prefs, theme: themeOverride as DesktopPreferences['theme'] } : { ...prefs }
    return { preferences, firstRunDone: mockFirstRunDone || !firstRun, version: VERSION }
  },
  completeFirstRun: async (_skip, _repoPath): Promise<DesktopSnapshot> => {
    mockFirstRunDone = true
    // 广播 state-changed 让 useDesktopSnapshot 立即刷新,退出向导(与 Rust 行为一致)
    if (typeof window !== 'undefined') {
      window.dispatchEvent(new CustomEvent('mock:state', { detail: current }))
    }
    return { preferences: { ...prefs }, firstRunDone: true, version: VERSION }
  },
  setTopbarHidden: async () => {},
  getCursorPosition: async () => [400, 600],
  savePreferences: async (p: DesktopPreferences) => {
    prefs = { ...p }
    try { localStorage.setItem('mock:preferences', JSON.stringify(prefs)) } catch { /* ignore */ }
    window.dispatchEvent(new CustomEvent('mock:prefs', { detail: prefs }))
    return { ...prefs }
  },
  checkForUpdate: async () => ({ ok: true, reason: '已是最新版本(mock)', version: null, error: null }),
  applyUpdate: async () => ({ ok: true }),
  openDsh: async () => {},
  openChat: async () => ({ status: 'ready', url: 'http://127.0.0.1:3080/', error: null }),
  closeChat: async () => {},
  getChatState: async () => ({ status: 'ready', url: 'http://127.0.0.1:3080/', error: null }),
  // M4.1:DeepSeek 工作区(mock 状态机)
  openDshWorkspace: async (): Promise<DshViewSnapshot> => {
    // 幂等:已就绪直接返回
    if (dshView.workspace === 'dsh' && dshView.status === 'ready') {
      return { ...dshView }
    }
    // 视图已就绪但工作区在启动器:直接切回(会话保持,不销毁不刷新)
    if (dshView.status === 'ready') {
      emitDshView({ workspace: 'dsh', status: 'ready', pendingEnter: false, error: null })
      return { ...dshView }
    }
    emitDshView({ workspace: 'dsh', status: 'creating', pendingEnter: true, error: '正在启动 DSH 服务并加载 DeepSeek 界面,就绪后自动进入…' })
    // 测试钩子:?dsh-fail=1 → 启动失败(accepted ≠ success,进入错误状态)
    if (new URLSearchParams(window.location.search).get('dsh-fail') === '1') {
      later(() => emitDshView({ workspace: 'dsh', status: 'failed', pendingEnter: false, error: 'DSH 服务启动失败(模拟)' }), 200)
      return { ...dshView }
    }
    if (current.state === 'running') {
      // 服务已就绪:模拟健康检查 + 视图加载
      later(() => {
        emitDshView({ workspace: 'dsh', status: 'loading', pendingEnter: true, error: null })
        later(() => emitDshView({ workspace: 'dsh', status: 'ready', pendingEnter: false, error: null }), 400)
      }, 150)
    } else {
      // 服务未就绪:启动服务,成功后自动进入
      simulateStart('normal')
    }
    return { ...dshView }
  },
  backToLauncher: async (): Promise<DshViewSnapshot> => {
    // 返回启动器:隐藏子视图,会话/状态保持(不销毁不刷新)
    emitDshView({ workspace: 'launcher', pendingEnter: false, error: null })
    return { ...dshView }
  },
  retryDshView: async (): Promise<DshViewSnapshot> => {
    emitDshView({ workspace: 'dsh', status: 'creating', pendingEnter: true, error: '正在重新连接 DeepSeek…' })
    if (current.state === 'running') {
      later(() => emitDshView({ workspace: 'dsh', status: 'ready', pendingEnter: false, error: null }), 400)
    } else {
      simulateStart('normal')
    }
    return { ...dshView }
  },
  setWorkspace: async (workspace: Workspace): Promise<DshViewSnapshot> => {
    if (workspace === 'launcher') return mockApi.backToLauncher()
    return mockApi.openDshWorkspace()
  },
  getDshViewState: async (): Promise<DshViewSnapshot> => ({ ...dshView }),
  openRepoDirectory: async () => {},
  openLogDirectory: async () => {},
  pickDirectory: async () => '/Users/yoahoug/Desktop/deepseek-harness',
  quitApp: async () => {},
  openCloneDialog: async () => ({
    lastGoodUrl: 'https://github.com/deepseek-ai/deepseek-harness.git',
    defaultTarget: '/Users/yoahoug/Desktop',
    officialUrl: 'https://github.com/deepseek-ai/deepseek-harness.git',
  }),
  submitCloneRequest: async (req) => {
    logEntry('git', 'info', `克隆请求:${req.url}(mock)`)
    patch({ state: 'installing', busy: true, phase: '克隆中…' })
    later(() => patch({ state: 'idle', busy: false, phase: '' }), 1200)
    return { ok: true }
  },
  getCloneState: async () => ({ lastGoodUrl: 'https://github.com/deepseek-ai/deepseek-harness.git' }),
  getInstallationSnapshot: async () => ({
    catalogVersion: 1,
    node: { version: 'v24.19.0', path: '/mock/node', verified: true, source: 'managed' },
    git: null,
    pnpm: null,
    installedAt: Date.now(),
    offered: { node: 'v24.9.0', git: null, pnpm: '11.7.0' },
  }),
  perfMark: async () => {},
  getPerfMetrics: async () => [{ name: 'process_start', ms: 0 }, { name: 'react_interactive', ms: 312 }],
  onStateChanged: async (cb) => {
    const h = (e: Event) => cb((e as CustomEvent<AppSnapshot>).detail)
    window.addEventListener('mock:state', h)
    return () => window.removeEventListener('mock:state', h)
  },
  onLogAppended: async (cb) => {
    const h = (e: Event) => cb((e as CustomEvent).detail as LogEntry)
    window.addEventListener('mock:log', h)
    return () => window.removeEventListener('mock:log', h)
  },
  onOpenPage: async (cb) => {
    const h = (e: Event) => cb((e as CustomEvent).detail as PageName)
    window.addEventListener('mock:page', h)
    return () => window.removeEventListener('mock:page', h)
  },
  onPreferencesChanged: async (cb) => {
    const h = (e: Event) => cb((e as CustomEvent).detail as DesktopPreferences)
    window.addEventListener('mock:prefs', h)
    return () => window.removeEventListener('mock:prefs', h)
  },
  onPerfMetrics: async (cb) => {
    const h = (e: Event) => cb((e as CustomEvent).detail as never)
    window.addEventListener('mock:perf', h)
    return () => window.removeEventListener('mock:perf', h)
  },
  onDshViewState: async (cb) => {
    const h = (e: Event) => cb((e as CustomEvent<DshViewSnapshot>).detail)
    window.addEventListener('mock:dshview', h)
    return () => window.removeEventListener('mock:dshview', h)
  },
  // ── M5:插件管理(mock 内存态) ───────────────────────────
  pluginsGetSnapshot: async (): Promise<PluginsSnapshot> => ({
    profiles: [
      {
        name: 'web',
        bundles: ['@deepseek-ai/dsh-base', '@deepseek-ai/dsh-web-app'],
        deps: {
          '@dsh-plugins/web-search-tavily': 'file:/Users/you/Desktop/dsh-plugins/packages/web-search-tavily',
          '@dsh-plugins/vision-bridge': 'file:/Users/you/Desktop/dsh-plugins/packages/vision-bridge',
        },
        patchOk: true,
      },
    ],
    rows: mockPluginRows,
    packages: [
      {
        dir: 'web-search-tavily',
        absDir: '/Users/you/Desktop/dsh-plugins/packages/web-search-tavily',
        name: '@dsh-plugins/web-search-tavily',
        version: '0.1.0',
        description: 'Tavily-backed search provider for the DeepSeek Harness web capability seam',
        isBundle: false,
        patchFile: null,
        installedIn: ['web'],
      },
      {
        dir: 'vision-bridge',
        absDir: '/Users/you/Desktop/dsh-plugins/packages/vision-bridge',
        name: '@dsh-plugins/vision-bridge',
        version: '0.1.0',
        description: 'Image understanding for the vision-less DeepSeek route',
        isBundle: false,
        patchFile: null,
        installedIn: ['web'],
      },
    ],
    profile: 'web',
    dumpError: null,
  }),
  pluginsSetEnabled: async (_profile, id, enabled): Promise<PatchWriteResult> => {
    const row = mockPluginRows.find((r) => r.id === id)
    if (row) row.enabled = enabled
    return {
      backup: 'cordis.patch.yml.bak-1',
      ok: true,
      summary: `${id} 已${enabled ? '启用' : '停用'}(mock)`,
      validated: true,
      error: null,
    }
  },
  pluginsSaveConfig: async (_profile, id, config, rawYaml): Promise<PatchWriteResult> => {
    const row = mockPluginRows.find((r) => r.id === id)
    if (row && rawYaml) {
      row.rawBlock = rawYaml
      row.configSource = rawYaml.includes('!!js') ? 'raw-yaml' : 'dump'
    } else if (row) {
      row.config = config
      row.configSource = 'dump'
    }
    return {
      backup: 'cordis.patch.yml.bak-1',
      ok: true,
      summary: `${id} 的 config 已固化整行(mock)`,
      validated: true,
      error: null,
    }
  },
  pluginsResetRow: async (_profile, id): Promise<PatchWriteResult> => ({
    backup: 'cordis.patch.yml.bak-1',
    ok: true,
    summary: `${id} 已重置(mock)`,
    validated: true,
    error: null,
  }),
  pluginsValidatePatch: async (): Promise<PatchWriteResult> => ({
    backup: null,
    ok: true,
    summary: '校验通过(mock)',
    validated: true,
    error: null,
  }),
  dshctlDumpConfig: async () => '# == @deepseek-ai/dsh-base\n- id: web\n  config:\n    searchProvider: tavily\n',
  pluginsOpenInExplorer: async () => {},
  pluginsInstallPackage: async (_profile, absDir): Promise<ActionAccepted> => {
    logEntry('dsh', 'info', `安装插件包:${absDir}(mock)`)
    return { ok: true }
  },
  pluginsRemovePackage: async (_profile, name): Promise<ActionAccepted> => {
    logEntry('dsh', 'info', `移除插件:${name}(mock)`)
    return { ok: true }
  },
  // ── M5:技能管理(mock 内存态) ───────────────────────────
  skillsGetSnapshot: async (): Promise<SkillsSnapshot> => ({
    roots: [
      { key: 'managed', label: '已管理 · $DSH_HOME/skills', path: '~/.dsh/skills', exists: true, managed: true, enabled: false },
      { key: 'codex', label: 'Codex', path: '~/.codex/skills', exists: true, managed: false, enabled: true },
      { key: 'claude', label: 'Claude Code', path: '~/.claude/skills', exists: true, managed: false, enabled: false },
      { key: 'cursor', label: 'Cursor', path: '~/.cursor/skills-cursor', exists: true, managed: false, enabled: false },
    ],
    skills: mockSkills,
    pluginsInstalled: true,
    skipped: ['bad-name:目录名非 kebab-case,跳过'],
  }),
  skillsCreate: async (name, description, whenToUse, body): Promise<SkillSummary> => {
    const s: SkillSummary = {
      name, description, whenToUse: whenToUse ?? null,
      modelInvocable: true, userInvocable: true, source: 'managed',
      dir: `~/.dsh/skills/${name}`, path: `~/.dsh/skills/${name}/SKILL.md`,
      sizeBytes: (body ?? '').length + 40, hasScripts: false,
    }
    mockSkills.push(s)
    window.dispatchEvent(new CustomEvent('mock:skills'))
    return s
  },
  skillsUpdate: async (name, description, whenToUse, body): Promise<SkillSummary> => {
    const s = mockSkills.find((x) => x.name === name)
    if (!s) throw new Error(`技能 ${name} 不存在(mock)`)
    s.description = description
    s.whenToUse = whenToUse ?? null
    if (body !== undefined) s.sizeBytes = body.length + 40
    window.dispatchEvent(new CustomEvent('mock:skills'))
    return { ...s }
  },
  skillsDelete: async (name) => {
    mockSkills = mockSkills.filter((s) => s.name !== name)
    window.dispatchEvent(new CustomEvent('mock:skills'))
  },
  skillsImport: async (sourcePath, name): Promise<SkillSummary> => {
    const s: SkillSummary = {
      name: name ?? 'imported-skill', description: '来自 ' + sourcePath, whenToUse: null,
      modelInvocable: true, userInvocable: true, source: 'managed',
      dir: `~/.dsh/skills/${name ?? 'imported-skill'}`,
      path: `~/.dsh/skills/${name ?? 'imported-skill'}/SKILL.md`,
      sizeBytes: 512, hasScripts: true,
    }
    mockSkills.push(s)
    window.dispatchEvent(new CustomEvent('mock:skills'))
    return s
  },
  skillsPreview: async (sourcePath) => `# 预览(mock)\n\n来源:${sourcePath}`,
  skillsEnableRoot: async (_profile, rootPath): Promise<PatchWriteResult> => ({
    backup: 'cordis.patch.yml.bak-1',
    ok: true,
    summary: `${rootPath} 已写入 skill-filesystem.customSkillDirs(mock)`,
    validated: true,
    error: null,
  }),
  onSkillsChanged: async (cb) => {
    const h = () => cb()
    window.addEventListener('mock:skills', h)
    return () => window.removeEventListener('mock:skills', h)
  },
}

function defaultPluginRows() {
  return [
    {
      id: 'web',
      module: '@deepseek-ai/dsh-web',
      layer: 'profile-patch' as const,
      layerLabel: '~/.dsh/profiles/web/cordis.patch.yml',
      inUserPatch: true,
      enabled: true,
      config: { searchProvider: 'tavily' },
      configSource: 'dump' as const,
      rawBlock: '- id: web\n  config:\n    searchProvider: tavily\n',
      editable: true,
      description: 'Web UI capability seam',
    },
    {
      id: 'session-persistence-jsonl',
      module: '@deepseek-ai/dsh-session-persistence-jsonl',
      layer: 'bundle' as const,
      layerLabel: '@deepseek-ai/dsh-base',
      inUserPatch: false,
      enabled: true,
      config: null,
      configSource: 'raw-yaml' as const,
      rawBlock: "- id: session-persistence-jsonl\n  name: '@deepseek-ai/dsh-session-persistence-jsonl'\n  config:\n    root: !!js dshHomePath('sessions')\n",
      editable: true,
      description: null,
    },
    {
      id: 'web-search-deepseek',
      module: '@deepseek-ai/dsh-web-search-deepseek',
      layer: 'bundle' as const,
      layerLabel: '@deepseek-ai/dsh-base',
      inUserPatch: true,
      enabled: false,
      config: { apiKeyEnv: 'DEEPSEEK_API_KEY' },
      configSource: 'dump' as const,
      rawBlock: '- id: web-search-deepseek\n  disabled: true\n',
      editable: true,
      description: null,
    },
  ] as import('@/types/schema').PluginRow[]
}

function defaultSkills() {
  return [
    {
      name: 'tavily-extract', description: 'Extract clean markdown from URLs via the Tavily CLI.',
      whenToUse: null, modelInvocable: true, userInvocable: true, source: 'claude',
      dir: '~/.claude/skills/tavily-extract', path: '~/.claude/skills/tavily-extract/SKILL.md',
      sizeBytes: 2048, hasScripts: false,
    },
    {
      name: 'win-host', description: 'Windows 算力主机统一资源入口(训练/采集/重任务)。',
      whenToUse: '需要 Win 主机算力时', modelInvocable: true, userInvocable: true, source: 'agents',
      dir: '~/.agents/skills/win-host', path: '~/.agents/skills/win-host/SKILL.md',
      sizeBytes: 4096, hasScripts: true,
    },
    {
      name: 'my-skill', description: '我的示例技能(mock)。',
      whenToUse: null, modelInvocable: true, userInvocable: true, source: 'managed',
      dir: '~/.dsh/skills/my-skill', path: '~/.dsh/skills/my-skill/SKILL.md',
      sizeBytes: 320, hasScripts: false,
    },
  ] as SkillSummary[]
}

let mockPluginRows: import('@/types/schema').PluginRow[] = defaultPluginRows()
let mockSkills: SkillSummary[] = defaultSkills()

/** vitest 每测后重置 M5 mock 内存态。 */
export function resetM5State() {
  mockPluginRows = defaultPluginRows()
  mockSkills = defaultSkills()
}

export { EVENTS }
