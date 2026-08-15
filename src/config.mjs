// dsh-launcher · 设置读写(~/.config/dsh-launcher.json)
// 零依赖:仅 Node 内置模块。所有路径支持 ~ 展开。
import { homedir } from 'node:os'
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { connect } from 'node:net'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

/** 设置文件根目录(可被环境变量覆盖,便于测试与可移植)。 */
export const CONFIG_DIR = process.env.DSH_LAUNCHER_CONFIG_DIR || join(homedir(), '.config')
/** 运行态目录:pid 文件、状态快照、日志。 */
export const STATE_DIR = process.env.DSH_LAUNCHER_STATE_DIR || join(homedir(), '.local', 'state', 'dsh-launcher')
export const LOGS_DIR = join(STATE_DIR, 'logs')
export const CONFIG_FILE = join(CONFIG_DIR, 'dsh-launcher.json')
export const STATE_FILE = join(STATE_DIR, 'state.json')
export const PID_FILE = join(STATE_DIR, 'launcher.pid')
export const WEB_PID_FILE = join(STATE_DIR, 'dshweb.pid')
export const DEV_PID_FILE = join(STATE_DIR, 'devweb.pid')

/** 启动器自身监听端口(固定,单实例锚点)。 */
export const LAUNCHER_PORT = 3090
export const LAUNCHER_HOST = '127.0.0.1'

/** 启动器根目录(server.mjs 所在包根)。 */
export const ROOT_DIR = fileURLToPath(new URL('..', import.meta.url))

/** 启动器版本:统一从 package.json 读取(单一事实来源)。 */
function readLauncherVersion() {
  try {
    const manifest = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'))
    if (typeof manifest.version === 'string') return manifest.version
  } catch { /* fallthrough */ }
  return '0.0.0'
}
export const LAUNCHER_VERSION = readLauncherVersion()

/** 默认设置(与方案 F11 对齐)。 */
export const DEFAULTS = Object.freeze({
  repoPath: join(homedir(), 'Desktop', 'deepseek-harness'),
  port: 3080,
  host: '127.0.0.1',
  dshHome: '',             // 空 = 继承环境默认(~/.dsh)
  autostart: false,        // 开机自启(LaunchAgent)
  openBrowser: true,       // 就绪后自动打开主界面
  autoUpdateCheck: true,   // 内置更新:启动/定时自动检查 GitHub Releases
  buildArgs: '',           // 构建参数透传(追加到 pnpm run build 之后)
  readyTimeoutMs: 120_000, // 就绪等待上限
  startTimeoutMs: 120_000, // 启动等待上限
})

let cache = null

/** 展开路径中的 ~ 与 $HOME。 */
export function expandPath(p) {
  if (typeof p !== 'string' || p === '') return p
  const home = homedir()
  if (p === '~') return home
  if (p.startsWith('~/') || p.startsWith('~\\')) return join(home, p.slice(2))
  if (p.startsWith('$HOME/')) return join(home, p.slice(6))
  return p
}

/** 读设置(带默认值合并,惰性缓存)。 */
export function loadConfig() {
  if (cache) return cache
  let disk = {}
  try {
    disk = JSON.parse(readFileSync(CONFIG_FILE, 'utf8'))
  } catch {
    disk = {}
  }
  cache = { ...DEFAULTS, ...disk }
  return cache
}

/** 合并补丁并落盘;返回新配置。 */
export function saveConfig(patch) {
  const next = { ...loadConfig(), ...patch }
  try {
    mkdirSync(CONFIG_DIR, { recursive: true })
    writeFileSync(CONFIG_FILE, `${JSON.stringify(next, null, 2)}\n`, 'utf8')
  } catch (err) {
    throw new Error(`设置写入失败 ${CONFIG_FILE}: ${err.message}`)
  }
  cache = next
  return next
}

/** 确保运行态目录存在。 */
export function ensureDirs() {
  mkdirSync(STATE_DIR, { recursive: true })
  mkdirSync(LOGS_DIR, { recursive: true })
}

/** 仓库路径是否存在且是 git 仓库。 */
export function repoUsable(repoPath) {
  if (!repoPath || !existsSync(repoPath)) return { ok: false, reason: '目录不存在' }
  if (!existsSync(join(repoPath, '.git'))) return { ok: false, reason: '不是 git 仓库(缺少 .git)' }
  return { ok: true }
}

/** 端口是否被占用(127.0.0.1)。 */
export function probePort(port) {
  return new Promise((resolve) => {
    const sock = connect({ host: '127.0.0.1', port, timeout: 800 })
    sock.once('connect', () => { sock.destroy(); resolve(true) })
    sock.once('timeout', () => { sock.destroy(); resolve(false) })
    sock.once('error', () => { sock.destroy(); resolve(false) })
  })
}
