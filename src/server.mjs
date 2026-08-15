// dsh-launcher · 服务入口:HTTP 静态 + JSON API + SSE + 动作编排
// 定位铁律:纯启动器。只做 git/pnpm/进程/日志/控制台,不承载任何 dsh 界面;
// 主界面永远是 dsh web(http://127.0.0.1:3080/),就绪后自动打开。
import { createServer } from 'node:http'
import { execFile, spawn } from 'node:child_process'
import { readFileSync, existsSync } from 'node:fs'
import { join, extname, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import {
  PID_FILE, STATE_FILE, WEB_PID_FILE, DEV_PID_FILE,
  LAUNCHER_PORT, LAUNCHER_HOST, LAUNCHER_VERSION,
  loadConfig, saveConfig, ensureDirs, expandPath, repoUsable, probePort,
} from './config.mjs'
import { log, subscribe as subLog, snapshot as logSnapshot, clearRing } from './log.mjs'
import { setState, state, subscribe as subState, STATES, fail, persist } from './state.mjs'
import {
  spawnWeb, spawnDevWeb, killTree, stopAll, children,
  isAlive, readPid, writePid, clearPid,
} from './process.mjs'
import * as repo from './repo.mjs'
import { installIfNeeded, runBuild } from './build.mjs'
import * as updater from './updater.mjs'
import { resolveTools, toolEnv, setDshNodeDir } from './tools.mjs'
import { resolveDshNode, installDshNode, nodeInRange, NODE_RANGE_MSG } from './nodeenv.mjs'

const ROOT = fileURLToPath(new URL('..', import.meta.url))
const PUBLIC = join(ROOT, 'public')
const SCRIPTS = join(ROOT, 'scripts')

// ── 启动自检 ─────────────────────────────────────────────

const envWarnings = []

/** 当前 Node 是否符合 dsh engines(^22.19 || >=24)。 */
function nodeInDshRange() {
  return nodeInRange(process.versions.node)
}

/** 兼容 Node 解析结果(启动时填充):{ path, version, source } | null。 */
let dshNode = null

async function checkNodeVersion() {
  dshNode = await resolveDshNode()
  if (nodeInDshRange()) {
    // 当前 Node 在范围内:直接用,无警告
    setDshNodeDir(null)
  } else if (dshNode) {
    // 当前 Node 不合规,但系统里有兼容 Node:自动选用
    setDshNodeDir(dirname(dshNode.path))
  } else {
    setDshNodeDir(null)
    envWarnings.push(
      `当前 Node ${process.versions.node} 不在 dsh 要求的版本范围(${NODE_RANGE_MSG})内,且未找到系统里已安装的兼容 Node;`
      + `开发模式(dev:web / tsx / tsdown)与「更新并构建 / 重建并重启」将不可用(tsdown 会崩溃)。`
      + `可在控制台「设置 → Node 运行时」一键安装 Node 24 LTS,或终端执行:brew install node@24 / nvm install 24`,
    )
  }
}

async function checkTools() {
  const resolved = resolveTools()
  const env = toolEnv(resolved)
  const pnpm = await new Promise((resolve) => {
    if (!resolved.pnpm) return resolve(null)
    execFile(resolved.pnpm, ['-v'], { timeout: 15000, env }, (err, stdout) => {
      if (err) resolve(null)
      else resolve(stdout.trim())
    })
  })
  const git = await new Promise((resolve) => {
    if (!resolved.git) return resolve(null)
    execFile(resolved.git, ['--version'], { timeout: 10000, env }, (err, stdout) => {
      if (err) resolve(null)
      else resolve(stdout.trim())
    })
  })
  if (!pnpm) envWarnings.push('未找到 pnpm,请先安装(brew install pnpm,或 corepack enable);「启动/开发模式/更新并构建」需要它')
  if (!git) envWarnings.push('未找到 git,「更新并构建」不可用')
  return { pnpm, git, resolved }
}
let tools = { pnpm: null, git: null, resolved: null }

// ── 内置更新(类 cc-switch:检查 GitHub Releases → 下载 → 切换 → 重启) ─────

function setUpdate(patch) {
  setState({ update: { ...state.update, ...patch } })
}

async function runUpdateCheck() {
  if (state.update.checking) return
  setUpdate({ checking: true, error: null })
  const r = await updater.checkForUpdate()
  setUpdate({
    checking: false,
    mode: r.mode,
    available: Boolean(r.available),
    version: r.version ?? null,
    url: r.url ?? null,
    size: r.size ?? null,
    notes: r.notes ?? null,
    message: r.message ?? null,
    error: r.error ?? null,
  })
  if (r.available) {
    log('launcher', `发现新版本 ${r.version}(当前 ${LAUNCHER_VERSION})— 控制台顶部可一键更新`, 'ok')
  } else if (r.message && r.mode !== 'git') {
    log('launcher', `更新检查:${r.message}`)
  }
  return r
}

async function runUpdateApply() {
  if (!state.update.available) return { ok: false, reason: 'no-update' }
  if (state.update.installing) return { ok: false, reason: 'busy' }
  setUpdate({ installing: true, progress: 0, error: null })
  const info = {
    version: state.update.version,
    url: state.update.url,
    size: state.update.size,
    current: LAUNCHER_VERSION,
  }
  const r = await updater.downloadAndApply(info, { onProgress: (p) => setUpdate({ progress: p }) })
  if (!r.ok) {
    setUpdate({ installing: false, error: r.error, message: r.error })
    return { ok: false, error: r.error }
  }
  // 旧进程退出:不触发 SIGTERM 处理器(避免停止托管中的 dsh web),新实例会召回它们
  setTimeout(() => process.exit(0), 300)
  return { ok: true, version: r.version }
}

// ── 工具函数 ─────────────────────────────────────────────

/** 进程命令行列(诊断用,尽力而为)。 */
function psCommand(pid) {
  return new Promise((resolve) => {
    execFile('ps', ['-o', 'command=', '-p', String(pid)], { timeout: 3000 }, (err, stdout) => {
      resolve(err ? '' : stdout.trim())
    })
  })
}

/** 仓库前端 dist 是否已构建(决定能否直接「启动」,无需先「更新并构建」)。 */
function distBuiltCheck(repoPath) {
  if (!repoPath) return null
  try {
    return existsSync(join(repoPath, 'apps', 'web', 'dist', 'index.html'))
  } catch {
    return null
  }
}

/** 端口占用者诊断。 */
async function portHolder(port) {
  try {
    const { stdout } = await new Promise((resolve, reject) => {
      execFile('lsof', ['-nP', '-iTCP:' + String(port), '-sTCP:LISTEN'], { timeout: 5000 }, (err, so) => err ? reject(err) : resolve({ stdout: so }))
    })
    const line = stdout.split(/\r?\n/)[1]
    if (!line) return null
    const parts = line.split(/\s+/)
    return { pid: parts[1], cmd: parts[0] }
  } catch {
    return null
  }
}

/** 打开浏览器。 */
function openBrowser(url) {
  if (process.env.DSH_NO_AUTOOPEN) {
    log('launcher', `(DSH_NO_AUTOOPEN 已设置,跳过自动打开)→ ${url}`, 'ok')
    return
  }
  const cmd = process.platform === 'darwin' ? 'open'
    : process.platform === 'win32' ? 'start' : 'xdg-open'
  log('launcher', `自动打开浏览器 → ${url}`, 'ok')
  const cp = spawn(cmd, [url], { stdio: 'ignore', detached: true })
  cp.unref()
}

let repoTimer = null

/** 刷新仓库状态快照(状态条用)。 */
async function refreshRepoStatus({ logSync = false } = {}) {
  const cfg = loadConfig()
  const usable = repoUsable(cfg.repoPath)
  if (!usable.ok) {
    setState({ repo: { ...state.repo, branch: '', head: '', dirty: false, behind: -1, ahead: -1 } })
    return
  }
  const st = await repo.repoStatus(cfg.repoPath, { syncAt: state.repo.syncAt })
  if (logSync) log('git', `分支 ${st.branch} @ ${st.head} · 落后 ${st.behind < 0 ? '—' : st.behind} · ${st.dirty ? '工作区有改动' : '工作区干净'}`)
  setState({ repo: st })
}

// ── 动作编排 ─────────────────────────────────────────────

/** 流程纪元:stop 会递增,进行中的流程检测到变化即放弃(防止停止后仍写入失败态)。 */
let epoch = 0
function bumpEpoch() { epoch += 1; return epoch }

function defaultUrl(cfg) {
  return `http://${cfg.host}:${String(cfg.port)}/`
}

/** 检测 dsh web 输出中的 HMR rebuilt 帧(开发模式免刷新热更生效的旁证)。 */
const HMR_RE = /rebuilt|hot.?update|hmr/i
function hmrHint(line) {
  if (HMR_RE.test(line) && !state.hmrActive) {
    setState({ hmrActive: true })
    log('launcher', '检测到 HMR rebuilt 帧 — 客户端插件/前端改动免刷新热更生效', 'ok')
  }
}

/** 拉起 dsh web 并等待就绪行 / 超时 / 退出。 */
function launchWeb(mode) {
  const cfg = loadConfig()
  const myEpoch = epoch
  let timedOut = false

  const timer = setTimeout(() => {
    timedOut = true
    if (state.state === STATES.STARTING && epoch === myEpoch) {
      log('launcher', `就绪等待超时(${Math.round(cfg.readyTimeoutMs / 1000)}s),停止并诊断`, 'warn')
      const child = children.web
      children.web = null
      killTree(child, 'dsh web', WEB_PID_FILE).then(() => {
        if (epoch !== myEpoch) return
        setState({ busy: false })
        fail('启动超时', `未在 ${Math.round(cfg.readyTimeoutMs / 1000)}s 内出现就绪行(dsh web: http://…);查看日志尾部,必要时先「更新并构建」`)
      })
    }
  }, cfg.readyTimeoutMs)

  const child = spawnWeb({
    cwd: cfg.repoPath,
    port: cfg.port,
    host: cfg.host,
    dshHome: cfg.dshHome,
    onLine: (line) => hmrHint(line),
    onReady: (url) => {
      clearTimeout(timer)
      if (epoch !== myEpoch) return
      setState({
        state: STATES.RUNNING, url, webPid: child.pid,
        startedAt: Date.now(), readyAt: Date.now(),
        error: null, busy: false, phase: '就绪',
      })
      log('launcher', `就绪行命中 → ${url} · 状态 → running`, 'ok')
      if (mode === 'dev') {
        log('launcher', '开发模式提示:客户端插件 / 前端改动免刷新热更;lib/ 产物改动需「重建并重启」')
      }
      persist()
      if (cfg.openBrowser) openBrowser(url)
    },
    onExit: (code) => {
      clearTimeout(timer)
      if (timedOut) return // 超时路径已处理
      if (children.web === child) {
        // 托管进程退出:先摘除注册,再决定是否报错
        children.web = null
        clearPid(WEB_PID_FILE)
        if (epoch !== myEpoch) return // 主动停止中,交给 stop 流程收尾
        const wasStarting = state.state === STATES.STARTING
        setState({ state: STATES.IDLE, webPid: null, hmrActive: false, startedAt: null, readyAt: null })
        if (wasStarting && !state.url) {
          setState({ busy: false })
          fail('dsh web 启动失败', `退出码 ${code};若前端 dist 未构建,请先「更新并构建」`)
        } else {
          fail('dsh web 意外退出', `退出码 ${code};查看日志尾部`)
        }
        persist()
      }
    },
  })
  return child
}

/** 停止已托管的 dsh web(不碰 dev:web)。 */
async function stopManagedWeb() {
  const child = children.web
  if (child) {
    children.web = null
    await killTree(child, 'dsh web', WEB_PID_FILE)
    setState({ webPid: null, hmrActive: false, url: null, startedAt: null, readyAt: null })
  }
}

/** 校验仓库可用;不可用则进入 failed 并返回 null。 */
function ensureRepo(cfg) {
  const u = repoUsable(cfg.repoPath)
  if (!u.ok) {
    fail(`仓库不可用:${cfg.repoPath}`, u.reason)
    return false
  }
  return true
}

/**
 * 开发模式 / 构建类动作需要 dsh 兼容 Node(tsx/tsdown 在不兼容版本下崩溃)。
 * 合规 → true;不合规但找到兼容 Node → 已自动选用,true;否则 fail 并返回 false。
 */
function requireDshNode(actionLabel) {
  if (nodeInDshRange() || dshNode) return true
  fail(
    `${actionLabel}需要 Node ${NODE_RANGE_MSG}`,
    `当前 Node ${process.versions.node} 不在 dsh 支持范围,tsx/tsdown 会崩溃(import-without-cache 的 load hook 报错)。`
    + `请在「设置 → Node 运行时」一键安装 Node 24 LTS,或在终端执行:brew install node@24 / nvm install 24`,
  )
  return false
}

/** 一键安装 Node 24 LTS(下载官方二进制到托管目录并自动选用)。 */
async function actionInstallNode() {
  if (state.busy) return { ok: false, reason: 'busy' }
  if (nodeInDshRange() || dshNode) {
    log('launcher', `Node 运行时已就绪(${dshNode ? `自动选用 ${dshNode.version}` : process.versions.node}),无需安装`)
    return { ok: true }
  }
  const myEpoch = epoch
  setState({ busy: true, error: null, state: STATES.STARTING, phase: '安装 Node 24 LTS…' })
  const r = await installDshNode({
    onStage: (p) => { if (epoch === myEpoch) setState({ phase: p }) },
    onLine: (l) => log('launcher', l),
  })
  if (epoch !== myEpoch) return { ok: false, aborted: true }
  if (!r.ok) {
    setState({ busy: false })
    fail('Node 安装失败', r.error)
    return { ok: false, error: r.error }
  }
  dshNode = await resolveDshNode()
  setDshNodeDir(dshNode ? dirname(dshNode.path) : null)
  setState({ busy: false, state: STATES.IDLE, phase: '' })
  log('launcher', `Node ${r.version} 安装完成并已自动选用 → ${r.path}`, 'ok')
  return { ok: true }
}

async function actionStart(mode = 'normal') {
  const cfg = loadConfig()
  if (!ensureRepo(cfg)) return { ok: false }
  if (state.busy) return { ok: false, reason: 'busy' }

  // 已在运行 → 直接召回
  if (state.state === STATES.RUNNING && state.webPid && isAlive(state.webPid)) {
    log('launcher', 'dsh web 已在运行,直接打开主界面')
    openBrowser(state.url || defaultUrl(cfg))
    return { ok: true, already: true }
  }

  // 端口占用检测
  if (await probePort(cfg.port)) {
    const holder = await portHolder(cfg.port)
    fail(
      `端口 ${cfg.port} 已被占用`,
      `${holder ? `占用进程 PID ${holder.pid}(${holder.cmd})` : '占用进程未知'}。请在「设置」中更换 dsh web 端口后重试,或先停止占用进程`,
    )
    return { ok: false, reason: 'port-busy' }
  }

  if (!tools.pnpm) {
    fail('未找到 pnpm', '请先安装 pnpm(brew install pnpm 或 corepack enable),然后重试')
    return { ok: false }
  }

  const myEpoch = epoch
  setState({ busy: true, error: null, mode, state: STATES.STARTING, phase: '启动 dsh web…' })
  persist()
  log('launcher', `状态 → starting · 拉起 dsh web(源码启动,端口 ${cfg.port})`)
  launchWeb(mode)
  return { ok: true }
}

async function actionDev() {
  const cfg = loadConfig()
  if (!ensureRepo(cfg)) return { ok: false }
  if (state.busy) return { ok: false, reason: 'busy' }
  if (!requireDshNode('开发模式')) return { ok: false, reason: 'node-unsupported' }
  if (state.state === STATES.RUNNING && state.webPid && isAlive(state.webPid)) {
    log('launcher', 'dsh web 已在运行;若需要热更,请先「停止」再进入开发模式')
    openBrowser(state.url || defaultUrl(cfg))
    return { ok: true, already: true }
  }
  if (await probePort(cfg.port)) {
    const holder = await portHolder(cfg.port)
    fail(
      `端口 ${cfg.port} 已被占用`,
      `${holder ? `占用进程 PID ${holder.pid}(${holder.cmd})` : '占用进程未知'}。请在「设置」中更换 dsh web 端口后重试`,
    )
    return { ok: false, reason: 'port-busy' }
  }
  if (!tools.pnpm) {
    fail('未找到 pnpm', '请先安装 pnpm,然后重试')
    return { ok: false }
  }

  const myEpoch = epoch
  setState({ busy: true, error: null, mode: 'dev', state: STATES.STARTING, phase: '启动开发模式…' })
  persist()

  // 先拉起 HMR watcher(初始构建耗时长,后台进行),再拉起 dsh web
  const devChild = spawnDevWeb(cfg.repoPath, {
    onExit: (code) => {
      if (state.mode === 'dev' && epoch === myEpoch && code !== null) {
        setState({ devPid: null })
        log('launcher', `dev:web 已退出(码 ${code})— 热更失效,可点「重建并重启」恢复`, 'warn')
      }
    },
  })
  setState({ devPid: devChild.pid })
  log('launcher', '开发模式:dsh web + pnpm run dev:web 同跑(HMR watcher 后台初始化)')
  launchWeb('dev')
  return { ok: true }
}

async function actionUpdate() {
  const cfg = loadConfig()
  if (!ensureRepo(cfg)) return { ok: false }
  if (state.busy) return { ok: false, reason: 'busy' }
  if (!requireDshNode('更新并构建')) return { ok: false, reason: 'node-unsupported' }
  if (!tools.git) { fail('未找到 git', '「更新并构建」需要 git,请先安装'); return { ok: false } }

  const myEpoch = epoch
  const mode = state.mode === 'dev' ? 'dev' : 'normal'
  setState({ busy: true, error: null, mode, state: STATES.SYNCING, phase: '同步远端…' })

  // 1. 同步
  const before = await repo.headShort(cfg.repoPath)
  const sync = await repo.gitSync(cfg.repoPath)
  if (epoch !== myEpoch) return { ok: false, aborted: true }
  if (!sync.ok) {
    if (sync.stage === 'conflict') {
      fail(
        'git 冲突:已报告,未破坏工作区',
        `${sync.error}${sync.conflicts.length ? `\n冲突文件:${sync.conflicts.join('、')}` : ''}`,
      )
    } else if (sync.stage === 'stash') {
      fail('自动暂存本地改动失败', sync.error)
    } else {
      fail(`同步远端失败(${sync.stage})`, sync.error)
    }
    return { ok: false }
  }
  setState({ repo: { ...state.repo, syncAt: Date.now() } })
  await refreshRepoStatus()

  // 2. 依赖安装(lockfile 变化才装)
  setState({ state: STATES.INSTALLING })
  const inst = await installIfNeeded(cfg.repoPath, {
    from: before,
    onStage: (p) => setState({ phase: p }),
  })
  if (epoch !== myEpoch) return { ok: false, aborted: true }
  if (!inst.ok) {
    fail('依赖安装失败', inst.tail.join(' ').slice(0, 300))
    return { ok: false }
  }

  // 3. 构建
  setState({ state: STATES.BUILDING, phase: '构建中…' })
  const b = await runBuild(cfg.repoPath, {
    buildArgs: cfg.buildArgs,
    onStage: (p) => setState({ phase: p }),
  })
  if (epoch !== myEpoch) return { ok: false, aborted: true }
  if (!b.ok) {
    fail(b.error, `退出码 ${b.code};查看日志尾部定位到阶段。修复后重试「更新并构建」`)
    return { ok: false }
  }

  // 4. 重启服务(同模式)
  await stopManagedWeb()
  setState({ state: STATES.STARTING, phase: '启动 dsh web…', mode })
  log('launcher', `更新并构建完成 → 启动 dsh web(模式:${mode === 'dev' ? '开发' : '标准'})`, 'ok')
  launchWeb(mode)
  return { ok: true }
}

async function actionRebuild() {
  const cfg = loadConfig()
  if (!ensureRepo(cfg)) return { ok: false }
  if (state.busy) return { ok: false, reason: 'busy' }
  if (!requireDshNode('重建并重启')) return { ok: false, reason: 'node-unsupported' }
  if (!tools.pnpm) { fail('未找到 pnpm', '请先安装 pnpm,然后重试'); return { ok: false } }

  const myEpoch = epoch
  const mode = state.mode === 'dev' ? 'dev' : 'normal'
  setState({ busy: true, error: null, mode, state: STATES.STOPPING, phase: '停止中…' })
  log('launcher', '重建并重启:停止 → 构建 → 启动')

  // 停掉全部托管进程(含 dev:web)
  await stopAll()
  if (epoch !== myEpoch) return { ok: false, aborted: true }

  setState({ state: STATES.BUILDING, phase: '构建中…' })
  const b = await runBuild(cfg.repoPath, { buildArgs: cfg.buildArgs, onStage: (p) => setState({ phase: p }) })
  if (epoch !== myEpoch) return { ok: false, aborted: true }
  if (!b.ok) {
    fail(b.error, '构建失败,服务已停止。修复后重试「重建并重启」')
    return { ok: false }
  }
  await refreshRepoStatus()

  setState({ state: STATES.STARTING, phase: '启动 dsh web…' })
  launchWeb(mode)
  return { ok: true }
}

async function actionStop() {
  const myEpoch = bumpEpoch()
  setState({ state: STATES.STOPPING, busy: true, phase: '停止中…', error: null })
  await stopAll()
  if (epoch !== myEpoch) return { ok: false } // 并发保护
  setState({
    state: STATES.IDLE, busy: false, mode: 'none', phase: '',
    webPid: null, devPid: null, url: null, hmrActive: false,
    startedAt: null, readyAt: null,
  })
  persist()
  log('launcher', '已停止全部进程(dsh web / dev:web / 流程子进程)', 'ok')
  return { ok: true }
}

/** 退出启动器本身:停止全部托管进程(dsh web / dev:web / 流程)后退出服务。 */
async function quitLauncher(reason) {
  log('launcher', `退出启动器:${reason} — 停止全部托管进程并退出服务`)
  await actionStop()
  // 稍等让 HTTP 响应先返回,再退出(等效优雅关机)
  setTimeout(() => process.exit(0), 200)
}

/** 动作分发。 */
const ACTIONS = {
  start: () => actionStart('normal'),
  dev: () => actionDev(),
  update: () => actionUpdate(),
  stop: () => actionStop(),
  rebuild: () => actionRebuild(),
  'install-node': () => actionInstallNode(),
  clear: () => { clearRing(); return Promise.resolve({ ok: true }) },
  quit: () => quitLauncher('控制台「退出启动器」'),
}

// ── HTTP 服务 ────────────────────────────────────────────

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.svg': 'image/svg+xml',
}

