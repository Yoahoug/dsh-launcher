// dsh-launcher · 动作矩阵 UI 辅助(禁用原因 / 操作文案)
import type { AppSnapshot, OperationKind } from '@/types/schema'

/** 动作当前被禁用的原因(无则 null)。按钮禁用时展示具体原因。 */
export function disabledReason(snap: AppSnapshot | null, action: string): string | null {
  if (!snap) return null
  return snap.disabledActions.find((d) => d.action === action)?.reason ?? null
}

const OP_LABELS: Record<OperationKind, string> = {
  install_node: '安装 Node',
  install_git: '安装 Git',
  install_pnpm: '安装 pnpm',
  install_toolchain: '安装工具链',
  clone_repo: '克隆仓库',
  full_setup: '一键安装',
  install_deps: '安装依赖',
  build: '构建',
  update_rebuild: '更新并构建',
  rebuild_restart: '重建并重启',
  start_web: '启动 dsh',
  start_dev: '启动开发模式',
  stop_all: '停止',
  self_update: '应用自更新',
  plugin_install: '安装插件',
  plugin_remove: '移除插件',
}

export function operationLabel(kind: OperationKind): string {
  return OP_LABELS[kind] ?? kind
}
