import { ExternalLink, SquareTerminal, Server } from 'lucide-react'
import { Badge, StatusDot } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { MainAction, type LaunchMode } from '@/components/dashboard/main-action'
import { SegmentedControl } from '@/components/ui/segmented-control'
import { formatDuration } from '@/lib/utils'
import type { AppSnapshot, UiActionName } from '@/types/schema'

const LAUNCH_MODES: { value: LaunchMode; label: string }[] = [
  { value: 'normal', label: '普通运行' },
  { value: 'dev', label: '开发模式 · HMR' },
]

function stateBadge(snap: AppSnapshot) {
  const s = snap.state
  if (s === 'running') {
    return snap.mode === 'dev' ? (
      <Badge variant="success"><StatusDot />HMR 活跃</Badge>
    ) : (
      <Badge variant="success"><StatusDot />运行中</Badge>
    )
  }
  if (s === 'failed') return <Badge variant="danger">失败</Badge>
  if (s === 'stopping') return <Badge variant="neutral">停止中</Badge>
  if (['syncing', 'installing', 'building', 'starting'].includes(s)) return <Badge variant="warning">{snap.phase || s}</Badge>
  return <Badge variant="neutral">空闲</Badge>
}

/** 服务主卡:复刻 cc-switch 卡片(圆角描边 + 悬停泛蓝 + 图标方块)。 */
export function ServiceCard({ snap, mode, onModeChange, onAction, onOpenDsh, onStop }: {
  snap: AppSnapshot
  mode: LaunchMode
  onModeChange: (mode: LaunchMode) => void
  onAction: (action: UiActionName) => void
  onOpenDsh: () => void
  onStop: () => void
}) {
  const { url, webPid, startedAt } = snap
  const running = snap.state === 'running'
  return (
    <Card className="group border-border">
      {/* 悬停渐晕(cc-switch ProviderCard) */}
      <div className="pointer-events-none absolute inset-0 bg-gradient-to-r from-blue-500/[0.07] to-transparent opacity-0 transition-opacity duration-500 group-hover:opacity-100" />
      <CardHeader className="relative">
        <CardTitle className="flex items-center gap-2.5">
          <span className="flex size-8 items-center justify-center rounded-lg border border-border bg-muted transition-transform duration-300 group-hover:scale-105">
            <Server className="size-4 text-blue-500" />
          </span>
          DeepSeek Harness
        </CardTitle>
        {stateBadge(snap)}
      </CardHeader>
      <CardContent className="relative flex items-end justify-between gap-6 pt-4">
        <div className="min-w-0">
          <p className="text-sm text-muted-foreground">
            {url ? (
              <>
                <span className="font-mono text-foreground">{url}</span>
                {webPid ? <span className="ml-2">PID {webPid}</span> : null}
                <span className="ml-2">已运行 {formatDuration(startedAt)}</span>
              </>
            ) : running ? (
              '服务运行中（上次运行记录未包含访问地址）'
            ) : (
              '尚未启动 — 就绪后自动打开主界面'
            )}
          </p>
          <p className="mt-1.5 text-xs text-muted-foreground/80">
            {running
              ? `当前为${snap.mode === 'dev' ? '开发模式（HMR）' : '普通运行'}，服务已由原生核心托管。`
              : '选择启动方式开始;仓库同步、环境检测等操作请进入下方功能入口。'}
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {running ? (
            <Button size="sm" onClick={onOpenDsh}>
              <ExternalLink /> 打开 dsh
            </Button>
          ) : (
            <div className="flex items-end gap-2">
              <div>
                <p className="mb-1.5 text-[11px] font-medium text-muted-foreground">启动方式</p>
                <SegmentedControl
                  options={LAUNCH_MODES}
                  value={mode}
                  onChange={onModeChange}
                  disabled={snap.busy}
                  ariaLabel="启动方式"
                />
              </div>
              <MainAction snap={snap} mode={mode} onAction={onAction} />
            </div>
          )}
          <Button
            variant="ghost"
            size="sm"
            disabled={!snap.webPid || snap.state === 'stopping'}
            onClick={onStop}
          >
            <SquareTerminal /> 停止
          </Button>
        </div>
      </CardContent>
    </Card>
  )
}