function serveStatic(res, pathname) {
  const file = pathname === '/' ? 'index.html' : pathname.slice(1)
  if (!/^[a-z0-9._-]+$/i.test(file) || !existsSync(join(PUBLIC, file))) {
    res.writeHead(404, { 'Content-Type': 'text/plain; charset=utf-8' })
    res.end('404 Not Found')
    return
  }
  const body = readFileSync(join(PUBLIC, file))
  const type = MIME[extname(file)] || 'application/octet-stream'
  res.writeHead(200, { 'Content-Type': type, 'Cache-Control': 'no-cache' })
  res.end(body)
}

function sendJson(res, obj, status = 200) {
  const body = JSON.stringify(obj)
  res.writeHead(status, { 'Content-Type': 'application/json; charset=utf-8' })
  res.end(body)
}

function readBody(req) {
  return new Promise((resolve) => {
    let data = ''
    req.on('data', (c) => { data += c; if (data.length > 1e6) req.destroy() })
    req.on('end', () => {
      try { resolve(data ? JSON.parse(data) : {}) } catch { resolve(null) }
    })
    req.on('error', () => resolve(null))
  })
}

function sse(res) {
  res.writeHead(200, {
    'Content-Type': 'text/event-stream; charset=utf-8',
    'Cache-Control': 'no-cache, no-transform',
    Connection: 'keep-alive',
    'X-Accel-Buffering': 'no',
  })
  res.write(': connected\n\n')
  const unsubLog = subLog((entry) => {
    res.write(`event: log\ndata: ${JSON.stringify(entry)}\n\n`)
  })
  const unsubState = subState(() => {
    res.write(`event: state\ndata: ${JSON.stringify(state)}\n\n`)
  })
  const keep = setInterval(() => { res.write(': ping\n\n') }, 20000)
  reqCleanup(res, () => { clearInterval(keep); unsubLog(); unsubState() })
}

