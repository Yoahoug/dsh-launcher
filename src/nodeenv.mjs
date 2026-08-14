// dsh-launcher · Node 运行时解析与安装
// 背景:dsh 工具链(tsx / tsdown / import-without-cache)要求 Node ^22.19 || >=24;
// Node 23 是 EOL 且不在范围,dev:web / 构建会直接崩溃(import-without-cache 的
// load hook 在 Node 23 下返回 source: undefined)。本模块:
//   1. 解析系统里已装的兼容 Node(nvm / volta / fnm / Homebrew keg / 本启动器托管目录),
//      供 server 把其 bin 目录注入子进程 PATH,自动避开不兼容的当前 Node;
//   2. 找不到时下载官方 LTS(Node 24)二进制到 STATE_DIR/node/<ver>/,解压后直接选用。
// 零运行时依赖,仅 Node 内置模块。
import { execFile, spawn } from 'node:child_process'
import { createWriteStream, existsSync, mkdirSync, readdirSync, renameSync, rmSync, statSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'
import https from 'node:https'
import http from 'node:http'
import { STATE_DIR } from './config.mjs'
import { unzip } from './zip.mjs'
import { log } from './log.mjs'

/** dsh engines 范围描述(与 server 的提示文案共用)。 */
export const NODE_RANGE_MSG = '^22.19 || >=24'

const HOME = homedir()
const NODE_BIN = process.platform === 'win32' ? 'node.exe' : 'node'

/** 解析 "v24.19.0" / "24.19.0" → [24,19,0];解析失败返回 null。 */
export function parseNodeVersion(v) {
  const m = /^v?(\d+)\.(\d+)\.(\d+)/.exec(String(v ?? ''))
  return m ? [Number(m[1]), Number(m[2]), Number(m[3])] : null
}
function cmp(a, b) {
  for (let i = 0; i < 3; i++) {
    if (a[i] !== b[i]) return a[i] - b[i]
  }
  return 0
}

/** 版本号是否在 dsh 范围(^22.19 || >=24)。 */
export function nodeInRange(v) {
  const p = parseNodeVersion(v)
  if (!p) return false
  return (p[0] === 22 && p[1] >= 19) || p[0] >= 24
}

/** 运行 node --version,超时/失败返回 null。 */
function probeVersion(bin, timeout = 8000) {
  return new Promise((resolve) => {
    execFile(bin, ['--version'], { timeout }, (err, stdout) => {
      if (err) return resolve(null)
      const v = String(stdout).trim()
      resolve(parseNodeVersion(v) ? v : null)
    })
  })
}

// ── 兼容 Node 解析 ────────────────────────────────────────

/**
 * 常见 Node 版本管理器安装目录(逐项尝试 bin/node 是否存在)。
 * 版本尽量从目录名推断(nvm/volta/fnm 目录名带 vX.Y.Z),推断不出的
 * (Homebrew keg)再现场跑 --version。
 */
function candidateBins() {
  const list = []
  const add = (root, rel, guess = null) => {
    if (!root) return
    const bin = join(root, rel)
    if (existsSync(bin)) list.push({ bin, guess })
  }
  const vroot = (base) => {
    if (!base || !existsSync(base)) return
    let entries = []
    try { entries = readdirSync(base, { withFileTypes: true }) } catch { return }
    for (const e of entries) {
      // 目录或指向目录的符号链接(nvm 常用 ln -s 别名)
      let isDir = e.isDirectory()
      if (!isDir && e.isSymbolicLink()) {
        try { isDir = statSync(join(base, e.name)).isDirectory() } catch { /* ignore */ }
      }
      if (!isDir) continue
      const m = /^v?(\d+)\.(\d+)\.(\d+)$/.exec(e.name)
      if (!m) continue
      const ver = `v${m[1]}.${m[2]}.${m[3]}`
      if (!nodeInRange(ver)) continue
      // nvm/volta/n: <root>/vX.Y.Z/bin/node;fnm: <root>/vX.Y.Z/installation/bin/node
      for (const rel of [`${e.name}/bin/${NODE_BIN}`, `${e.name}/installation/bin/${NODE_BIN}`]) {
        add(base, rel, ver)
      }
    }
  }

  // nvm
  vroot(join(HOME, '.nvm', 'versions', 'node'))
  // volta
  vroot(join(HOME, '.volta', 'tools', 'image', 'node'))
  // fnm(macOS 新路径 + linux 路径)
  vroot(join(HOME, 'Library', 'Application Support', 'fnm', 'node-versions'))
  vroot(join(HOME, '.local', 'share', 'fnm', 'node-versions'))
  // Homebrew keg-only node@22 / node@24(目录名不带版本,guess=null)
  for (const keg of ['/opt/homebrew/opt/node@22', '/usr/local/opt/node@22', '/opt/homebrew/opt/node@24', '/usr/local/opt/node@24']) {
    add(keg, `bin/${NODE_BIN}`)
  }
  // 本启动器托管目录(STATE_DIR/node/<vX.Y.Z>/bin/node,由 installDshNode 写入)
  vroot(join(STATE_DIR, 'node'))
  return list
}

let cached = null // { path, version, source } | null
let cacheValid = false

/** 使解析缓存失效(安装 Node 后调用)。 */
export function invalidateDshNodeCache() {
  cached = null
  cacheValid = false
}

/**
 * 解析可用的 dsh 兼容 Node。优先:
 *   1) 当前进程 execPath 若在范围;
 *   2) 扫描到的最高版本(22/24 都算,24 优先)。
 * @returns {{path:string, version:string, source:string} | null}
 */
export async function resolveDshNode() {
  if (cacheValid) return cached
  const curVersion = await probeVersion(process.execPath)
  if (curVersion && nodeInRange(curVersion)) {
    cached = { path: process.execPath, version: curVersion, source: '当前进程' }
    cacheValid = true
    return cached
  }

  // 扫描候选:guess 有值直接用,否则现场跑 --version 确认
  const found = []
  for (const { bin, guess } of candidateBins()) {
    let version = guess
    if (!version) version = await probeVersion(bin)
    if (version && nodeInRange(version)) found.push({ bin, version })
  }
  // 版本从高到低(24 优先于 22)
  found.sort((a, b) => cmp(parseNodeVersion(b.version), parseNodeVersion(a.version)))
  if (found.length > 0) {
    cached = { path: found[0].bin, version: found[0].version, source: '系统安装' }
    cacheValid = true
    return cached
  }
  cached = null
  cacheValid = true
  return null
}

// ── 官方 Node 下载安装 ────────────────────────────────────

/** 平台 → 官方分发文件名后缀。 */
function platformSuffix() {
  const p = process.platform
  const a = process.arch
  if (p === 'darwin') return a === 'arm64' ? 'darwin-arm64' : 'darwin-x64'
  if (p === 'win32') return 'win-x64'
  if (p === 'linux') return a === 'arm64' ? 'linux-arm64' : 'linux-x64'
  return null
}

function httpsGet(url, { headers = {}, timeout = 300000, onData = null } = {}) {
  return new Promise((resolve, reject) => {
    const transport = url.startsWith('https:') ? https : http
    const req = transport.get(url, {
      headers: { 'User-Agent': 'dsh-launcher', Accept: 'application/octet-stream', ...headers },
    }, (res) => {
      if ([301, 302, 303, 307, 308].includes(res.statusCode) && res.headers.location) {
        res.resume()
        httpsGet(new URL(res.headers.location, url).href, { headers, timeout, onData }).then(resolve, reject)
        return
      }
      if (res.statusCode !== 200) {
        res.resume()
        reject(new Error(`HTTP ${res.statusCode}`))
        return
      }
      if (onData) { onData(res); resolve(null); return }
      const chunks = []
      res.on('data', (c) => chunks.push(c))
      res.on('end', () => resolve(Buffer.concat(chunks)))
    })
    req.setTimeout(timeout, () => req.destroy(new Error('请求超时')))
    req.on('error', reject)
  })
}

/** 查询 nodejs.org dist index.json,取最新的 LTS v24。 */
async function latestLts24() {
  const buf = await httpsGet('https://nodejs.org/dist/index.json', { headers: { Accept: 'application/json' } })
  const list = JSON.parse(buf.toString('utf8'))
  const v24 = list
    .filter((e) => /^v24\.\d+\.\d+$/.test(e.version) && e.lts)
    .sort((a, b) => cmp(parseNodeVersion(b.version), parseNodeVersion(a.version)))
  if (v24.length === 0) throw new Error('nodejs.org index 中无 v24 LTS')
  return v24[0].version
}

/**
 * 下载并安装官方 Node 24 LTS 到 STATE_DIR/node/<vX.Y.Z>/。
 * @returns {{ok:true, path:string, version:string} | {ok:false, error:string}}
 */
export async function installDshNode({ onStage = () => {}, onLine = () => {} } = {}) {
  const suffix = platformSuffix()
  if (!suffix) return { ok: false, error: `不支持的平台 ${process.platform}/${process.arch}` }

  let version
  try {
    onStage('查询 Node 24 最新 LTS…')
    version = await latestLts24()
  } catch (err) {
    return { ok: false, error: `查询 nodejs.org 失败:${err.message}` }
  }
  onLine(`将安装 Node ${version}(官方 LTS,平台 ${suffix})`)

  const base = join(STATE_DIR, 'node')
  const targetDir = join(base, version)
  const binPath = join(targetDir, 'bin', NODE_BIN)
  if (existsSync(binPath)) {
    onLine(`Node ${version} 已存在(${binPath})`)
    return { ok: true, path: binPath, version }
  }

  const isWin = suffix.startsWith('win')
  const file = `node-${version}-${suffix}${isWin ? '.zip' : '.tar.gz'}`
  const url = `https://nodejs.org/dist/${version}/${file}`
  const tmpDir = join(base, `.tmp-${version}`)
  const tmpFile = join(base, file)

  try {
    mkdirSync(base, { recursive: true })
    rmSync(tmpDir, { recursive: true, force: true })
    rmSync(tmpFile, { force: true })

    // 1. 下载
    onStage('下载 Node 二进制…')
    let received = 0
    let lastPct = -1
    await new Promise((resolve, reject) => {
      httpsGet(url, {
        onData: (res) => {
          const total = Number(res.headers['content-length'] || 0)
          const out = createWriteStream(tmpFile)
          res.on('data', (c) => {
            received += c.length
            if (total) {
              const pct = Math.min(99, Math.round((received / total) * 100))
              if (pct !== lastPct) {
                lastPct = pct
                onStage(`下载 Node 二进制… ${pct}%`)
                if (pct % 20 === 0) onLine(`下载中 ${pct}%(共 ${Math.round(total / 1048576)}MB)`)
              }
            }
          })
          res.pipe(out)
          out.on('finish', resolve)
          out.on('error', reject)
          res.on('error', reject)
        },
      }).catch(reject)
    })

    // 2. 解压
    onStage('解压 Node…')
    mkdirSync(tmpDir, { recursive: true })
    if (isWin) {
      unzip(tmpFile, tmpDir)
    } else {
      await new Promise((resolve, reject) => {
        const tar = spawn('tar', ['-xzf', tmpFile, '-C', tmpDir], { stdio: ['ignore', 'pipe', 'pipe'] })
        tar.on('error', (err) => reject(new Error(`无法调用 tar:${err.message}`)))
        tar.on('close', (code) => code === 0 ? resolve() : reject(new Error(`tar 解压失败(码 ${code})`)))
      })
    }

    // 3. 定位并移入版本目录
    const inner = join(tmpDir, `node-${version}-${suffix}`)
    const srcBin = isWin ? join(inner, NODE_BIN) : join(inner, 'bin', NODE_BIN)
    if (!existsSync(srcBin)) throw new Error(`解压产物缺少 node 可执行文件(${srcBin})`)
    rmSync(targetDir, { recursive: true, force: true })
    renameSync(inner, targetDir)

    // 4. 校验
    const v = await probeVersion(binPath, 10000)
    if (!v || !nodeInRange(v)) {
      throw new Error(`安装后的 Node 版本异常(${v ?? '无法运行'}),已清理`)
    }

    rmSync(tmpFile, { force: true })
    rmSync(tmpDir, { recursive: true, force: true })
    onLine(`Node ${v} 安装完成 → ${binPath}`)
    invalidateDshNodeCache()
    return { ok: true, path: binPath, version: v }
  } catch (err) {
    rmSync(tmpDir, { recursive: true, force: true })
    rmSync(tmpFile, { force: true })
    rmSync(targetDir, { recursive: true, force: true })
    log('launcher', `Node 安装失败:${err.message}`, 'err')
    return { ok: false, error: err.message }
  }
}
