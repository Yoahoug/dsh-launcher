import { useEffect, useState } from 'react'
import { FolderOpen, GitFork, Loader2, X } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { api } from '@/hooks/use-app'
import type { CloneDialogData } from '@/types/schema'

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
  }, [])

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
                <label className="mb-1 block text-[12px] text-muted-foreground">目标目录</label>
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
                  目标目录必须为空或不存在;非空目录绝不会被覆盖。
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
          <Button variant="outline" size="sm" onClick={() => void submit(false)} disabled={submitting || !data}>
            <GitFork className="size-4" /> 仅克隆
          </Button>
          <Button size="sm" onClick={() => void submit(true)} disabled={submitting || !data}>
            {submitting ? <Loader2 className="size-4 animate-spin" /> : <GitFork className="size-4" />}
            一键安装并启动
          </Button>
        </div>
      </div>
    </div>
  )
}
