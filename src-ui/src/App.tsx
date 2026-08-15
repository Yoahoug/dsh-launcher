import * as React from 'react'
import { AnimatePresence, motion } from 'framer-motion'
import { Dashboard } from '@/components/dashboard/dashboard'
import { OperationBanner } from '@/components/dashboard/operation-banner'
import { TopBar } from '@/components/dashboard/topbar'
import { DshWorkspace } from '@/components/dsh-workspace/dsh-workspace'
import type { LaunchMode } from '@/components/dashboard/main-action'
import { EnvPage } from '@/components/env/env-page'
import { FirstRunPage } from '@/components/first-run/first-run'
import { LogsPage } from '@/components/logs/logs-page'
import { SideNav } from '@/components/nav/side-nav'
import { RepoPage } from '@/components/repo/repo-page'
import { SettingsPage } from '@/components/settings/settings-page'
import { ToastProvider, useToast } from '@/components/ui/toast'
import { api, useAppSnapshot, useDesktopSnapshot, useDshViewState, usePage } from '@/hooks/use-app'
import { applyTheme } from '@/lib/theme'
import type { ActionAccepted, LogLevel, PageName, UiActionName, Workspace } from '@/types/schema'

/** 命令受理结果 → toast 反馈。长任务 accepted ≠ success；真实成功由状态终态体现。 */
function useActionFeedback() {
  const { toast } = useToast()
  return React.useCallback(
    (res: ActionAccepted, verb: string) => {
      if (res.ok) {
        toast({ kind: 'info', title: `${verb}已受理`, detail: '任务已开始，完成状态以进度与运行状态为准。' })
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
  const dsh = useDshViewState()
  const feedback = useActionFeedback()
  const [mode, setMode] = React.useState<LaunchMode>('normal')
  // ?page=repo|env|logs|settings|first-run 可直达页面(浏览器预览用)
  const urlPage = new URLSearchParams(window.location.search).get('page') as PageName | null
  const [page, setPage] = usePage(
    urlPage === 'repo' || urlPage === 'env' || urlPage === 'logs' || urlPage === 'settings' || urlPage === 'first-run'
      ? urlPage
      : 'dashboard',
  )
  const [logsLevel, setLogsLevel] = React.useState<LogLevel | undefined>(undefined)

  // 主题:偏好驱动 + 系统变化监听
  React.useEffect(() => {
    if (desktop) return applyTheme(desktop.preferences.theme)
    applyTheme('system')
    return () => {}
  }, [desktop?.preferences.theme])

  // 性能测量点:react_interactive(首帧可交互)
  React.useEffect(() => {
    void api.perfMark('react_interactive')
  }, [])

  // 当前工作区:launcher / dsh(Rust 侧 dsh_view 事件驱动;Rust 未就绪前默认 launcher)
  const workspace: Workspace = dsh?.workspace ?? 'launcher'

  const goDashboard = React.useCallback(() => setPage('dashboard'), [setPage])

  const openLogs = React.useCallback(
    (level?: LogLevel) => {
      setLogsLevel(level)
      setPage('logs')
    },
    [setPage],
  )

  const switchWorkspace = React.useCallback(
    (w: Workspace) => {
      if (w === workspace) return // 幂等:连续点击不重复创建
      void api.setWorkspace(w)
    },
    [workspace],
  )

  const retryDsh = React.useCallback(() => {
    void api.retryDshView()
  }, [])

  const openDshWorkspace = React.useCallback(() => {
    void api.openDshWorkspace()
  }, [])

  const openInBrowser = React.useCallback(() => {
    void api.openDsh()
  }, [])

  const handleAction = (a: UiActionName) => {
    if (a === 'open-dsh') {
      // M4.1:进入主窗口内 DeepSeek 工作区(子 WebView,不弹独立窗口、不开浏览器)
      openDshWorkspace()
      return
    }
    const run = (): Promise<ActionAccepted> => {
      // 取消进行中的任务:直接取消(无需原生确认);stop/rebuild 属危险动作走确认
      if (a === 'cancel') return api.runAction('cancel')
      if (a === 'stop' || a === 'rebuild') return api.confirmAndRun(a)
      return api.runAction(a)
    }
    void run().then((res) => feedback(res, VERBS[a] ?? a))
  }

  if (!snap || !desktop) {
    return (
      <div className="flex h-full items-center justify-center text-muted-foreground">
        加载中…
      </div>
    )
  }

  // 首次运行:无有效仓库时进入引导(全屏向导,不套壳)
  if (!desktop.firstRunDone && page !== 'first-run') {
    return <FirstRunPage onDone={goDashboard} onOpenSettings={() => setPage('settings')} />
  }

  return (
    <div className="flex h-screen flex-col overflow-hidden bg-background text-foreground selection:bg-primary/30">
      <TopBar
        snap={snap}
        workspace={workspace}
        dshStatus={dsh?.status ?? 'creating'}
        onSwitchWorkspace={switchWorkspace}
        onOpenDsh={openDshWorkspace}
        onOpenLogs={() => openLogs()}
        onOpenRepo={() => void api.openRepoDirectory()}
        onOpenSettings={() => setPage('settings')}
      />

      {workspace === 'dsh' ? (
        /* DeepSeek 工作区:子 WebView(原生)覆盖标题栏以下区域;
           loading/error 状态由前端组件呈现(占位/加载/断线错误卡) */
        <div className="min-h-0 flex-1 pt-16">
          <DshWorkspace
            dsh={dsh}
            onBackToLauncher={() => switchWorkspace('launcher')}
            onRetry={retryDsh}
            onOpenLogs={() => openLogs()}
            onOpenInBrowser={openInBrowser}
          />
        </div>
      ) : (
        /* 启动器工作区:长任务横幅 + 侧边菜单 + 内容区 */
        <div className="flex min-h-0 flex-1 flex-col pt-16">
          {/* 长任务横幅:operationId/阶段/取消(accepted ≠ success,终态才显示成功) */}
          <OperationBanner snap={snap} onCancel={() => handleAction('cancel')} />

          <div className="flex min-h-0 flex-1">
            <SideNav page={page} onNavigate={setPage} />
            <main className="min-h-0 flex-1 overflow-hidden">
              <AnimatePresence mode="wait">
                <motion.div
                  key={page}
                  className="flex h-full flex-col"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0 }}
                  transition={{ duration: 0.2 }}
                >
                  {page === 'dashboard' && (
                    <Dashboard
                      snap={snap}
                      mode={mode}
                      onModeChange={setMode}
                      onAction={handleAction}
                      onOpenDsh={openDshWorkspace}
                      onJumpLogs={() => openLogs('err')}
                    />
                  )}
                  {page === 'repo' && (
                    <RepoPage
                      snap={snap}
                      onUpdate={() => handleAction('update')}
                      onRebuild={() => handleAction('rebuild')}
                    />
                  )}
                  {page === 'env' && (
                    <EnvPage
                      snap={snap}
                      onInstallNode={() => handleAction('install-node')}
                      onInstallGit={() => handleAction('install-git')}
                      onInstallPnpm={() => handleAction('install-pnpm')}
                      onInstallToolchain={() => handleAction('install-toolchain')}
                    />
                  )}
                  {page === 'logs' && <LogsPage initialLevel={logsLevel} onBack={goDashboard} />}
                  {page === 'settings' && <SettingsPage onBack={goDashboard} />}
                  {page === 'first-run' && (
                    <FirstRunPage onDone={goDashboard} onOpenSettings={() => setPage('settings')} />
                  )}
                </motion.div>
              </AnimatePresence>
            </main>
          </div>
        </div>
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
  'install-git': '安装托管 Git',
  'install-pnpm': '安装托管 pnpm',
  'install-toolchain': '安装托管工具链',
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
