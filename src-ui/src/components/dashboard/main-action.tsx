import { Loader2, Play, RefreshCw, StopCircle, ExternalLink } from 'lucide-react'
import { Button } from '@/components/ui/button'
import type { AppSnapshot } from '@/types/schema'
import type { UiActionName } from '@/types/schema'

const ACTION_LABEL: Record<string, string> = {
  syncing: '同步中',
  installing: '安装依赖',
  building: '构建中',
  starting: '启动中',
  running: '打开 dsh',
  stopping: '停止中',
  failed: '重试',
}

export type LaunchMode = 'normal' | 'dev'

/** 服务卡主操作:启动方式与当前状态共同决定文案和动作。 */
export function MainAction({
  snap,
  mode,
  onAction,
}: {
  snap: AppSnapshot | null
  mode: LaunchMode
  onAction: (a: UiActionName) => void
}) {
  if (!snap) return <Button size="sm" variant="default" disabled><Loader2 className="animate-spin" />加载中</Button>
  const { state, busy } = snap
  const busyNow = busy || ['syncing', 'installing', 'building', 'starting', 'stopping'].includes(state)

  let action: UiActionName
  if (state === 'running') action = 'open-dsh'
  else if (state === 'failed') action = mode === 'dev' ? 'dev' : 'start'
  else if (state === 'idle') action = mode === 'dev' ? 'dev' : 'start'
  else if (state === 'stopping') action = 'stop'
  else action = 'cancel'

  const Icon = busyNow ? Loader2 : state === 'running' ? ExternalLink : state === 'failed' ? RefreshCw : state === 'stopping' ? StopCircle : Play
  const label = state === 'idle'
    ? mode === 'dev' ? '开发模式启动' : '普通启动'
    : state === 'failed'
      ? mode === 'dev' ? '重试开发模式' : '重试普通启动'
      : ACTION_LABEL[state]

  return (
    <Button
      size="sm"
      variant="default"
      disabled={busyNow}
      title={label}
      data-tauri-drag-region="false"
      onClick={() => onAction(action)}
      className={`min-w-[126px] ${busyNow ? 'animate-pulse' : ''}`}
    >
      <Icon className={busyNow ? 'animate-spin' : ''} />
      {label}
    </Button>
  )
}
