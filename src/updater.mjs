// dsh-launcher · 内置更新器(类 cc-switch:检查 GitHub Releases 最新版 → 下载 → 切换 → 重启)
// 安装形态:
//   git      — 从 git 检出运行 → 用控制台「更新并构建」,不走内置更新
//   app      — macOS .app 包(Contents/Resources)
//   portable — Windows 便携包 / 手工打包(根目录有 launcher.json)
// 更新机制:下载平台 zip → 解压到 apps/<version>/ → 校验 → 原子切换 launcher.json 的
//          current 指针 → 拉起新版本 server → 旧进程退出(托管中的 dsh web 由新进程召回)。
import { spawn } from 'node:child_process'
import { createWriteStream, existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs'
import { join, normalize } from 'node:path'
import { fileURLToPath } from 'node:url'
import https from 'node:https'
import http from 'node:http'
import { LAUNCHER_VERSION, ROOT_DIR, STATE_DIR } from './config.mjs'
import { unzip } from './zip.mjs'
import { log } from './log.mjs'

/** GitHub Releases 最新版 API(可被环境变量覆盖,便于测试)。 */
const UPDATE_API = process.env.DSH_UPDATE_URL || 'https://api.github.com/repos/Yoahoug/dsh-launcher/releases/latest'
const OWNER_REPO = 'Yoahoug/dsh-launcher'

function httpsGet(url, { headers = {}, timeout = 20000, onData = null } = {}) {
  return new Promise((resolve, reject) => {
    const transport = url.startsWith('https:') ? https : http
    const req = transport.get(url, {
      headers: { 'User-Agent': `dsh-launcher/${LAUNCHER_VERSION}`, Accept: 'application/octet-stream', ...headers },
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

/** 版本号 → [maj,min,patch](去 v 前缀)。 */
function parseVersion(v) {
  const m = /^v?(\d+)\.(\d+)\.(\d+)/.exec(String(v ?? ''))
  return m ? [Number(m[1]), Number(m[2]), Number(m[3])] : null
}
function cmpVersion(a, b) {
  for (let i = 0; i < 3; i++) {
    if (a[i] !== b[i]) return a[i] - b[i]
  }
  return 0
}

/** 当前安装形态。 */
export function installMode() {
  if (existsSync(join(ROOT_DIR, '.git'))) return 'git'
  if (process.platform === 'darwin' && ROOT_DIR.includes('.app/Contents/Resources')) return 'app'
  if (existsSync(join(packageRoot(), 'launcher.json'))) return 'portable'
  return 'dev'
}

/**
 * 包根目录:server 运行于 <package>/apps/<ver>/src/server.mjs,
 * 包根(launcher.json / apps/ 所在)在 server 文件上两级。
 */
export function packageRoot() {
  return normalize(join(ROOT_DIR, '..', '..'))
}

/** 平台资产匹配:mac → 含 darwin;win → 含 windows。 */
function pickAsset(assets) {
  const want = process.platform === 'darwin' ? 'darwin' : process.platform === 'win32' ? 'windows' : null
  if (!want) return null
  return (assets || []).find((a) => /\.zip$/i.test(a.name) && a.name.includes(want)) || null
}

/** 检查更新。 */
export async function checkForUpdate() {
  const mode = installMode()
  if (mode === 'git' || mode === 'dev') {
    return { available: false, mode, reason: 'git-checkout', message: 'git 检出模式:请用控制台「更新并构建」拉取最新代码' }
  }
  let release
  try {
    const buf = await httpsGet(UPDATE_API, { headers: { Accept: 'application/vnd.github+json' } })
    release = JSON.parse(buf.toString('utf8'))
  } catch (err) {
    return { available: false, mode, reason: 'network', error: err.message, message: '检查更新失败(网络或 GitHub 不可达)' }
  }
  const remote = parseVersion(release.tag_name)
  const local = parseVersion(LAUNCHER_VERSION)
  if (!remote || !local) return { available: false, mode, reason: 'parse' }
  if (cmpVersion(remote, local) <= 0) {
    return { available: false, mode, current: LAUNCHER_VERSION, latest: release.tag_name, message: '已是最新版本' }
  }
  const asset = pickAsset(release.assets)
  if (!asset) {
    return { available: false, mode, reason: 'no-asset', latest: release.tag_name, message: `发现新版本 ${release.tag_name},但无当前平台资产` }
  }
  return {
    available: true,
    mode,
    current: LAUNCHER_VERSION,
    version: release.tag_name,
    url: asset.browser_download_url || asset.url,
    size: asset.size,
    notes: String(release.body || '').slice(0, 600),
  }
}

/** 下载到 STATE_DIR/update/,返回文件路径。 */
async function downloadZip(info, onProgress) {
  mkdirSync(join(STATE_DIR, 'update'), { recursive: true })
  const dest = join(STATE_DIR, 'update', `update-${info.version}.zip`)
  let received = 0
  const total = info.size || 0
  await new Promise((resolve, reject) => {
    httpsGet(info.url, {
      onData: (res) => {
        const out = createWriteStream(dest)
        const total2 = Number(res.headers['content-length'] || total)
        res.on('data', (c) => {
          received += c.length
          if (total2 && onProgress) onProgress(Math.min(99, Math.round((received / total2) * 100)))
        })
        res.pipe(out)
        out.on('finish', resolve)
        out.on('error', reject)
        res.on('error', reject)
      },
    }).catch((err) => reject(new Error(`下载失败:${err.message}`)))
  })
  return dest
}

/**
 * 在解压目录里定位 <ver> 版本目录(兼容三种包布局:裸 apps/、.app、windows-x64 便携包)。
 * @returns 版本目录绝对路径,找不到返回 null
 */
function findVersionDir(payloadDir, ver) {
  const rels = [
    ['apps', ver],
    ['dsh-launcher.app', 'Contents', 'Resources', 'apps', ver],
    ['dsh-launcher-windows-x64', 'apps', ver],
  ]
  for (const rel of rels) {
    const dir = join(payloadDir, ...rel)
    if (existsSync(join(dir, 'src', 'server.mjs'))) return dir
  }
  return null
}

/** 下载并应用更新;成功后返回 { ok:true, version }。 */
export async function downloadAndApply(info, { onProgress = () => {} } = {}) {
  const mode = installMode()
  if (mode !== 'app' && mode !== 'portable') {
    return { ok: false, error: `当前形态(${mode})不支持内置更新` }
  }
  const ver = info.version.replace(/^v/, '')
  const updDir = join(STATE_DIR, 'update')
  const payloadDir = join(updDir, 'payload')
  const pkgRoot = packageRoot()
  const targetVerDir = join(pkgRoot, 'apps', ver)

  try {
    // 1. 下载
    log('launcher', `开始下载更新 ${info.current} → ${info.version} …`)
    const zipPath = await downloadZip(info, (pct) => onProgress(pct))
    // 2. 解压并定位版本目录
    rmSync(payloadDir, { recursive: true, force: true })
    mkdirSync(payloadDir, { recursive: true })
    log('launcher', '解压更新包 …')
    unzip(zipPath, payloadDir)
    const verDir = findVersionDir(payloadDir, ver)
    if (!verDir) {
      throw new Error(`更新包缺少 apps/${ver}/src/server.mjs,已中止(未做任何改动)`)
    }
    // 3. 校验版本
    const newJson = join(verDir, 'package.json')
    if (existsSync(newJson)) {
      const pkg = JSON.parse(readFileSync(newJson, 'utf8'))
      if (pkg.version && pkg.version !== ver) {
        throw new Error(`更新包版本不符(${pkg.version} ≠ ${ver}),已中止`)
      }
    }
    // 4. 移入 apps/ 目录(保留旧版本便于回滚)
    rmSync(targetVerDir, { recursive: true, force: true })
    renameSync(verDir, targetVerDir)
    // 5. 原子切换 current 指针
    const ljPath = join(pkgRoot, 'launcher.json')
    const ljTmp = join(pkgRoot, 'launcher.json.tmp')
    writeFileSync(ljTmp, `${JSON.stringify({ current: ver }, null, 2)}\n`, 'utf8')
    renameSync(ljTmp, ljPath)
    // 6. 清理临时文件
    rmSync(zipPath, { force: true })
    rmSync(payloadDir, { recursive: true, force: true })
    // 7. 拉起新版本 server(旧进程随后退出,托管中的 dsh web 由新进程召回)
    const newServer = join(targetVerDir, 'src', 'server.mjs')
    log('launcher', `更新就绪 → 重启到 v${ver}(${newServer})`, 'ok')
    const child = spawn(process.execPath, [newServer], {
      detached: true,
      stdio: 'ignore',
      env: { ...process.env, DSH_LAUNCHER_UPDATED_FROM: LAUNCHER_VERSION },
    })
    child.unref()
    return { ok: true, version: ver }
  } catch (err) {
    // 回滚:删除半成品
    rmSync(targetVerDir, { recursive: true, force: true })
    rmSync(payloadDir, { recursive: true, force: true })
    log('launcher', `更新失败:${err.message}`, 'err')
    return { ok: false, error: err.message }
  }
}

export { OWNER_REPO }