const reqCleanup = (res, fn) => {
  res.on('close', fn)
}

/** 校验设置补丁,返回 { ok, error?, patch }。 */
function validateConfig(patch) {
  const out = {}
  if ('repoPath' in patch) {
    const p = expandPath(String(patch.repoPath || '').trim())
    out.repoPath = p
    const u = repoUsable(p)
    if (!u.ok) return { ok: false, error: `仓库路径${u.reason}` }
  }
  if ('port' in patch) {
    const n = Number(patch.port)
    if (!Number.isInteger(n) || n < 1 || n > 65535) return { ok: false, error: '端口必须是 1–65535 的数字' }
    out.port = n
  }
  if ('host' in patch) out.host = String(patch.host || '127.0.0.1').trim()
  if ('dshHome' in patch) out.dshHome = String(patch.dshHome || '').trim()
  if ('buildArgs' in patch) out.buildArgs = String(patch.buildArgs || '').trim()
  if ('readyTimeoutMs' in patch) {
    const n = Number(patch.readyTimeoutMs)
    if (Number.isInteger(n) && n >= 5000) out.readyTimeoutMs = n
  }
  if ('openBrowser' in patch) out.openBrowser = Boolean(patch.openBrowser)
  if ('autoUpdateCheck' in patch) out.autoUpdateCheck = Boolean(patch.autoUpdateCheck)
  if ('autostart' in patch) out.autostart = Boolean(patch.autostart)
  return { ok: true, patch: out }
}

