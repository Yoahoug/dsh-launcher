import * as React from 'react'
import {
  Download,
  Eye,
  FileText,
  Pencil,
  Plug,
  PlugZap,
  Plus,
  RefreshCw,
  Search,
  ShieldCheck,
  Sparkles,
  Trash2,
  X,
} from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input, Textarea } from '@/components/ui/input'
import { useToast } from '@/components/ui/toast'
import { api } from '@/hooks/use-app'
import { cn } from '@/lib/utils'
import type {
  ActiveSkill,
  AppSnapshot,
  SkillRoot,
  SkillSource,
  SkillSummary,
  SkillsActiveSnapshot,
  SkillsControlState,
  SkillsSnapshot,
} from '@/types/schema'

const SOURCE_LABEL: Record<SkillSource, string> = {
  managed: '已管理',
  codex: 'Codex',
  claude: 'Claude Code',
  cursor: 'Cursor',
  opencode: 'OpenCode',
  agents: 'Agents',
  project: '项目技能',
  custom: '自定义',
}

/** 简单模态壳(与 Clone 弹窗同风格)。 */
function Modal({
  title,
  icon,
  onClose,
  children,
  width = 'max-w-[560px]',
}: {
  title: string
  icon: React.ReactNode
  onClose: () => void
  children: React.ReactNode
  width?: string
}) {
  return (
    <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/40 p-4 backdrop-blur-sm">
      <div className={cn('max-h-[85vh] w-full overflow-hidden overflow-y-auto rounded-xl border border-border bg-card shadow-xl', width)}>
        <div className="flex items-center justify-between border-b border-border px-5 py-4">
          <div className="flex items-center gap-2">
            <span className="flex size-7 items-center justify-center rounded-lg border border-border bg-muted">{icon}</span>
            <h2 className="text-sm font-semibold">{title}</h2>
          </div>
          <button
            onClick={onClose}
            className="rounded-lg p-1 text-muted-foreground hover:bg-muted hover:text-foreground"
            aria-label="关闭"
          >
            <X className="size-4" />
          </button>
        </div>
        <div className="p-5">{children}</div>
      </div>
    </div>
  )
}

/** 新建/编辑技能表单。 */
function SkillFormDialog({
  mode,
  initial,
  onClose,
  onDone,
}: {
  mode: 'create' | 'edit'
  initial?: SkillSummary
  onClose: () => void
  onDone: () => void
}) {
  const { toast } = useToast()
  const [name, setName] = React.useState(initial?.name ?? '')
  const [description, setDescription] = React.useState(initial?.description ?? '')
  const [whenToUse, setWhenToUse] = React.useState(initial?.whenToUse ?? '')
  const [body, setBody] = React.useState('')

  // 编辑模式:预填既有正文(空则保留原正文,更新不覆盖)
  React.useEffect(() => {
    if (mode !== 'edit' || !initial) return
    let mounted = true
    void api
      .skillsPreview(initial.path)
      .then((c) => mounted && setBody(c))
      .catch(() => {})
    return () => {
      mounted = false
    }
  }, [mode, initial])
  const [saving, setSaving] = React.useState(false)

  const save = async () => {
    if (!/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(name)) {
      toast({ kind: 'error', title: '技能名不合法', detail: '必须为 kebab-case:小写字母/数字/中划线' })
      return
    }
    if (!description.trim()) {
      toast({ kind: 'error', title: '描述不能为空' })
      return
    }
    setSaving(true)
    try {
      if (mode === 'create') {
        await api.skillsCreate(name, description.trim(), whenToUse.trim() || null, body)
      } else {
        await api.skillsUpdate(name, description.trim(), whenToUse.trim() || null, body)
      }
      toast({ kind: 'success', title: mode === 'create' ? '技能已创建' : '技能已更新' })
      onDone()
      onClose()
    } catch (e) {
      toast({ kind: 'error', title: mode === 'create' ? '创建失败' : '更新失败', detail: String(e) })
    } finally {
      setSaving(false)
    }
  }

  return (
    <Modal
      title={mode === 'create' ? '新建技能' : `编辑技能 · ${initial?.name}`}
      icon={<Sparkles className="size-4 text-blue-500" />}
      onClose={onClose}
    >
      <div className="space-y-3">
        <div>
          <label htmlFor="skill-name" className="mb-1 block text-xs font-medium text-muted-foreground">
            技能名(kebab-case,如 my-skill)
          </label>
          <Input
            id="skill-name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            disabled={mode === 'edit'}
            spellCheck={false}
            placeholder="my-skill"
          />
        </div>
        <div>
          <label htmlFor="skill-description" className="mb-1 block text-xs font-medium text-muted-foreground">描述(frontmatter description)</label>
          <Textarea id="skill-description" value={description} onChange={(e) => setDescription(e.target.value)} rows={3} />
        </div>
        <div>
          <label htmlFor="skill-when" className="mb-1 block text-xs font-medium text-muted-foreground">whenToUse(可选)</label>
          <Input id="skill-when" value={whenToUse} onChange={(e) => setWhenToUse(e.target.value)} spellCheck={false} placeholder="何时使用该技能" />
        </div>
        <div>
          <label htmlFor="skill-body" className="mb-1 block text-xs font-medium text-muted-foreground">正文(可选;保存时保留既有正文)</label>
          <Textarea
            id="skill-body"
            value={body}
            onChange={(e) => setBody(e.target.value)}
            rows={8}
            className="font-mono text-xs"
            placeholder="技能正文…"
          />
        </div>
        <div className="flex justify-end gap-2 pt-1">
          <Button variant="outline" size="sm" onClick={onClose}>取消</Button>
          <Button size="sm" onClick={() => void save()} disabled={saving}>
            {saving ? '保存中…' : '保存'}
          </Button>
        </div>
      </div>
    </Modal>
  )
}

