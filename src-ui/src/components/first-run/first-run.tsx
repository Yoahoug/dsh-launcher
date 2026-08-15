import * as React from 'react'
import { AnimatePresence, motion } from 'framer-motion'
import { CheckCircle2, FolderOpen, Loader2, RefreshCw } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useToast } from '@/components/ui/toast'
import { api } from '@/hooks/use-app'
import logoUrl from '@/assets/logo.svg'
import type { EnvironmentSnapshot } from '@/types/schema'

type Step = 'welcome' | 'repo' | 'env' | 'node' | 'done'

const DEFAULT_REPO = '~/Desktop/deepseek-harness'

const STEP_ANIM = {
  initial: { opacity: 0, y: 8 },
  animate: { opacity: 1, y: 0 },
  exit: { opacity: 0, y: -8 },
  transition: { duration: 0.2, ease: [0.16, 1, 0.3, 1] as const },
}

/** 首次运行引导:仓库选择 → 环境检测 → 托管 Node → 完成。失败不白屏,可进设置。 */
export function FirstRunPage({ onDone, onOpenSettings }: { onDone: () => void; onOpenSettings: () => void }) {
  const { toast } = useToast()
  const [step, setStep] = React.useState<Step>('welcome')
  const [repoPath, setRepoPath] = React.useState(DEFAULT_REPO)
  const [env, setEnv] = React.useState<EnvironmentSnapshot | null>(null)
  const [checking, setChecking] = React.useState(false)
  const [installing, setInstalling] = React.useState(false)
  const [saving, setSaving] = React.useState(false)

  const runEnvCheck = async (path?: string) => {
    setChecking(true)
    try {
      if (path && path !== env?.repoPath) {
        await api.saveSettings({ repoPath: path })
      }
      const e = await api.inspectEnvironment()
      setEnv(e)
      setStep('env')
      return e
    } catch (err) {
      toast({ kind: 'error', title: '环境检测失败', detail: String(err) })
      return null
    } finally {
      setChecking(false)
    }
  }

  const installNode = async () => {
    setInstalling(true)
    try {
      const r = await api.runAction('install-node')
      if (r.ok) {
        toast({ kind: 'success', title: '托管 Node 安装完成' })
        await runEnvCheck()
      } else {
        toast({ kind: 'error', title: '安装失败', detail: r.reason })
      }
    } finally {
      setInstalling(false)
    }
  }

  const finish = async () => {
    setSaving(true)
    try {
      await api.saveSettings({ repoPath })
      await api.inspectEnvironment()
      toast({ kind: 'success', title: '设置已保存' })
      onDone()
    } catch (err) {
      toast({ kind: 'error', title: '保存失败', detail: String(err) })
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="flex h-full items-center justify-center p-8">
      <motion.div
        initial={{ opacity: 0, y: 16, scale: 0.98 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        transition={{ duration: 0.3, ease: [0.16, 1, 0.3, 1] }}
        className="glass-card w-full max-w-lg rounded-xl p-8"
      >
        <div className="flex items-center gap-3">
          <img src={logoUrl} alt="" className="size-10 rounded-xl" />
          <div>
            <h1 className="text-xl font-semibold">欢迎使用 DSH Launcher</h1>
            <p className="mt-0.5 text-xs text-muted-foreground">DeepSeek Harness 桌面启动器 · v0.4.0</p>
          </div>
        </div>
        <p className="mt-4 text-sm leading-relaxed text-muted-foreground">
          一个纯粹的 DeepSeek Harness 启动器:托管开发流程、构建与更新,让 dsh web
          稳定运行在后台。首次使用需要选择仓库并确认运行环境。
        </p>

        <div className="mt-6 min-h-[180px]">
          <AnimatePresence mode="wait">
            {step === 'welcome' && (
              <motion.div key="welcome" {...STEP_ANIM} className="flex justify-end gap-2">
                <Button variant="ghost" onClick={onOpenSettings}>
                  稍后配置
                </Button>
                <Button onClick={() => setStep('repo')}>开始配置</Button>
              </motion.div>
            )}

            {step === 'repo' && (
              <motion.div key="repo" {...STEP_ANIM}>
                <div className="space-y-2">
                  <p className="text-sm font-medium">仓库位置</p>
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
                    默认自动探测 ~/Desktop/deepseek-harness,可手动选择其它目录
                  </p>
                </div>
                <div className="mt-6 flex justify-end gap-2">
                  <Button variant="ghost" onClick={onOpenSettings}>
                    跳过(稍后设置)
                  </Button>
                  <Button onClick={() => void runEnvCheck(repoPath)} disabled={checking}>
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
                  <EnvRow label="Node" value={env.node.current || '未找到'} ok={env.node.inRange} />
                  <EnvRow label="pnpm" value={env.pnpm ?? '未找到'} ok={Boolean(env.pnpm)} />
                  <EnvRow label="git" value={env.git ?? '未找到'} ok={Boolean(env.git)} />
                  <EnvRow label="dist" value={env.distBuilt === null ? '未知' : env.distBuilt ? '已构建 ✓' : '未构建(将自动构建)'} ok={env.distBuilt !== false} />
                  {env.warnings.map((w, i) => (
                    <p key={i} className="text-xs text-amber-500">⚠ {w}</p>
                  ))}
                </div>
                <div className="mt-6 flex items-center justify-between">
                  <div className="flex gap-2">
                    <Button variant="ghost" size="sm" onClick={() => void runEnvCheck()} disabled={checking}>
                      <RefreshCw className={checking ? 'animate-spin' : ''} /> 重新检测
                    </Button>
                    <Button variant="ghost" size="sm" onClick={() => setStep('repo')}>
                      修改仓库
                    </Button>
                    {!env?.node.inRange && !env?.node.current && (
                      <Button variant="outline" size="sm" onClick={() => void installNode()} disabled={installing}>
                        {installing ? <Loader2 className="animate-spin" /> : null}
                        安装托管 Node 24
                      </Button>
                    )}
                  </div>
                  <Button size="sm" onClick={() => void finish()} disabled={saving || !env?.repoUsable.ok}>
                    {saving ? '保存中…' : '完成,进入主界面'}
                  </Button>
                </div>
              </motion.div>
            )}

            {step === 'done' && (
              <motion.div key="done" {...STEP_ANIM} className="flex flex-col items-center gap-3 py-4">
                <CheckCircle2 className="size-10 text-emerald-500" />
                <p className="text-sm">配置完成</p>
                <Button onClick={onDone}>进入主界面</Button>
              </motion.div>
            )}
          </AnimatePresence>
        </div>
      </motion.div>
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
