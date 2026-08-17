#!/usr/bin/env node
/**
 * 把已构建的 DeepSeek Harness 源码整理成 Launcher 正式运行资源。
 *
 * 用法:
 *   DSH_HARNESS_SOURCE=/path/to/deepseek-harness node scripts/bundle-harness-source.mjs
 *   node scripts/bundle-harness-source.mjs --source /path/to/deepseek-harness --output bundled/harness
 *
 * 只复制生产运行需要的 lib/dist/config/package.json 和生产依赖安装所需的 lockfile；
 * 不复制源码、测试、node_modules 或 devDependencies。脚本只删除上一次 manifest
 * 列出的生成文件，不触碰输出目录中的其它文件。
 */
import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { join, relative, resolve, sep } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const SCRIPT_DIR = resolve(fileURLToPath(new URL('.', import.meta.url)))
const PROJECT_ROOT = resolve(SCRIPT_DIR, '..')
const BUNDLE_MANIFEST = 'bundle-manifest.json'

function parseArgs(argv) {
  const args = [...argv]
  let source = process.env.DSH_HARNESS_SOURCE
  let output = resolve(PROJECT_ROOT, 'bundled/harness')
  while (args.length > 0) {
    const arg = args.shift()
    if (arg === '--source') source = args.shift()
    else if (arg === '--output') output = resolve(args.shift())
    else if (arg === '--help') {
      console.log('用法: node scripts/bundle-harness-source.mjs --source <DSH_ROOT> [--output <DIR>]')
      process.exit(0)
    } else {
      throw new Error(`未知参数: ${arg}`)
    }
  }
  if (!source) throw new Error('缺少 DSH 源码根目录:请设置 DSH_HARNESS_SOURCE 或传入 --source')
  return { source: resolve(source), output }
}

function readJson(path) {
  return JSON.parse(readFileSync(path, 'utf8'))
}

function productionPackageJson(sourcePath) {
  const value = readJson(sourcePath)
  delete value.devDependencies
  delete value.scripts
  return `${JSON.stringify(value, null, 2)}\n`
}

function copyFile(source, target) {
  mkdirSync(resolve(target, '..'), { recursive: true })
  writeFileSync(target, readFileSync(source))
}

function copyTree(source, target, { excludeMaps = false } = {}) {
  const info = statSync(source)
  if (info.isFile()) {
    if (!excludeMaps || !source.endsWith('.map')) copyFile(source, target)
    return
  }
  for (const entry of readdirSync(source, { withFileTypes: true })) {
    if (entry.name === 'node_modules' || entry.name === 'tests' || entry.name === 'test') continue
    copyTree(join(source, entry.name), join(target, entry.name), { excludeMaps })
  }
}

function copyPackage(sourceRoot, relativeRoot, outputRoot) {
  const source = join(sourceRoot, relativeRoot)
  const pkg = join(source, 'package.json')
  if (!existsSync(pkg)) return false
  const manifest = readJson(pkg)
  const target = join(outputRoot, relativeRoot)
  mkdirSync(target, { recursive: true })
  writeFileSync(join(target, 'package.json'), productionPackageJson(pkg))
  for (const name of ['lib', 'config', 'assets', 'bin', 'prebuilds', 'dist']) {
    const candidate = join(source, name)
    if (existsSync(candidate)) copyTree(candidate, join(target, name), { excludeMaps: name === 'dist' })
  }
  const bins = typeof manifest.bin === 'string'
    ? [manifest.bin]
    : Object.values(manifest.bin ?? {})
  for (const bin of bins) {
    if (typeof bin !== 'string' || bin.startsWith('/') || bin.includes('..')) continue
    const candidate = join(source, bin)
    if (existsSync(candidate)) copyFile(candidate, join(target, bin))
  }
  return true
}

function collectWorkspacePackages(sourceRoot, outputRoot) {
  for (const group of ['packages', 'vendor', 'native']) {
    const groupRoot = join(sourceRoot, group)
    if (!existsSync(groupRoot)) continue
    const walk = (dir, relDir) => {
      for (const entry of readdirSync(dir, { withFileTypes: true })) {
        if (entry.name === 'node_modules' || entry.name === 'tests' || entry.name === 'test') continue
        const child = join(dir, entry.name)
        const rel = join(relDir, entry.name)
        if (!entry.isDirectory()) continue
        if (existsSync(join(child, 'package.json'))) copyPackage(sourceRoot, rel, outputRoot)
        // A workspace member may itself contain nested workspace packages,
        // such as native/landlock-run/packages/entry.
        walk(child, rel)
      }
    }
    walk(groupRoot, group)
  }
}

