import { Cpu, GitBranch, ScrollText, Server, Settings } from 'lucide-react'
import { cn } from '@/lib/utils'
import type { PageName } from '@/types/schema'

const NAV_ITEMS: { key: PageName; label: string; icon: typeof Server }[] = [
  { key: 'dashboard', label: '服务', icon: Server },
  { key: 'repo', label: '仓库与构建', icon: GitBranch },
  { key: 'env', label: '工具链', icon: Cpu },
  { key: 'logs', label: '运行日志', icon: ScrollText },
  { key: 'settings', label: '设置', icon: Settings },
]

/** 左侧导航菜单(复刻 cc-switch 选中态:实心蓝底白字)。 */
export function SideNav({
  page,
  onNavigate,
}: {
  page: PageName
  onNavigate: (p: PageName) => void
}) {
  return (
    <nav
      data-tauri-drag-region="false"
      className="flex w-52 shrink-0 flex-col gap-1 border-r border-border bg-card/40 p-3"
      aria-label="主导航"
    >
      <p className="px-3 pb-2 pt-1 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground/70">
        导航
      </p>
      {NAV_ITEMS.map((item) => {
        const active = page === item.key
        return (
          <button
            key={item.key}
            role="tab"
            aria-selected={active}
            onClick={() => onNavigate(item.key)}
            className={cn(
              'flex items-center gap-2.5 rounded-lg px-3 py-2 text-sm font-medium transition-all duration-200',
              active
                ? 'bg-blue-500 text-white shadow-sm dark:bg-blue-600'
                : 'text-muted-foreground hover:bg-muted hover:text-foreground',
            )}
          >
            <item.icon className="size-4 shrink-0" />
            {item.label}
          </button>
        )
      })}
    </nav>
  )
}
