// dsh-launcher · 状态机契约测试:常量、合并、订阅、fail、持久化
import { test, before, after } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { createSandbox } from '../helpers/env.mjs'

const sb = createSandbox()

let mod
before(async () => {
  process.env.DSH_LAUNCHER_CONFIG_DIR = sb.configDir
  process.env.DSH_LAUNCHER_STATE_DIR = sb.stateDir
  mod = await import('../../src/state.mjs')
})
after(() => sb.cleanup())

test('状态全集与默认值', () => {
  assert.deepEqual(Object.values(mod.STATES).sort(), [
    'building', 'failed', 'idle', 'installing', 'running', 'starting', 'stopping', 'syncing',
  ])
  assert.equal(mod.state.state, mod.STATES.IDLE)
  assert.equal(mod.state.mode, 'none')
  assert.equal(mod.state.busy, false)
  assert.deepEqual(mod.state.error, null)
})

test('setState 浅合并并广播', () => {
  const seen = []
  const unsub = mod.subscribe((s) => seen.push(s))
  mod.setState({ state: mod.STATES.STARTING, phase: '启动 dsh web…' })
  assert.equal(mod.state.state, mod.STATES.STARTING)
  assert.equal(mod.state.phase, '启动 dsh web…')
  assert.equal(seen.length, 1)
  // 未 patch 字段保留
  assert.equal(mod.state.mode, 'none')
  unsub()
  mod.setState({ state: mod.STATES.IDLE })
  assert.equal(seen.length, 1)
})

test('fail:进入 failed + 诊断 + busy false + 持久化', () => {
  mod.setState({ busy: true })
  mod.fail('启动超时', '未在 120s 内出现就绪行')
  assert.equal(mod.state.state, mod.STATES.FAILED)
  assert.equal(mod.state.busy, false)
  assert.deepEqual(mod.state.error, { summary: '启动超时', detail: '未在 120s 内出现就绪行' })
  const disk = JSON.parse(readFileSync(join(sb.stateDir, 'state.json'), 'utf8'))
  assert.equal(disk.state, 'failed')
})

test('persist:最小召回字段落盘', () => {
  mod.setState({
    state: mod.STATES.RUNNING, mode: 'dev', url: 'http://127.0.0.1:3080/',
    startedAt: 123, readyAt: 456, hmrActive: true,
  })
  mod.persist()
  const disk = JSON.parse(readFileSync(join(sb.stateDir, 'state.json'), 'utf8'))
  assert.deepEqual(disk, {
    mode: 'dev', url: 'http://127.0.0.1:3080/', port: '3080',
    startedAt: 123, readyAt: 456, hmrActive: true, state: 'running',
  })
})
