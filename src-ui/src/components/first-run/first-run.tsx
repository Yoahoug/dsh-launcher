import * as React from 'react'
import { AnimatePresence, motion } from 'framer-motion'
import {
  FolderOpen,
  GitFork,
  Loader2,
  RefreshCw,
  ShieldCheck,
  Wrench,
} from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useToast } from '@/components/ui/toast'
import { api, useAppSnapshot } from '@/hooks/use-app'
import { CloneDialog } from '@/components/repo/clone-dialog'
import logoUrl from '@/assets/logo.svg'
import type { EnvironmentSnapshot, OperationKind, OperationSnapshot } from '@/types/schema'

type Step = 'welcome' | 'repo' | 'env'

const DEFAULT_REPO = '~/Desktop/deepseek-harness'

const STEP_ANIM = {
  initial: { opacity: 0, y: 8 },
  animate: { opacity: 1, y: 0 },
  exit: { opacity: 0, y: -8 },
  transition: { duration: 0.2, ease: [0.16, 1, 0.3, 1] as const },
}

/** 长任务中文名(与 Rust OperationKind.label 对齐;仅向导内需要展示的)。 */
const OP_LABEL: Record<OperationKind, string> = {
  install_node: '安装托管 Node',
  install_git: '安装托管 Git',
  install_pnpm: '安装托管 pnpm',
  install_toolchain: '安装托管工具链',
  clone_repo: '克隆仓库',
  full_setup: '一键克隆并初始化',
  install_deps: '安装依赖',
  build: '构建',
  update_rebuild: '更新并构建',
  rebuild_restart: '重建并重启',
  start_web: '启动 dsh',
  start_dev: '启动开发模式',
  stop_all: '停止',
  self_update: '应用自更新',
}

/**
 * 首次运行引导:仓库(选择已有 / 一键克隆)→ 环境检测 → 一键安装缺失环境 → 完成。
 * - 跳过(稍后配置)可随时退出向导进入启动器,不会卡死;
 * - 长任务(克隆/安装)进度实时展示,终态后自动重新检测环境;
 * - 「完成」需要仓库可用;失败不白屏,可跳过进设置/仓库页继续。
 */
