import { GitBranch, RefreshCw, RotateCcw } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import type { AppSnapshot } from '@/types/schema'

export function RepoCard({ snap, onUpdate, onRebuild }: {
  snap: AppSnapshot
  onUpdate: () => void
  onRebuild: () => void
}) {
  const r = snap.repo
  const syncLabel = r.syncAt ? new Date(r.syncAt).toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' }) : '从未同步'
  return (
    <Card className="h-full">
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <span className="flex size-8 items-center justify-center rounded-xl bg-[var(--primary)]/10">
            <GitBranch className="size-4 text-[var(--primary)]" />
          </span>
          仓库与构建
        </CardTitle>
        <Badge variant={r.dirty ? 'warning' : 'neutral'}>
          {r.dirty ? `工作区有改动 (${r.dirtyFiles || '?'})` : '工作区干净'}
        </Badge>
      </CardHeader>
      <CardContent>
        <div className="space-y-1.5 text-sm text-[var(--muted-foreground)]">
          <p className="font-mono text-[var(--foreground)]">{r.branch || '—'} @ {r.head || '—'}</p>
          <p>{r.behind >= 0 ? `落后 ${r.behind} 个提交` : '落后 —'} · 最近同步 {syncLabel}</p>
        </div>
        <div className="mt-3 flex items-center gap-2">
          <Button variant="outline" size="sm" disabled={snap.busy} onClick={onUpdate}>
            <RefreshCw /> 更新并构建
          </Button>
          <Button variant="ghost" size="sm" disabled={snap.busy} onClick={onRebuild}>
            <RotateCcw /> 重建并重启
          </Button>
        </div>
      </CardContent>
    </Card>
  )
}
