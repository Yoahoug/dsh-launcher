import * as React from 'react'
import {
  Download,
  Eye,
  FileText,
  Pencil,
  Plus,
  RefreshCw,
  Search,
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
import type { SkillRoot, SkillSource, SkillSummary, SkillsSnapshot } from '@/types/schema'

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
  skill,
  onClose,
}: {
  skill: SkillSummary
  onClose: () => void
}) {
  const [content, setContent] = React.useState<string | null>(null)
  const [error, setError] = React.useState<string | null>(null)
  React.useEffect(() => {
    let mounted = true
    void api
      .skillsPreview(skill.path)
      .then((c) => mounted && setContent(c))
      .catch((e) => mounted && setError(String(e)))
    return () => {
      mounted = false
    }
  }, [skill.path])
  return (
    <Modal title={`预览 · ${skill.name}`} icon={<Eye className="size-4 text-blue-500" />} onClose={onClose} width="max-w-[720px]">
      <p className="mb-2 break-all font-mono text-[11px] text-muted-foreground">{skill.path}</p>
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

/** 单张技能卡片。 */
function SkillCard({
  skill,
  onEdit,
  onDelete,
  onImport,
  onPreview,
}: {
  skill: SkillSummary
  onEdit?: () => void
  onDelete?: () => void
  onImport?: () => void
  onPreview: () => void
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
          </div>
          <p className="mt-1 line-clamp-2 text-xs text-muted-foreground" title={skill.description}>
            {skill.description}
          </p>
          {skill.whenToUse && (
            <p className="mt-1 text-[11px] text-amber-600 dark:text-amber-400">何时使用:{skill.whenToUse}</p>
          )}
          <p className="mt-1 truncate font-mono text-[10px] text-muted-foreground/70" title={skill.dir}>
            {skill.dir}
          </p>
        </div>
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

/** 技能管理子界面:独立管理增删改 + 自动扫描本机外部技能。 */
export function SkillsPage() {
  const { toast } = useToast()
  const [snap, setSnap] = React.useState<SkillsSnapshot | null>(null)
  const [query, setQuery] = React.useState('')
  const [loading, setLoading] = React.useState(false)
  const [showCreate, setShowCreate] = React.useState(false)
  const [editing, setEditing] = React.useState<SkillSummary | null>(null)
  const [showImport, setShowImport] = React.useState(false)
  const [previewing, setPreviewing] = React.useState<SkillSummary | null>(null)
  const [enabling, setEnabling] = React.useState<string | null>(null)
  const [profileName, setProfileName] = React.useState('web')

  const load = React.useCallback(async () => {
    setLoading(true)
    try {
      setSnap(await api.skillsGetSnapshot())
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
              onPreview={() => setPreviewing(skill)}
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

  const managedRoot = snap?.roots.find((r) => r.key === 'managed')

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* 工具栏 */}
      <div className="flex shrink-0 items-center gap-2 border-b border-border bg-card/70 px-5 py-2.5">
        <Sparkles className="size-4 text-blue-500" />
        <span className="text-sm font-semibold">技能</span>
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
            <Badge variant="success">skill-external-roots 已安装:外部技能在模型侧可直接调用</Badge>
          ) : (
            <Badge variant="warning">未检测到 skill-external-roots 插件</Badge>
          )}
          <span>扫描根:managed / codex / claude / cursor / opencode / agents / project / 自定义</span>
          {snap.skipped.length > 0 && (
            <span className="text-amber-500" title={snap.skipped.join('\n')}>
              {snap.skipped.length} 个条目被跳过(悬停查看)
            </span>
          )}
          {!managedRoot?.exists && (
            <span className="text-amber-500">managed 根({managedRoot?.path})不存在,新建技能将自动创建</span>
          )}
        </div>
      )}

      {/* 内容 */}
      <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
        <div className="mx-auto flex max-w-[980px] flex-col gap-6">
          {!snap && <p className="py-10 text-center text-sm text-muted-foreground">扫描中…</p>}
          {snap && filtered.length === 0 && (
            <p className="py-10 text-center text-sm text-muted-foreground">没有匹配的技能</p>
          )}
          {SOURCE_ORDER.map((source) => renderSourceSection(source))}
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
      {previewing && <PreviewDialog skill={previewing} onClose={() => setPreviewing(null)} />}
    </div>
  )
}
