import { useEffect, useMemo, useState } from 'react'
import { FolderOpen, GitFork, Loader2, X } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { api } from '@/hooks/use-app'
import type { CloneDialogData, EnvironmentSnapshot } from '@/types/schema'

/** 从 URL 推导仓库目录名(与后端 repo_name_from_url 一致)。 */
function repoNameFromUrl(url: string): string {
  const u = url.trim().replace(/[\\/]+$/, '')
  if (!u) return ''
  const pathPart = u.includes('@') && u.includes(':') && !u.startsWith('ssh://') && !u.startsWith('http')
    ? (u.split(':')[1] ?? u)
    : (() => {
        try {
          return new URL(u).pathname
        } catch {
          return u
        }
      })()
  const last = pathPart.replace(/[\\/]+$/, '').split('/').pop() ?? ''
  const name = last.replace(/\.git$/i, '')
  if (!name || name === '.' || name === '..' || /[\\/:*?"<>|]/.test(name)) return ''
  return name
}

/** 最终克隆位置:target 为放置位置(父目录),自动追加仓库目录名。 */
function finalDirOf(target: string, repo: string): string {
  if (!target || !repo) return ''
  const base = target.replace(/[\\/]+$/, '')
  const last = base.split(/[\\/]/).pop() ?? ''
  return last === repo ? base : `${base}/${repo}`
}

/** 应用内 Clone 弹窗:M0/M1 契约
 *  - URL 默认填「上一次远端验证通过或 clone 成功」的地址(非法/失败输入绝不覆盖好地址)
 *  - 目标目录用原生选择器
 *  - 网络源显示(国内镜像/官方/自定义);高级分支选项
 *  - 仅克隆 / 一键全套(克隆+安装+构建+post-check+提交+启动)
 */
export function CloneDialog({
  onClose,
  onSubmitted,
}: {
  onClose: () => void
  onSubmitted: () => void
}) {
  const [data, setData] = useState<CloneDialogData | null>(null)
  const [env, setEnv] = useState<EnvironmentSnapshot | null>(null)
  const [url, setUrl] = useState('')
  const [targetDir, setTargetDir] = useState('')
  const [source, setSource] = useState<'mirror' | 'official' | 'custom'>('mirror')
  const [branch, setBranch] = useState('')
  const [showAdvanced, setShowAdvanced] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [submitting, setSubmitting] = useState(false)

  useEffect(() => {
    void api.openCloneDialog().then((d) => {
      setData(d)
      setUrl(d.lastGoodUrl ?? d.officialUrl)
      setTargetDir(d.defaultTarget)
    })
    // 展示 git 是否可用;缺失时阻止提交并给出明确安装指引(Windows 可装托管 MinGit)
    void api
      .inspectEnvironment()
      .then(setEnv)
      .catch(() => setEnv(null))
  }, [])

  const gitMissing = env ? env.git.status !== 'detected' : false

  /** 预览:自动生成的最终克隆目录(与后端逻辑一致)。 */
  const repoName = useMemo(() => repoNameFromUrl(url), [url])
  const finalDir = useMemo(() => finalDirOf(targetDir, repoName), [targetDir, repoName])

  const pickDir = async () => {
    const picked = await api.pickDirectory()
    if (picked) setTargetDir(picked)
  }

  const submit = async (full: boolean) => {
    setError(null)
    if (!url.trim()) {
      setError('克隆地址不能为空')
      return
    }
    if (!targetDir.trim()) {
      setError('目标目录不能为空')
      return
    }
    setSubmitting(true)
    try {
      const res = await api.submitCloneRequest(
        {
          url: url.trim(),
          targetDir: targetDir.trim(),
          source,
          branch: showAdvanced && branch.trim() ? branch.trim() : null,
        },
        full,
      )
      if (res.ok) {
        onSubmitted()
        onClose()
      } else {
        setError(res.reason ?? '请求被拒绝')
      }
    } catch (e) {
      setError(String(e))
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/40 p-4 backdrop-blur-sm">
      <div className="w-full max-w-[560px] overflow-hidden rounded-xl border border-border bg-card shadow-xl">
        <div className="flex items-center justify-between border-b border-border px-5 py-4">
          <div className="flex items-center gap-2">
            <GitFork className="size-4 text-blue-500" />
            <h2 className="text-sm font-semibold">克隆 DeepSeek Harness 仓库</h2>
          </div>
          <button
            onClick={onClose}
            className="rounded-lg p-1 text-muted-foreground hover:bg-muted hover:text-foreground"
            aria-label="关闭"
          >
            <X className="size-4" />
          </button>
        </div>

        <div className="space-y-4 px-5 py-4">
          {!data ? (
            <div className="flex items-center gap-2 py-6 text-muted-foreground">
              <Loader2 className="size-4 animate-spin" /> 加载弹窗数据…
            </div>
          ) : (
            <>
              <div>
                <label className="mb-1 block text-[12px] text-muted-foreground">
                  克隆地址(默认填充上次成功地址;只允许 HTTPS 与受控 SSH,URL 不得含凭证)
                </label>
                <Input
                  value={url}
                  onChange={(e) => setUrl(e.target.value)}
                  placeholder="https://github.com/deepseek-ai/deepseek-harness.git"
                  spellCheck={false}
                  autoFocus
                />
              </div>

              <div>
                <label className="mb-1 block text-[12px] text-muted-foreground">
                  放置位置(父目录,可为桌面等非空目录)
                </label>
                <div className="flex gap-2">
                  <Input
                    value={targetDir}
                    onChange={(e) => setTargetDir(e.target.value)}
                    placeholder="/Users/you/Desktop"
                    spellCheck={false}
                  />
                  <Button variant="outline" size="sm" onClick={() => void pickDir()}>
                    <FolderOpen className="size-4" /> 选择…
                  </Button>
                </div>
                <p className="mt-1 text-[11px] text-muted-foreground">
                  与 git clone 一致:将自动在放置位置下新建仓库目录,如
                </p>
                <p className="mt-0.5 truncate font-mono text-[11px] text-blue-600 dark:text-blue-400">
                  {finalDir || '← 填写 URL 后显示目标位置'}
                </p>
                <p className="mt-1 text-[11px] text-muted-foreground">
                  目标仓库目录已存在且非空时绝不会被覆盖。
                </p>
              </div>

              <div>
                <label className="mb-1 block text-[12px] text-muted-foreground">网络源</label>
                <div className="flex gap-1.5">
                  {(
                    [
                      ['mirror', '国内镜像'],
                      ['official', '官方源'],
                      ['custom', '自定义'],
                    ] as const
                  ).map(([v, label]) => (
                    <button
                      key={v}
                      onClick={() => setSource(v)}
                      className={`rounded-lg border px-3 py-1.5 text-[12px] ${
                        source === v
                          ? 'border-blue-500 bg-blue-500/10 text-blue-600 dark:text-blue-400'
                          : 'border-border text-muted-foreground hover:bg-muted'
                      }`}
                    >
                      {label}
                    </button>
                  ))}
                </div>
                <p className="mt-1 text-[11px] text-muted-foreground">
                  默认国内镜像(clone 地址由用户提供,Launcher 工具链下载固定走国内镜像)。
                </p>
              </div>

              <div>
                <button
                  onClick={() => setShowAdvanced((v) => !v)}
                  className="text-[12px] text-blue-600 hover:underline dark:text-blue-400"
                >
                  {showAdvanced ? '收起高级选项' : '高级选项(分支)…'}
                </button>
                {showAdvanced && (
                  <div className="mt-2">
                    <label className="mb-1 block text-[12px] text-muted-foreground">
                      指定分支(留空 = 自动发现远端 HEAD,不硬编码 main/master)
                    </label>
                    <Input
                      value={branch}
                      onChange={(e) => setBranch(e.target.value)}
                      placeholder="留空自动"
                      spellCheck={false}
                    />
                  </div>
                )}
              </div>

              {gitMissing && (
                <p className="rounded-lg border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-[12px] leading-relaxed text-amber-600 dark:text-amber-400">
                  未检测到 git,克隆需要 git。请关闭本弹窗,在「环境」步骤点击「一键安装缺失环境」
                  (Windows 会安装托管 MinGit;macOS/Linux 请安装系统 Git)后再回来克隆。
                </p>
              )}

              {error && (
                <p className="rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2 text-[12px] text-red-600 dark:text-red-400">
                  {error}
                </p>
              )}
            </>
          )}
        </div>

        <div className="flex items-center justify-end gap-2 border-t border-border bg-muted/30 px-5 py-3">
          <Button variant="ghost" size="sm" onClick={onClose} disabled={submitting}>
            取消
          </Button>
          <Button
            variant="outline"
            size="sm"
            onClick={() => void submit(false)}
            disabled={submitting || !data || gitMissing}
            title={gitMissing ? '未检测到 git,请先安装 git' : undefined}
          >
            <GitFork className="size-4" /> 仅克隆
          </Button>
          <Button
            size="sm"
            onClick={() => void submit(true)}
            disabled={submitting || !data || gitMissing}
            title={gitMissing ? '未检测到 git,请先安装 git' : undefined}
          >
            {submitting ? <Loader2 className="size-4 animate-spin" /> : <GitFork className="size-4" />}
            克隆并初始化(安装+构建)
          </Button>
        </div>
      </div>
    </div>
  )
}
