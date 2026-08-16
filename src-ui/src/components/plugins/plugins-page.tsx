import * as React from 'react'
import {
  Boxes,
  ChevronDown,
  ChevronRight,
  FolderOpen,
  Package,
  Puzzle,
  RefreshCw,
  RotateCcw,
  Save,
  Search,
  ShieldAlert,
  TerminalSquare,
} from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input, Textarea } from '@/components/ui/input'
import { Select } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { useToast } from '@/components/ui/toast'
import { api } from '@/hooks/use-app'
import { cn } from '@/lib/utils'
import type {
  AppSnapshot,
  OperationKind,
  PluginRow,
  PluginsSnapshot,
  SettingsSnapshot,
} from '@/types/schema'

type GroupKey = 'all' | 'official' | 'dsh-plugins' | 'user' | 'disabled'

const GROUPS: { key: GroupKey; label: string }[] = [
  { key: 'all', label: '全部' },
  { key: 'official', label: '官方内置' },
  { key: 'dsh-plugins', label: 'dsh-plugins' },
  { key: 'user', label: '用户补丁' },
  { key: 'disabled', label: '已停用' },
]

const LAYER_BADGE: Record<PluginRow['layer'], { label: string; variant: 'neutral' | 'primary' | 'info' | 'warning' }> = {
  bundle: { label: '组合包', variant: 'neutral' },
  'profile-patch': { label: '用户补丁', variant: 'primary' },
  'home-patch': { label: 'home 补丁', variant: 'info' },
  overlay: { label: 'overlay', variant: 'warning' },
}

function inGroup(row: PluginRow, group: GroupKey): boolean {
  switch (group) {
    case 'all':
      return true
    case 'official':
      return row.layer === 'bundle' && !row.module.startsWith('@dsh-plugins/')
    case 'dsh-plugins':
      return row.module.startsWith('@dsh-plugins/')
    case 'user':
      return row.inUserPatch || row.layer === 'profile-patch' || row.layer === 'home-patch'
    case 'disabled':
      return !row.enabled
  }
}

function deepClone(v: unknown): unknown {
  return JSON.parse(JSON.stringify(v))
}

/** 是否为密钥类字段(回显用 password 输入,永不明文展示给旁观者)。 */
function isSecretKey(key: string): boolean {
  return /(key|secret|token|password|apikey)/i.test(key)
}

/** 由生效 config 自动生成的通用表单(string/number/boolean/string[]/嵌套对象折叠)。 */
function ConfigEditor({
  config,
  onChange,
  depth = 0,
}: {
  config: Record<string, unknown>
  onChange: (v: Record<string, unknown>) => void
  depth?: number
}) {
  const set = (key: string, value: unknown) => onChange({ ...config, [key]: value })
  return (
    <div className={cn('space-y-2.5', depth > 0 && 'rounded-lg border border-border/70 p-3')}>
      {Object.entries(config).map(([key, value]) => (
        <ConfigField key={key} name={key} value={value} onChange={(v) => set(key, v)} depth={depth} />
      ))}
    </div>
  )
}

