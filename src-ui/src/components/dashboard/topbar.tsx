import { ExternalLink, FolderOpen, ScrollText, Settings } from 'lucide-react'
import { Button } from '@/components/ui/button'
import logoUrl from '@/assets/logo.svg'
import type { AppSnapshot } from '@/types/schema'

export function TopBar({
  snap,
  onOpenDsh,
  onOpenLogs,
  onOpenRepo,
  onOpenSettings,
}: {
  snap: AppSnapshot | null
  onOpenDsh: () => void
  onOpenLogs: () => void
  onOpenRepo: () => void
  onOpenSettings: () => void
}) {
  return (
    <header
      data-tauri-drag-region
      className="flex h-[72px] shrink-0 items-center gap-4 border-b border-[var(--border)] bg-[var(--header)] pl-[84px] pr-5 backdrop-blur-2xl"
    >
      <div className="flex min-w-0 items-center gap-3" data-tauri-drag-region>
        <img src={logoUrl} alt="" className="size-9 rounded-[11px] shadow-sm" data-tauri-drag-region />
        <div className="leading-tight" data-tauri-drag-region>
          <span className="block whitespace-nowrap text-[15px] font-semibold tracking-[-0.01em]" data-tauri-drag-region>
            DSH Launcher
          </span>
          <span className="mt-0.5 block text-[10px] font-medium text-[var(--muted-foreground)]" data-tauri-drag-region>
            v{snap?.version ?? '0.3.0'} · Desktop Core
          </span>
        </div>
        <Button
          variant="ghost"
          size="icon"
          className="ml-0.5 size-8 rounded-full"
          data-tauri-drag-region="false"
          title="设置"
          onClick={onOpenSettings}
        >
          <Settings className="size-4 text-[var(--muted-foreground)]" />
        </Button>
      </div>

      <div className="ml-auto flex items-center gap-1" data-tauri-drag-region>
        <Button variant="ghost" size="icon" className="size-9 rounded-full" data-tauri-drag-region="false" title="打开 dsh" onClick={onOpenDsh}>
          <ExternalLink className="size-4 text-[var(--muted-foreground)]" />
        </Button>
        <Button variant="ghost" size="icon" className="size-9 rounded-full" data-tauri-drag-region="false" title="日志" onClick={onOpenLogs}>
          <ScrollText className="size-4 text-[var(--muted-foreground)]" />
        </Button>
        <Button variant="ghost" size="icon" className="size-9 rounded-full" data-tauri-drag-region="false" title="仓库目录" onClick={onOpenRepo}>
          <FolderOpen className="size-4 text-[var(--muted-foreground)]" />
        </Button>
      </div>
    </header>
  )
}