/** 开机自启(LaunchAgent)安装/卸载。 */
function applyAutostart(enabled) {
  const script = join(SCRIPTS, enabled ? 'install-launch-agent.sh' : 'uninstall-launch-agent.sh')
  log('launcher', `${enabled ? '安装' : '卸载'}开机自启 LaunchAgent …`)
  const cp = spawn('bash', [script], { stdio: 'ignore', detached: true })
  cp.unref()
  cp.on('error', (err) => log('launcher', `自启脚本执行失败:${err.message}`, 'err'))
}

function handleApi(req, res, pathname) {
  // ── SSE ──
  if (pathname === '/api/events') { sse(res); return true }

  // ── GET ──
  if (req.method === 'GET') {
    if (pathname === '/api/state') { sendJson(res, { ok: true, state }); return true }
    if (pathname === '/api/logs') {
      const since = Number(new URL(req.url, 'http://x').searchParams.get('since')) || 0
      sendJson(res, { ok: true, logs: logSnapshot(since), sources: ['launcher', 'dsh web', 'dev:web', 'git', 'pnpm'] })
      return true
    }
    if (pathname === '/api/config') {
      const cfg = loadConfig()
      sendJson(res, {
        ok: true,
        config: cfg,
        usable: repoUsable(cfg.repoPath),
        distBuilt: distBuiltCheck(cfg.repoPath),
        tools: {
          ...tools,
          node: {
            current: process.versions.node,
            inRange: nodeInDshRange(),
            used: dshNode ? dshNode.path : null,
            usedVersion: dshNode ? dshNode.version : null,
            usedSource: dshNode ? dshNode.source : null,
          },
        },
        warnings: envWarnings,
        launcher: { pid: process.pid, version: LAUNCHER_VERSION, port: LAUNCHER_PORT },
      })
      return true
    }
    if (pathname === '/api/health') { sendJson(res, { ok: true, pid: process.pid, version: LAUNCHER_VERSION }); return true }
    if (pathname === '/api/update') {
      sendJson(res, { ok: true, update: state.update, version: LAUNCHER_VERSION, mode: updater.installMode() })
      return true
    }
    return false
  }

  // ── POST ──
  if (req.method === 'POST') {
    if (pathname === '/api/action') {
      void (async () => {
        const body = await readBody(req)
        if (!body || typeof body.action !== 'string') { sendJson(res, { ok: false, reason: '缺少 action' }, 400); return }
        const fn = ACTIONS[body.action]
        if (!fn) { sendJson(res, { ok: false, reason: `未知动作 ${body.action}` }, 400); return }
        const r = await fn()
        sendJson(res, { ok: r?.ok !== false, reason: r?.reason, aborted: r?.aborted })
      })()
      return true
    }
    if (pathname === '/api/update') {
      void (async () => {
        const body = await readBody(req)
        const action = body?.action
        if (action === 'check') {
          const r = await runUpdateCheck()
          sendJson(res, { ok: true, update: state.update, result: r })
        } else if (action === 'apply') {
          const r = await runUpdateApply()
          sendJson(res, { ok: r.ok !== false, reason: r.reason, error: r.error, version: r.version })
        } else {
          sendJson(res, { ok: false, reason: '未知动作,用 check / apply' }, 400)
        }
      })()
      return true
    }
    if (pathname === '/api/config') {
      void (async () => {
        const body = await readBody(req)
        if (!body) { sendJson(res, { ok: false, reason: '请求体无效' }, 400); return }
        const v = validateConfig(body)
        if (!v.ok) { sendJson(res, { ok: false, reason: v.error }, 400); return }
        try {
          const cfg = saveConfig(v.patch)
          log('launcher', `设置已保存:${JSON.stringify(v.patch)}`)
          if ('autostart' in v.patch) applyAutostart(v.patch.autostart)
          if ('repoPath' in v.patch || 'port' in v.patch) {
            await refreshRepoStatus()
            if (state.state === STATES.RUNNING) {
              log('launcher', '提示:仓库路径 / 端口变更需「重建并重启」后生效', 'warn')
            }
          }
          sendJson(res, { ok: true, config: cfg })
        } catch (err) {
          sendJson(res, { ok: false, reason: err.message }, 500)
        }
      })()
      return true
    }
    return false
  }
  return false
}

