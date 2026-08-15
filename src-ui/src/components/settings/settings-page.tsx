import * as React from 'react'
import { FolderOpen, Globe, RefreshCw, Server, Wrench } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Select } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { useToast } from '@/components/ui/toast'
import { api } from '@/hooks/use-app'
import { useAppSnapshot } from '@/hooks/use-app'
import type { CloseBehavior, DesktopPreferences, SettingsSnapshot, Theme } from '@/types/schema'

const THEME_OPTIONS: { value: Theme; label: string }[] = [
  { value: 'system', label: '跟随系统' },
  { value: 'light', label: '浅色' },
  { value: 'dark', label: '深色' },
]

const CLOSE_OPTIONS: { value: CloseBehavior; label: string }[] = [
  { value: 'tray', label: '隐藏到托盘' },
  { value: 'quit', label: '退出应用(不停止 dsh)' },
]

function Section({ icon, title, children }: { icon: React.ReactNode; title: string; children: React.ReactNode }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          {icon}
          {title}
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-4">{children}</CardContent>
    </Card>
  )
}

function Field({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-6">
      <div className="min-w-0">
        <p className="text-[13px] font-medium text-[var(--foreground)]">{label}</p>
        {hint ? <p className="mt-0.5 text-xs text-[var(--muted-foreground)]">{hint}</p> : null}
      </div>
      <div className="w-56 shrink-0">{children}</div>
    </div>
  )
}

function validateSettings(s: SettingsSnapshot): string | null {
  if (!s.repoPath.trim()) return '仓库路径不能为空'
  if (s.port < 1 || s.port > 65535) return '端口必须在 1–65535 之间'
  if (!s.host.trim()) return 'host 不能为空'
  return null
}

