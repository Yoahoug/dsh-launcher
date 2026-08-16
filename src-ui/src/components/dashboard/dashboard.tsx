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
  // 只有开发模式依赖本地仓库；普通模式使用安装包内的预构建 Harness。
  const noRepo = mode === 'dev' && snap.repo.behind < 0 && !snap.repo.branch
  return (
    <div className="h-full overflow-y-auto px-6 py-8">
      <div className="mx-auto flex max-w-[720px] flex-col gap-4">
        <ErrorCard
          snap={snap}
          onJumpLogs={onJumpLogs}
          onRetry={() => onAction(snap.mode === 'dev' ? 'dev' : 'start')}
        />
        {noRepo && (
          <div className="rounded-xl border border-amber-500/25 bg-amber-500/[0.05] px-5 py-3.5 animate-slide-down dark:bg-amber-500/[0.08]">
            <p className="text-sm font-medium text-amber-600 dark:text-amber-400">
              尚未配置可用的 DeepSeek Harness 仓库
            </p>
            <p className="mt-1 text-xs text-muted-foreground">
              请通过左侧「仓库与构建 → 克隆仓库」一键克隆并初始化,或在「工具链」中一键安装缺失环境(Node / Git /
              pnpm);完成后回到这里点击启动。
            </p>
          </div>
        )}
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
