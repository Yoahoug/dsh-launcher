import { Loader2 } from 'lucide-react'
import { ErrorCard } from '@/components/dashboard/error-card'
import { EnvCard } from '@/components/dashboard/env-card'
import { RepoCard } from '@/components/dashboard/repo-card'
import { ServiceCard } from '@/components/dashboard/service-card'
import type { AppSnapshot } from '@/types/schema'
import type { UiActionName } from '@/types/schema'

export function Dashboard({
  snap,
  onAction,
  onOpenDsh,
  onJumpLogs,
}: {
  snap: AppSnapshot | null
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
    <main className="flex-1 space-y-4 overflow-y-auto p-5">
      <ErrorCard
        snap={snap}
        onJumpLogs={onJumpLogs}
        onRetry={() => onAction(snap.mode === 'dev' ? 'dev' : 'start')}
      />
      <ServiceCard snap={snap} onOpenDsh={onOpenDsh} onStop={() => onAction('stop')} />
      <RepoCard
        snap={snap}
        onUpdate={() => onAction('update')}
        onRebuild={() => onAction('rebuild')}
      />
      <EnvCard onInstallNode={() => onAction('install-node')} />
      <p className="pb-1 text-center text-[11px] text-[var(--muted-foreground)]">
        主界面为 dsh web(http://127.0.0.1:3080/),launcher 只负责托管与维护
      </p>
    </main>
  )
}