/** 设置页:基础/行为/外观/运行时/更新/关于。 */
export function SettingsPage({ onBack }: { onBack: () => void }) {
  const snap = useAppSnapshot()
  const { toast } = useToast()
  const [engine, setEngine] = React.useState<SettingsSnapshot | null>(null)
  const [prefs, setPrefs] = React.useState<DesktopPreferences | null>(null)
  const [dirty, setDirty] = React.useState(false)
  const [saving, setSaving] = React.useState(false)
  const [checking, setChecking] = React.useState(false)
  const [envVersion, setEnvVersion] = React.useState(0)
  const leaving = React.useRef(false)

  React.useEffect(() => {
    let mounted = true
    void Promise.all([api.getSettings(), api.getDesktopSnapshot()]).then(([s, d]) => {
      if (!mounted) return
      setEngine(s)
      setPrefs(d.preferences)
    })
    return () => {
      mounted = false
    }
  }, [])

  // 离开前提示未保存修改
  React.useEffect(() => {
    const before = (e: BeforeUnloadEvent) => {
      if (dirty) e.preventDefault()
    }
    window.addEventListener('beforeunload', before)
    return () => window.removeEventListener('beforeunload', before)
  }, [dirty])

  const goBack = () => {
    if (dirty && !window.confirm('有未保存的修改,确定离开吗?')) return
    leaving.current = true
    onBack()
  }

  const save = async () => {
    if (!engine || !prefs) return
    const err = validateSettings(engine)
    if (err) {
      toast({ kind: 'error', title: '设置无效', detail: err })
      return
    }
    setSaving(true)
    const prevEngine = engine
    const prevPrefs = prefs
    try {
      const [savedEngine, savedPrefs] = await Promise.all([
        api.saveSettings(engine),
        api.savePreferences(prefs),
      ])
      setEngine(savedEngine)
      setPrefs(savedPrefs)
      setDirty(false)
      toast({ kind: 'success', title: '设置已保存' })
    } catch (e) {
      // 回滚 optimistic state
      setEngine(prevEngine)
      setPrefs(prevPrefs)
      toast({ kind: 'error', title: '保存失败', detail: String(e) })
    } finally {
      setSaving(false)
    }
  }

  const setEngineField = <K extends keyof SettingsSnapshot>(k: K, v: SettingsSnapshot[K]) => {
    setEngine((s) => (s ? { ...s, [k]: v } : s))
    setDirty(true)
  }

  const setPrefsField = <K extends keyof DesktopPreferences>(k: K, v: DesktopPreferences[K]) => {
    setPrefs((p) => (p ? { ...p, [k]: v } : p))
    setDirty(true)
  }

  const checkUpdate = async () => {
    setChecking(true)
    try {
      const r = await api.checkForUpdate()
      const message = r.message ?? r.error
      if (r.available) {
        toast({ kind: 'success', title: '发现新版本', detail: message ?? undefined })
      } else if (!message) {
        toast({ kind: 'success', title: '当前已是最新版本', detail: `v${snap?.version ?? '?'}` })
      } else {
        toast({ kind: 'warning', title: '检查更新', detail: message })
      }
    } finally {
      setChecking(false)
    }
  }

  if (!engine || !prefs) {
    return (
      <main className="flex flex-1 items-center justify-center text-[var(--muted-foreground)]">
        加载设置中…
      </main>
    )
  }

  const running = snap?.state === 'running'

  return (
    <main className="flex-1 space-y-4 overflow-y-auto p-5">
      <div className="flex items-center gap-2">
        <Button variant="ghost" size="sm" onClick={goBack}>
          ← 返回
        </Button>
        <h2 className="text-sm font-semibold">设置</h2>
        <div className="flex-1" />
        <Button size="sm" onClick={() => void save()} disabled={!dirty || saving}>
          {saving ? '保存中…' : '保存'}
        </Button>
      </div>

      <Section icon={<Server className="size-4 text-[var(--primary)]" />} title="基础">
        <Field label="仓库路径" hint="DeepSeek Harness 源码目录">
          <div className="flex gap-1.5">
            <Input
              value={engine.repoPath}
              onChange={(e) => setEngineField('repoPath', e.target.value)}
              spellCheck={false}
            />
            <Button
              variant="outline"
              size="icon"
              title="选择目录"
              onClick={() => void api.pickDirectory().then((p) => p && (setEngineField('repoPath', p), setDirty(true)))}
            >
              <FolderOpen />
            </Button>
          </div>
        </Field>
        <Field label="端口" hint="dsh web 监听端口(默认 3080)">
          <Input
            type="number"
            min={1}
            max={65535}
            value={engine.port}
            onChange={(e) => setEngineField('port', Number(e.target.value))}
          />
        </Field>
        <Field label="Host" hint="监听地址">
          <Input value={engine.host} onChange={(e) => setEngineField('host', e.target.value)} spellCheck={false} />
        </Field>
        <Field label="DSH_HOME" hint="留空 = 继承环境默认">
          <Input value={engine.dshHome} onChange={(e) => setEngineField('dshHome', e.target.value)} spellCheck={false} />
        </Field>
      </Section>

      <Section icon={<Wrench className="size-4 text-[var(--primary)]" />} title="行为">
        <Field label="关闭窗口时" hint="托盘模式关闭后仍可在托盘召回">
          <Select
            options={CLOSE_OPTIONS}
            value={prefs.closeBehavior}
            onChange={(v) => setPrefsField('closeBehavior', v)}
            aria-label="关闭窗口时"
          />
        </Field>
        <Field label="开机启动" hint="登录系统后自动启动">
          <Switch
            checked={prefs.launchOnStartup}
            onCheckedChange={(v) => setPrefsField('launchOnStartup', v)}
            label="开机启动"
          />
        </Field>
        <Field label="静默启动" hint="启动时不显示主窗口(托盘可召回)">
          <Switch
            checked={prefs.silentStartup}
            onCheckedChange={(v) => setPrefsField('silentStartup', v)}
            label="静默启动"
          />
        </Field>
        <Field label="就绪后打开 dsh" hint="启动完成后自动打开浏览器">
          <Switch
            checked={engine.openBrowser}
            onCheckedChange={(v) => setEngineField('openBrowser', v)}
            label="就绪后打开 dsh"
          />
        </Field>
        <Field label="停止并退出前确认" hint="托盘「停止服务并退出」二次确认">
          <Switch
            checked={prefs.confirmStopAndQuit}
            onCheckedChange={(v) => setPrefsField('confirmStopAndQuit', v)}
            label="停止并退出前确认"
          />
        </Field>
        <Field label="显示托盘图标">
          <Switch
            checked={prefs.showTrayIcon}
            onCheckedChange={(v) => setPrefsField('showTrayIcon', v)}
            label="显示托盘图标"
          />
        </Field>
      </Section>

      <Section icon={<Globe className="size-4 text-[var(--primary)]" />} title="外观">
        <Field label="主题" hint="跟随系统或手动指定">
          <Select options={THEME_OPTIONS} value={prefs.theme} onChange={(v) => setPrefsField('theme', v)} aria-label="主题" />
        </Field>
      </Section>

      <Section icon={<RefreshCw className="size-4 text-[var(--primary)]" />} title="运行时">
        <RuntimeCard refreshKey={envVersion} />
        <div className="flex items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              setEnvVersion((v) => v + 1)
              toast({ kind: 'info', title: '已重新检测环境' })
            }}
          >
            重新检测
          </Button>
          <Button variant="outline" size="sm" onClick={() => void api.runAction('install-node').then((r) => r.ok ? toast({ kind: 'success', title: '托管 Node 安装完成' }) : toast({ kind: 'error', title: '安装失败', detail: r.reason }))}>
            安装托管 Node
          </Button>
          {running ? (
            <p className="ml-auto text-xs text-[var(--warning)]">服务运行中:部分引擎设置重启后生效</p>
          ) : null}
        </div>
      </Section>

      <Section icon={<RefreshCw className="size-4 text-[var(--primary)]" />} title="更新">
        <Field label="当前版本" hint="桌面版">
          <Input value={`v${snap?.version ?? '?'}`} readOnly disabled />
        </Field>
        <Field label="自动检查更新" hint="启动时检查 GitHub Releases">
          <Switch
            checked={engine.autoUpdateCheck}
            onCheckedChange={(v) => setEngineField('autoUpdateCheck', v)}
            label="自动检查更新"
          />
        </Field>
        <div>
          <Button variant="outline" size="sm" onClick={() => void checkUpdate()} disabled={checking}>
            {checking ? '检查中…' : '立即检查更新'}
          </Button>
        </div>
      </Section>

      <Section icon={<Server className="size-4 text-[var(--primary)]" />} title="关于">
        <p className="text-[13px] text-[var(--muted-foreground)]">
          DSH Launcher — DeepSeek Harness 桌面启动器 v{snap?.version}
          <br />
          GitHub:{' '}
          <a
            className="text-[var(--primary)] underline"
            href="https://github.com/Yoahoug/dsh-launcher"
            target="_blank"
            rel="noreferrer"
          >
            Yoahoug/dsh-launcher
          </a>
        </p>
      </Section>
    </main>
  )
}

