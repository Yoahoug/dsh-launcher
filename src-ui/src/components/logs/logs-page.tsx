import * as React from 'react'
import { ArrowDown, ArrowLeft, Copy, Eraser, FolderOpen, Pause, Play, Search } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Select } from '@/components/ui/select'
import { useToast } from '@/components/ui/toast'
import { api } from '@/hooks/use-app'
import { cn } from '@/lib/utils'
import type { LogEntry, LogLevel } from '@/types/schema'

const RING_CAP = 2_000

const LEVELS: { value: LogLevel | 'all'; label: string }[] = [
  { value: 'all', label: '全部级别' },
  { value: 'info', label: 'info' },
  { value: 'ok', label: 'ok' },
  { value: 'warn', label: 'warn' },
  { value: 'err', label: 'err' },
]

const LEVEL_CLASS: Record<LogLevel, string> = {
  info: 'text-[var(--muted-foreground)]',
  ok: 'text-[var(--success)]',
  warn: 'text-[var(--warning)]',
  err: 'text-[var(--danger)]',
}

function fmtTime(ts: number): string {
  const d = new Date(ts)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`
}

/** 日志页:历史 ring + 实时订阅 + 筛选/搜索/暂停/复制/清空/打开目录。 */
export function LogsPage({ initialLevel, onBack }: { initialLevel?: LogLevel; onBack: () => void }) {
  const [logs, setLogs] = React.useState<LogEntry[]>([])
  const [sources, setSources] = React.useState<string[]>([])
  const [source, setSource] = React.useState('all')
  const [level, setLevel] = React.useState<LogLevel | 'all'>(initialLevel ?? 'all')
  const [query, setQuery] = React.useState('')
  const [paused, setPaused] = React.useState(false)
  const [scrolledUp, setScrolledUp] = React.useState(false)
  const boxRef = React.useRef<HTMLDivElement>(null)
  const { toast } = useToast()

  // 初始拉取历史 ring
  React.useEffect(() => {
    let mounted = true
    void api.getLogs(0).then((page) => {
      if (!mounted) return
      setLogs(page.logs.slice(-RING_CAP))
      setSources(page.sources)
    })
    return () => {
      mounted = false
    }
  }, [])

  // 实时订阅(暂停时仍入队,但由用户恢复查看)
  React.useEffect(() => {
    const unsub = api.onLogAppended((entry) => {
      setLogs((prev) => {
        if (prev.some((l) => l.id === entry.id)) return prev
        const next = [...prev, entry]
        return next.length > RING_CAP ? next.slice(next.length - RING_CAP) : next
      })
    })
    return () => {
      void unsub.then((fn) => fn())
    }
  }, [])

  // 自动滚动:仅当未暂停且用户未上滚时
  React.useEffect(() => {
    const box = boxRef.current
    if (box && !paused && !scrolledUp) {
      box.scrollTop = box.scrollHeight
    }
  }, [logs, paused, scrolledUp])

  const onScroll = () => {
    const box = boxRef.current
    if (!box) return
    setScrolledUp(box.scrollHeight - box.scrollTop - box.clientHeight > 40)
  }

  const filtered = logs.filter((l) => {
    if (source !== 'all' && l.src !== source) return false
    if (level !== 'all' && l.level !== level) return false
    if (query && !l.text.toLowerCase().includes(query.toLowerCase())) return false
    return true
  })

  const copy = async (text: string, label: string) => {
    try {
      await navigator.clipboard.writeText(text)
      toast({ kind: 'success', title: `${label}已复制` })
    } catch {
      toast({ kind: 'error', title: '复制失败' })
    }
  }

  const clear = async () => {
    try {
      await api.clearLogs()
      setLogs([])
      toast({ kind: 'success', title: '日志已清空(本地视图)' })
    } catch (e) {
      toast({ kind: 'error', title: '清空失败', detail: String(e) })
    }
  }

  const empty = filtered.length === 0

  return (
    <main className="flex flex-1 flex-col overflow-hidden">
      <div
        data-tauri-drag-region
        className="flex h-[72px] shrink-0 items-center gap-3 border-b border-[var(--border)] bg-[var(--header)] pl-[84px] pr-5 backdrop-blur-2xl"
      >
        <Button variant="ghost" size="sm" onClick={onBack} aria-label="← 返回" data-tauri-drag-region="false">
          <ArrowLeft /> 返回
        </Button>
        <div data-tauri-drag-region>
          <h2 className="text-[15px] font-semibold" data-tauri-drag-region>运行日志</h2>
          <p className="mt-0.5 text-[10px] text-[var(--muted-foreground)]" data-tauri-drag-region>{filtered.length} 条记录 · 最多保留 {RING_CAP} 条</p>
        </div>
        <div className="flex-1" data-tauri-drag-region />
        <Button variant={paused ? 'primary' : 'ghost'} size="sm" onClick={() => setPaused((p) => !p)} data-tauri-drag-region="false">
          {paused ? <Play /> : <Pause />}
          {paused ? '继续' : '暂停'}
        </Button>
        <Button variant="ghost" size="sm" onClick={() => void copy(filtered.map((l) => l.text).join('\n'), '全部日志')} data-tauri-drag-region="false">
          <Copy /> 复制
        </Button>
        <Button variant="ghost" size="sm" onClick={() => void clear()} data-tauri-drag-region="false">
          <Eraser /> 清空
        </Button>
        <Button variant="ghost" size="sm" onClick={() => void api.openLogDirectory()} data-tauri-drag-region="false">
          <FolderOpen /> 打开目录
        </Button>
      </div>

      <div className="flex shrink-0 items-center gap-2 border-b border-[var(--border)] bg-[var(--card)]/70 px-5 py-2.5">
        <Select
          className="h-8 w-36 text-xs"
          options={[
            { value: 'all', label: '全部来源' },
            ...sources.map((s) => ({ value: s, label: s })),
          ]}
          value={source}
          onChange={setSource}
          aria-label="日志来源"
        />
        <Select
          className="h-8 w-28 text-xs"
          options={LEVELS}
          value={level}
          onChange={setLevel}
          aria-label="日志级别"
        />
        <div className="relative flex-1">
          <Search className="pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-[var(--muted-foreground)]" />
          <Input
            className="h-8 w-full pl-8 text-xs"
            placeholder="搜索日志…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
      </div>

      <div
        ref={boxRef}
        onScroll={onScroll}
        className="relative m-4 mt-3 flex-1 overflow-y-auto rounded-2xl border border-[var(--border)] bg-[var(--card)] px-4 py-3 font-mono text-[12px] leading-relaxed shadow-[var(--shadow-card)]"
        role="log"
        aria-live="polite"
      >
        {empty ? (
          <div className="flex h-full flex-col items-center justify-center gap-1 text-[var(--muted-foreground)]">
            <p>暂无日志</p>
            <p className="text-xs">执行动作后日志会实时出现在这里</p>
          </div>
        ) : (
          filtered.map((l) => (
            <div key={l.id} className="-mx-2 flex gap-3 rounded-lg px-2 py-1 whitespace-pre-wrap break-all hover:bg-[var(--muted)]/70">
              <span className="shrink-0 text-[var(--muted-foreground)]">{fmtTime(l.ts)}</span>
              <span className={cn('w-12 shrink-0 uppercase', LEVEL_CLASS[l.level])}>{l.level}</span>
              <span className="w-16 shrink-0 text-[var(--muted-foreground)]">{l.src}</span>
              <span className="min-w-0 flex-1 text-[var(--foreground)]">{l.text}</span>
            </div>
          ))
        )}
        {!empty && (paused || scrolledUp) && (
          <button
            onClick={() => {
              setPaused(false)
              setScrolledUp(false)
              const box = boxRef.current
              if (box) box.scrollTop = box.scrollHeight
            }}
            className="sticky bottom-3 left-1/2 -translate-x-1/2 rounded-full border border-[var(--border)] bg-[var(--card)] px-3 py-1.5 text-xs font-medium shadow-md transition-colors hover:bg-[var(--muted)]"
          >
            <ArrowDown className="mr-1 inline size-3.5" />
            跳到底部
          </button>
        )}
      </div>
    </main>
  )
}
