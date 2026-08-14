// dsh-launcher · LogHub:环形缓冲 + 文件落盘 + SSE 广播
// 日志来源:dsh web / dev:web / git / pnpm / launcher;级别:info/ok/warn/err。
// 落盘按日期轮转:~/.local/state/dsh-launcher/logs/YYYY-MM-DD.log
import { appendFileSync } from 'node:fs'
import { join } from 'node:path'
import { LOGS_DIR } from './config.mjs'

const RING_SIZE = 4000 // 环形缓冲上限(条)
const SRC_ORDER = ['launcher', 'dsh web', 'dev:web', 'git', 'pnpm']

let seq = 0
const entries = []
const subscribers = new Set()
let today = null
let fileHandle = null // 惰性打开,保持简单:逐条 appendFileSync(频率可控)

function dayKey(d = new Date()) {
  const p = (n) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`
}

function todayFile() {
  const key = dayKey()
  if (key !== today) {
    today = key
    // 新的一天,换文件
    fileHandle = join(LOGS_DIR, `${key}.log`)
  }
  return fileHandle
}

/**
 * 写一条日志。
 * @param {string} src 来源
 * @param {string} text 内容
 * @param {'info'|'ok'|'warn'|'err'} level 级别
 */
export function log(src, text, level = 'info') {
  const entry = {
    id: ++seq,
    ts: Date.now(),
    src,
    level,
    text,
  }
  entries.push(entry)
  if (entries.length > RING_SIZE) entries.splice(0, entries.length - RING_SIZE)

  // 落盘(尽力而为,失败不阻塞)
  try {
    const iso = new Date(entry.ts).toISOString()
    appendFileSync(todayFile(), `[${iso}] [${src}] [${level}] ${text}\n`, 'utf8')
  } catch {
    /* 落盘失败不致命 */
  }

  for (const fn of subscribers) {
    try { fn(entry) } catch { /* 订阅者异常不扩散 */ }
  }
  return entry
}

/** 订阅新日志。返回取消函数。 */
export function subscribe(fn) {
  subscribers.add(fn)
  return () => subscribers.delete(fn)
}

/** 取快照(id 递增;since 之后的部分)。 */
export function snapshot(since = 0) {
  return entries.filter((e) => e.id > since)
}

/** 清空内存缓冲(文件保留,便于回溯)。 */
export function clearRing() {
  entries.length = 0
  log('launcher', '日志缓冲已清空(文件日志保留,见 ~/.local/state/dsh-launcher/logs/)')
}

/** 来源清单(供前端筛选渲染)。 */
export function sources() {
  return SRC_ORDER
}
