import { ExternalLink, FolderOpen, ScrollText, Settings } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { SegmentedControl } from '@/components/ui/segmented-control'
import { MainAction } from '@/components/dashboard/main-action'
import type { AppSnapshot } from '@/types/schema'
import type { UiActionName } from '@/types/schema'

export type ModeTab = 'normal' | 'dev' | 'maintenance'

const TABS: { value: ModeTab; label: string }[] = [
  { value: 'normal', label: '普通运行' },
  { value: 'dev', label: '开发模式' },
  { value: 'maintenance', label: '维护' },
]

export function TopBar({
  snap,
  mode,
  onModeChange,
  onAction,
  onOpenDsh,
  onOpenLogs,
  onOpenRepo,
  onOpenSettings,
}: {
  snap: AppSnapshot | null
  mode: ModeTab
  onModeChange: (m: ModeTab) => void
  onAction: (a: UiActionName) => void
  onOpenDsh: () => void
  onOpenLogs: () => void
  onOpenRepo: () => void
  onOpenSettings: () => void
}) {
  return (
    <header
      data-tauri-drag-region
      className="flex h-16 shrink-0 items-center gap-4 border-b border-[var(--border)] bg-[var(--background)] px-5"
    >
      <div className="flex min-w-0 items-center gap-2.5" data-tauri-drag-region>
        <img src="/src/assets/logo.svg" alt="" className="size-7 rounded-lg" data-tauri-drag-region />
        <span className="whitespace-nowrap text-[15px] font-semibold" data-tauri-drag-region>
          DSH Launcher
        </span>
        <Button
          variant="ghost"
          size="icon"
          className="size-8"
          data-tauri-drag-region="false"
          title="设置"
          onClick={onOpenSettings}
        >
          <Settings className="size-4 text-[var(--muted-foreground)]" />
        </Button>
      </div>

      <div className="flex flex-1 justify-center" data-tauri-drag-region>
        <SegmentedControl
          options={TABS}
          value={mode}
          onChange={onModeChange}
          disabled={snap?.busy ?? false}
        />
      </div>

      <div className="flex items-center gap-1.5" data-tauri-drag-region>
        <Button variant="ghost" size="icon" className="size-8" data-tauri-drag-region="false" title="打开 dsh" onClick={onOpenDsh}>
          <ExternalLink className="size-4 text-[var(--muted-foreground)]" />
        </Button>
        <Button variant="ghost" size="icon" className="size-8" data-tauri-drag-region="false" title="日志" onClick={onOpenLogs}>
          <ScrollText className="size-4 text-[var(--muted-foreground)]" />
        </Button>
        <Button variant="ghost" size="icon" className="size-8" data-tauri-drag-region="false" title="仓库目录" onClick={onOpenRepo}>
          <FolderOpen className="size-4 text-[var(--muted-foreground)]" />
        </Button>
        <div className="ml-2" data-tauri-drag-region="false">
          <MainAction snap={snap} mode={mode} onAction={onAction} />
        </div>
      </div>
    </header>
  )
}
