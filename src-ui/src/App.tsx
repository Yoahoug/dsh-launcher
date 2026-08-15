import * as React from 'react'
import { Dashboard } from '@/components/dashboard/dashboard'
import { TopBar } from '@/components/dashboard/topbar'
import type { LaunchMode } from '@/components/dashboard/main-action'
import { FirstRunPage } from '@/components/first-run/first-run'
import { LogsPage } from '@/components/logs/logs-page'
import { SettingsPage } from '@/components/settings/settings-page'
import { ToastProvider, useToast } from '@/components/ui/toast'
import { api, useAppSnapshot, useDesktopSnapshot, usePage } from '@/hooks/use-app'
import { applyTheme } from '@/lib/theme'
import type { ActionAccepted, LogLevel, UiActionName } from '@/types/schema'

/** 动作结果 → toast 反馈。 */
function useActionFeedback() {
  const { toast } = useToast()
  return React.useCallback(
    (res: ActionAccepted, verb: string) => {
      if (res.ok) {
        toast({ kind: 'success', title: `${verb}成功` })
      } else if (res.aborted) {
        toast({ kind: 'info', title: '已取消', detail: res.reason })
      } else {
        toast({ kind: 'error', title: `${verb}失败`, detail: res.reason })
      }
    },
    [toast],
  )
}

function AppInner() {
  const snap = useAppSnapshot()
  const desktop = useDesktopSnapshot()
  const feedback = useActionFeedback()
  const [mode, setMode] = React.useState<LaunchMode>('normal')
  const [page, setPage] = usePage('dashboard')
  const [logsLevel, setLogsLevel] = React.useState<LogLevel | undefined>(undefined)

  // 主题:偏好驱动 + 系统变化监听
  React.useEffect(() => {
    if (desktop) return applyTheme(desktop.preferences.theme)
    applyTheme('system')
    return () => {}
  }, [desktop?.preferences.theme])

  const openLogs = (level?: LogLevel) => {
    setLogsLevel(level)
    setPage('logs')
  }

  const handleAction = (a: UiActionName) => {
    if (a === 'open-dsh') {
      void api.openDsh()
      return
    }
    const run = (): Promise<ActionAccepted> =>
      a === 'cancel' || a === 'stop' || a === 'rebuild'
        ? api.confirmAndRun(a === 'cancel' ? 'stop' : a)
        : api.runAction(a)
    void run().then((res) => feedback(res, VERBS[a] ?? a))
  }

  if (!snap || !desktop) {
    return (
      <div className="flex h-full items-center justify-center text-[var(--muted-foreground)]">
        加载中…
      </div>
    )
  }

  // 首次运行:无有效仓库时进入引导
  if (!desktop.firstRunDone && page !== 'first-run') {
    return <FirstRunPage onDone={() => setPage('dashboard')} onOpenSettings={() => setPage('settings')} />
  }

  const goDashboard = () => setPage('dashboard')

  return (
    <div className="flex h-full flex-col bg-[var(--background)] text-[var(--foreground)]">
      {page === 'dashboard' && (
        <>
          <TopBar
            snap={snap}
            onOpenDsh={() => void api.openDsh()}
            onOpenLogs={() => openLogs()}
            onOpenRepo={() => void api.openRepoDirectory()}
            onOpenSettings={() => setPage('settings')}
          />
          <Dashboard
            snap={snap}
            mode={mode}
            onModeChange={setMode}
            onAction={handleAction}
            onOpenDsh={() => void api.openDsh()}
            onJumpLogs={() => openLogs('err')}
          />
        </>
      )}
      {page === 'logs' && <LogsPage initialLevel={logsLevel} onBack={goDashboard} />}
      {page === 'settings' && <SettingsPage onBack={goDashboard} />}
      {page === 'first-run' && (
        <FirstRunPage onDone={goDashboard} onOpenSettings={() => setPage('settings')} />
      )}
    </div>
  )
}

const VERBS: Record<string, string> = {
  start: '启动',
  dev: '开发模式启动',
  update: '更新构建',
  rebuild: '重建',
  stop: '停止',
  'stop-and-quit': '停止并退出',
  'install-node': '安装托管 Node',
  cancel: '停止',
}

function App() {
  return (
    <ToastProvider>
      <AppInner />
    </ToastProvider>
  )
}

export default App
