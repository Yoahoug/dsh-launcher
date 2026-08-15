import { AlertTriangle } from 'lucide-react'
import { useState } from 'react'
import { cn } from '@/lib/utils'
import type { AppSnapshot } from '@/types/schema'

/** 失败态红色摘要卡,可展开详情。 */
export function ErrorCard({ snap }: { snap: AppSnapshot }) {
  const [open, setOpen] = useState(false)
  if (!snap.error) return null
  return (
    <div className="rounded-[var(--radius-card)] border border-[var(--danger)]/25 bg-[var(--danger)]/8 px-5 py-3.5">
      <button
        className="flex w-full items-center gap-2.5 text-left"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
      >
        <AlertTriangle className="size-4 shrink-0 text-[var(--danger)]" />
        <span className="flex-1 text-sm font-medium text-[var(--danger)]">{snap.error.summary}</span>
        <span className="text-xs text-[var(--muted-foreground)]">{open ? '收起' : '展开详情'}</span>
      </button>
      {open && snap.error.detail ? (
        <pre className={cn('mt-2.5 overflow-x-auto whitespace-pre-wrap rounded-lg bg-[var(--card)] p-3 font-mono text-xs leading-relaxed')}>
          {snap.error.detail}
        </pre>
      ) : null}
    </div>
  )
}
