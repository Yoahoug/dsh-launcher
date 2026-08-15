import { ExternalLink, SquareTerminal, Server } from 'lucide-react'
import { Badge, StatusDot } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { formatDuration } from '@/lib/utils'
import type { AppSnapshot } from '@/types/schema'

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

export function ServiceCard({ snap, onOpenDsh, onStop }: {
  snap: AppSnapshot
  onOpenDsh: () => void
  onStop: () => void
}) {
  const { url, webPid, startedAt } = snap
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Server className="size-4 text-[var(--primary)]" />
          DeepSeek Harness
        </CardTitle>
        {stateBadge(snap)}
      </CardHeader>
      <CardContent>
        <p className="text-sm text-[var(--muted-foreground)]">
          {url ? (
            <>
              <span className="font-mono text-[var(--foreground)]">{url}</span>
              {webPid ? <span className="ml-2">PID {webPid}</span> : null}
              <span className="ml-2">已运行 {formatDuration(startedAt)}</span>
            </>
          ) : (
            '尚未启动 — 就绪后自动打开主界面'
          )}
        </p>
        <div className="mt-3 flex items-center gap-2">
          <Button
            variant={snap.state === 'running' ? 'default' : 'outline'}
            size="sm"
            disabled={snap.state !== 'running'}
            onClick={onOpenDsh}
          >
            <ExternalLink /> 打开 dsh
          </Button>
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