/** 从外部导入对话框。 */
function ImportDialog({
  onClose,
  onDone,
}: {
  onClose: () => void
  onDone: () => void
}) {
  const { toast } = useToast()
  const [sourcePath, setSourcePath] = React.useState('')
  const [name, setName] = React.useState('')
  const [preview, setPreview] = React.useState<string | null>(null)
  const [busy, setBusy] = React.useState(false)

  const doPreview = async () => {
    if (!sourcePath.trim()) return
    setBusy(true)
    try {
      setPreview(await api.skillsPreview(sourcePath.trim()))
    } catch (e) {
      toast({ kind: 'error', title: '预览失败', detail: String(e) })
    } finally {
      setBusy(false)
    }
  }

  const doImport = async () => {
    setBusy(true)
    try {
      await api.skillsImport(sourcePath.trim(), name.trim() || null)
      toast({ kind: 'success', title: '导入成功', detail: `已拷贝到 $DSH_HOME/skills${name.trim() ? `/${name.trim()}` : ''}` })
      onDone()
      onClose()
    } catch (e) {
      toast({ kind: 'error', title: '导入失败', detail: String(e) })
    } finally {
      setBusy(false)
    }
  }

  return (
    <Modal title="从外部导入技能" icon={<Download className="size-4 text-blue-500" />} onClose={onClose}>
      <div className="space-y-3">
        <div>
          <label className="mb-1 block text-xs font-medium text-muted-foreground">
            源路径(技能目录 &lt;name&gt;/SKILL.md 或 &lt;name&gt;.md)
          </label>
          <div className="flex gap-1.5">
            <Input
              value={sourcePath}
              onChange={(e) => setSourcePath(e.target.value)}
              spellCheck={false}
              placeholder="~/.codex/skills/my-skill"
            />
            <Button variant="outline" size="sm" onClick={() => void doPreview()} disabled={busy || !sourcePath.trim()}>
              预览
            </Button>
          </div>
        </div>
        <div>
          <label className="mb-1 block text-xs font-medium text-muted-foreground">目标名(可选,默认取源 frontmatter name)</label>
          <Input value={name} onChange={(e) => setName(e.target.value)} spellCheck={false} placeholder="kebab-case" />
        </div>
        {preview !== null && (
          <pre className="max-h-48 overflow-y-auto whitespace-pre-wrap rounded-lg border border-border bg-muted/50 p-3 font-mono text-[11px] text-muted-foreground">
            {preview}
          </pre>
        )}
        <div className="flex justify-end gap-2 pt-1">
          <Button variant="outline" size="sm" onClick={onClose}>取消</Button>
          <Button size="sm" onClick={() => void doImport()} disabled={busy || !sourcePath.trim()}>
            导入到 dsh
          </Button>
        </div>
      </div>
    </Modal>
  )
}

/** 预览对话框。 */
function PreviewDialog({
  name,
  path,
  onClose,
}: {
  name: string
  path: string
  onClose: () => void
}) {
  const [content, setContent] = React.useState<string | null>(null)
  const [error, setError] = React.useState<string | null>(null)
  React.useEffect(() => {
    let mounted = true
    void api
      .skillsPreview(path)
      .then((c) => mounted && setContent(c))
      .catch((e) => mounted && setError(String(e)))
    return () => {
      mounted = false
    }
  }, [path])
  return (
    <Modal title={`预览 · ${name}`} icon={<Eye className="size-4 text-blue-500" />} onClose={onClose} width="max-w-[720px]">
      <p className="mb-2 break-all font-mono text-[11px] text-muted-foreground">{path}</p>
      {error && <p className="text-xs text-red-500">{error}</p>}
      {content === null && !error && <p className="text-xs text-muted-foreground">加载中…</p>}
      {content !== null && (
        <pre className="max-h-[55vh] overflow-y-auto whitespace-pre-wrap rounded-lg border border-border bg-muted/50 p-3 font-mono text-xs text-foreground">
          {content}
        </pre>
      )}
      <div className="mt-3 flex justify-end">
        <Button variant="outline" size="sm" onClick={onClose}>关闭</Button>
      </div>
    </Modal>
  )
}

const SOURCE_ORDER: SkillSource[] = ['managed', 'codex', 'claude', 'cursor', 'opencode', 'agents', 'custom', 'project']
const ROOT_CONTROL_KEYS = new Set(['codex', 'claude', 'cursor', 'opencode'])

type PreviewTarget = { name: string; path: string }
type RuntimeSnapshot = Pick<AppSnapshot, 'state' | 'mode' | 'hmrActive'>

function normalizePath(path: string) {
  return path.replaceAll('\\', '/').replace(/\/+$/, '')
}

/** 返回扫描到某技能时命中的根目录(同名技能仍按名称控制,这里只做来源说明)。 */
function discoveredRoot(skill: SkillSummary, roots: SkillRoot[]) {
  const skillDir = normalizePath(skill.dir)
  return roots
    .filter((root) => {
      const rootPath = normalizePath(root.path)
      return skillDir === rootPath || skillDir.startsWith(`${rootPath}/`)
    })
    .sort((a, b) => normalizePath(b.path).length - normalizePath(a.path).length)[0]?.path ?? null
}

