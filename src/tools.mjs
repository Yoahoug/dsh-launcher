// dsh-launcher · 外部工具(node / pnpm / git)显式解析
// 背景:Finder 双击启动的 .app 只有极简 PATH(/usr/bin:/bin:…),不含 /opt/homebrew/bin;
// 因此 spawn('pnpm') 这类 PATH 查找会失败。这里按「PATH → 常见安装目录 → nvm 扫描」
// 解析出绝对路径,并在拉起子进程时把这些目录注入 PATH(同时解决 corepack shim 的
// `#!/usr/bin/env node` 需要 node 在 PATH 的问题)。
import { existsSync, readdirSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'

const HOME = homedir()

/** 常见安装目录。 */
function knownDirs() {
  return ['/opt/homebrew/bin', '/usr/local/bin', '/opt/local/bin', join(HOME, '.local', 'share', 'pnpm')]
}

/** 扫描 ~/.nvm/versions/node/<v*>/bin/<name>,取最高版本。 */
function scanNvm(name) {
  const root = join(HOME, '.nvm', 'versions', 'node')
  if (!existsSync(root)) return null
  let best = null
  let bestKey = [0, 0, 0]
  try {
    for (const entry of readdirSync(root, { withFileTypes: true })) {
      if (!entry.isDirectory()) continue
      const m = /^v(\d+)\.(\d+)\.(\d+)$/.exec(entry.name)
      if (!m) continue
      const key = [Number(m[1]), Number(m[2]), Number(m[3])]
      const cand = join(root, entry.name, 'bin', name)
      if (existsSync(cand) && (key[0] > bestKey[0] || (key[0] === bestKey[0] && key[1] > bestKey[1]) ||
          (key[0] === bestKey[0] && key[1] === bestKey[1] && key[2] > bestKey[2]))) {
        bestKey = key
        best = cand
      }
    }
  } catch { /* ignore */ }
  return best
}

/** 解析命令绝对路径:PATH → 已知目录 → nvm。找不到返回 null。 */
export function resolveExecutable(name) {
  const path = process.env.PATH || ''
  for (const dir of path.split(path.includes(';') ? ';' : ':').filter(Boolean)) {
    const cand = join(dir, name)
    if (existsSync(cand)) return cand
  }
  for (const dir of knownDirs()) {
    const cand = join(dir, name)
    if (existsSync(cand)) return cand
  }
  return scanNvm(name)
}

/** 一次解析全部工具;node 直接用当前进程的 execPath(必然正确)。 */
export function resolveTools() {
  const node = process.execPath
  const nodeDir = dirname(node)
  const pnpm = resolveExecutable('pnpm')
  const git = resolveExecutable('git')
  return {
    node,
    nodeDir,
    pnpm,
    pnpmDir: pnpm ? dirname(pnpm) : null,
    git,
    gitDir: git ? dirname(git) : null,
  }
}

/** 兼容 Node(dsh 范围)的 bin 目录;由 server 启动时按 nodeenv 解析结果注入。 */
let dshNodeDir = null

/** 设置兼容 Node 的 bin 目录(null = 用当前进程 Node)。 */
export function setDshNodeDir(dir) {
  dshNodeDir = dir || null
}

/** 子进程环境:把工具目录注入 PATH(排在前面),确保 pnpm/git/其 shim 都能解析。 */
export function toolEnv(tools = resolveTools()) {
  const sep = process.platform === 'win32' ? ';' : ':'
  const extra = [dshNodeDir, tools.nodeDir, tools.pnpmDir, tools.gitDir].filter(Boolean)
  const base = (process.env.PATH || '').split(sep).filter(Boolean)
  return { ...process.env, PATH: [...new Set([...extra, ...base])].join(sep) }
}
