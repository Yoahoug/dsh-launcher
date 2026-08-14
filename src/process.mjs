// dsh-launcher · ProcessManager:进程托管(单实例 / 进程树 / 就绪检测)
// 所有托管子进程以 detached 启动(独立进程组),停止按进程组 SIGTERM → 5s → SIGKILL。
import { spawn } from 'node:child_process'
import { createInterface } from 'node:readline'
import { readFileSync, unlinkSync, writeFileSync } from 'node:fs'
import { WEB_PID_FILE, DEV_PID_FILE } from './config.mjs'
import { log } from './log.mjs'
import { toolEnv } from './tools.mjs'

/** 就绪行正则(与 dsh 仓库测试 apps/web/tests 同款)。 */
export const READY_RE = /dsh web: (http:\/\/[^\s]+)/

/** 托管子进程注册表(进程组 leader)。 */
export const children = {
  web: null, // dsh web
  dev: null, // pnpm run dev:web
  op: null,  // 流程子进程(git / pnpm install / pnpm build)
}

// ── pid 文件 ─────────────────────────────────────────────

export function writePid(file, pid) {
  try { writeFileSync(file, `${String(pid)}\n`, 'utf8') } catch { /* ignore */ }
}
export function readPid(file) {
  try { return Number(readFileSync(file, 'utf8').trim()) || null } catch { return null }
}
export function clearPid(file) {
  try { unlinkSync(file) } catch { /* ignore */ }
}
export function isAlive(pid) {
  if (!Number.isInteger(pid) || pid <= 0) return false
  try { process.kill(pid, 0); return true } catch { return false }
}

// ── 就绪 / 输出挂接 ──────────────────────────────────────

function attach(child, src, handlers = {}) {
  const rlOut = createInterface({ input: child.stdout })
  const rlErr = createInterface({ input: child.stderr })

  rlOut.on('line', (line) => {
    log(src, line)
    if (handlers.onLine) handlers.onLine(line, 'out')
    const m = READY_RE.exec(line)
    if (m && handlers.onReady) handlers.onReady(m[1])
  })
  rlErr.on('line', (line) => {
    log(src, line, 'warn')
    if (handlers.onLine) handlers.onLine(line, 'err')
  })
  child.on('exit', (code, signal) => {
    log(src, `进程退出 code=${code} signal=${signal ?? ''}`.trimEnd())
    if (handlers.onExit) handlers.onExit(code, signal)
  })
  child.on('error', (err) => {
    log(src, `spawn 失败:${err.message}`, 'err')
    if (handlers.onExit) handlers.onExit(null, null, err)
  })
  return child
}

// ── 启动 ─────────────────────────────────────────────────

/**
 * 源码启动 dsh web:spawn pnpm dsh web --port …(等价 node --import tsx/esm apps/cli/src/bin.ts web)。
 */
export function spawnWeb({ cwd, port, host = '127.0.0.1', dshHome = '', onLine, onReady, onExit }) {
  const args = ['dsh', 'web', '--port', String(port)]
  if (host && host !== '127.0.0.1') args.push('--host', host)
  const env = toolEnv()
  if (dshHome) env.DSH_HOME = dshHome
  const child = spawn('pnpm', args, { cwd, env, detached: true, stdio: ['ignore', 'pipe', 'pipe'] })
  children.web = child
  writePid(WEB_PID_FILE, child.pid)
  log('launcher', `拉起 dsh web:pnpm ${args.join(' ')}`)
  attach(child, 'dsh web', { onLine, onReady, onExit })
  return child
}

/** 开发模式:HMR watcher(pnpm run dev:web → tsx scripts/dev-web.ts --poll)。 */
export function spawnDevWeb(cwd, { onLine, onExit } = {}) {
  const child = spawn('pnpm', ['run', 'dev:web'], {
    cwd, env: toolEnv(), detached: true, stdio: ['ignore', 'pipe', 'pipe'],
  })
  children.dev = child
  writePid(DEV_PID_FILE, child.pid)
  log('launcher', '拉起 dev:web → pnpm run dev:web (Vite/tsdown HMR watcher)')
  attach(child, 'dev:web', { onLine, onExit })
  return child
}

// ── 停止(进程组) ─────────────────────────────────────────

function waitExit(child, ms) {
  if (child.exitCode !== null || child.signalCode !== null) return Promise.resolve(true)
  return new Promise((resolve) => {
    const t = setTimeout(() => resolve(false), ms)
    child.once('exit', () => { clearTimeout(t); resolve(true) })
    child.once('error', () => { clearTimeout(t); resolve(true) })
  })
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

/** 进程组是否仍存在(detached 子进程 = 组 leader)。 */
function groupAlive(pid) {
  try { process.kill(-pid, 0); return true } catch { return false }
}

/**
 * 对进程组发 SIGTERM,宽限 5s 后 SIGKILL;顺带清理 pid 文件。
 * child 可以是 ChildProcess,也可以是召回时重建的 { pid } 伪句柄。
 */
export async function killTree(child, label = '进程', pidFile = null) {
  if (!child || child.pid === undefined) return
  const pid = child.pid
  const isReal = typeof child.once === 'function'
  try {
    process.kill(-pid, 'SIGTERM')
  } catch {
    try { process.kill(pid, 'SIGTERM') } catch { /* 已死 */ }
  }
  log('launcher', `${label}(PID ${pid}) 收到 SIGTERM`)
  let dead = false
  if (isReal) {
    dead = await waitExit(child, 5000)
  }
  if (!dead) {
    const t0 = Date.now()
    while (Date.now() - t0 < 5000) {
      if (!groupAlive(pid)) { dead = true; break }
      await sleep(200)
    }
  }
  if (!dead) {
    log('launcher', `${label} 5s 未退出,发送 SIGKILL`, 'warn')
    try { process.kill(-pid, 'SIGKILL') } catch {
      try { process.kill(pid, 'SIGKILL') } catch { /* ignore */ }
    }
  }
  if (pidFile) clearPid(pidFile)
}

/** 停止全部托管进程(op/dev/web)。 */
export async function stopAll() {
  const labels = { web: 'dsh web', dev: 'dev:web', op: '流程子进程' }
  for (const key of ['op', 'dev', 'web']) {
    const child = children[key]
    if (child) {
      await killTree(child, labels[key], key === 'web' ? WEB_PID_FILE : key === 'dev' ? DEV_PID_FILE : null)
      children[key] = null
    }
  }
}
