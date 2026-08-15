// dsh-launcher · DeepSeek 工作区视图(主窗口内,标题栏以下全部区域)
//
// 状态语义与 Rust dsh_view 对齐:
// - ready:原生子 WebView(dsh-content)已覆盖该区域,这里渲染占位,
//   避免任何 DOM 干扰(子 WebView 是原生视图,层级在 WebView 之上);
// - not_created/creating/loading:显示加载状态(服务启动中 / 视图加载中);
// - failed/disconnected:显示明确的内嵌错误状态,提供重试/日志/返回启动器入口,
//   绝不静默打开系统浏览器、绝不进入空白页面。
import { ExternalLink, Loader2, RefreshCw, ScrollText, Undo2, WifiOff } from 'lucide-react'
import { Button } from '@/components/ui/button'
import type { DshViewSnapshot } from '@/types/schema'

export function DshWorkspace({
  dsh,
  onBackToLauncher,
  onRetry,
  onOpenLogs,
  onOpenInBrowser,
}: {
  dsh: DshViewSnapshot | null
  onBackToLauncher: () => void
  onRetry: () => void
  onOpenLogs: () => void
  onOpenInBrowser: () => void
}) {
  const status = dsh?.status ?? 'creating'
  const error = dsh?.error ?? null

  if (status === 'ready') {
    // 子 WebView(原生)已覆盖标题栏以下全部区域;此处为空占位。
    return <div className="h-full w-full" data-tauri-drag-region="false" aria-hidden />
  }

  if (status === 'disconnected' || status === 'failed') {
    const disconnected = status === 'disconnected'
    return (
      <div className="flex h-full items-center justify-center p-8" data-tauri-drag-region="false">
        <div className="w-full max-w-md rounded-2xl border border-border bg-card/60 p-8 text-center shadow-sm">
          <WifiOff className="mx-auto size-10 text-red-500" />
          <h2 className="mt-4 text-lg font-semibold">
            {disconnected ? 'DeepSeek 连接已断开' : 'DeepSeek 工作区不可用'}
          </h2>
          <p className="mt-2 text-sm text-muted-foreground">
            {error ?? (disconnected
              ? 'DSH 服务已停止或失去响应,服务恢复后会自动重连。'
              : '启动失败,请查看日志或重试。')}
          </p>
          <div className="mt-6 flex flex-wrap items-center justify-center gap-2">
            <Button variant="default" size="sm" onClick={onRetry}>
              <RefreshCw className="size-4" />
              重试
            </Button>
            <Button variant="ghost" size="sm" onClick={onOpenLogs}>
              <ScrollText className="size-4" />
              查看日志
            </Button>
            <Button variant="ghost" size="sm" onClick={onOpenInBrowser}>
              <ExternalLink className="size-4" />
              在浏览器打开
            </Button>
            <Button variant="ghost" size="sm" onClick={onBackToLauncher}>
              <Undo2 className="size-4" />
              返回启动器
            </Button>
          </div>
        </div>
      </div>
    )
  }

  // not_created / creating:加载状态(服务启动中 / 视图创建中)
  return (
    <div className="flex h-full items-center justify-center p-8" data-tauri-drag-region="false">
      <div className="w-full max-w-md rounded-2xl border border-border bg-card/60 p-8 text-center shadow-sm">
        <Loader2 className="mx-auto size-10 animate-spin text-blue-500" />
        <h2 className="mt-4 text-lg font-semibold">正在启动 DeepSeek 工作区…</h2>
        <p className="mt-2 text-sm text-muted-foreground">
          {error ?? '正在启动 DSH 服务并加载界面,就绪后自动进入。'}
        </p>
        <div className="mt-6 flex flex-wrap items-center justify-center gap-2">
          <Button variant="ghost" size="sm" onClick={onOpenLogs}>
            <ScrollText className="size-4" />
            查看日志
          </Button>
          <Button variant="ghost" size="sm" onClick={onBackToLauncher}>
            <Undo2 className="size-4" />
            返回启动器
          </Button>
        </div>
      </div>
    </div>
  )
}
