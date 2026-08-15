import { Ban, Loader2, XCircle } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { operationLabel } from '@/lib/actions'
import type { AppSnapshot } from '@/types/schema'

/** 长任务状态横幅:显示 operationId/阶段/进度;可取消时提供取消按钮。
 *  只显示运行中的任务;终态成功由 toast/快照状态体现。 */
export function OperationBanner({
  snap,
  onCancel,
}: {
  snap: AppSnapshot
  onCancel: () => void
}) {
  const op = snap.operation
  if (!op || op.status === 'success') return null

  const cancelled = op.status === 'cancelled' || op.status === 'interrupted'
  const failed = op.status === 'failed'

  return (
    <div
      data-tauri-drag-region="false"
      className={`flex items-center gap-3 border-b px-4 py-2 text-[12px] ${
        failed
          ? 'border-red-500/30 bg-red-500/10 text-red-500'
          : cancelled
            ? 'border-amber-500/30 bg-amber-500/10 text-amber-600'
            : 'border-blue-500/20 bg-blue-500/10 text-blue-600 dark:text-blue-400'
      }`}
    >
      {failed ? (
        <XCircle className="size-3.5 shrink-0" />
      ) : cancelled ? (
        <Ban className="size-3.5 shrink-0" />
      ) : (
        <Loader2 className="size-3.5 shrink-0 animate-spin" />
      )}
      <span className="min-w-0 flex-1 truncate">
        {failed
          ? `任务失败:${op.error ?? '未知错误'}`
          : cancelled
            ? op.error ?? '任务已取消(可重试,已完成的安全步骤不会重复)'
            : `任务 #${op.operationId} · ${operationLabel(op.kind)} — ${op.stage}${op.progress != null ? ` ${op.progress}%` : ''}`}
      </span>
      {op.cancellable && !failed && !cancelled && (
        <Button variant="ghost" size="sm" className="h-6 text-[12px]" onClick={onCancel}>
          取消
        </Button>
      )}
    </div>
  )
}