function ConfigField({
  name,
  value,
  onChange,
  depth,
}: {
  name: string
  value: unknown
  onChange: (v: unknown) => void
  depth: number
}) {
  const [open, setOpen] = React.useState(false)
  const label = <span className="w-40 shrink-0 truncate font-mono text-xs text-muted-foreground" title={name}>{name}</span>

  if (typeof value === 'boolean') {
    return (
      <div className="flex items-center justify-between gap-3">
        {label}
        <Switch checked={value} onCheckedChange={onChange} label={name} />
      </div>
    )
  }
  if (typeof value === 'number') {
    return (
      <div className="flex items-center justify-between gap-3">
        {label}
        <Input
          type="number"
          className="h-8 w-40 text-xs"
          value={String(value)}
          onChange={(e) => onChange(Number(e.target.value))}
          aria-label={name}
        />
      </div>
    )
  }
  if (typeof value === 'string') {
    return (
      <div className="flex items-center justify-between gap-3">
        {label}
        <Input
          type={isSecretKey(name) ? 'password' : 'text'}
          className="h-8 w-56 text-xs"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          aria-label={name}
          spellCheck={false}
        />
      </div>
    )
  }
  if (Array.isArray(value) && value.every((v) => typeof v === 'string')) {
    return (
      <div className="flex items-start justify-between gap-3">
        {label}
        <Textarea
          className="h-20 w-56 text-xs"
          value={value.join('\n')}
          onChange={(e) =>
            onChange(e.target.value.split('\n').map((s) => s.trim()).filter(Boolean))
          }
          aria-label={name}
        />
      </div>
    )
  }
  if (Array.isArray(value) && value.length === 0) {
    return (
      <div className="flex items-center justify-between gap-3">
        {label}
        <span className="text-xs text-muted-foreground">空数组</span>
      </div>
    )
  }
  if (typeof value === 'object' && value !== null) {
    const entries = Object.entries(value as Record<string, unknown>)
    if (depth < 2 && entries.length > 0) {
      return (
        <div className="flex flex-col gap-1.5">
          <button
            type="button"
            onClick={() => setOpen((o) => !o)}
            className="flex items-center gap-1.5 text-xs font-medium text-foreground hover:text-blue-500"
          >
            {open ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />}
            <span className="font-mono">{name}</span>
            <span className="text-muted-foreground">(嵌套对象 · {entries.length} 键)</span>
          </button>
          {open && (
            <ConfigEditor
              config={value as Record<string, unknown>}
              onChange={(v) => onChange(v)}
              depth={depth + 1}
            />
          )}
        </div>
      )
    }
    // 深层/复杂结构:JSON 文本编辑
    return (
      <div className="flex items-start justify-between gap-3">
        {label}
        <Textarea
          className="h-24 w-64 font-mono text-xs"
          value={JSON.stringify(value, null, 2)}
          onChange={(e) => {
            try {
              onChange(JSON.parse(e.target.value))
            } catch {
              /* 非法 JSON 暂不提交 */
            }
          }}
          aria-label={name}
          spellCheck={false}
        />
      </div>
    )
  }
  // null / undefined / 其它
  return (
    <div className="flex items-center justify-between gap-3">
      {label}
      <span className="font-mono text-xs text-muted-foreground">{String(value)}</span>
    </div>
  )
}

