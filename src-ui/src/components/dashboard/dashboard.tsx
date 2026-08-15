import { Loader2 } from 'lucide-react'
import { ErrorCard } from '@/components/dashboard/error-card'
import { ServiceCard } from '@/components/dashboard/service-card'
import type { LaunchMode } from '@/components/dashboard/main-action'
import type { AppSnapshot } from '@/types/schema'
import type { UiActionName } from '@/types/schema'

/** 服务主页(单视图聚焦,复刻 cc-switch:一屏只做一件事)。 */
export function Dashboard({
  snap,
  mode,
  onModeChange,
  onAction,
  onOpenDsh,
  onJumpLogs,
}: {
  snap: AppSnapshot | null
  mode: LaunchMode
  onModeChange: (mode: LaunchMode) => void
  onAction: (a: UiActionName) => void
  onOpenDsh: () => void
  onJumpLogs: () => void
}) {
  if (!snap) {
    return (
      <div className="flex flex-1 items-center justify-center">
        <Loader2 className="size-6 animate-spin text-muted-foreground" />
      </div>
    )
  }
  return (
    <div className="h-full overflow-y-auto px-6 py-8">
      <div className="mx-auto flex max-w-[720px] flex-col gap-4">
        <ErrorCard
          snap={snap}
          onJumpLogs={onJumpLogs}
          onRetry={() => onAction(snap.mode === 'dev' ? 'dev' : 'start')}
        />
        <div className="animate-slide-up">
          <ServiceCard
            snap={snap}
            mode={mode}
            onModeChange={onModeChange}
            onAction={onAction}
            onOpenDsh={onOpenDsh}
            onStop={() => onAction('stop')}
          />
        </div>
        <p className="pb-1 pt-1 text-center text-[11px] text-muted-foreground">
          DeepSeek Harness 运行于本机 · Launcher 仅负责启动、更新与进程托管
        </p>
      </div>
    </div>
  )
}
