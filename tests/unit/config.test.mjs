// dsh-launcher · 配置契约测试:默认值合并、落盘、路径校验、端口探测
import { test, before, after } from 'node:test'
import assert from 'node:assert/strict'
import { mkdirSync } from 'node:fs'
import { createServer } from 'node:net'
import { homedir } from 'node:os'
import { join } from 'node:path'
import { createSandbox } from '../helpers/env.mjs'

const sb = createSandbox()

let mod
before(async () => {
  process.env.DSH_LAUNCHER_CONFIG_DIR = sb.configDir
  process.env.DSH_LAUNCHER_STATE_DIR = sb.stateDir
  mod = await import('../../src/config.mjs')
})
after(() => sb.cleanup())

test('默认配置与磁盘合并(缺字段用默认值)', () => {
  const cfg = mod.loadConfig()
  assert.equal(cfg.port, 3080)
  assert.equal(cfg.host, '127.0.0.1')
  assert.equal(cfg.readyTimeoutMs, 120000)
  assert.equal(cfg.repoPath, join(homedir(), 'Desktop', 'deepseek-harness'))
  const next = mod.saveConfig({ port: 4100, readyTimeoutMs: 30000 })
  assert.equal(next.port, 4100)
  assert.equal(mod.loadConfig().port, 4100)
  assert.equal(mod.loadConfig().readyTimeoutMs, 30000)
  // 未 patch 的字段保留默认值
  assert.equal(mod.loadConfig().openBrowser, true)
})

test('expandPath 展开 ~ 与 $HOME', () => {
  const home = homedir()
  assert.equal(mod.expandPath('~/foo/bar'), join(home, 'foo', 'bar'))
  assert.equal(mod.expandPath('$HOME/x'), join(home, 'x'))
  assert.equal(mod.expandPath('/abs/path'), '/abs/path')
  assert.equal(mod.expandPath(''), '')
})

test('repoUsable:目录缺失 / 缺 .git / 正常', () => {
  assert.equal(mod.repoUsable(sb.repoDir).ok, true)
  assert.equal(mod.repoUsable(join(sb.root, 'nope')).ok, false)
  const notGit = join(sb.root, 'not-git')
  mkdirSync(notGit)
  assert.equal(mod.repoUsable(notGit).ok, false)
})

test('probePort:占用 true / 空闲 false', async () => {
  const srv = createServer()
  await new Promise((r) => srv.listen(0, '127.0.0.1', r))
  const port = srv.address().port
  assert.equal(await mod.probePort(port), true)
  assert.equal(await mod.probePort(port + 1), false)
  await new Promise((r) => srv.close(r))
})