/** 单张插件卡片(可展开:配置表单 / 原始 YAML)。 */
function PluginCard({
  row,
  profile,
  onMutated,
  onReset,
}: {
  row: PluginRow
  profile: string
  onMutated: () => void
  onReset: (id: string) => void
}) {
  const { toast } = useToast()
  const [expanded, setExpanded] = React.useState(false)
  const [rawMode, setRawMode] = React.useState(row.configSource === 'raw-yaml')
  const [draft, setDraft] = React.useState<Record<string, unknown>>(() =>
    row.config && typeof row.config === 'object' ? deepClone(row.config) as Record<string, unknown> : {},
  )
  const [rawText, setRawText] = React.useState(row.rawBlock)
  const [saving, setSaving] = React.useState(false)
  const [busy, setBusy] = React.useState(false)
  const badge = LAYER_BADGE[row.layer]

  // 行变化时重置草稿(profile/数据刷新后)
  React.useEffect(() => {
    setDraft(row.config && typeof row.config === 'object' ? deepClone(row.config) as Record<string, unknown> : {})
    setRawText(row.rawBlock)
    setRawMode(row.configSource === 'raw-yaml')
  }, [row.id, row.config, row.rawBlock, row.configSource, profile])

  const toggleEnabled = async (enabled: boolean) => {
    setBusy(true)
    try {
      const r = await api.pluginsSetEnabled(profile, row.id, enabled)
      if (r.ok && r.validated) {
        toast({ kind: 'success', title: enabled ? '已启用' : '已停用', detail: r.summary })
        onMutated()
      } else {
        toast({ kind: 'error', title: '操作失败', detail: r.error ?? r.summary })
      }
    } catch (e) {
      toast({ kind: 'error', title: '操作失败', detail: String(e) })
    } finally {
      setBusy(false)
    }
  }

  const save = async () => {
    setSaving(true)
    try {
      const r = rawMode
        ? await api.pluginsSaveConfig(profile, row.id, {}, rawText)
        : await api.pluginsSaveConfig(profile, row.id, draft)
      if (r.ok && r.validated) {
        toast({
          kind: 'success',
          title: '已保存并校验通过',
          detail: `${r.summary}${r.backup ? `(备份:${r.backup})` : ''} · 运行中的 dsh web 已热重载`,
        })
        onMutated()
      } else {
        toast({ kind: 'error', title: '保存失败', detail: r.error ?? r.summary })
      }
    } catch (e) {
      toast({ kind: 'error', title: '保存失败', detail: String(e) })
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="rounded-xl border border-border bg-card transition-all duration-300 hover:border-border-hover">
      {/* 头部:名称 + 来源徽标 + 启用开关 */}
      <div className="flex items-center gap-3 px-4 py-3">
        <button
          type="button"
          onClick={() => setExpanded((e) => !e)}
          className="flex min-w-0 flex-1 items-center gap-2 text-left"
          aria-expanded={expanded}
          aria-label={expanded ? `收起 ${row.id}` : `展开 ${row.id}`}
        >
          {expanded ? <ChevronDown className="size-4 shrink-0 text-muted-foreground" /> : <ChevronRight className="size-4 shrink-0 text-muted-foreground" />}
          <span className="truncate font-mono text-sm font-semibold text-foreground">{row.id}</span>
          <Badge variant={badge.variant}>{badge.label}</Badge>
          {row.configSource === 'raw-yaml' && <Badge variant="warning">!!js 原始 YAML</Badge>}
          {!row.enabled && <Badge variant="danger">已停用</Badge>}
        </button>
        <div className="flex shrink-0 items-center gap-2">
          {row.module && (
            <span className="hidden max-w-52 truncate font-mono text-[11px] text-muted-foreground lg:block" title={row.module}>
              {row.module}
            </span>
          )}
          <Switch
            checked={row.enabled}
            onCheckedChange={(v) => void toggleEnabled(v)}
            disabled={busy}
            label={`启用 ${row.id}`}
          />
        </div>
      </div>

      {/* 正文:说明 + 管辖 */}
      {(row.description || row.layerLabel) && !expanded && (
        <div className="flex items-center gap-2 px-11 pb-3">
          {row.description && (
            <p className="min-w-0 flex-1 truncate text-xs text-muted-foreground" title={row.description}>
              {row.description}
            </p>
          )}
          <span className="max-w-64 truncate font-mono text-[11px] text-muted-foreground/70" title={row.layerLabel}>
            来源:{row.layerLabel}
          </span>
        </div>
      )}

      {/* 展开:配置表单 / 原始 YAML */}
      {expanded && (
        <div className="space-y-3 border-t border-border/60 px-4 py-3">
          {row.description && (
            <p className="text-xs text-muted-foreground">{row.description}</p>
          )}
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-[11px] text-muted-foreground">来源层:{row.layerLabel}</span>
            <span className="text-[11px] text-muted-foreground">模块:{row.module || '—'}</span>
            {row.inUserPatch && <Badge variant="primary">已有用户补丁条目</Badge>}
            {!row.inUserPatch && row.layer !== 'overlay' && (
              <span className="text-[11px] text-amber-500">将写入 profile 补丁覆盖(整行重述,上游更新不改变覆盖)</span>
            )}
          </div>

          {!row.editable ? (
            <p className="text-xs text-muted-foreground">overlay 层(--patch argv)为只读视图。</p>
          ) : (
            <>
              {row.configSource === 'raw-yaml' ? (
                <div>
                  <p className="mb-1.5 text-xs text-amber-600 dark:text-amber-400">
                    该行含 <code className="font-mono">!!js</code> 表达式,dump-config 不求值;锁定为原始 YAML 模式,禁止表单化。
                  </p>
                  <Textarea
                    className="h-40 w-full font-mono text-xs"
                    value={rawText}
                    onChange={(e) => setRawText(e.target.value)}
                    spellCheck={false}
                    aria-label={`${row.id} 原始 YAML`}
                  />
                </div>
              ) : (
                <div className="space-y-2">
                  <div className="flex items-center gap-2">
                    <span className="text-[11px] text-muted-foreground">配置(表单由生效 config 自动生成;保存固化整行全量键,非深合并)</span>
                    <button
                      type="button"
                      onClick={() => setRawMode((m) => !m)}
                      className="ml-auto text-[11px] font-medium text-blue-500 hover:underline"
                    >
                      {rawMode ? '切换回表单' : '切换原始 YAML 高级模式'}
                    </button>
                  </div>
                  {rawMode ? (
                    <Textarea
                      className="h-40 w-full font-mono text-xs"
                      value={rawText}
                      onChange={(e) => setRawText(e.target.value)}
                      spellCheck={false}
                      aria-label={`${row.id} 原始 YAML`}
                    />
                  ) : (
                    <ConfigEditor config={draft} onChange={setDraft} />
                  )}
                </div>
              )}
              <div className="flex flex-wrap items-center gap-2">
                <Button size="sm" onClick={() => void save()} disabled={saving}>
                  <Save /> {saving ? '保存中…' : '保存配置'}
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => onReset(row.id)}
                  disabled={!row.inUserPatch}
                  title={row.inUserPatch ? '移除用户补丁条目,回落到上层默认' : '该行没有用户补丁条目'}
                >
                  <RotateCcw /> 重置
                </Button>
                <p className="ml-auto text-[11px] text-muted-foreground">
                  保存流程:备份 → 写入补丁 → dump-config 校验;失败自动回滚
                </p>
              </div>
            </>
          )}
        </div>
      )}
    </div>
  )
}