/** 单张技能卡片。 */
function SkillCard({
  skill,
  injected,
  injectChecked,
  onToggleInject,
  onEdit,
  onDelete,
  onImport,
  onPreview,
  discoveredRootPath,
  duplicateRoots,
}: {
  skill: SkillSummary
  /** 是否已注入运行中 dsh(与已启动清单去重标记)。 */
  injected?: boolean
  /** 注入开关当前值(未传则不显示开关)。 */
  injectChecked?: boolean
  onToggleInject?: (on: boolean) => void
  onEdit?: () => void
  onDelete?: () => void
  onImport?: () => void
  onPreview: () => void
  discoveredRootPath?: string | null
  duplicateRoots?: string[]
}) {
  return (
    <div className="rounded-xl border border-border bg-card p-3.5 transition-all duration-300 hover:border-border-hover">
      <div className="flex items-start gap-2">
        <FileText className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-1.5">
            <span className="font-mono text-sm font-semibold text-foreground">{skill.name}</span>
            <Badge variant={skill.modelInvocable ? 'success' : 'neutral'}>
              模型{skill.modelInvocable ? '可调用' : '禁用'}
            </Badge>
            <Badge variant={skill.userInvocable ? 'info' : 'neutral'}>
              用户{skill.userInvocable ? '可调用' : '禁用'}
            </Badge>
            {skill.hasScripts && <Badge variant="primary">含资源</Badge>}
            {injected !== undefined && (
              <Badge variant={injected ? 'success' : 'warning'}>
                {injected ? '已注入 ✓' : '未注入'}
              </Badge>
            )}
            {duplicateRoots && duplicateRoots.length > 1 && (
              <Badge variant="warning">同名 ×{duplicateRoots.length}</Badge>
            )}
          </div>
          <p className="mt-1 line-clamp-2 text-xs text-muted-foreground" title={skill.description}>
            {skill.description}
          </p>
          {skill.whenToUse && (
            <p className="mt-1 text-[11px] text-amber-600 dark:text-amber-400">何时使用:{skill.whenToUse}</p>
          )}
          {discoveredRootPath && (
            <p className="mt-1 truncate font-mono text-[10px] text-muted-foreground/70" title={discoveredRootPath}>
              发现根目录:{discoveredRootPath}
            </p>
          )}
          <p className="mt-0.5 truncate font-mono text-[10px] text-muted-foreground/50" title={skill.dir}>
            技能路径:{skill.dir}
          </p>
          {duplicateRoots && duplicateRoots.length > 1 && (
            <p className="mt-0.5 line-clamp-2 text-[10px] text-amber-600 dark:text-amber-400" title={duplicateRoots.join('\n')}>
              同名来源:{duplicateRoots.join('、')}
            </p>
          )}
        </div>
        {onToggleInject && (
          <InjectionSwitch
            checked={injectChecked ?? true}
            label={`注入 ${skill.name}`}
            onToggle={onToggleInject}
          />
        )}
      </div>
      <div className="mt-2.5 flex items-center gap-1.5">
        <Button variant="ghost" size="sm" onClick={onPreview}><Eye /> 预览</Button>
        {onImport && (
          <Button variant="outline" size="sm" onClick={onImport}><Download /> 导入到 dsh</Button>
        )}
        <span className="ml-auto text-[10px] text-muted-foreground">{Math.max(1, Math.round(skill.sizeBytes / 1024))} KB</span>
        {onEdit && (
          <Button variant="ghost" size="sm" onClick={onEdit} aria-label={`编辑 ${skill.name}`}><Pencil /></Button>
        )}
        {onDelete && (
          <Button variant="ghost" size="sm" onClick={onDelete} aria-label={`删除 ${skill.name}`} className="text-red-500 hover:text-red-600">
            <Trash2 />
          </Button>
        )}
      </div>
    </div>
  )
}

/** 注入开关:关闭 = 该技能默认不再注入 dsh(写控制文件,插件热更新)。 */
function InjectionSwitch({
  checked,
  label,
  onToggle,
  disabled,
}: {
  checked: boolean
  label: string
  onToggle: (on: boolean) => void
  disabled?: boolean
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onToggle(!checked)}
      className={cn(
        'relative h-5 w-9 shrink-0 rounded-full transition-colors duration-200 disabled:opacity-50',
        checked ? 'bg-emerald-500' : 'bg-muted',
      )}
    >
      <span
        className={cn(
          'absolute top-0.5 size-4 rounded-full bg-background shadow transition-all duration-200',
          checked ? 'left-[18px]' : 'left-0.5',
        )}
      />
    </button>
  )
}

