import { Loader2, Play, RefreshCw, StopCircle, ExternalLink } from 'lucide-react'
import { Button } from '@/components/ui/button'
import type { AppSnapshot } from '@/types/schema'
import type { UiActionName } from '@/types/schema'

const ACTION_LABEL: Record<string, string> = {
  idle: '启动',
  syncing: '同步中',
  installing: '安装依赖',
  building: '构建中',
  starting: '启动中',
  running: '打开 dsh',
  stopping: '停止中',
  failed: '重试',
}

/** 右上圆形主操作:状态决定图标与动作。 */
export function MainAction({
  snap,
  mode,
  onAction,
}: {
  snap: AppSnapshot | null
  mode: 'normal' | 'dev' | 'maintenance'
  onAction: (a: UiActionName) => void
}) {
  if (!snap) return <Button size="icon-round" variant="default" disabled><Loader2 className="animate-spin" /></Button>
  const { state, busy } = snap
  const busyNow = busy || ['syncing', 'installing', 'building', 'starting', 'stopping'].includes(state)

  let action: UiActionName
  if (state === 'running') action = 'open-dsh'
  else if (state === 'failed') action = mode === 'maintenance' ? 'update' : mode === 'dev' ? 'dev' : 'start'
  else if (state === 'idle') action = mode === 'maintenance' ? 'update' : mode === 'dev' ? 'dev' : 'start'
  else if (state === 'stopping') action = 'stop'
  else action = 'cancel'

  const Icon = busyNow ? Loader2 : state === 'running' ? ExternalLink : state === 'failed' ? RefreshCw : state === 'stopping' ? StopCircle : Play

  return (
    <Button
      size="icon-round"
      variant="default"
      disabled={busyNow}
      title={ACTION_LABEL[state]}
      data-tauri-drag-region="false"
      onClick={() => onAction(action)}
      className={busyNow ? 'animate-pulse' : ''}
    >
      <Icon className={busyNow ? 'animate-spin' : ''} />
    </Button>
  )
}
