import { AlertTriangle, ArrowRight, Copy, RefreshCw } from 'lucide-react'
import { useState } from 'react'
import { useToast } from '@/components/ui/toast'
import { cn } from '@/lib/utils'
import type { AppSnapshot } from '@/types/schema'

/** 失败态红色摘要卡:复制详情 / 跳转日志 / 重试(cc-switch 危险横幅风格)。 */
export function ErrorCard({
  snap,
  onJumpLogs,
  onRetry,
}: {
  snap: AppSnapshot
  onJumpLogs: () => void
  onRetry: () => void
}) {
  const [open, setOpen] = useState(false)
  const { toast } = useToast()
  if (!snap.error) return null

  const copyDetail = async () => {
    const text = `${snap.error!.summary}\n${snap.error!.detail}`
    try {
      await navigator.clipboard.writeText(text)
      toast({ kind: 'success', title: '错误详情已复制' })
    } catch {
      toast({ kind: 'error', title: '复制失败' })
    }
  }

  return (
    <div className="rounded-xl border border-red-500/25 bg-red-500/[0.04] px-5 py-3.5 animate-slide-down dark:bg-red-500/[0.08]">
      <button
        className="flex w-full items-center gap-2.5 text-left"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
      >
        <AlertTriangle className="size-4 shrink-0 text-red-500" />
        <span className="flex-1 text-sm font-medium text-red-600 dark:text-red-400">{snap.error.summary}</span>
        <span className="text-xs text-muted-foreground">{open ? '收起' : '展开详情'}</span>
      </button>
      {open && snap.error.detail ? (
        <pre className={cn('mt-2.5 overflow-x-auto whitespace-pre-wrap rounded-lg bg-card p-3 font-mono text-xs leading-relaxed')}>
          {snap.error.detail}
        </pre>
      ) : null}
      <div className="mt-2 flex items-center gap-2" onClick={(e) => e.stopPropagation()}>
        <button
          className="inline-flex items-center gap-1 rounded-md px-2 py-1 text-xs text-red-500 hover:bg-red-500/10"
          onClick={() => void copyDetail()}
        >
          <Copy className="size-3.5" /> 复制详情
        </button>
        <button
          className="inline-flex items-center gap-1 rounded-md px-2 py-1 text-xs text-red-500 hover:bg-red-500/10"
          onClick={onJumpLogs}
        >
          <ArrowRight className="size-3.5" /> 查看日志
        </button>
        <button
          className="inline-flex items-center gap-1 rounded-md px-2 py-1 text-xs text-red-500 hover:bg-red-500/10"
          onClick={onRetry}
        >
          <RefreshCw className="size-3.5" /> 重试
        </button>
      </div>
    </div>
  )
}
