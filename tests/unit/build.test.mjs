// dsh-launcher · BuildManager 契约测试:lockfile 判断、安装、构建阶段与失败定位
import { test, before, after } from 'node:test'
import assert from 'node:assert/strict'
import { createSandbox, sandboxEnv } from '../helpers/env.mjs'

const sb = createSandbox()
process.env = sandboxEnv(sb)

let mod
before(async () => {
  mod = await import('../../src/build.mjs')
})
after(() => sb.cleanup())

test('installIfNeeded:lockfile 未变化 → 跳过安装', async () => {
  const r = await mod.installIfNeeded(sb.repoDir, { from: 'abc1230' })
  assert.equal(r.needed, false)
  assert.equal(r.ok, true)
})

test('installIfNeeded:lockfile 变化 → pnpm install;失败给出 tail', async () => {
  process.env.FAKE_GIT_LOCKFILE = '1'
  const ok = await mod.installIfNeeded(sb.repoDir, { from: 'abc1230' })
  assert.equal(ok.needed, true)
  assert.equal(ok.ok, true)
  process.env.FAKE_PNPM_INSTALL_FAIL = '1'
  const bad = await mod.installIfNeeded(sb.repoDir, { from: 'abc1230' })
  assert.equal(bad.needed, true)
  assert.equal(bad.ok, false)
  assert.equal(bad.error, '依赖安装失败')
  delete process.env.FAKE_GIT_LOCKFILE
  delete process.env.FAKE_PNPM_INSTALL_FAIL
})

test('runBuild:阶段行上报(onStage 顺序)+ 成功', async () => {
  const stages = []
  const r = await mod.runBuild(sb.repoDir, { onStage: (p) => stages.push(p) })
  assert.equal(r.ok, true)
  assert.ok(stages.includes('构建 lib(host)…'), `阶段行缺失:${stages.join('|')}`)
  assert.ok(stages.includes('构建 lib(client)…'))
  assert.ok(stages.includes('构建 web 前端…'))
  assert.ok(stages.includes('构建完成 ✓'))
})

test('runBuild:失败定位到 tsc 阶段', async () => {
  process.env.FAKE_PNPM_BUILD_FAIL = '1'
  const r = await mod.runBuild(sb.repoDir, {})
  assert.equal(r.ok, false)
  assert.equal(r.error, 'tsc 类型检查错误')
  assert.ok(r.tail.some((l) => /TS1234/.test(l)))
  delete process.env.FAKE_PNPM_BUILD_FAIL
})
