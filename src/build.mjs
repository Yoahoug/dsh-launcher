// dsh-launcher · BuildManager:依赖安装(lockfile 比对)+ 构建(阶段进度)
// pnpm run build 内部为 build:lib:host → build:lib:client → build:web,按行识别阶段上报。
import { spawn } from 'node:child_process'
import { log } from './log.mjs'
import { children } from './process.mjs'
import { lockfileChanged } from './repo.mjs'

const STAGE_RE = /build:lib:host|build:lib:client|build:web/
const STAGE_LABEL = {
  'build:lib:host': '构建 lib(host)…',
  'build:lib:client': '构建 lib(client)…',
  'build:web': '构建 web 前端…',
}

/** 定位失败阶段(tsc / tsdown / vite)。 */
function blameStage(tail) {
  const joined = tail.join('\n')
  if (/error TS\d|TS\d{4}/.test(joined)) return 'tsc 类型检查错误'
  if (/tsdown/.test(joined)) return 'tsdown 打包错误'
  if (/vite|rollup/i.test(joined)) return 'vite 构建错误'
  return '构建错误'
}

/** 运行 pnpm 命令,流式输出;track=true 时注册为可停止的流程子进程。 */
export function runPnpm(cwd, args, { label, track = true, onLine } = {}) {
  return new Promise((resolve) => {
    const child = spawn('pnpm', args, { cwd, env: { ...process.env }, stdio: ['ignore', 'pipe', 'pipe'] })
    if (track) children.op = child
    const tail = []
    const push = (line, level = 'info') => {
      tail.push(line)
      if (tail.length > 40) tail.shift()
      log('pnpm', line, level)
      if (onLine) onLine(line)
    }
    child.stdout.on('data', (b) => String(b).split(/\r?\n/).filter(Boolean).forEach((l) => push(l)))
    child.stderr.on('data', (b) => String(b).split(/\r?\n/).filter(Boolean).forEach((l) => push(l, 'warn')))
    child.on('error', (err) => {
      log('pnpm', `无法执行 pnpm:${err.message}`, 'err')
      if (track) children.op = null
      resolve({ ok: false, code: -1, tail: [`pnpm: ${err.message}`], stage: null })
    })
    child.on('close', (code) => {
      if (track) children.op = null
      log('pnpm', `${label} → 退出码 ${code}${code === 0 ? ' ✓' : ' ✗'}`)
      resolve({ ok: code === 0, code, tail, stage: null })
    })
  })
}

/** 安装依赖(仅当 pnpm-lock.yaml 变化)。返回 { needed, ok, error, tail }。 */
export async function installIfNeeded(cwd, { from, onStage }) {
  const changed = await lockfileChanged(cwd, from)
  if (!changed) {
    log('pnpm', 'pnpm-lock.yaml 无变化,跳过 pnpm install', 'ok')
    return { needed: false, ok: true }
  }
  if (onStage) onStage('安装依赖(pnpm install)…')
  log('pnpm', 'pnpm-lock.yaml 有变化 → pnpm install')
  const r = await runPnpm(cwd, ['install'], { label: 'pnpm install' })
  if (!r.ok) {
    return { needed: true, ok: false, error: '依赖安装失败', tail: r.tail }
  }
  return { needed: true, ok: true }
}

/** 构建:pnpm run build [args],按阶段上报。 */
export async function runBuild(cwd, { buildArgs = '', onStage } = {}) {
  if (onStage) onStage('构建中…')
  log('pnpm', 'pnpm run build 开始(阶段:build:lib:host → build:lib:client → build:web)')
  const args = ['run', 'build', ...(buildArgs ? buildArgs.split(/\s+/) : [])]
  const r = await runPnpm(cwd, args, {
    label: 'pnpm run build',
    onLine: (line) => {
      const m = STAGE_RE.exec(line)
      if (m && onStage) onStage(`${STAGE_LABEL[m[0]] ?? line}`)
    },
  })
  if (r.ok) {
    if (onStage) onStage('构建完成 ✓')
    log('pnpm', '构建完成 ✓', 'ok')
    return { ok: true }
  }
  const stage = blameStage(r.tail)
  log('pnpm', `${stage} — 构建失败,退出码 ${r.code}`, 'err')
  return { ok: false, error: stage, tail: r.tail }
}