/** dsh-plugins 联动面板:可安装包列表。 */
function PackagesPanel({
  snap,
  profile,
  pluginsPath,
  onInstallAll,
  onInstalled,
  onRemoved,
}: {
  snap: PluginsSnapshot
  profile: string
  pluginsPath: string
  onInstallAll: () => void
  onInstalled: () => void
  onRemoved: () => void
}) {
  const { toast } = useToast()
  const [busyName, setBusyName] = React.useState<string | null>(null)

  const install = async (pkg: (typeof snap.packages)[number]) => {
    if (busyName) return
    setBusyName(pkg.name)
    try {
      const r = await api.pluginsInstallPackage(profile, pkg.absDir)
      if (r.ok) {
        toast({
          kind: 'info',
          title: '安装已受理',
          detail: `构建 + dsh plugin add 进行中,完成后点「刷新」查看新卡片`,
        })
        onInstalled()
      } else {
        toast({ kind: 'error', title: '安装失败', detail: r.reason })
      }
    } catch (e) {
      toast({ kind: 'error', title: '安装失败', detail: String(e) })
    } finally {
      setBusyName(null)
    }
  }

  const remove = async (pkg: (typeof snap.packages)[number]) => {
    if (busyName) return
    setBusyName(pkg.name)
    try {
      const r = await api.pluginsRemovePackage(profile, pkg.name)
      if (r.ok) {
        toast({ kind: 'info', title: '移除已受理', detail: 'dsh plugin remove 进行中,完成后点「刷新」' })
        onRemoved()
      } else {
        toast({ kind: 'error', title: '移除失败', detail: r.reason })
      }
    } catch (e) {
      toast({ kind: 'error', title: '移除失败', detail: String(e) })
    } finally {
      setBusyName(null)
    }
  }

  return (
    <div className="flex w-72 shrink-0 flex-col rounded-xl border border-border bg-card">
      <div className="flex items-center gap-2 border-b border-border px-4 py-3">
        <Package className="size-4 text-blue-500" />
        <h3 className="text-sm font-semibold">dsh-plugins 仓库</h3>
        {pluginsPath ? (
          <button
            type="button"
            title="打开目录"
            onClick={() => void api.pluginsOpenInExplorer(pluginsPath).catch(() => {})}
            className="ml-auto text-muted-foreground hover:text-foreground"
          >
            <FolderOpen className="size-4" />
          </button>
        ) : null}
      </div>
      <div className="border-b border-border/70 px-3 py-2">
        <Button size="sm" className="w-full" onClick={onInstallAll}>
          <RefreshCw /> {pluginsPath ? '同步并安装' : '从 GitHub 拉取并安装'}
        </Button>
      </div>
      <div className="flex-1 space-y-2 overflow-y-auto p-3">
        {!pluginsPath && (
          <p className="rounded-lg border border-dashed border-border p-3 text-xs text-muted-foreground">
            当前尚未命中本地仓库路径；点击上方按钮会从 GitHub 拉取到默认目录，也可以在设置「插件与技能」中填写仓库根。
          </p>
        )}
        {pluginsPath && (
          <p className="rounded-lg bg-muted/50 px-2.5 py-2 text-[10px] text-muted-foreground" title={pluginsPath}>
            生效仓库: <span className="font-mono">{pluginsPath}</span>
          </p>
        )}
        {snap.packages.length === 0 && pluginsPath && (
          <p className="text-xs text-muted-foreground">未扫描到 packages/*(检查路径是否指向仓库根)。</p>
        )}
        {snap.packages.map((pkg) => {
          const installed = pkg.installedIn.includes(profile)
          return (
            <div key={pkg.name} className="rounded-lg border border-border/70 p-2.5">
              <div className="flex items-center gap-1.5">
                <span className="min-w-0 flex-1 truncate font-mono text-xs font-semibold" title={pkg.name}>
                  {pkg.name}
                </span>
                {pkg.isBundle && <Badge variant="primary">bundle</Badge>}
                <span className="shrink-0 text-[10px] text-muted-foreground">v{pkg.version}</span>
              </div>
              {pkg.description && (
                <p className="mt-1 line-clamp-2 text-[11px] text-muted-foreground" title={pkg.description}>
                  {pkg.description}
                </p>
              )}
              <div className="mt-2 flex items-center gap-1.5">
                {installed ? (
                  <>
                    <Badge variant="success">已安装到 {profile}</Badge>
                    <Button
                      variant="outline"
                      size="sm"
                      className="ml-auto h-7 px-2 text-[11px]"
                      onClick={() => void remove(pkg)}
                      disabled={busyName === pkg.name}
                    >
                      移除
                    </Button>
                  </>
                ) : (
                  <Button
                    size="sm"
                    className="ml-auto h-7 px-2 text-[11px]"
                    onClick={() => void install(pkg)}
                    disabled={busyName === pkg.name}
                  >
                    <Boxes /> 安装到 {profile}
                  </Button>
                )}
                <button
                  type="button"
                  title="打开包目录"
                  onClick={() => void api.pluginsOpenInExplorer(pkg.absDir).catch(() => {})}
                  className="text-muted-foreground hover:text-foreground"
                >
                  <FolderOpen className="size-3.5" />
                </button>
              </div>
            </div>
          )
        })}
      </div>
    </div>
  )
}

/** 插件管理子界面:官方插件管理增强版(卡片化 + 启停配置 + dsh-plugins 联动)。 */
export function PluginsPage() {
  const { toast } = useToast()
  const [snap, setSnap] = React.useState<PluginsSnapshot | null>(null)
  const [settings, setSettings] = React.useState<SettingsSnapshot | null>(null)
  const [profile, setProfile] = React.useState('')
  const [group, setGroup] = React.useState<GroupKey>('all')
  const [query, setQuery] = React.useState('')
  const [loading, setLoading] = React.useState(false)
  const [error, setError] = React.useState<string | null>(null)
  const lastOp = React.useRef<{ id: number; kind: OperationKind; status: string } | null>(null)

  const load = React.useCallback(
    async (p?: string) => {
      setLoading(true)
      try {
        const target = p ?? profile
        const s = await api.pluginsGetSnapshot(target || undefined)
        setSnap(s)
        setProfile(s.profile ?? target)
        setError(s.dumpError)
        if (!s.profile && target) {
          setError(`profile「${target}」不存在,请选择其它 profile`)
        }
      } catch (e) {
        setError(String(e))
      } finally {
        setLoading(false)
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [profile],
  )

  // 初始:读设置拿到默认 profile + dshPluginsPath
  React.useEffect(() => {
    let mounted = true
    void api.getSettings().then((s) => {
      if (!mounted) return
      setSettings(s)
      setProfile(s.profileName)
      void load(s.profileName)
    })
    return () => {
      mounted = false
    }
  }, [])

  // 安装/移除长任务完成后自动刷新
  React.useEffect(() => {
    const unsub = api.onStateChanged((s: AppSnapshot) => {
      const op = s.operation
      const prev = lastOp.current
      if (op && prev && prev.id === op.operationId && prev.status !== op.status && op.status === 'success') {
        if (op.kind === 'plugin_install' || op.kind === 'plugin_remove') {
          void load()
          toast({ kind: 'success', title: op.kind === 'plugin_install' ? '插件安装完成' : '插件移除完成' })
          if (op.kind === 'plugin_install' && s.state === 'running' && window.confirm('插件已安装。当前运行中的 dsh 会先尝试热重载，是否现在完整重启 dsh 以确保新插件生效？')) {
            void api.runAction('rebuild').then((r) => {
              if (!r.ok) toast({ kind: 'warning', title: '重启未受理', detail: r.reason })
            }).catch((e) => toast({ kind: 'warning', title: '重启失败', detail: String(e) }))
          }
        }
      }
      lastOp.current = op
        ? { id: op.operationId, kind: op.kind, status: op.status }
        : null
    })
    return () => {
      void unsub.then((fn) => fn())
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [profile])

  const rows = React.useMemo(() => {
    if (!snap) return []
    return snap.rows.filter((r) => {
      if (!inGroup(r, group)) return false
      if (query && !r.id.toLowerCase().includes(query.toLowerCase()) && !r.module.toLowerCase().includes(query.toLowerCase())) {
        return false
      }
      return true
    })
  }, [snap, group, query])

  const onMutated = React.useCallback(() => void load(), [load])
  const onReset = React.useCallback(
    (id: string) => {
      void api.pluginsResetRow(profile, id).then((r) => {
        if (r.ok && r.validated) {
          toast({ kind: 'success', title: '已重置', detail: `${r.summary} · 运行中的 dsh web 已热重载` })
          void load()
        } else {
          toast({ kind: 'error', title: '重置失败', detail: r.error ?? r.summary })
        }
      })
    },
    [profile, load, toast],
  )

  const pluginsPath = settings?.dshPluginsPath ?? ''
  const effectivePluginsPath = snap?.pluginsPath ?? pluginsPath

  const installAll = React.useCallback(() => {
    if (!profile) {
      toast({ kind: 'warning', title: '请先选择 profile' })
      return
    }
    void api.pluginsInstallAll(profile).then((r) => {
      if (r.ok) {
        toast({ kind: 'info', title: '仓库安装已受理', detail: '将同步 dsh-plugins 并依次安装全部插件。' })
      } else {
        toast({ kind: 'error', title: '仓库安装失败', detail: r.reason })
      }
    }).catch((e) => toast({ kind: 'error', title: '仓库安装失败', detail: String(e) }))
  }, [profile, toast])

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* 工具栏 */}
      <div className="flex shrink-0 items-center gap-2 border-b border-border bg-card/70 px-5 py-2.5">
        <Puzzle className="size-4 text-blue-500" />
        <span className="text-sm font-semibold">插件</span>
        <Select
          className="ml-2 h-8 w-36 text-xs"
          aria-label="profile"
          options={[
            { value: '', label: '选择 profile…' },
            ...(snap?.profiles ?? []).map((p) => ({ value: p.name, label: p.name })),
          ]}
          value={profile}
          onChange={(v) => {
            setProfile(v)
            void load(v)
          }}
        />
        <div className="relative flex-1 max-w-xs">
          <Search className="pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            className="h-8 pl-8 text-xs"
            placeholder="搜索插件 id / 模块…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
        <Button variant="outline" size="sm" onClick={() => void load()} disabled={loading}>
          <RefreshCw className={loading ? 'animate-spin' : ''} /> 刷新
        </Button>
      </div>

      {/* 分组标签 */}
      <div className="flex shrink-0 items-center gap-1 border-b border-border px-5 py-2">
        {GROUPS.map((g) => {
          const count =
            g.key === 'all'
              ? (snap?.rows.length ?? 0)
              : snap?.rows.filter((r) => inGroup(r, g.key)).length ?? 0
          return (
            <button
              key={g.key}
              onClick={() => setGroup(g.key)}
              className={cn(
                'rounded-md px-2.5 py-1 text-xs font-medium transition-all',
                group === g.key
                  ? 'bg-blue-500 text-white dark:bg-blue-600'
                  : 'text-muted-foreground hover:bg-muted hover:text-foreground',
              )}
            >
              {g.label}
              <span className="ml-1 opacity-70">{count}</span>
            </button>
          )
        })}
        <span className="ml-auto text-[11px] text-muted-foreground">
          {snap?.profiles.length ?? 0} 个 profile · 生效配置来自 <code className="font-mono">--dump-config</code>
        </span>
      </div>

      {/* 主体:列表 + dsh-plugins 面板 */}
      <div className="flex min-h-0 flex-1 gap-4 overflow-hidden p-4">
        <div className="min-h-0 flex-1 space-y-2 overflow-y-auto pr-1">
          {error && (
            <div className="flex items-center gap-2 rounded-lg border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-xs text-amber-600 dark:text-amber-400">
              <ShieldAlert className="size-4 shrink-0" />
              <span className="min-w-0 flex-1 break-all">{error}</span>
              <Button variant="ghost" size="sm" onClick={() => setError(null)}>关闭</Button>
            </div>
          )}
          {!snap && !error && (
            <div className="py-10 text-center text-sm text-muted-foreground">
              {loading ? '加载组合视图中…' : '暂无数据'}
            </div>
          )}
          {rows.length === 0 && snap && (
            <div className="py-10 text-center text-sm text-muted-foreground">
              没有符合筛选条件的插件行
            </div>
          )}
          {rows.map((row) => (
            <PluginCard
              key={`${profile}:${row.id}`}
              row={row}
              profile={profile}
              onMutated={onMutated}
              onReset={onReset}
            />
          ))}
        </div>
        {snap && (
          <PackagesPanel
            snap={snap}
            profile={profile}
            pluginsPath={effectivePluginsPath}
            onInstallAll={installAll}
            onInstalled={onMutated}
            onRemoved={onMutated}
          />
        )}
      </div>

      {/* 底部说明 */}
      <div className="flex shrink-0 items-center gap-3 border-t border-border px-5 py-2 text-[11px] text-muted-foreground">
        <TerminalSquare className="size-3.5" />
        <span>
          补丁按 id 整行替换 config(非深合并);保存后自动备份 + dump-config 校验,运行中的 dsh web 无需重启即热重载。
          含 <code className="font-mono">!!js</code> 的行锁定原始 YAML 模式。
        </span>
      </div>
    </div>
  )
}