function removePreviousGeneratedFiles(output) {
  const manifestPath = join(output, BUNDLE_MANIFEST)
  if (!existsSync(manifestPath)) return
  let previous
  try {
    previous = readJson(manifestPath)
  } catch {
    previous = null
  }
  for (const entry of previous?.files ?? []) {
    if (typeof entry.path !== 'string' || entry.path.includes('..')) continue
    rmSync(join(output, entry.path), { force: true })
  }
  rmSync(manifestPath, { force: true })
}

function sha256(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex')
}

function listFiles(root, current = root) {
  const result = []
  for (const entry of readdirSync(current, { withFileTypes: true })) {
    if (entry.name === BUNDLE_MANIFEST || entry.name === 'node_modules') continue
    const path = join(current, entry.name)
    if (entry.isDirectory()) result.push(...listFiles(root, path))
    else if (entry.isFile()) result.push(path)
  }
  return result
}

export function bundleHarness({ source, output }) {
  const sourceRoot = resolve(source)
  const outputRoot = resolve(output)
  const required = [
    'apps/cli/lib/bin.js',
    'apps/cli/package.json',
    'apps/web/dist/index.html',
    'apps/web/package.json',
    'package.json',
    'pnpm-lock.yaml',
  ]
  for (const path of required) {
    if (!existsSync(join(sourceRoot, path))) throw new Error(`DSH 源码缺少正式构建输入: ${path}`)
  }

  mkdirSync(outputRoot, { recursive: true })
  removePreviousGeneratedFiles(outputRoot)
  copyFile(join(sourceRoot, 'pnpm-lock.yaml'), join(outputRoot, 'pnpm-lock.yaml'))
  if (existsSync(join(sourceRoot, 'pnpm-workspace.yaml'))) {
    copyFile(join(sourceRoot, 'pnpm-workspace.yaml'), join(outputRoot, 'pnpm-workspace.yaml'))
  }
  // pnpm-workspace.yaml may reference patchedDependencies required by a
  // production install (for example node-pty). Keep only the patch inputs,
  // not the source repository's unrelated tooling files.
  if (existsSync(join(sourceRoot, 'patches'))) {
    copyTree(join(sourceRoot, 'patches'), join(outputRoot, 'patches'))
  }
  writeFileSync(join(outputRoot, 'package.json'), productionPackageJson(join(sourceRoot, 'package.json')))
  copyPackage(sourceRoot, 'apps/cli', outputRoot)
  copyPackage(sourceRoot, 'apps/web', outputRoot)
  copyTree(join(sourceRoot, 'apps/web/dist'), join(outputRoot, 'apps/web/dist'), { excludeMaps: true })
  collectWorkspacePackages(sourceRoot, outputRoot)

  const files = listFiles(outputRoot)
    .map((path) => {
      const relativePath = relative(outputRoot, path).split(sep).join('/')
      const stat = statSync(path)
      return { path: relativePath, size: stat.size, sha256: sha256(path) }
    })
    .sort((a, b) => a.path.localeCompare(b.path))
  const bundleHash = createHash('sha256')
    .update(files.map((file) => `${file.path}\0${file.size}\0${file.sha256}\n`).join(''))
    .digest('hex')
  const manifest = {
    schema: 1,
    bundleHash,
    sourceVersion: readJson(join(sourceRoot, 'package.json')).version ?? null,
    generatedAt: new Date().toISOString(),
    files,
  }
  writeFileSync(join(outputRoot, BUNDLE_MANIFEST), `${JSON.stringify(manifest, null, 2)}\n`)
  return manifest
}

const isMain = process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href
if (isMain) {
  try {
    const result = bundleHarness(parseArgs(process.argv.slice(2)))
    console.log(`Harness bundle 已生成: ${result.bundleHash}`)
  } catch (error) {
    console.error(error instanceof Error ? error.message : error)
    process.exitCode = 1
  }
}