export function FirstRunPage({ onDone }: { onDone: () => void }) {
  const { toast } = useToast()
  const snap = useAppSnapshot()
  const [step, setStep] = React.useState<Step>('welcome')
  const [repoPath, setRepoPath] = React.useState(DEFAULT_REPO)
  const [env, setEnv] = React.useState<EnvironmentSnapshot | null>(null)
  const [checking, setChecking] = React.useState(false)
  const [saving, setSaving] = React.useState(false)
  const [cloneOpen, setCloneOpen] = React.useState(false)

  const stepRef = React.useRef(step)
  stepRef.current = step
  const envRef = React.useRef(env)
  envRef.current = env

  // 当前长任务(queued/running 显示进度卡;终态由 watcher 处理)
  const op = snap?.operation ?? null
  const opActive =
    op && (op.status === 'queued' || op.status === 'running') ? (op as OperationSnapshot) : null

  /** 仅重新检测环境(不切步骤;克隆/安装终态后调用;force=true 绕过文件缓存)。 */
  const detect = React.useCallback(
    async (force = false): Promise<EnvironmentSnapshot | null> => {
      try {
        const e = await api.inspectEnvironment(force)
        setEnv(e)
        return e
      } catch (err) {
        toast({ kind: 'error', title: '环境检测失败', detail: String(err) })
        return null
      }
    },
    [toast],
  )

  // 进入「已有仓库」步骤时先轻量检测一次环境(git 状态展示在克隆按钮旁,避免克隆时才报 git 缺失)
  React.useEffect(() => {
    if (step === 'repo' && !envRef.current) void detect()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [step])

  /** 仓库步骤:保存路径 + 检测环境 + 进入环境步骤。 */
  const runEnvCheck = async (path?: string) => {
    setChecking(true)
    try {
      if (path && path !== envRef.current?.repoPath) {
        await api.saveSettings({ repoPath: path })
      }
      const e = await detect()
      if (e) setStep('env')
    } finally {
      setChecking(false)
    }
  }

  /** 长任务终态(成功/失败/取消)→ 重新检测环境;克隆/一键安装成功自动进入环境步骤。 */
  const lastOpId = React.useRef<number | null>(null)
  React.useEffect(() => {
    if (!op) return
    const terminal =
      op.status === 'success' ||
      op.status === 'failed' ||
      op.status === 'cancelled' ||
      op.status === 'interrupted'
    if (!terminal) return
    if (lastOpId.current === op.operationId) return
    lastOpId.current = op.operationId
    const kind = op.kind
    if (op.status === 'success') {
      void detect().then((e) => {
        if ((kind === 'clone_repo' || kind === 'full_setup') && stepRef.current === 'repo' && e?.repoUsable.ok) {
          setStep('env')
        }
      })
    } else if (op.status === 'failed') {
      toast({ kind: 'error', title: '任务失败', detail: op.error ?? '查看运行日志了解详情' })
      void detect()
    }
  }, [op, detect, toast])

  /** 一键安装缺失环境(Node + Git + pnpm 托管,仅装缺失项)。 */
  const installAll = async () => {
    const r = await api.runAction('install-toolchain')
    if (!r.ok) toast({ kind: 'error', title: '安装未受理', detail: r.reason })
  }

  const finish = async () => {
    setSaving(true)
    try {
      // 仓库路径已在「检测环境」/克隆流程中保存(completeFirstRun 只标记引导完成,
      // 避免用向导里的旧输入覆盖克隆后的真实路径)
      await api.completeFirstRun(false)
      toast({ kind: 'success', title: '设置已保存' })
      onDone()
    } catch (err) {
      toast({ kind: 'error', title: '保存失败', detail: String(err) })
    } finally {
      setSaving(false)
    }
  }

  /** 跳过(稍后配置):标记 firstRunSkipped,进入启动器(可在设置/仓库页补配置)。 */
  const skip = async () => {
    try {
      await api.completeFirstRun(true)
      onDone()
    } catch (err) {
      toast({ kind: 'error', title: '跳过失败', detail: String(err) })
    }
  }

  return (
    <div className="flex h-full items-center justify-center overflow-y-auto bg-background p-8">
      <motion.div
        initial={{ opacity: 0, y: 16, scale: 0.98 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        transition={{ duration: 0.3, ease: [0.16, 1, 0.3, 1] }}
        className="w-full max-w-lg rounded-xl border border-border bg-card p-8 shadow-xl"
      >
        <div className="flex items-center gap-3">
          <img src={logoUrl} alt="" className="size-10 rounded-xl" />
          <div>
            <h1 className="text-xl font-semibold">欢迎使用 DSH Launcher</h1>
            <p className="mt-0.5 text-xs text-muted-foreground">
              DeepSeek Harness 桌面启动器 · v{snap?.version ?? '0.6.0'}
            </p>
          </div>
        </div>
        <p className="mt-4 text-sm leading-relaxed text-muted-foreground">
          一个纯粹的 DeepSeek Harness 启动器:托管开发流程、构建与更新,让 dsh web
          稳定运行在后台。首次使用需要选择仓库并确认运行环境,也可稍后配置。
        </p>

        {/* 长任务进度卡(克隆/安装期间常驻显示,可取消;含进度条) */}
        {opActive && (
          <div className="mt-4 rounded-xl border border-blue-500/25 bg-blue-500/[0.05] px-4 py-3">
            <div className="flex items-center gap-2.5">
              <Loader2 className="size-4 shrink-0 animate-spin text-blue-500" />
              <p className="min-w-0 flex-1 truncate text-sm font-medium">
                {OP_LABEL[opActive.kind] ?? '任务'}…
              </p>
              <Button variant="ghost" size="sm" onClick={() => void api.runAction('cancel')}>
                取消
              </Button>
            </div>
            <p className="mt-1 pl-6 text-xs text-muted-foreground">
              {opActive.stage}
              {opActive.progress != null ? ` ${opActive.progress}%` : ''}
            </p>
            <div className="mt-2 ml-6 h-1.5 w-[calc(100%-1.5rem)] overflow-hidden rounded-full bg-black/10 dark:bg-white/10">
              {opActive.progress != null ? (
                <div
                  className="h-full rounded-full bg-blue-500 transition-[width] duration-300"
                  style={{ width: `${Math.max(2, Math.min(100, opActive.progress))}%` }}
                />
              ) : (
                <div className="animate-indeterminate h-full w-2/5 rounded-full bg-blue-500" />
              )}
            </div>
          </div>
        )}

        <div className="mt-6 min-h-[180px]">
          <AnimatePresence mode="wait">
            {step === 'welcome' && (
              <motion.div key="welcome" {...STEP_ANIM} className="flex justify-end gap-2">
                <Button variant="ghost" onClick={() => void skip()}>
                  稍后配置
                </Button>
                <Button onClick={() => setStep('repo')}>开始配置</Button>
              </motion.div>
            )}

            {step === 'repo' && (
              <motion.div key="repo" {...STEP_ANIM}>
                <div className="space-y-2">
                  <p className="text-sm font-medium">已有仓库位置</p>
                  <div className="flex gap-2">
                    <Input
                      value={repoPath}
                      onChange={(e) => setRepoPath(e.target.value)}
                      spellCheck={false}
                      placeholder="/Users/you/Desktop/deepseek-harness"
                    />
                    <Button
                      variant="outline"
                      size="icon"
                      title="选择目录"
                      onClick={() => void api.pickDirectory().then((p) => p && setRepoPath(p))}
                    >
                      <FolderOpen />
                    </Button>
                  </div>
                  <p className="text-xs text-muted-foreground">
                    默认自动探测 ~/Desktop/deepseek-harness;也可以直接克隆官方仓库到本机。
                  </p>
                </div>

                <div className="mt-4 space-y-2 rounded-xl border border-border bg-card/60 p-4">
                  <div className="flex items-center gap-2">
                    <GitFork className="size-4 text-blue-500" />
                    <p className="text-sm font-medium">还没有仓库?</p>
                  </div>
                  <p className="text-xs text-muted-foreground">
                    一键克隆 DeepSeek Harness 并完成依赖安装与构建;克隆需要 git,缺失时可先在下方
                    「检测环境」步骤一键安装托管 git(Windows)或系统 git(macOS/Linux)。
                  </p>
                  {env && (
                    <p
                      className={`text-xs ${
                        env.git.status === 'detected' ? 'text-emerald-500' : 'text-amber-500'
                      }`}
                    >
                      git:{' '}
                      {env.git.status === 'detected'
                        ? `已就绪 ${env.git.version ? `(${env.git.version})` : ''}`
                        : '未找到 —— 克隆前请先安装(克隆弹窗会给出指引)'}
                    </p>
                  )}
                  <Button size="sm" variant="outline" onClick={() => setCloneOpen(true)} disabled={Boolean(opActive)}>
                    <GitFork /> 克隆仓库并初始化
                  </Button>
                </div>

                <div className="mt-6 flex justify-end gap-2">
                  <Button variant="ghost" onClick={() => void skip()}>
                    跳过(稍后设置)
                  </Button>
                  <Button onClick={() => void runEnvCheck(repoPath)} disabled={checking || Boolean(opActive)}>
                    {checking ? <Loader2 className="animate-spin" /> : null}
                    检测环境
                  </Button>
                </div>
              </motion.div>
            )}

            {step === 'env' && env && (
              <motion.div key="env" {...STEP_ANIM}>
                <div className="space-y-3 rounded-xl border border-border bg-card p-4">
                  <EnvRow label="仓库" value={env.repoUsable.ok ? '可用 ✓' : `不可用:${env.repoUsable.reason ?? '未知'}`} ok={env.repoUsable.ok} />
                  <EnvRow label="Node" value={env.node.version ?? '未找到'} ok={env.node.status === 'detected'} />
                  <EnvRow label="pnpm" value={env.pnpm.version ?? '未找到'} ok={env.pnpm.status === 'detected'} />
                  <EnvRow label="git" value={env.git.version ?? '未找到'} ok={env.git.status === 'detected'} />
                  <EnvRow label="dist" value={env.distBuilt === null ? '未知' : env.distBuilt ? '已构建 ✓' : '未构建(将自动构建)'} ok={env.distBuilt !== false} />
                  {env.warnings.map((w, i) => (
                    <p key={i} className="text-xs text-amber-500">⚠ {w}</p>
                  ))}
                </div>

                <div className="mt-4 flex flex-wrap items-center gap-2">
                  {!env.repoUsable.ok && (
                    <Button variant="outline" size="sm" onClick={() => setCloneOpen(true)} disabled={Boolean(opActive)}>
                      <GitFork /> 克隆仓库
                    </Button>
                  )}
                  {env.node.status !== 'detected' && (
                    <Button variant="outline" size="sm" onClick={() => void api.runAction('install-node')} disabled={Boolean(opActive)}>
                      <Wrench /> 安装托管 Node 24
                    </Button>
                  )}
                  {(env.node.status !== 'detected' || env.pnpm.status !== 'detected' || env.git.status !== 'detected') && (
                    <Button variant="outline" size="sm" onClick={() => void installAll()} disabled={Boolean(opActive)}>
                      <ShieldCheck /> 一键安装缺失环境
                    </Button>
                  )}
                  <Button variant="ghost" size="sm" onClick={() => void detect(true)} disabled={checking}>
                    <RefreshCw className={checking ? 'animate-spin' : ''} /> 重新检测
                  </Button>
                  <Button variant="ghost" size="sm" onClick={() => setStep('repo')}>
                    修改仓库
                  </Button>
                </div>

                <div className="mt-6 flex items-center justify-between gap-2">
                  <Button variant="ghost" size="sm" onClick={() => void skip()}>
                    跳过(稍后设置)
                  </Button>
                  <Button size="sm" onClick={() => void finish()} disabled={saving || !env.repoUsable.ok || Boolean(opActive)}>
                    {saving ? '保存中…' : '完成,进入主界面'}
                  </Button>
                </div>
              </motion.div>
            )}
          </AnimatePresence>
        </div>
      </motion.div>

      {cloneOpen && (
        <CloneDialog
          onClose={() => setCloneOpen(false)}
          onSubmitted={() => setCloneOpen(false)}
        />
      )}
    </div>
  )
}

function EnvRow({ label, value, ok }: { label: string; value: string; ok: boolean }) {
  return (
    <div className="flex items-center justify-between text-[13px]">
      <span className="text-muted-foreground">{label}</span>
      <span className={ok ? 'text-emerald-500' : 'text-red-500'}>{value}</span>
    </div>
  )
}
