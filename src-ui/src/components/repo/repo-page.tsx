import { useState } from 'react'
import { GitBranch, GitFork, RefreshCw, RotateCcw } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { CloneDialog } from '@/components/repo/clone-dialog'
import { disabledReason } from '@/lib/actions'
import type { AppSnapshot } from '@/types/schema'

/** 仓库与构建子界面(独立视图,复刻 cc-switch 单视图聚焦)。
 *  M1:集成 Clone 弹窗与动作矩阵禁用原因。 */
export function RepoPage({
  snap,
  onUpdate,
  onRebuild,
}: {
  snap: AppSnapshot
  onUpdate: () => void
  onRebuild: () => void
}) {
  const [cloneOpen, setCloneOpen] = useState(false)
  const r = snap.repo
  const syncLabel = r.syncAt
    ? new Date(r.syncAt).toLocaleString('zh-CN', { hour: '2-digit', minute: '2-digit', second: '2-digit' })
    : '从未同步'

  const rows = [
    { label: '分支', value: r.branch || '—', mono: true },
    { label: 'HEAD', value: r.head || '—', mono: true },
    { label: '落后上游', value: r.behind >= 0 ? `${r.behind} 个提交` : '—' },
    { label: '本地超前', value: r.ahead > 0 ? `${r.ahead} 个提交` : '无' },
    { label: '最近同步', value: syncLabel },
  ]

  const updateReason = disabledReason(snap, 'update')
  const rebuildReason = disabledReason(snap, 'rebuild')
  const cloneReason = disabledReason(snap, 'clone-repo')

  return (
    <div className="h-full overflow-y-auto px-6 py-8">
      <div className="mx-auto flex max-w-[720px] flex-col gap-4">
        <div className="group relative overflow-hidden rounded-xl border border-border bg-card transition-all duration-300 hover:border-border-hover hover:shadow-sm">
          <div className="pointer-events-none absolute inset-0 bg-gradient-to-r from-blue-500/[0.07] to-transparent opacity-0 transition-opacity duration-500 group-hover:opacity-100" />
          <div className="relative flex items-center justify-between gap-3 px-5 pt-5">
            <div className="flex items-center gap-2.5">
              <span className="flex size-8 items-center justify-center rounded-lg border border-border bg-muted">
                <GitBranch className="size-4 text-blue-500" />
              </span>
              <h2 className="text-base font-semibold leading-none">仓库与构建</h2>
            </div>
            <Badge variant={r.dirty ? 'warning' : 'neutral'}>
              {r.dirty ? `工作区有改动 (${r.dirtyFiles || '?'})` : '工作区干净'}
            </Badge>
          </div>
          <div className="px-5 pb-5 pt-4">
            <div className="divide-y divide-border/60">
              {rows.map((row) => (
                <div key={row.label} className="flex items-center justify-between gap-6 py-2.5">
                  <span className="text-[13px] text-muted-foreground">{row.label}</span>
                  <span className={row.mono ? 'font-mono text-[13px] text-foreground' : 'text-[13px] text-foreground'}>
                    {row.value}
                  </span>
                </div>
              ))}
            </div>
            <div className="mt-4 flex flex-wrap items-center gap-2">
              <Button
                variant="outline"
                size="sm"
                disabled={Boolean(cloneReason) || cloneOpen}
                title={cloneReason ?? '克隆仓库(弹窗填写 URL/目录/网络源/分支)'}
                onClick={() => setCloneOpen(true)}
              >
                <GitFork /> 克隆仓库
              </Button>
              <Button variant="outline" size="sm" disabled={Boolean(updateReason)} title={updateReason ?? undefined} onClick={onUpdate}>
                <RefreshCw /> 更新并构建
              </Button>
              <Button variant="ghost" size="sm" disabled={Boolean(rebuildReason)} title={rebuildReason ?? undefined} onClick={onRebuild}>
                <RotateCcw /> 重建并重启
              </Button>
              <p className="ml-auto text-xs text-muted-foreground">
                {cloneReason ?? updateReason ?? '克隆在 staging 中完成,校验通过才原子提交;目标非空不覆盖'}
              </p>
            </div>
          </div>
        </div>
      </div>

      {cloneOpen && (
        <CloneDialog
          onClose={() => setCloneOpen(false)}
          onSubmitted={() => {
            // 提交后关闭弹窗,任务横幅会展示进度
          }}
        />
      )}
    </div>
  )
}
