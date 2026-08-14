// dsh-launcher · RepoManager:git 同步(只读状态 + fetch / stash / pull --rebase --autostash)
// 铁律:冲突只报告、绝不 reset --hard;本地改动默认 stash,可恢复。
import { spawn } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { log } from './log.mjs'
import { children } from './process.mjs'
import { toolEnv } from './tools.mjs'

/** 运行 git 命令,流式输出到日志;返回 { code, lines, tail }。 */
export function runGit(cwd, args, { label = `git ${args[0] ?? ''}` } = {}) {
  return new Promise((resolve) => {
    const child = spawn('git', args, { cwd, env: toolEnv(), stdio: ['ignore', 'pipe', 'pipe'] })
    children.op = child
    const lines = []
    let tail = []
    const onLine = (line) => {
      lines.push(line)
      tail.push(line)
      if (tail.length > 40) tail.shift()
      log('git', line)
    }
    child.stdout.on('data', (buf) => String(buf).split(/\r?\n/).filter(Boolean).forEach(onLine))
    child.stderr.on('data', (buf) => String(buf).split(/\r?\n/).filter(Boolean).forEach(onLine))
    child.on('error', (err) => {
      log('git', `无法执行 git:${err.message}`, 'err')
      resolve({ code: -1, lines, tail: [`git: ${err.message}`] })
    })
    child.on('close', (code) => {
      children.op = null
      log('git', `${label} → 退出码 ${code}`)
      resolve({ code, lines, tail })
    })
  })
}

/** 执行 git 并静默收集输出(状态查询类,不打扰日志)。 */
function runGitQuiet(cwd, args) {
  return new Promise((resolve) => {
    const child = spawn('git', args, { cwd, env: toolEnv(), stdio: ['ignore', 'pipe', 'pipe'] })
    let out = ''
    child.stdout.on('data', (b) => { out += b })
    child.on('error', () => resolve({ code: -1, out: '' }))
    child.on('close', (code) => resolve({ code, out: out.trim() }))
  })
}

/** 当前分支名。 */
export async function currentBranch(cwd) {
  const r = await runGitQuiet(cwd, ['branch', '--show-current'])
  return r.code === 0 ? r.out : ''
}

/** 短 HEAD。 */
export async function headShort(cwd) {
  const r = await runGitQuiet(cwd, ['rev-parse', '--short', 'HEAD'])
  return r.code === 0 ? r.out : ''
}

/** 相对 origin/<branch> 落后/领先数(依赖最近一次 fetch)。 */
export async function aheadBehind(cwd, branch) {
  const r = await runGitQuiet(cwd, ['rev-list', '--left-right', '--count', `HEAD...refs/remotes/origin/${branch}`])
  if (r.code !== 0 || !/^\d+\s+\d+$/.test(r.out)) return { ahead: -1, behind: -1 }
  const [a, b] = r.out.split(/\s+/).map(Number)
  return { ahead: a, behind: b }
}

/** 工作区是否 dirty(porcelain 非空)。 */
export async function isDirty(cwd) {
  const r = await runGitQuiet(cwd, ['status', '--porcelain'])
  if (r.code !== 0) return false
  return r.out !== ''
}

/** 冲突文件列表(diff-filter=U)。 */
export async function conflictedFiles(cwd) {
  const r = await runGitQuiet(cwd, ['diff', '--name-only', '--diff-filter=U'])
  return r.code === 0 && r.out ? r.out.split(/\r?\n/).filter(Boolean) : []
}

/** rebase 是否进行中。 */
export function rebaseInProgress(cwd) {
  try {
    return readFileSync(join(cwd, '.git', 'rebase-merge', 'head-name'), 'utf8') !== '' ||
      readFileSync(join(cwd, '.git', 'rebase-apply', 'rebasing'), 'utf8') !== ''
  } catch {
    return false
  }
}

/** 仓库状态快照(供状态条)。 */
export async function repoStatus(cwd, { syncAt = null } = {}) {
  const branch = await currentBranch(cwd)
  const [head, dirty, ab] = await Promise.all([
    headShort(cwd), isDirty(cwd), aheadBehind(cwd, branch),
  ])
  return {
    branch, head, dirty, dirtyFiles: 0,
    ahead: ab.ahead, behind: ab.behind, syncAt,
    remoteUpToDate: ab.behind === 0,
  }
}

/** fetch origin(网络失败给出可读诊断)。 */
export async function gitFetch(cwd) {
  const r = await runGit(cwd, ['fetch', 'origin'], { label: 'git fetch origin' })
  if (r.code === 0) return { ok: true }
  const detail = r.tail.join(' ').slice(0, 400)
  return {
    ok: false,
    error: detail.includes('Could not resolve host') || detail.includes('Failed to connect') || detail.includes('Operation timed out')
      ? '网络无法连接远端(检查网络/代理/远端地址)'
      : detail,
  }
}

/** 完整同步:fetch → dirty 自动 stash → pull --rebase --autostash → 冲突只报告。 */
export async function gitSync(cwd) {
  log('git', 'git fetch origin …')
  const fetch = await gitFetch(cwd)
  if (!fetch.ok) return { ok: false, stage: 'fetch', error: fetch.error }

  const branch = await currentBranch(cwd)
  const ab = await aheadBehind(cwd, branch)
  const dirty = await isDirty(cwd)

  let stashed = false
  if (dirty) {
    log('git', '工作区有未提交改动,自动 git stash push -u(可随时 git stash pop 恢复)')
    const st = await runGit(cwd, ['stash', 'push', '-u', '-m', `dsh-launcher autostash ${Date.now()}`],
      { label: 'git stash push -u' })
    if (st.code !== 0) {
      return { ok: false, stage: 'stash', error: '自动暂存失败:工作区有冲突性改动或文件被占用', tail: st.tail }
    }
    stashed = true
  }

  log('git', `git pull --rebase --autostash(落后 ${ab.behind >= 0 ? ab.behind : '?'} 个提交)`)
  const pull = await runGit(cwd, ['pull', '--rebase', '--autostash'], { label: 'git pull --rebase --autostash' })
  if (pull.code !== 0) {
    const conflicts = await conflictedFiles(cwd)
    const inRebase = rebaseInProgress(cwd)
    if (conflicts.length > 0 || inRebase) {
      return {
        ok: false, stage: 'conflict', stashed, conflicts,
        error: 'rebase 冲突:工作区未被破坏,请手动解决(编辑冲突文件 → git add → git rebase --continue;或 git rebase --abort 放弃本次合并)',
        tail: pull.tail,
      }
    }
    return { ok: false, stage: 'pull', stashed, error: pull.tail.join(' ').slice(0, 400) }
  }

  log('git', `pull 完成 → ${await headShort(cwd)};落后 ${ab.behind} → ${ab.behind === 0 ? '已是最新' : '已更新'}`, 'ok')
  return { ok: true, stashed, behind: ab.behind, dirty }
}

/** lockfile(pnpm-lock.yaml)在 from..HEAD 之间是否变化。 */
export async function lockfileChanged(cwd, from) {
  if (!from) return false
  const r = await runGitQuiet(cwd, ['diff', '--name-only', `${from}..HEAD`, '--', 'pnpm-lock.yaml'])
  return r.code === 0 && r.out !== ''
}
