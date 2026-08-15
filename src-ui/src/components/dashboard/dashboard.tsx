import { Loader2 } from 'lucide-react'
import { ErrorCard } from '@/components/dashboard/error-card'
import { EnvCard } from '@/components/dashboard/env-card'
import { RepoCard } from '@/components/dashboard/repo-card'
import { ServiceCard } from '@/components/dashboard/service-card'
import type { LaunchMode } from '@/components/dashboard/main-action'
import type { AppSnapshot } from '@/types/schema'
import type { UiActionName } from '@/types/schema'

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
        <Loader2 className="size-6 animate-spin text-[var(--muted-foreground)]" />
      </div>
    )
  }
  return (
    <main className="flex-1 overflow-y-auto px-6 py-6">
      <div className="mx-auto grid max-w-[1120px] grid-cols-2 gap-4">
        <div className="col-span-2">
          <ErrorCard
            snap={snap}
            onJumpLogs={onJumpLogs}
            onRetry={() => onAction(snap.mode === 'dev' ? 'dev' : 'start')}
          />
        </div>
        <div className="col-span-2">
          <ServiceCard
            snap={snap}
            mode={mode}
            onModeChange={onModeChange}
            onAction={onAction}
            onOpenDsh={onOpenDsh}
            onStop={() => onAction('stop')}
          />
        </div>
        <RepoCard
          snap={snap}
          onUpdate={() => onAction('update')}
          onRebuild={() => onAction('rebuild')}
        />
        <EnvCard onInstallNode={() => onAction('install-node')} />
        <p className="col-span-2 pb-1 pt-1 text-center text-[11px] text-[var(--muted-foreground)]">
          DeepSeek Harness 运行于本机 · Launcher 仅负责启动、更新与进程托管
        </p>
      </div>
    </main>
  )
}
