// dsh-launcher · 集成测试 helper:spawn 真实 server.mjs(隔离沙箱)+ HTTP API 客户端
// 测试用:fake git/pnpm 由 PATH 前置注入,配置/状态目录指向临时目录。
import { spawn } from 'node:child_process'
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { sandboxEnv, waitFor } from './env.mjs'

export const SERVER_PATH = fileURLToPath(new URL('../../src/server.mjs', import.meta.url))
export const BASE = 'http://127.0.0.1:3090'

/** 启动 server.mjs(真实进程),等待 /api/health 就绪。返回 { child, stop, output }。 */
export async function spawnServer(sb, extraEnv = {}) {
  const child = spawn(process.execPath, [SERVER_PATH], {
    env: sandboxEnv(sb, extraEnv),
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  let out = ''
  child.stdout.on('data', (b) => { out += b })
  child.stderr.on('data', (b) => { out += b })
  await waitFor(async () => {
    if (child.exitCode !== null) throw new Error(`server 提前退出(code=${child.exitCode}):\n${out}`)
    try {
      const r = await fetch(`${BASE}/api/health`, { signal: AbortSignal.timeout(800) })
      return r.ok
    } catch {
      return false
    }
  }, { timeout: 20000, label: 'server /api/health 就绪' })
  return {
    child,
    output: () => out,
    stop: async () => {
      if (child.exitCode !== null) return
      child.kill('SIGTERM')
      await new Promise((r) => child.once('exit', r))
    },
  }
}

/** GET/POST JSON 到 3090 API。 */
export async function api(pathname, { method = 'GET', body = null } = {}) {
  const res = await fetch(`${BASE}${pathname}`, {
    method,
    headers: body ? { 'Content-Type': 'application/json' } : undefined,
    body: body ? JSON.stringify(body) : undefined,
  })
  const json = await res.json()
  return { status: res.status, ...json }
}

/** 轮询 /api/state 直到谓词成立,返回最终 state。 */
export async function waitState(pred, { timeout = 15000 } = {}) {
  let last = null
  await waitFor(async () => {
    last = (await api('/api/state')).state
    return pred(last)
  }, { timeout, label: 'state 条件' })
  return last
}

/** 动作快捷方式。 */
export function act(action) {
  return api('/api/action', { method: 'POST', body: { action } })
}

/** 预写 state.json(模拟上次运行遗留,供召回测试)。 */
export function writeStateFile(sb, patch) {
  mkdirSync(join(sb.stateDir, 'logs'), { recursive: true })
  writeFileSync(join(sb.stateDir, 'state.json'), `${JSON.stringify(patch, null, 2)}\n`, 'utf8')
}
