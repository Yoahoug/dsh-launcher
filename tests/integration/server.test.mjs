// dsh-launcher · 集成契约测试:真实 server.mjs + fake 工具
// 覆盖:状态转移/就绪/超时/早退/epoch 防回写/端口占用/召回/动作与 API 校验。
// 注意:launcher 端口固定 3090,本文件由 --test-concurrency=1 串行执行。
import { test, after } from 'node:test'
import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { createServer, connect } from 'node:net'
import { readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { createSandbox, sandboxEnv, waitFor } from '../helpers/env.mjs'
import { spawnServer, api, act, waitState, writeStateFile, BASE } from '../helpers/server.mjs'

const sandboxes = []
function newSb() {
  const sb = createSandbox()
  sandboxes.push(sb)
  return sb
}
after(() => {
  for (const sb of sandboxes) sb.cleanup()
})

/** 启动隔离 server,预置合法仓库路径与短超时,测试结束自动停止。 */
async function withServer(t, env = {}, fn) {
  const sb = newSb()
  const srv = await spawnServer(sb, env)
  t.after(async () => { await srv.stop() })
  await api('/api/config', {
    method: 'POST',
    body: { repoPath: sb.repoDir, readyTimeoutMs: 5000, openBrowser: false },
  })
  return { sb, srv }
}

test('启动即 idle,提供 launcher/config 元信息', async (t) => {
  const { sb } = await withServer(t)
  const st = await api('/api/state')
  assert.equal(st.ok, true)
  assert.equal(st.state.state, 'idle')
  assert.equal(st.state.busy, false)
  assert.equal(st.state.mode, 'none')
  assert.equal(st.state.repo.branch, 'main')
  assert.equal(st.state.repo.head, 'abc1234')

  const cfg = await api('/api/config')
  assert.equal(cfg.config.port, 3080)
  assert.equal(cfg.config.repoPath, sb.repoDir)
  assert.equal(cfg.usable.ok, true)
  assert.equal(cfg.tools.resolved.pnpm, join(sb.binDir, 'pnpm'))
  assert.equal(cfg.tools.resolved.git, join(sb.binDir, 'git'))
  assert.equal(cfg.launcher.pid > 0, true)
})

test('start:starting → running,就绪行命中 URL/webPid,state.json 持久化', async (t) => {
  const { sb } = await withServer(t)
  const r = await act('start')
  assert.equal(r.ok, true)
  await waitState((s) => s.state === 'starting', { timeout: 8000 })
  const running = await waitState((s) => s.state === 'running', { timeout: 8000 })
  assert.equal(running.mode, 'normal')
  assert.match(running.url, /^http:\/\/127\.0\.0\.1:\d+\/$/)
  assert.ok(running.webPid > 0)
  const disk = JSON.parse(readFileSync(join(sb.stateDir, 'state.json'), 'utf8'))
  assert.equal(disk.state, 'running')
  // 日志中出现就绪行命中
  const logs = await api('/api/logs')
  assert.ok(logs.logs.some((l) => /就绪行命中/.test(l.text)))
})

test('running 时重复 start:只召回不重复拉起(already)', async (t) => {
  await withServer(t)
  await act('start')
  await waitState((s) => s.state === 'running')
  const before = (await api('/api/state')).state.webPid
  const r = await act('start')
  assert.equal(r.ok, true)
  assert.equal(r.already, true)
  const after = (await api('/api/state')).state.webPid
  assert.equal(after, before, '不得重复拉起新进程')
})

test('stop:idle + pid 文件清理 + 进程组被杀', async (t) => {
  const { sb } = await withServer(t)
  await act('start')
  const st = await waitState((s) => s.state === 'running')
  const pid = st.webPid
  const r = await act('stop')
  assert.equal(r.ok, true)
  await waitState((s) => s.state === 'idle')
  assert.equal((await api('/api/state')).state.mode, 'none')
  assert.throws(() => readFileSync(join(sb.stateDir, 'dshweb.pid')), 'pid 文件应清理')
  assert.throws(() => readFileSync(join(sb.stateDir, 'devweb.pid')))
  await waitFor(() => {
    try { process.kill(pid, 0); return false } catch { return true }
  }, { timeout: 8000, label: 'web 进程组死亡' })
})

test('dev 模式:dev:web + dsh web 同跑,running(mode=dev)', async (t) => {
  const { sb } = await withServer(t)
  const r = await act('dev')
  assert.equal(r.ok, true)
  const running = await waitState((s) => s.state === 'running', { timeout: 8000 })
  assert.equal(running.mode, 'dev')
  assert.ok(running.devPid > 0)
  assert.ok(readFileSync(join(sb.stateDir, 'devweb.pid'), 'utf8').trim())
  // stop 应同时清掉 dev:web
  await act('stop')
  await waitState((s) => s.state === 'idle')
  assert.throws(() => readFileSync(join(sb.stateDir, 'devweb.pid')))
})

test('readiness 超时 → failed(带诊断,不悬挂)', async (t) => {
  await withServer(t, { FAKE_PNPM_NO_READY: '1' })
  await act('start')
  const failed = await waitState((s) => s.state === 'failed', { timeout: 20000 })
  assert.match(failed.error.summary, /启动超时/)
  assert.match(failed.error.detail, /就绪行/)
  assert.equal(failed.busy, false)
})

test('子进程早退 → failed 明确诊断', async (t) => {
  // EXIT_EARLY 且无就绪行:启动中即崩溃
  await withServer(t, { FAKE_PNPM_EXIT_EARLY: '1', FAKE_PNPM_NO_READY: '1' })
  await act('start')
  const failed = await waitState((s) => s.state === 'failed', { timeout: 10000 })
  assert.match(failed.error.summary, /启动失败/)
  assert.equal(failed.webPid, null)
})

test('epoch:停止后旧流程不得回写 running(防竞态)', async (t) => {
  // 慢就绪(2.5s):start 后立即 stop,等待旧就绪行出现,状态必须仍是 idle
  await withServer(t, { FAKE_PNPM_READY_DELAY: '2.5' })
  await act('start')
  await waitState((s) => s.state === 'starting', { timeout: 8000 })
  const r = await act('stop')
  assert.equal(r.ok, true)
  await waitState((s) => s.state === 'idle', { timeout: 8000 })
  // 等待超过就绪延迟
  await new Promise((r2) => setTimeout(r2, 3500))
  const st = await api('/api/state')
  assert.equal(st.state.state, 'idle', '停止后旧就绪行不得把状态改回 running')
  assert.equal(st.state.busy, false)
  assert.equal(st.state.webPid, null)
})

test('端口 3080 被占用 → failed 端口诊断(不误杀占用进程)', async (t) => {
  const { sb } = await withServer(t)
  const blocker = createServer()
  await new Promise((r) => blocker.listen(3080, '127.0.0.1', r))
  t.after(async () => { await new Promise((r) => blocker.close(r)) })
  const before = (await api('/api/state')).state
  const r = await act('start')
  assert.equal(r.ok, false)
  assert.equal(r.reason, 'port-busy')
  const failed = await waitState((s) => s.state === 'failed', { timeout: 8000 })
  assert.match(failed.error.summary, /端口 3080 已被占用/)
  assert.ok(before.busy === false)
  // 占用进程未被误杀:仍可连接
  await waitFor(() => new Promise((r2) => {
    const s = connect(3080, '127.0.0.1', () => { s.destroy(); r2(true) })
    s.on('error', () => r2(false))
  }))
})

test('召回:state.json + dshweb.pid + 端口 + 命令行匹配 → 直接 running;停止后清理', async (t) => {
  const sb = newSb()
  // 遗留进程:fake pnpm 常驻(命令行含 pnpm/dsh)
  const legacy = spawn(join(sb.binDir, 'pnpm'), ['dsh', 'web', '--port', '3080'], {
    env: sandboxEnv(sb), stdio: 'ignore', detached: true,
  })
  legacy.unref()
  const blocker = createServer()
  await new Promise((r) => blocker.listen(3080, '127.0.0.1', r))
  writeStateFile(sb, {
    state: 'running', mode: 'normal', url: 'http://127.0.0.1:3080/',
    port: 3080, startedAt: Date.now(), readyAt: Date.now(), hmrActive: false,
  })
  writeFileSync(join(sb.stateDir, 'dshweb.pid'), `${legacy.pid}\n`, 'utf8')
  const srv = await spawnServer(sb)
  t.after(async () => { await srv.stop(); await new Promise((r) => blocker.close(r)) })
  const running = await waitState((s) => s.state === 'running' && s.webPid === legacy.pid, { timeout: 8000 })
  assert.equal(running.url, 'http://127.0.0.1:3080/')
  // 召回后可正常停止并清掉遗留进程
  const r = await act('stop')
  assert.equal(r.ok, true)
  await waitState((s) => s.state === 'idle')
  await waitFor(() => {
    try { process.kill(legacy.pid, 0); return false } catch { return true }
  }, { timeout: 8000, label: '召回进程停止' })
})

test('API 校验:未知动作 / 缺失动作 / 配置边界', async (t) => {
  await withServer(t)
  const bad = await act('nope')
  assert.equal(bad.status, 400)
  assert.match(bad.reason, /未知动作/)
  const noBody = await fetch(`${BASE}/api/action`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}' })
  assert.equal(noBody.status, 400)
  const badPort = await api('/api/config', { method: 'POST', body: { port: 99999 } })
  assert.equal(badPort.status, 400)
  const badRepo = await api('/api/config', { method: 'POST', body: { repoPath: '/nonexistent/path' } })
  assert.equal(badRepo.status, 400)
  const good = await api('/api/config', { method: 'POST', body: { port: 4100 } })
  assert.equal(good.ok, true)
  assert.equal(good.config.port, 4100)
  const cfg = await api('/api/config')
  assert.equal(cfg.config.port, 4100)
})

test('日志 API:since 增量 + 来源清单', async (t) => {
  await withServer(t)
  const first = await api('/api/logs')
  assert.equal(first.ok, true)
  assert.ok(first.sources.includes('launcher'))
  assert.ok(first.sources.includes('dsh web'))
  const lastId = first.logs.at(-1).id
  const inc = await api(`/api/logs?since=${lastId}`)
  assert.ok(inc.logs.every((l) => l.id > lastId))
  const any = first.logs[0]
  assert.ok(['info', 'ok', 'warn', 'err'].includes(any.level))
  assert.equal(typeof any.ts, 'number')
})

// ── 桌面桥接鉴权(过渡期 contract)──────────────────────────

const TOKEN = 'test-token-abc123'

function authedFetch(pathname, { method = 'GET', body = null, token = TOKEN, origin = null } = {}) {
  const headers = {}
  if (token !== null) headers.Authorization = `Bearer ${token}`
  if (origin) headers.Origin = origin
  return fetch(`${BASE}${pathname}`, {
    method,
    headers: body ? { 'Content-Type': 'application/json', ...headers } : headers,
    body: body ? JSON.stringify(body) : undefined,
  })
}

test('token 模式:无/错 token 被拒,正确 token 放行;health 例外', async (t) => {
  const { sb } = await withServer(t, { DSH_LAUNCHER_TOKEN: TOKEN })
  // 无 token → 401
  assert.equal((await authedFetch('/api/state', { token: null })).status, 401)
  // 错 token → 401
  assert.equal((await authedFetch('/api/state', { token: 'wrong' })).status, 401)
  // 正确 token → 200
  const ok = await authedFetch('/api/state')
  assert.equal(ok.status, 200)
  assert.equal((await ok.json()).state.state, 'idle')
  // health 例外:无 token 可访问(仅版本/pid)
  const health = await authedFetch('/api/health', { token: null })
  assert.equal(health.status, 200)
  assert.ok((await health.json()).pid > 0)
  // 动作与日志同样受保护
  assert.equal((await authedFetch('/api/action', { method: 'POST', body: { action: 'start' }, token: null })).status, 401)
  assert.equal((await authedFetch('/api/logs', { token: 'nope' })).status, 401)
})

test('token 模式:非允许 Origin 被拒(403)', async (t) => {
  await withServer(t, { DSH_LAUNCHER_TOKEN: TOKEN })
  assert.equal((await authedFetch('/api/state', { origin: 'https://evil.example' })).status, 403)
  assert.equal((await authedFetch('/api/state', { origin: 'http://127.0.0.1:3090' })).status, 200)
  assert.equal((await authedFetch('/api/state', { origin: 'http://localhost:3090' })).status, 200)
})

test('token 模式:SSE 支持 ?token= 查询参数(legacy 控制台),错误 token 拒绝', async (t) => {
  await withServer(t, { DSH_LAUNCHER_TOKEN: TOKEN })
  const good = await fetch(`${BASE}/api/events?token=${TOKEN}`)
  assert.equal(good.status, 200)
  good.body?.cancel()
  const bad = await fetch(`${BASE}/api/events?token=wrong`)
  assert.equal(bad.status, 401)
  bad.body?.cancel()
})

test('token 模式:index.html 注入 token 供 legacy 控制台 fetch 包装', async (t) => {
  await withServer(t, { DSH_LAUNCHER_TOKEN: TOKEN })
  const res = await fetch(`${BASE}/`)
  const html = await res.text()
  assert.ok(html.includes('window.__DSH_LAUNCHER_TOKEN__'), '页面应注入 token')
})

test('无 token 模式(旧启动方式):不要求鉴权,行为不变', async (t) => {
  await withServer(t)
  const r = await api('/api/state')
  assert.equal(r.ok, true)
  const r2 = await act('clear')
  assert.equal(r2.ok, true)
})

test('detach:daemon 退出但 dsh web 继续运行(pid 文件保留供召回)', async (t) => {
  const { sb } = await withServer(t, { FAKE_PNPM_READY_DELAY: '0.2' })
  await act('start')
  const running = await waitState((s) => s.state === 'running')
  const webPid = running.webPid
  const r = await act('detach')
  assert.equal(r.ok, true)
  // daemon 退出
  await waitFor(async () => {
    try {
      const resp = await fetch(`${BASE}/api/health`, { signal: AbortSignal.timeout(800) })
      return !resp.ok
    } catch { return true }
  }, { timeout: 8000, label: 'daemon 退出' })
  // dsh web 仍在运行,pid 文件保留
  try { process.kill(webPid, 0); assert.ok(true, 'dsh web 未被停止') } catch { assert.fail('dsh web 不应被停止') }
  assert.ok(readFileSync(join(sb.stateDir, 'dshweb.pid'), 'utf8').trim())
  // 清理遗留进程
  try { process.kill(-webPid, 'SIGKILL') } catch { /* ignore */ }
})
