import { useEffect, useState } from 'react'
import { Cpu, HardDriveDownload, RefreshCw, ShieldCheck, Wrench } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { useToast } from '@/components/ui/toast'
import { api, isTauri } from '@/hooks/use-app'
import { disabledReason } from '@/lib/actions'
import type {
  AppSnapshot,
  EnvironmentSnapshot,
  InstallationSnapshot,
  InstalledComponent,
  ToolRuntime,
} from '@/types/schema'

/** 运行环境子界面:以「当前实际生效的工具链」为主(版本/来源/路径/检测状态),
 *  托管能力(catalog)作为可选的次要区域,不替代当前工具安装状态。 */
export function EnvPage({
  snap,
  onInstallNode,
  onInstallGit,
  onInstallPnpm,
  onInstallToolchain,
}: {
  snap: AppSnapshot
  onInstallNode: () => void
  onInstallGit: () => void
  onInstallPnpm: () => void
  onInstallToolchain: () => void
}) {
  const { toast } = useToast()
  const [env, setEnv] = useState<EnvironmentSnapshot | null>(null)
  const [inst, setInst] = useState<InstallationSnapshot | null>(null)
  const [checking, setChecking] = useState(false)

  const check = async (force = false) => {
    setChecking(true)
    try {
      const [e, i] = await Promise.all([
        api.inspectEnvironment(force),
        api.getInstallationSnapshot(),
      ])
      setEnv(e)
      setInst(i)
    } catch (err) {
      toast({ kind: 'error', title: '环境检测失败', detail: String(err) })
    } finally {
      setChecking(false)
    }
  }

  useEffect(() => {
    void check()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const ok = env?.warnings?.length === 0
  const platform = env?.platform ?? ''

  const nodeReason = disabledReason(snap, 'install-node')
  const gitReason = disabledReason(snap, 'install-git')
  const pnpmReason = disabledReason(snap, 'install-pnpm')
  const toolchainReason = disabledReason(snap, 'install-toolchain')

  return (
    <div className="h-full overflow-y-auto px-6 py-8">
      <div className="mx-auto flex max-w-[720px] flex-col gap-4">
        {/* ── 当前实际生效的工具链(页面主信息) ─────────────────────── */}
        <div className="group relative overflow-hidden rounded-xl border border-border bg-card transition-all duration-300 hover:border-border-hover hover:shadow-sm">
          <div className="pointer-events-none absolute inset-0 bg-gradient-to-r from-blue-500/[0.07] to-transparent opacity-0 transition-opacity duration-500 group-hover:opacity-100" />
          <div className="relative flex items-center justify-between gap-3 px-5 pt-5">
            <div className="flex items-center gap-2.5">
              <span className="flex size-8 items-center justify-center rounded-lg border border-border bg-muted">
                <Cpu className="size-4 text-blue-500" />
              </span>
              <h2 className="text-base font-semibold leading-none">当前生效工具链</h2>
            </div>
            <Badge variant={ok === false ? 'danger' : ok ? 'success' : 'neutral'}>
              {ok === false ? '异常' : ok ? '正常' : '检测中'}
            </Badge>
          </div>
          <div className="px-5 pb-5 pt-4">
            <div className="divide-y divide-border/60">
              {env ? (
                <>
                  <ToolRow label="Node" tool={env.node} />
                  <ToolRow label="pnpm" tool={env.pnpm} />
                  <ToolRow label="git" tool={env.git} />
                  <div className="flex items-center justify-between gap-6 py-2.5">
                    <span className="text-[13px] text-muted-foreground">前端 dist</span>
                    <span className="font-mono text-[13px] text-foreground">
                      {env.distBuilt == null
                        ? '未知'
                        : env.distBuilt
                          ? '已构建 ✓'
                          : '未构建(启动时自动构建)'}
                    </span>
                  </div>
                  <div className="flex items-center justify-between gap-6 py-2.5">
                    <span className="text-[13px] text-muted-foreground">仓库</span>
                    <span className="font-mono text-[13px] text-foreground">
                      {env.repoUsable.ok ? '可用 ✓' : `不可用:${env.repoUsable.reason ?? '未知'}`}
                    </span>
                  </div>
                </>
              ) : (
                <p className="py-3 text-[13px] text-muted-foreground">检测中…</p>
              )}
            </div>
            {env?.warnings.length ? (
              <div className="mt-3 space-y-1">
                {env.warnings.map((w, i) => (
                  <p key={i} className="text-xs text-amber-500">⚠ {w}</p>
                ))}
              </div>
            ) : null}
            <div className="mt-4 flex flex-wrap items-center gap-2">
              <Button variant="outline" size="sm" onClick={() => void check(true)} disabled={checking}>
                <RefreshCw className={checking ? 'animate-spin' : ''} /> 重新检测
              </Button>
              <p className="ml-auto text-xs text-muted-foreground">
                dsh 要求 Node ^22.19 或 &gt;=24;系统版本满足要求时默认继续使用系统版本,不强迫重复安装
                {!isTauri() ? '· (浏览器预览 mock)' : ''}
              </p>
            </div>
          </div>
        </div>

        {/* ── 可选托管工具链(catalog 状态为次要区域,不替代当前安装状态) ── */}
        <div className="rounded-xl border border-border bg-card p-5">
          <div className="mb-3 flex items-center gap-2">
            <ShieldCheck className="size-4 text-emerald-500" />
            <h3 className="text-sm font-semibold">可选托管工具链</h3>
            {inst && (
              <Badge variant="neutral" className="ml-auto">
                catalog v{inst.catalogVersion} · 签名已验证
              </Badge>
            )}
          </div>
          <div className="divide-y divide-border/60">
            {env && inst && (
              <>
                <ManagedRow
                  label="Node"
                  component={inst.node}
                  offered={inst.offered.node}
                  current={env.node}
                  installLabel="安装托管 Node"
                  switchLabel="切换到托管 Node"
                  onInstall={onInstallNode}
                  disabled={Boolean(nodeReason)}
                  reason={nodeReason ?? undefined}
                />
                <ManagedRow
                  label="pnpm"
                  component={inst.pnpm}
                  offered={inst.offered.pnpm}
                  current={env.pnpm}
                  installLabel="安装托管 pnpm"
                  switchLabel="切换到托管 pnpm"
                  onInstall={onInstallPnpm}
                  disabled={Boolean(pnpmReason)}
                  reason={pnpmReason ?? undefined}
                />
                {platform === 'windows' ? (
                  <ManagedRow
                    label="Git(MinGit)"
                    component={inst.git}
                    offered={inst.offered.git ?? ''}
                    current={env.git}
                    installLabel="安装托管 Git"
                    switchLabel="切换到托管 Git"
                    onInstall={onInstallGit}
                    disabled={Boolean(gitReason)}
                    reason={gitReason ?? undefined}
                  />
                ) : (
                  <div className="flex items-center justify-between gap-6 py-2">
                    <span className="text-[12px] text-muted-foreground">Git(MinGit)</span>
                    <span className="text-[12px] text-muted-foreground">
                      macOS/Linux 使用系统 Git,无需托管
                    </span>
                  </div>
                )}
              </>
            )}
          </div>
          <div className="mt-4 flex flex-wrap items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              disabled={Boolean(toolchainReason)}
              title={toolchainReason ?? undefined}
              onClick={onInstallToolchain}
            >
              <Wrench /> 安装托管工具链
            </Button>
            <p className="ml-auto text-xs text-muted-foreground">
              可选能力:仅当系统工具缺失或不满足要求时才有必要安装
            </p>
          </div>
          <p className="mt-3 text-[11px] text-muted-foreground">
            工具链安装在应用数据目录的版本化目录;子进程 PATH 由 Launcher 显式组装,
            不修改系统 PATH、全局 npm/Git 配置,默认不需要管理员权限。下载固定走国内镜像并校验长度+SHA-256。
          </p>
        </div>
      </div>
    </div>
  )
}

const SOURCE_LABEL: Record<string, string> = {
  system: '系统安装',
  managed: 'Launcher 托管',
  corepack: '项目本地/Corepack',
}

const STATUS_LABEL: Record<string, string> = {
  detected: '自检通过',
  incompatible: '版本不兼容',
  missing: '未安装',
}

function sourceVariant(source?: string | null) {
  if (source === 'managed') return 'primary' as const
  if (source === 'corepack') return 'info' as const
  return 'neutral' as const
}

function statusVariant(status: string) {
  if (status === 'detected') return 'success' as const
  if (status === 'incompatible') return 'warning' as const
  return 'danger' as const
}

/** 单项工具:版本 + 来源 + 检测状态 + (仅托管)SHA-256 校验标记 + 实际路径。 */
function ToolRow({ label, tool }: { label: string; tool: ToolRuntime }) {
  const missing = tool.status === 'missing'
  return (
    <div className="py-2.5">
      <div className="flex flex-wrap items-center justify-between gap-x-6 gap-y-1">
        <span className="text-[13px] text-muted-foreground">{label}</span>
        <div className="flex flex-wrap items-center justify-end gap-1.5">
          {missing ? (
            <span className="text-[13px] font-semibold text-red-500">未安装</span>
          ) : (
            <>
              <span className="font-mono text-[13px] text-foreground">{tool.version}</span>
              {tool.source ? (
                <Badge variant={sourceVariant(tool.source)}>{SOURCE_LABEL[tool.source]}</Badge>
              ) : null}
              <Badge variant={statusVariant(tool.status)}>{STATUS_LABEL[tool.status]}</Badge>
              {tool.verified ? <Badge variant="success">SHA-256 已校验 ✓</Badge> : null}
            </>
          )}
        </div>
      </div>
      {tool.path ? (
        <div className="mt-1 flex justify-end">
          <span className="max-w-full truncate font-mono text-[11px] text-muted-foreground" title={tool.path}>
            {tool.path}
          </span>
        </div>
      ) : null}
      {tool.hint ? (
        <p className="mt-1 text-right text-xs text-amber-500">⚠ {tool.hint}</p>
      ) : null}
    </div>
  )
}

/** 托管组件行:已安装 → 版本 + SHA-256 校验;未安装 → 未安装 + 安装/切换到托管。 */
function ManagedRow({
  label,
  component,
  offered,
  current,
  installLabel,
  switchLabel,
  onInstall,
  disabled,
  reason,
}: {
  label: string
  component: InstalledComponent | null
  offered: string
  current: ToolRuntime
  installLabel: string
  switchLabel: string
  onInstall: () => void
  disabled: boolean
  reason?: string
}) {
  const systemActive = current.status !== 'missing'
  return (
    <div className="flex items-center justify-between gap-6 py-2">
      <span className="text-[12px] text-muted-foreground">{label}</span>
      <div className="flex flex-wrap items-center justify-end gap-2">
        {component ? (
          <>
            <span className="font-mono text-[12px] text-foreground">
              已安装 v{component.version}
            </span>
            <Badge variant="success">SHA-256 已校验 ✓</Badge>
          </>
        ) : (
          <>
            <span className="text-[12px] text-muted-foreground">
              {systemActive
                ? `未安装(当前使用${current.source === 'managed' ? '托管' : '系统'}版本)`
                : `未安装(可安装 v${offered})`}
            </span>
            <Button
              variant="outline"
              size="sm"
              disabled={disabled}
              title={reason ?? undefined}
              onClick={onInstall}
            >
              <HardDriveDownload /> {systemActive ? switchLabel : installLabel}
            </Button>
          </>
        )}
      </div>
    </div>
  )
}