// ── 启动 ─────────────────────────────────────────────────

/** 启动时召回先前托管的运行中服务(pid 文件 + 端口 + 命令行校验)。 */
async function reconcile() {
  let saved = null
  try { saved = JSON.parse(readFileSync(STATE_FILE, 'utf8')) } catch { saved = null }
  const webPid = readPid(WEB_PID_FILE)
  const devPid = readPid(DEV_PID_FILE)
  const cfg = loadConfig()

  if (
    saved && saved.state === 'running' && webPid && isAlive(webPid)
    && await probePort(saved.port ?? cfg.port)
  ) {
    const cmd = await psCommand(webPid)
    if (/dsh|bin\.ts|pnpm/.test(cmd)) {
      const mode = saved.mode === 'dev' ? 'dev' : 'normal'
      // 重建可杀句柄(伪对象):按 pid 对进程组 SIGTERM/SIGKILL
      children.web = { pid: webPid }
      if (mode === 'dev' && devPid && isAlive(devPid)) children.dev = { pid: devPid }
      setState({
        state: STATES.RUNNING, mode, url: saved.url,
        webPid, devPid: mode === 'dev' && devPid && isAlive(devPid) ? devPid : null,
        hmrActive: Boolean(saved.hmrActive),
        startedAt: saved.startedAt ?? Date.now(),
        readyAt: saved.readyAt ?? null,
        busy: false, error: null,
      })
      log('launcher', `召回运行中的 dsh web(${saved.url}) — 控制台可随时查看日志 / 停止 / 重启`, 'ok')
      return
    }
    log('launcher', 'pid 文件指向的进程不是 dsh web,忽略召回', 'warn')
  }
  clearPid(WEB_PID_FILE)
  clearPid(DEV_PID_FILE)
  setState({ state: STATES.IDLE, busy: false, error: null })
}