function RuntimeCard({ refreshKey }: { refreshKey: number }) {
  const [env, setEnv] = React.useState<{ node: string; pnpm: string; git: string; dist: string; warnings: string[] } | null>(null)

  React.useEffect(() => {
    let mounted = true
    void api.inspectEnvironment().then((e) => {
      if (!mounted) return
      setEnv({
        node: `${e.node.current}${e.node.inRange ? '' : ' (版本不在范围内)'}`,
        pnpm: e.pnpm ?? '未找到',
        git: e.git ?? '未找到',
        dist: e.distBuilt === null ? '未知' : e.distBuilt ? '已构建' : '未构建',
        warnings: e.warnings,
      })
    })
    return () => {
      mounted = false
    }
  }, [refreshKey])

  if (!env) return <p className="text-xs text-[var(--muted-foreground)]">检测中…</p>

  return (
    <div className="space-y-1.5 text-[13px]">
      <Row label="Node" value={env.node} />
      <Row label="pnpm" value={env.pnpm} />
      <Row label="git" value={env.git} />
      <Row label="dist" value={env.dist} />
      {env.warnings.map((w, i) => (
        <p key={i} className="text-xs text-[var(--warning)]">
          ⚠ {w}
        </p>
      ))}
    </div>
  )
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex justify-between gap-4">
      <span className="text-[var(--muted-foreground)]">{label}</span>
      <span className="font-mono text-[var(--foreground)]">{value}</span>
    </div>
  )
}
