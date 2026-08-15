import { ExternalLink, FolderOpen, ScrollText, Settings } from 'lucide-react'
import { Button } from '@/components/ui/button'
import logoUrl from '@/assets/logo.svg'
import type { AppSnapshot } from '@/types/schema'

/** 全局玻璃 header:品牌 + 右侧动作图标。 */
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
      className="fixed inset-x-0 top-0 z-50 h-16 border-b border-border bg-background/80 backdrop-blur-md"
    >
      <div className="flex h-full items-center justify-between gap-2 pl-[84px] pr-4">
        <div className="flex min-w-0 items-center gap-3" data-tauri-drag-region>
          <img src={logoUrl} alt="" className="size-8 rounded-lg" data-tauri-drag-region />
          <div className="leading-tight" data-tauri-drag-region>
            <span className="block whitespace-nowrap text-[15px] font-semibold" data-tauri-drag-region>
              DSH Launcher
            </span>
            <span className="mt-0.5 block text-[10px] font-medium text-muted-foreground" data-tauri-drag-region>
              v{snap?.version ?? '0.4.0'} · Desktop Core
            </span>
          </div>
        </div>

        <div className="flex items-center gap-1" data-tauri-drag-region="false">
          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8 rounded-lg text-muted-foreground hover:text-foreground"
            data-tauri-drag-region="false"
            title="打开 dsh"
            onClick={onOpenDsh}
          >
            <ExternalLink className="size-4" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8 rounded-lg text-muted-foreground hover:text-foreground"
            data-tauri-drag-region="false"
            title="日志"
            onClick={onOpenLogs}
          >
            <ScrollText className="size-4" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8 rounded-lg text-muted-foreground hover:text-foreground"
            data-tauri-drag-region="false"
            title="仓库目录"
            onClick={onOpenRepo}
          >
            <FolderOpen className="size-4" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8 rounded-lg text-muted-foreground hover:text-foreground"
            data-tauri-drag-region="false"
            title="设置"
            onClick={onOpenSettings}
          >
            <Settings className="size-4" />
          </Button>
        </div>
      </div>
    </header>
  )
}