async function main() {
  ensureDirs()
  await checkNodeVersion()
  tools = await checkTools()
  if (nodeInDshRange()) {
    log('launcher', `Node ${process.versions.node} 就绪(当前进程,符合 dsh ${NODE_RANGE_MSG})`, 'ok')
  } else if (dshNode) {
    log('launcher', `当前 Node ${process.versions.node} 不在 dsh 范围(${NODE_RANGE_MSG}),自动选用 Node ${dshNode.version} (${dshNode.source}: ${dshNode.path})`, 'ok')
  }
  if (tools.pnpm) log('launcher', `pnpm ${tools.pnpm} 就绪(${tools.resolved?.pnpm ?? 'PATH'})`)
  if (tools.git) log('launcher', `${tools.git} 就绪(${tools.resolved?.git ?? 'PATH'})`)
  if (envWarnings.length) envWarnings.forEach((w) => log('launcher', w, 'warn'))

  writePid(PID_FILE, process.pid)
  log('launcher', `dsh-launcher v${LAUNCHER_VERSION} 启动(pid ${process.pid})· 控制台 http://${LAUNCHER_HOST}:${LAUNCHER_PORT}/`)

  await reconcile()
  await refreshRepoStatus({ logSync: true })

  // 内置更新:记录安装形态;打包形态且开启自动检查时,启动即查 + 每 6h 复查
  setUpdate({ mode: updater.installMode() })
  if (updater.installMode() === 'git') {
    log('launcher', '运行形态:git 检出 — 代码更新请用「更新并构建」(内置更新仅面向打包安装)')
  } else {
    log('launcher', `运行形态:${updater.installMode()} · 当前版本 v${LAUNCHER_VERSION}`)
  }
  if (loadConfig().autoUpdateCheck) {
    void runUpdateCheck()
    const updTimer = setInterval(() => {
      if (loadConfig().autoUpdateCheck) void runUpdateCheck()
    }, 6 * 3600 * 1000)
    updTimer.unref()
  }

  // 周期刷新仓库状态 + 看门狗(30s)
  repoTimer = setInterval(() => {
    void refreshRepoStatus()
    // 看门狗:召回或正常托管的 dsh web 若意外死亡,给出诊断
    if (state.state === STATES.RUNNING && state.webPid && !isAlive(state.webPid)) {
      children.web = null
      clearPid(WEB_PID_FILE)
      setState({ state: STATES.IDLE, webPid: null, hmrActive: false, startedAt: null, readyAt: null, url: null })
      fail('dsh web 意外退出', '进程已不在(pid 检查失败);查看日志尾部')
      persist()
    }
  }, 30_000)
  repoTimer.unref()

  const server = createServer((req, res) => {
    const pathname = new URL(req.url, `http://${req.headers.host}`).pathname
    if (pathname.startsWith('/api/')) {
      if (!handleApi(req, res, pathname)) sendJson(res, { ok: false, reason: '404' }, 404)
      return
    }
    serveStatic(res, pathname)
  })

  server.on('error', (err) => {
    if (err.code === 'EADDRINUSE') {
      log('launcher', `控制台端口 ${LAUNCHER_PORT} 已被占用 — 已有实例在运行?请在浏览器打开 http://${LAUNCHER_HOST}:${LAUNCHER_PORT}/`, 'err')
    } else {
      log('launcher', `HTTP 服务错误:${err.message}`, 'err')
    }
    process.exit(1)
  })

  // 内置更新重启:旧实例仍在退出中(占着 3090),等它释放(≤5s)再监听
  if (process.env.DSH_LAUNCHER_UPDATED_FROM) {
    log('launcher', `更新重启中(来自 v${process.env.DSH_LAUNCHER_UPDATED_FROM}),等待旧实例释放端口…`)
    for (let i = 0; i < 50; i++) {
      if (!(await probePort(LAUNCHER_PORT))) break
      await new Promise((r) => setTimeout(r, 100))
    }
  }

  server.listen(LAUNCHER_PORT, LAUNCHER_HOST, () => {
    log('launcher', `控制台服务就绪 → http://${LAUNCHER_HOST}:${LAUNCHER_PORT}/`, 'ok')
  })

  // 优雅退出:清理托管进程
  const shutdown = (sig) => {
    log('launcher', `收到 ${sig},停止托管进程并退出`)
    void stopAll().finally(() => process.exit(0))
  }
  process.on('SIGTERM', () => shutdown('SIGTERM'))
  process.on('SIGINT', () => shutdown('SIGINT'))

  // 不闪退:未捕获异常只记录
  process.on('uncaughtException', (err) => log('launcher', `未捕获异常:${err.stack ?? err.message}`, 'err'))
  process.on('unhandledRejection', (err) => log('launcher', `未处理的 Promise 拒绝:${err?.stack ?? err}`, 'err'))
}

main().catch((err) => {
  console.error('dsh-launcher 启动失败:', err)
  process.exit(1)
})
