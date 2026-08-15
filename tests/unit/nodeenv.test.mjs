// dsh-launcher · Node 版本契约测试:解析、范围判断、就绪解析主路径
import { test, before, after } from 'node:test'
import assert from 'node:assert/strict'
import { createSandbox } from '../helpers/env.mjs'

const sb = createSandbox()

let mod
before(async () => {
  process.env.DSH_LAUNCHER_CONFIG_DIR = sb.configDir
  process.env.DSH_LAUNCHER_STATE_DIR = sb.stateDir
  mod = await import('../../src/nodeenv.mjs')
})
after(() => sb.cleanup())

test('parseNodeVersion:合法与非法', () => {
  assert.deepEqual(mod.parseNodeVersion('v24.19.0'), [24, 19, 0])
  assert.deepEqual(mod.parseNodeVersion('24.19.0'), [24, 19, 0])
  assert.deepEqual(mod.parseNodeVersion('v22.19.1'), [22, 19, 1])
  assert.equal(mod.parseNodeVersion('abc'), null)
  assert.equal(mod.parseNodeVersion(''), null)
  assert.equal(mod.parseNodeVersion(null), null)
})

test('nodeInRange:dsh engines ^22.19 || >=24', () => {
  assert.equal(mod.nodeInRange('v24.19.0'), true)
  assert.equal(mod.nodeInRange('v24.0.0'), true)
  assert.equal(mod.nodeInRange('v26.7.0'), true)
  assert.equal(mod.nodeInRange('v22.19.0'), true)
  assert.equal(mod.nodeInRange('v22.18.9'), false)
  assert.equal(mod.nodeInRange('v23.11.0'), false) // Node 23:EOL,tsx/tsdown 崩溃
  assert.equal(mod.nodeInRange('v20.9.0'), false)
})

test('resolveDshNode:当前进程在范围内直接返回(当前进程)', async () => {
  const r = await mod.resolveDshNode()
  assert.ok(r, '应解析出可用 Node')
  assert.equal(r.source, '当前进程')
  assert.equal(r.version, `v${process.versions.node}`)
  assert.ok(mod.nodeInRange(r.version), '解析出的版本必须在 dsh 范围内')
})