/** 已启动子界面的单条注入技能。 */
function ActiveSkillCard({
  skill,
  checked,
  onToggle,
  toggling,
  onPreview,
}: {
  skill: ActiveSkill
  checked: boolean
  onToggle: (on: boolean) => void
  toggling: boolean
  onPreview: () => void
}) {
  return (
    <div className="rounded-xl border border-border bg-card p-3.5 transition-all duration-300 hover:border-border-hover">
      <div className="flex items-start gap-2">
        <PlugZap className="mt-0.5 size-4 shrink-0 text-emerald-500" />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-1.5">
            <span className="font-mono text-sm font-semibold text-foreground">{skill.name}</span>
            <Badge variant="success">已启动</Badge>
            <Badge variant="neutral">模型{skill.modelInvocable ? '可调用' : '禁用'}</Badge>
            <Badge variant="neutral">用户{skill.userInvocable ? '可调用' : '禁用'}</Badge>
          </div>
          <p className="mt-1 line-clamp-2 text-xs text-muted-foreground" title={skill.description}>
            {skill.description}
          </p>
          {skill.whenToUse && (
            <p className="mt-1 text-[11px] text-amber-600 dark:text-amber-400">何时使用:{skill.whenToUse}</p>
          )}
          <p className="mt-1 truncate font-mono text-[10px] text-muted-foreground/70" title={skill.root}>
            根:{skill.root}
          </p>
          <p className="mt-0.5 truncate font-mono text-[10px] text-muted-foreground/50" title={skill.path}>
            {skill.path}
          </p>
        </div>
        <InjectionSwitch
          checked={checked}
          label={`${checked ? '关闭' : '开启'}注入 ${skill.name}`}
          onToggle={(on) => onToggle(on)}
          disabled={toggling}
        />
      </div>
      <div className="mt-2.5 flex items-center gap-1.5">
        <Button variant="ghost" size="sm" onClick={onPreview}><Eye /> 预览</Button>
      </div>
    </div>
  )
}

/** 已启动页的系统自带技能:由 dsh 内置 skill-filesystem 管理,不经过外部注入开关。 */
function BuiltinSkillCard({ skill }: { skill: SkillSummary }) {
  return (
    <div className="rounded-xl border border-border bg-card p-3.5 transition-all duration-300 hover:border-border-hover">
      <div className="flex items-start gap-2">
        <FileText className="mt-0.5 size-4 shrink-0 text-blue-500" />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-1.5">
            <span className="font-mono text-sm font-semibold text-foreground">{skill.name}</span>
            <Badge variant="primary">系统自带</Badge>
            <Badge variant="neutral">模型{skill.modelInvocable ? '可调用' : '禁用'}</Badge>
            <Badge variant="neutral">用户{skill.userInvocable ? '可调用' : '禁用'}</Badge>
          </div>
          <p className="mt-1 line-clamp-2 text-xs text-muted-foreground" title={skill.description}>
            {skill.description}
          </p>
          <p className="mt-1 truncate font-mono text-[10px] text-muted-foreground/70" title={skill.path}>
            {skill.path}
          </p>
        </div>
      </div>
    </div>
  )
}

