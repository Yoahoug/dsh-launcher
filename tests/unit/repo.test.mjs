// dsh-launcher · RepoManager 契约测试:只读状态、fetch/stash/pull 行为、冲突只报告
import { test, before, after } from 'node:test'
import assert from 'node:assert/strict'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { createSandbox, sandboxEnv } from '../helpers/env.mjs'

const sb = createSandbox()
process.env = sandboxEnv(sb)

let mod
before(async () => {
  mod = await import('../../src/repo.mjs')
})
after(() => sb.cleanup())

test('只读状态:分支/HEAD/dirty/落后', async () => {
  const st = await mod.repoStatus(sb.repoDir, { syncAt: 42 })
  assert.equal(st.branch, 'main')
  assert.equal(st.head, 'abc1234')
  assert.equal(st.dirty, false)
  assert.equal(st.behind, 0)
  assert.equal(st.ahead, 0)
  assert.equal(st.syncAt, 42)
  assert.equal(st.remoteUpToDate, true)
})

test('dirty 工作区可检测(不影响 HEAD)', async () => {
  process.env.FAKE_GIT_DIRTY = '1'
  assert.equal(await mod.isDirty(sb.repoDir), true)
  delete process.env.FAKE_GIT_DIRTY
  assert.equal(await mod.isDirty(sb.repoDir), false)
})

test('gitSync 干净工作区:fetch + pull 成功', async () => {
  const r = await mod.gitSync(sb.repoDir)
  assert.equal(r.ok, true)
  assert.equal(r.stashed, false)
})

test('gitSync fetch 网络失败:stage=fetch + 可读诊断', async () => {
  process.env.FAKE_GIT_FETCH_FAIL = '1'
  const r = await mod.gitSync(sb.repoDir)
  assert.equal(r.ok, false)
  assert.equal(r.stage, 'fetch')
  assert.match(r.error, /网络|无法连接/)
  delete process.env.FAKE_GIT_FETCH_FAIL
})

test('gitSync stash 失败:stage=stash,不碰工作区', async () => {
  process.env.FAKE_GIT_DIRTY = '1'
  process.env.FAKE_GIT_STASH_FAIL = '1'
  const r = await mod.gitSync(sb.repoDir)
  assert.equal(r.ok, false)
  assert.equal(r.stage, 'stash')
  delete process.env.FAKE_GIT_DIRTY
  delete process.env.FAKE_GIT_STASH_FAIL
})

test('gitSync rebase 冲突:只报告 + 冲突文件清单 + 不执行破坏性操作', async () => {
  mkdirSync(join(sb.repoDir, 'src'), { recursive: true })
  writeFileSync(join(sb.repoDir, 'src', 'foo.js'), 'local change\n', 'utf8')
  process.env.FAKE_GIT_DIRTY = '1'
  process.env.FAKE_GIT_CONFLICT = '1'
  process.env.FAKE_GIT_CONFLICTS = 'src/foo.js'
  const r = await mod.gitSync(sb.repoDir)
  assert.equal(r.ok, false)
  assert.equal(r.stage, 'conflict')
  assert.deepEqual(r.conflicts, ['src/foo.js'])
  assert.match(r.error, /冲突.*未破坏工作区|rebase 冲突/)
  // 工作区文件未被破坏
  const content = readFileSync(join(sb.repoDir, 'src', 'foo.js'), 'utf8')
  assert.equal(content, 'local change\n')
  assert.equal(mod.rebaseInProgress(sb.repoDir), true)
  delete process.env.FAKE_GIT_DIRTY
  delete process.env.FAKE_GIT_CONFLICT
  delete process.env.FAKE_GIT_CONFLICTS
})

test('lockfileChanged:from..HEAD 中 pnpm-lock.yaml 是否变化', async () => {
  process.env.FAKE_GIT_LOCKFILE = '1'
  assert.equal(await mod.lockfileChanged(sb.repoDir, 'abc1230'), true)
  delete process.env.FAKE_GIT_LOCKFILE
  assert.equal(await mod.lockfileChanged(sb.repoDir, 'abc1230'), false)
  assert.equal(await mod.lockfileChanged(sb.repoDir, ''), false)
})
