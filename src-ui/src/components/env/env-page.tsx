import { useEffect, useState } from 'react'
import { Cpu, HardDriveDownload, RefreshCw, ShieldCheck, Wrench } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { useToast } from '@/components/ui/toast'
import { api, isTauri } from '@/hooks/use-app'
import { disabledReason } from '@/lib/actions'
import type { AppSnapshot, EnvironmentSnapshot, InstallationSnapshot } from '@/types/schema'

/** 运行环境子界面(独立视图,复刻 cc-switch 单视图聚焦)。
 *  M1:托管工具链安装按钮 + 动作矩阵禁用原因 + 来源展示。 */
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

  const check = async () => {
    setChecking(true)
    try {
      const [e, i] = await Promise.all([api.inspectEnvironment(), api.getInstallationSnapshot()])
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

  const nodeLabel = env?.node
    ? env.node.inRange
      ? env.node.current
      : env.node.usedVersion
        ? `${env.node.usedVersion}(${env.node.usedSource})`
        : `${env.node.current}(不支持)`
    : '检测中…'

  const rows = [
    { label: 'Node', value: nodeLabel, ok: env ? env.node.inRange : null },
    { label: 'pnpm', value: env?.pnpm ?? '未找到', ok: env ? Boolean(env.pnpm) : null },
    { label: 'git', value: env?.git ?? '未找到', ok: env ? Boolean(env.git) : null },
    { label: '前端 dist', value: env == null || env.distBuilt == null ? '未知' : env.distBuilt ? '已构建 ✓' : '未构建(启动时自动构建)', ok: env?.distBuilt !== false },
    { label: '仓库', value: env?.repoUsable.ok ? '可用 ✓' : `不可用:${env?.repoUsable.reason ?? '未知'}`, ok: env?.repoUsable.ok },
  ]

  const nodeReason = disabledReason(snap, 'install-node')
  const gitReason = disabledReason(snap, 'install-git')
  const pnpmReason = disabledReason(snap, 'install-pnpm')
  const toolchainReason = disabledReason(snap, 'install-toolchain')

  return (
    <div className="h-full overflow-y-auto px-6 py-8">
      <div className="mx-auto flex max-w-[720px] flex-col gap-4">
        <div className="group relative overflow-hidden rounded-xl border border-border bg-card transition-all duration-300 hover:border-border-hover hover:shadow-sm">
          <div className="pointer-events-none absolute inset-0 bg-gradient-to-r from-blue-500/[0.07] to-transparent opacity-0 transition-opacity duration-500 group-hover:opacity-100" />
          <div className="relative flex items-center justify-between gap-3 px-5 pt-5">
            <div className="flex items-center gap-2.5">
              <span className="flex size-8 items-center justify-center rounded-lg border border-border bg-muted">
                <Cpu className="size-4 text-blue-500" />
              </span>
              <h2 className="text-base font-semibold leading-none">运行环境</h2>
            </div>
            <Badge variant={ok === false ? 'danger' : ok ? 'success' : 'neutral'}>
              {ok === false ? '异常' : ok ? '正常' : '检测中'}
            </Badge>
          </div>
          <div className="px-5 pb-5 pt-4">
            <div className="divide-y divide-border/60">
              {rows.map((row) => (
                <div key={row.label} className="flex items-center justify-between gap-6 py-2.5">
                  <span className="text-[13px] text-muted-foreground">{row.label}</span>
                  <span className="font-mono text-[13px] text-foreground">{row.value}</span>
                </div>
              ))}
            </div>
            {env?.warnings.length ? (
              <div className="mt-3 space-y-1">
                {env.warnings.map((w, i) => (
                  <p key={i} className="text-xs text-amber-500">⚠ {w}</p>
                ))}
              </div>
            ) : null}
            <div className="mt-4 flex flex-wrap items-center gap-2">
              <Button variant="outline" size="sm" onClick={() => void check()} disabled={checking}>
                <RefreshCw className={checking ? 'animate-spin' : ''} /> 重新检测
              </Button>
              {env?.node && !env.node.inRange && !env.node.usedVersion ? (
                <Button variant="outline" size="sm" disabled={Boolean(nodeReason)} title={nodeReason ?? undefined} onClick={onInstallNode}>
                  <HardDriveDownload /> 安装托管 Node
                </Button>
              ) : null}
              {!env?.pnpm ? (
                <Button variant="outline" size="sm" disabled={Boolean(pnpmReason)} title={pnpmReason ?? undefined} onClick={onInstallPnpm}>
                  <HardDriveDownload /> 安装托管 pnpm
                </Button>
              ) : null}
              {!env?.git ? (
                <Button variant="outline" size="sm" disabled={Boolean(gitReason)} title={gitReason ?? undefined} onClick={onInstallGit}>
                  <HardDriveDownload /> 安装托管 Git
                </Button>
              ) : null}
              <Button variant="outline" size="sm" disabled={Boolean(toolchainReason)} title={toolchainReason ?? undefined} onClick={onInstallToolchain}>
                <Wrench /> 一键安装工具链
              </Button>
              <p className="ml-auto text-xs text-muted-foreground">
                dsh 要求 Node ^22.19 或 &gt;=24;托管工具链只写应用数据目录,不改系统 PATH
                {!isTauri() ? '· (浏览器预览 mock)' : ''}
              </p>
            </div>
          </div>
        </div>

        {/* 托管工具链状态(签名 catalog + 来源;不改系统 PATH/全局配置) */}
        <div className="rounded-xl border border-border bg-card p-5">
          <div className="mb-3 flex items-center gap-2">
            <ShieldCheck className="size-4 text-emerald-500" />
            <h3 className="text-sm font-semibold">托管工具链(签名 runtime catalog)</h3>
            {inst && (
              <Badge variant="neutral" className="ml-auto">
                catalog v{inst.catalogVersion} · 已校验
              </Badge>
            )}
          </div>
          <div className="divide-y divide-border/60">
            {(
              [
                ['Node', inst?.node],
                ['Git(MinGit)', inst?.git],
                ['pnpm', inst?.pnpm],
              ] as const
            ).map(([label, c]) => (
              <div key={label} className="flex items-center justify-between gap-6 py-2">
                <span className="text-[12px] text-muted-foreground">{label}</span>
                <span className="font-mono text-[12px] text-foreground">
                  {c ? `${c.version} · ${c.source} · 已校验 ✓` : '未安装(可用系统工具或一键安装)'}
                </span>
              </div>
            ))}
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
