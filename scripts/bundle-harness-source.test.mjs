import { mkdtempSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { bundleHarness } from './bundle-harness-source.mjs'

function fixture() {
  const root = mkdtempSync(join(tmpdir(), 'dsh-harness-fixture-'))
  const write = (path, value) => {
    const full = join(root, path)
    mkdirSync(join(full, '..'), { recursive: true })
    writeFileSync(full, value)
  }
  write('package.json', JSON.stringify({ name: 'fixture', version: '1.0.0', devDependencies: { vitest: '1' }, dependencies: { chalk: '1' } }))
  write('pnpm-lock.yaml', 'lockfileVersion: 9.0\n')
  write('pnpm-workspace.yaml', 'packages:\n  - packages/*/*\n\npatchedDependencies:\n  fixture: patches/fixture.patch\n')
  write('patches/fixture.patch', 'fixture patch\n')
  write('apps/cli/package.json', JSON.stringify({ name: '@fixture/cli', devDependencies: { typescript: '1' } }))
  write('apps/cli/lib/bin.js', 'console.log("fixture")\n')
  write('apps/cli/tests/should-not-copy.js', 'nope\n')
  write('apps/web/package.json', JSON.stringify({ name: '@fixture/web' }))
  write('apps/web/dist/index.html', '<!doctype html>\n')
  write('apps/web/dist/index.js.map', '{}\n')
  write('packages/core/core/package.json', JSON.stringify({ name: '@fixture/core', main: 'lib/index.js', bin: 'bin.js' }))
  write('packages/core/core/lib/index.js', 'export {}\n')
  write('packages/core/core/bin.js', '#!/usr/bin/env node\n')
  write('native/landlock-run/package.json', JSON.stringify({ name: '@fixture/native-workspace' }))
  write('native/landlock-run/packages/entry/package.json', JSON.stringify({ name: '@fixture/native-entry', main: 'lib/index.js' }))
  write('native/landlock-run/packages/entry/lib/index.js', 'export {}\n')
  return root
}

test('bundleHarness copies only production inputs and creates a manifest', () => {
  const source = fixture()
  const output = mkdtempSync(join(tmpdir(), 'dsh-harness-output-'))
  const manifest = bundleHarness({ source, output })
  assert.equal(manifest.schema, 1)
  assert.equal(typeof manifest.bundleHash, 'string')
  assert.ok(readFileSync(join(output, 'apps/cli/lib/bin.js'), 'utf8').includes('fixture'))
  assert.ok(!readFileSync(join(output, 'package.json'), 'utf8').includes('devDependencies'))
  assert.ok(!readFileSync(join(output, 'apps/cli/package.json'), 'utf8').includes('devDependencies'))
  assert.equal(readFileSync(join(output, 'patches/fixture.patch'), 'utf8'), 'fixture patch\n')
  assert.ok(readFileSync(join(output, 'native/landlock-run/packages/entry/lib/index.js'), 'utf8').includes('export'))
  assert.ok(readFileSync(join(output, 'packages/core/core/bin.js'), 'utf8').includes('node'))
  assert.ok(!readFileSync(join(output, 'bundle-manifest.json'), 'utf8').includes('should-not-copy'))
  assert.ok(!readFileSync(join(output, 'bundle-manifest.json'), 'utf8').includes('index.js.map'))
})
