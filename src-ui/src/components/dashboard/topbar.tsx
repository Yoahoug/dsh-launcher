import { useRef } from 'react'
import { ExternalLink, FolderOpen, ScrollText, Settings } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { SegmentedControl } from '@/components/ui/segmented-control'
import { useTitleBarDrag } from '@/hooks/use-titlebar-drag'
import { useTopbarAutohide } from '@/hooks/use-topbar-autohide'
import { cn } from '@/lib/utils'
import logoUrl from '@/assets/logo.svg'
import type { AppSnapshot, DshViewStatus, Workspace } from '@/types/schema'

/** 全局玻璃 header:品牌 + 工作区切换 + 右侧动作图标。
 *  整个标题栏(含中央空白区)可拖动窗口;按钮/链接/输入框不触发拖动。
 *  全屏 + DeepSeek 工作区:顶部栏自动收起(悬浮顶部显示),让 DeepSeek 真全屏。 */
export function TopBar({
  snap,
  workspace,
  dshStatus,
  onSwitchWorkspace,
  onOpenDsh,
  onOpenLogs,
  onOpenRepo,
  onOpenSettings,
}: {
  snap: AppSnapshot | null
  workspace: Workspace
  dshStatus: DshViewStatus
  onSwitchWorkspace: (w: Workspace) => void
  onOpenDsh: () => void
  onOpenLogs: () => void
  onOpenRepo: () => void
  onOpenSettings: () => void
}) {
  const headerRef = useRef<HTMLElement | null>(null)
  // 可靠拖动:pointerdown(主键 + 非交互目标)→ startDragging;双击空白 → 最大化切换
  useTitleBarDrag(headerRef)
  // 全屏自动隐藏(仅 DeepSeek 工作区);fullscreen 同时用于全屏时左对齐(无红绿灯留白)
  const { fullscreen, hidden } = useTopbarAutohide(workspace)

  return (
    <header
      ref={headerRef}
      data-tauri-drag-region="deep"
      className={cn(
        'fixed inset-x-0 top-0 z-50 h-16 border-b border-border bg-background/80 backdrop-blur-md',
        'transition-transform duration-300 ease-out will-change-transform',
        hidden ? '-translate-y-full' : 'translate-y-0',
      )}
    >
      <div
        className={cn(
          'flex h-full items-center gap-3 pr-4 transition-[padding] duration-300',
          // 窗口模式留出 macOS 红绿灯;全屏时贴左对齐(与左侧栏/窗口左边对齐,更美观)
          fullscreen ? 'pl-4' : 'pl-[84px]',
        )}
      >
        <div className="flex min-w-0 shrink-0 items-center gap-3">
          <img src={logoUrl} alt="" className="size-8 rounded-lg" />
          <div className="leading-tight">
            <span className="block whitespace-nowrap text-[15px] font-semibold">
              DSH Launcher
            </span>
            <span className="mt-0.5 block text-[10px] font-medium text-muted-foreground">
              v{snap?.version ?? '0.5.0'} · Desktop Core
            </span>
          </div>
        </div>

        {/* 工作区切换:启动器 / DeepSeek(标题栏常驻入口) */}
        <div className="ml-2 shrink-0">
          <SegmentedControl<Workspace>
            ariaLabel="工作区"
            value={workspace}
            onChange={onSwitchWorkspace}
            options={[
              { value: 'launcher', label: '启动器' },
              { value: 'dsh', label: 'DeepSeek' },
            ]}
          />
        </div>

        {/* 中央空白区:可拖动(deep drag region + pointerdown) */}
        <div className="min-w-0 flex-1" />

        <div className="flex shrink-0 items-center gap-1">
          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8 rounded-lg text-muted-foreground hover:text-foreground"
            data-tauri-drag-region="false"
            title={workspace === 'dsh' ? 'DeepSeek 工作区' : '进入 DeepSeek 工作区'}
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
      {/* 状态提示(仅 dsh 工作区断线时,标题栏内轻量提示) */}
      {workspace === 'dsh' && dshStatus !== 'ready' && dshStatus !== 'loading' && (
        <div className="absolute bottom-0 left-1/2 -translate-x-1/2 pb-0.5 text-[10px] font-medium text-muted-foreground/70">
          {dshStatus === 'disconnected' ? 'DeepSeek 已断开' : dshStatus === 'failed' ? 'DeepSeek 不可用' : ''}
        </div>
      )}
    </header>
  )
}