/** 技能管理子界面:独立管理增删改 + 自动扫描本机外部技能。 */
export function SkillsPage({
  onOpenPlugins,
  runtime,
}: {
  onOpenPlugins?: () => void
  runtime?: RuntimeSnapshot | null
}) {
  const { toast } = useToast()
  const [tab, setTab] = React.useState<'active' | 'discover'>('active')
  const [snap, setSnap] = React.useState<SkillsSnapshot | null>(null)
  const [activeSnap, setActiveSnap] = React.useState<SkillsActiveSnapshot | null>(null)
  const [control, setControl] = React.useState<SkillsControlState | null>(null)
  const [query, setQuery] = React.useState('')
  const [loading, setLoading] = React.useState(false)
  const [showCreate, setShowCreate] = React.useState(false)
  const [editing, setEditing] = React.useState<SkillSummary | null>(null)
  const [showImport, setShowImport] = React.useState(false)
  const [previewing, setPreviewing] = React.useState<PreviewTarget | null>(null)
  const [enabling, setEnabling] = React.useState<string | null>(null)
  const [toggling, setToggling] = React.useState<string | null>(null)
  const [profileName, setProfileName] = React.useState('web')

  const load = React.useCallback(async () => {
    setLoading(true)
    try {
      const [s, a, c] = await Promise.all([
        api.skillsGetSnapshot(),
        api.skillsGetActive(),
        api.skillsGetControl(),
      ])
      setSnap(s)
      setActiveSnap(a)
      setControl(c)
    } catch (e) {
      toast({ kind: 'error', title: '技能扫描失败', detail: String(e) })
    } finally {
      setLoading(false)
    }
  }, [toast])

  React.useEffect(() => {
    void load()
    void api.getSettings().then((s) => setProfileName(s.profileName))
  }, [load])

  // 写操作后事件驱动刷新(与 Rust skills_changed 对齐)
  React.useEffect(() => {
    const unsub = api.onSkillsChanged(() => void load())
    return () => {
      void unsub.then((fn) => fn())
    }
  }, [load])

  const filtered = React.useMemo(() => {
    if (!snap) return []
    if (!query) return snap.skills
    const q = query.toLowerCase()
    return snap.skills.filter(
      (s) => s.name.toLowerCase().includes(q) || s.description.toLowerCase().includes(q),
    )
  }, [snap, query])

  /** 已启动清单按技能名判断注入状态;同名来源只做展示标注,不拆成多个控制项。 */
  const injectedSet = React.useMemo(
    () => new Set((activeSnap?.skills ?? []).map((s) => s.name)),
    [activeSnap],
  )

  const builtinSkills = React.useMemo(
    () => (snap?.skills ?? []).filter((s) => s.source === 'managed' || s.source === 'project'),
    [snap],
  )
  const activeCount = builtinSkills.length + (activeSnap?.skills.length ?? 0)

  const grouped = React.useMemo(() => {
    const map = new Map<SkillSource, SkillSummary[]>()
    for (const s of filtered) {
      const arr = map.get(s.source) ?? []
      arr.push(s)
      map.set(s.source, arr)
    }
    return map
  }, [filtered])

  const rootsByKey = React.useMemo(() => {
    const map = new Map<string, SkillRoot[]>()
    for (const r of snap?.roots ?? []) {
      const arr = map.get(r.key) ?? []
      arr.push(r)
      map.set(r.key, arr)
    }
    return map
  }, [snap])

  const duplicateRootsByName = React.useMemo(() => {
    const map = new Map<string, Set<string>>()
    for (const skill of snap?.skills ?? []) {
      if (skill.source === 'managed' || skill.source === 'project') continue
      const root = discoveredRoot(skill, snap?.roots ?? []) ?? skill.dir
      const roots = map.get(skill.name) ?? new Set<string>()
      roots.add(root)
      map.set(skill.name, roots)
    }
    return new Map([...map].map(([name, roots]) => [name, [...roots]]))
  }, [snap])

  const normalRunning = runtime?.state === 'running' && runtime.mode === 'normal'
  const devHotReload = runtime?.state === 'running' && runtime.mode === 'dev' && runtime.hmrActive

  const reloadHint = normalRunning
    ? '当前普通模式不支持热重载,请重启 dsh 后生效'
    : devHotReload
      ? '开发模式 HMR 将在约 1-2 秒内更新'
      : runtime?.state === 'running'
        ? '当前运行模式未确认热重载,如未生效请重启 dsh'
        : 'dsh 未运行,下次启动时生效'

  const del = async (skill: SkillSummary) => {
    if (!window.confirm(`确定删除技能「${skill.name}」?(${skill.dir})`)) return
    try {
      await api.skillsDelete(skill.name)
      toast({ kind: 'success', title: '已删除', detail: `${skill.name}(managed 根)` })
    } catch (e) {
      toast({ kind: 'error', title: '删除失败', detail: String(e) })
    }
  }

  const enableRoot = async (root: SkillRoot) => {
    setEnabling(root.path)
    try {
      const r = await api.skillsEnableRoot(profileName, root.path)
      if (r.ok && r.validated) {
        toast({ kind: 'success', title: '已启用', detail: `${root.path} → skill-filesystem.customSkillDirs(已热重载)` })
        void load()
      } else {
        toast({ kind: 'error', title: '启用失败', detail: r.error ?? r.summary })
      }
    } catch (e) {
      toast({ kind: 'error', title: '启用失败', detail: String(e) })
    } finally {
      setEnabling(null)
    }
  }

  /** 注入开关:关闭 = 该技能默认不再注入 dsh(写控制文件,插件约 1-2 秒热更新)。 */
  const toggleSkill = async (name: string, on: boolean) => {
    setToggling(name)
    try {
      const r = await api.skillsSetInjected(name, on)
      if (r.ok) {
        setControl((c) => (c ? { ...c, skills: { ...c.skills, [name]: on } } : c))
        toast({ kind: 'success', title: on ? '已开启注入' : '已关闭注入', detail: `${r.summary};${reloadHint}` })
        // 插件 1.5s 轮询 + 重新收集 → active 清单约 3s 后更新
        window.setTimeout(() => void load(), 3200)
      } else {
        toast({ kind: 'error', title: '开关失败', detail: r.summary })
      }
    } catch (e) {
      toast({ kind: 'error', title: '开关失败', detail: String(e) })
    } finally {
      setToggling(null)
    }
  }

  /** 按外部工具族根目录关闭全部已启动技能(Cursor 等受支持的根)。 */
  const toggleRoot = async (rootKey: string, skills: ActiveSkill[]) => {
    const toggleId = `root:${rootKey}`
    setToggling(toggleId)
    try {
      const r = ROOT_CONTROL_KEYS.has(rootKey)
        ? await api.skillsSetRootInjected(rootKey, false)
        : (await Promise.all(skills.map((skill) => api.skillsSetInjected(skill.name, false)))).every((result) => result.ok)
          ? { ok: true, summary: `${rootKey} 根目录下技能已关闭注入`, enabled: false }
          : { ok: false, summary: `${rootKey} 根目录关闭失败`, enabled: false }
      if (r.ok) {
        setControl((c) => (c ? { ...c, roots: { ...c.roots, [rootKey]: false } } : c))
        toast({ kind: 'success', title: '已关闭根目录注入', detail: `${r.summary};${reloadHint}` })
        window.setTimeout(() => void load(), 3200)
      } else {
        toast({ kind: 'error', title: '关闭根目录失败', detail: r.summary })
      }
    } catch (e) {
      toast({ kind: 'error', title: '关闭根目录失败', detail: String(e) })
    } finally {
      setToggling(null)
    }
  }

  /** 一键启用注入控制:把 skillControlFile/activeFile 写进 skill-external-roots 行。 */
  const enableControl = async () => {
    try {
      const r = await api.skillsEnableControl(profileName)
      if (r.ok && r.validated) {
        toast({ kind: 'success', title: '已启用注入控制', detail: 'skill-external-roots 已写入 skillControlFile/activeFile(已热重载)' })
        void load()
      } else {
        toast({ kind: 'error', title: '启用失败', detail: r.error ?? r.summary })
      }
    } catch (e) {
      toast({ kind: 'error', title: '启用失败', detail: String(e) })
    }
  }

  const toggleFor = (name: string) => (on: boolean) => void toggleSkill(name, on)

  const renderSourceSection = (source: SkillSource) => {
    const skills = grouped.get(source) ?? []
    if (skills.length === 0 && source !== 'managed') return null
    const roots = rootsByKey.get(source) ?? []
    return (
      <section key={source} className="space-y-2">
        <div className="flex flex-wrap items-center gap-2 px-1">
          <h3 className="text-sm font-semibold">{SOURCE_LABEL[source]}</h3>
          <Badge variant="neutral">{skills.length}</Badge>
          {source === 'managed' && (
            <div className="ml-auto flex items-center gap-2">
              <Button size="sm" onClick={() => setShowCreate(true)}><Plus /> 新建技能</Button>
              <Button variant="outline" size="sm" onClick={() => setShowImport(true)}><Download /> 从外部导入</Button>
            </div>
          )}
          {source !== 'managed' && source !== 'project' && (
            <div className="ml-auto flex flex-wrap items-center gap-2">
              {roots.map((root) => (
                <div key={root.path} className="flex items-center gap-1.5">
                  <span className="max-w-52 truncate font-mono text-[10px] text-muted-foreground" title={root.path}>
                    {root.path}
                  </span>
                  {root.enabled ? (
                    <Badge variant="success">已启用 ✓</Badge>
                  ) : (
                    <Button
                      variant="outline"
                      size="sm"
                      className="h-6 px-2 text-[11px]"
                      onClick={() => void enableRoot(root)}
                      disabled={enabling === root.path}
                    >
                      一键启用
                    </Button>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
        <div className="grid grid-cols-1 gap-2 xl:grid-cols-2">
          {skills.map((skill) => (
            <SkillCard
              key={`${skill.source}:${skill.name}:${skill.path}`}
              skill={skill}
              injected={source !== 'managed' && source !== 'project' ? injectedSet.has(skill.name) : undefined}
              injectChecked={
                source !== 'managed' && source !== 'project'
                  ? (control?.skills[skill.name] ?? injectedSet.has(skill.name))
                  : undefined
              }
              onToggleInject={
                snap?.pluginsInstalled && source !== 'managed' && source !== 'project'
                  ? toggleFor(skill.name)
                  : undefined
              }
              onPreview={() => setPreviewing(skill)}
              discoveredRootPath={source !== 'managed' && source !== 'project' ? discoveredRoot(skill, snap?.roots ?? []) : null}
              duplicateRoots={source !== 'managed' && source !== 'project' ? duplicateRootsByName.get(skill.name) : undefined}
              onEdit={source === 'managed' ? () => setEditing(skill) : undefined}
              onDelete={source === 'managed' ? () => void del(skill) : undefined}
              onImport={source !== 'managed' && source !== 'project' ? () => void importSkill(skill) : undefined}
            />
          ))}
        </div>
      </section>
    )
  }

  const importSkill = async (skill: SkillSummary) => {
    try {
      await api.skillsImport(skill.dir, skill.name)
      toast({ kind: 'success', title: '导入成功', detail: `${skill.name} → $DSH_HOME/skills/${skill.name}` })
    } catch (e) {
      toast({ kind: 'error', title: '导入失败', detail: String(e) })
    }
  }

  const controlEnabled = activeSnap?.controlFile != null

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* 工具栏 */}
      <div className="flex shrink-0 items-center gap-2 border-b border-border bg-card/70 px-5 py-2.5">
        <Sparkles className="size-4 text-blue-500" />
        <span className="text-sm font-semibold">技能</span>

        {/* 两个子界面:已启动 / 外部发现 */}
        <div className="ml-1 flex items-center rounded-lg border border-border bg-muted/40 p-0.5">
          <button
            type="button"
            onClick={() => setTab('active')}
            className={cn(
              'flex items-center gap-1.5 rounded-md px-3 py-1 text-xs font-medium transition-colors',
              tab === 'active' ? 'bg-card text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground',
            )}
          >
            <PlugZap className="size-3.5 text-emerald-500" />
            已启动
            <span className="rounded-full bg-emerald-500/10 px-1.5 text-[10px] text-emerald-600 dark:text-emerald-400">
              {activeCount}
            </span>
          </button>
          <button
            type="button"
            onClick={() => setTab('discover')}
            className={cn(
              'flex items-center gap-1.5 rounded-md px-3 py-1 text-xs font-medium transition-colors',
              tab === 'discover' ? 'bg-card text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground',
            )}
          >
            <Search className="size-3.5" />
            外部发现
          </button>
        </div>

        <div className="relative ml-2 max-w-xs flex-1">
          <Search className="pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            className="h-8 pl-8 text-xs"
            placeholder="搜索技能名称 / 描述…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
        <Button variant="outline" size="sm" onClick={() => void load()} disabled={loading}>
          <RefreshCw className={loading ? 'animate-spin' : ''} /> 重新扫描
        </Button>
      </div>

      {/* 提示条 */}
      {snap && (
        <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-border px-5 py-2 text-[11px] text-muted-foreground">
          {snap.pluginsInstalled ? (
            <Badge variant="success">skill-external-roots 已安装</Badge>
          ) : (
            <>
              <Badge variant="warning">未检测到 skill-external-roots 插件</Badge>
              {onOpenPlugins && (
                <Button variant="outline" size="sm" className="h-6 px-2 text-[11px]" onClick={onOpenPlugins}>
                  前往插件安装
                </Button>
              )}
            </>
          )}
          {normalRunning && (
            <Badge variant="warning">当前普通模式:技能开关不会热重载,需要重启 dsh</Badge>
          )}
          {devHotReload && (
            <Badge variant="success">当前开发模式 · HMR:技能开关可热重载</Badge>
          )}
          {tab === 'active' ? (
            <>
              <Badge variant={controlEnabled ? 'success' : 'warning'}>
                {controlEnabled ? '注入控制已启用' : '注入控制未启用'}
              </Badge>
              <span>{normalRunning ? '普通模式不会热重载,请重启 dsh 后生效' : reloadHint}</span>
            </>
          ) : (
            <>
              <span>「已注入 ✓」= 运行中 dsh 实际加载(与已启动清单去重)</span>
              <span>关闭开关 = 默认不注入 dsh</span>
              {snap.skipped.length > 0 && (
                <span className="text-amber-500" title={snap.skipped.join('\n')}>
                  {snap.skipped.length} 个条目被跳过(悬停查看)
                </span>
              )}
            </>
          )}
        </div>
      )}

      {/* 内容 */}
      <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
        <div className="mx-auto flex max-w-[980px] flex-col gap-6">
          {tab === 'active' ? (
            <ActiveSkillsView
              activeSnap={activeSnap}
              builtinSkills={builtinSkills}
              roots={snap?.roots ?? []}
              control={control}
              controlEnabled={controlEnabled}
              pluginsInstalled={snap?.pluginsInstalled ?? false}
              runtime={runtime}
              toggling={toggling}
              onToggle={toggleSkill}
              onToggleRoot={toggleRoot}
              onPreview={(skill) => setPreviewing({ name: skill.name, path: skill.path })}
              onEnableControl={enableControl}
              onRefresh={() => void load()}
              loading={loading}
            />
          ) : (
            <>
              {!snap && <p className="py-10 text-center text-sm text-muted-foreground">扫描中…</p>}
              {snap && filtered.length === 0 && (
                <p className="py-10 text-center text-sm text-muted-foreground">没有匹配的技能</p>
              )}
              {SOURCE_ORDER.map((source) => renderSourceSection(source))}
            </>
          )}
        </div>
      </div>

      {/* 对话框 */}
      {showCreate && (
        <SkillFormDialog
          mode="create"
          onClose={() => setShowCreate(false)}
          onDone={() => void load()}
        />
      )}
      {editing && (
        <SkillFormDialog
          mode="edit"
          initial={editing}
          onClose={() => setEditing(null)}
          onDone={() => void load()}
        />
      )}
      {showImport && (
        <ImportDialog onClose={() => setShowImport(false)} onDone={() => void load()} />
      )}
      {previewing && <PreviewDialog name={previewing.name} path={previewing.path} onClose={() => setPreviewing(null)} />}
    </div>
  )
}

/** 「已启动」子界面:运行中 dsh 实际注入的技能清单(插件回写)。 */
function ActiveSkillsView({
  activeSnap,
  builtinSkills,
  roots,
  control,
  controlEnabled,
  pluginsInstalled,
  runtime,
  toggling,
  onToggle,
  onToggleRoot,
  onPreview,
  onEnableControl,
  onRefresh,
  loading,
}: {
  activeSnap: SkillsActiveSnapshot | null
  builtinSkills: SkillSummary[]
  roots: SkillRoot[]
  control: SkillsControlState | null
  controlEnabled: boolean
  pluginsInstalled: boolean
  runtime?: RuntimeSnapshot | null
  toggling: string | null
  onToggle: (name: string, on: boolean) => void
  onToggleRoot: (rootKey: string, skills: ActiveSkill[]) => void
  onPreview: (skill: ActiveSkill) => void
  onEnableControl: () => void
  onRefresh: () => void
  loading: boolean
}) {
  if (!activeSnap) {
    return <p className="py-10 text-center text-sm text-muted-foreground">加载中…</p>
  }
  const skills = activeSnap.skills
  const externalSkills = skills
  const externalGroups = new Map<string, ActiveSkill[]>()
  for (const skill of externalSkills) {
    const group = externalGroups.get(skill.root) ?? []
    group.push(skill)
    externalGroups.set(skill.root, group)
  }
  const normalRunning = runtime?.state === 'running' && runtime.mode === 'normal'
  const devHotReload = runtime?.state === 'running' && runtime.mode === 'dev' && runtime.hmrActive
  const writtenAt = activeSnap.writtenAt
    ? new Date(activeSnap.writtenAt).toLocaleString()
    : null
  return (
    <div className="flex flex-col gap-3">
      {!controlEnabled && pluginsInstalled && (
        <div className="flex flex-wrap items-center gap-3 rounded-xl border border-amber-500/30 bg-amber-500/5 p-4">
          <Plug className="size-5 shrink-0 text-amber-500" />
          <div className="min-w-0 flex-1 text-xs text-muted-foreground">
            <p className="font-medium text-foreground">尚未启用注入控制</p>
            <p className="mt-0.5">
              skill-external-roots 行未配置 <code className="font-mono">skillControlFile</code> /
              <code className="font-mono">activeFile</code>,因此无法按技能开关、也无法回写已启动清单。
              点击右侧按钮一键写入补丁(整行重述 + 校验 + 热重载)。
            </p>
          </div>
          <Button size="sm" onClick={onEnableControl}>
            <ShieldCheck className="size-4" /> 启用注入控制
          </Button>
        </div>
      )}

      {controlEnabled && (
        <div className="flex flex-wrap items-center gap-2 rounded-xl border border-border bg-card/60 px-4 py-2.5 text-[11px] text-muted-foreground">
          <Badge variant="info">数据源:skill-external-roots 回写</Badge>
          <span className="truncate font-mono" title={activeSnap.file}>{activeSnap.file}</span>
          <span className="ml-auto flex items-center gap-2">
            {writtenAt && <span>更新于 {writtenAt}</span>}
            <Button variant="ghost" size="sm" className="h-6 px-2 text-[11px]" onClick={onRefresh} disabled={loading}>
              <RefreshCw className={loading ? 'animate-spin' : 'size-3'} /> 刷新
            </Button>
          </span>
        </div>
      )}

      {normalRunning && (
        <div className="rounded-xl border border-amber-500/30 bg-amber-500/5 px-4 py-3 text-xs text-amber-700 dark:text-amber-300">
          当前 dsh 为普通模式。技能开关已写入控制文件，但运行中的服务不支持热重载；请重启 dsh 后生效。
        </div>
      )}
      {devHotReload && (
        <div className="rounded-xl border border-emerald-500/30 bg-emerald-500/5 px-4 py-3 text-xs text-emerald-700 dark:text-emerald-300">
          当前 dsh 为开发模式 · HMR，技能开关会在约 1-2 秒内热重载。
        </div>
      )}

      {activeSnap.error && (
        <div className="rounded-xl border border-red-500/30 bg-red-500/5 px-4 py-2.5 text-xs text-red-600 dark:text-red-400">
          清单解析失败:{activeSnap.error}
        </div>
      )}

      {!pluginsInstalled && (
        <div className="rounded-xl border border-amber-500/30 bg-amber-500/5 px-4 py-3 text-xs text-muted-foreground">
          外部发现技能依赖 <code className="font-mono">@dsh-plugins/skill-external-roots</code>；请先到“插件”页安装 dsh-plugins 仓库中的插件。
        </div>
      )}

      <section className="space-y-2">
        <div className="flex items-center gap-2 px-1">
          <h3 className="text-sm font-semibold">系统自带</h3>
          <Badge variant="neutral">{builtinSkills.length}</Badge>
        </div>
        {builtinSkills.length > 0 ? (
          <div className="grid grid-cols-1 gap-2 xl:grid-cols-2">
            {builtinSkills.map((skill) => <BuiltinSkillCard key={`${skill.source}:${skill.name}:${skill.path}`} skill={skill} />)}
          </div>
        ) : (
          <p className="rounded-xl border border-dashed border-border px-4 py-6 text-center text-xs text-muted-foreground">
            暂无系统自带技能
          </p>
        )}
      </section>

      <section className="space-y-2">
        <div className="flex items-center gap-2 px-1">
          <h3 className="text-sm font-semibold">外部发现</h3>
          <Badge variant="neutral">{externalSkills.length}</Badge>
        </div>
      {controlEnabled && externalSkills.length === 0 && !activeSnap.error && (
        <div className="flex flex-col items-center gap-2 py-10 text-center text-sm text-muted-foreground">
          <PlugZap className="size-6 text-muted-foreground/50" />
          <p>暂无已启动技能</p>
          <p className="max-w-md text-xs">
            {activeSnap.controlFileExists
              ? '控制文件存在但 dsh 尚未回写注入清单——请确认 dsh 正在运行,然后点「刷新」;若刚关闭全部技能,这是预期结果。'
              : '控制文件尚未写入——在「外部发现」子界面打开任一技能开关后,运行中的 dsh 会在一两秒内完成注入并回写此清单。'}
          </p>
        </div>
      )}

      {externalSkills.length > 0 && (
        <div className="flex flex-col gap-4">
          {[...externalGroups].map(([rootPath, groupSkills]) => {
            const root = roots.find((item) => normalizePath(item.path) === normalizePath(rootPath))
            const rootKey = root?.key
            const familyLabel = rootKey === 'cursor' ? 'Cursor' : root?.label ?? '此根目录'
            return (
              <section key={rootPath} className="space-y-2">
                <div className="flex flex-wrap items-center gap-2 px-1">
                  <h4 className="text-sm font-semibold">{familyLabel}</h4>
                  <Badge variant="neutral">{groupSkills.length}</Badge>
                  <span className="min-w-0 flex-1 truncate font-mono text-[10px] text-muted-foreground" title={rootPath}>
                    根目录:{rootPath}
                  </span>
                  {rootKey && ROOT_CONTROL_KEYS.has(rootKey) && (
                    <Button
                      variant="outline"
                      size="sm"
                      className="h-7 px-2 text-[11px]"
                      aria-label={`关闭${familyLabel}全部`}
                      onClick={() => onToggleRoot(rootKey, groupSkills)}
                      disabled={toggling === `root:${rootKey}`}
                    >
                      关闭{familyLabel}全部
                    </Button>
                  )}
                </div>
                <div className="grid grid-cols-1 gap-2 xl:grid-cols-2">
                  {groupSkills.map((skill) => (
                    <ActiveSkillCard
                      key={`${skill.name}:${skill.root}:${skill.path}`}
                      skill={skill}
                      checked={control?.skills[skill.name] ?? (rootKey ? control?.roots[rootKey] ?? true : true)}
                      onToggle={(on) => onToggle(skill.name, on)}
                      toggling={toggling === skill.name || toggling === `root:${rootKey}`}
                      onPreview={() => onPreview(skill)}
                    />
                  ))}
                </div>
              </section>
            )
          })}
        </div>
      )}
      </section>

      <p className="px-1 text-[11px] text-muted-foreground/70">
        说明:此清单由运行中 dsh 的 skill-external-roots 插件在每次收集后回写(内容变化才写)。关闭开关
        后插件在约 1-2 秒内热更新,清单会随之下一次刷新消失;「外部发现」子界面的「已注入 ✓」标记同步
        反映。(control 文件:{control?.file ?? '-'})
      </p>
    </div>
  )
}
