import * as React from 'react'
import {
  Archive,
  ChevronDown,
  Folder,
  MoreHorizontal,
  Search,
  SlidersHorizontal,
  Trash2,
} from 'lucide-react'
import { api } from '@/hooks/use-app'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useToast } from '@/components/ui/toast'
import type { ArchiveGroup, ArchiveSession, ArchivesSnapshot } from '@/types/schema'

type ChatFilter = 'all' | 'project' | 'no-project'

function formatTime(value: number | null): string {
  if (!value) return '时间未知'
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return '时间未知'
  const pad = (n: number) => String(n).padStart(2, '0')
  return date.getFullYear() + '年' + (date.getMonth() + 1) + '月' + date.getDate() + '日，' + pad(date.getHours()) + ':' + pad(date.getMinutes())
}

function sessionMatches(session: ArchiveSession, group: ArchiveGroup, query: string): boolean {
  if (!query.trim()) return true
  const needle = query.trim().toLocaleLowerCase()
  return [session.title, session.sessionId, group.title, group.path ?? '']
    .some((value) => value.toLocaleLowerCase().includes(needle))
}

function FilterSelect({
  icon: Icon,
  label,
  value,
  options,
  onChange,
  ariaLabel,
}: {
  icon: typeof Folder
  label: string
  value: string
  options: { value: string; label: string }[]
  onChange: (value: string) => void
  ariaLabel: string
}) {
  return (
    <label className="relative block min-w-0">
      <Icon className="pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" strokeWidth={1.8} />
      <select
        aria-label={ariaLabel}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        className="h-8 w-full cursor-pointer appearance-none rounded-lg border border-border bg-card pl-8 pr-8 text-xs font-medium text-foreground outline-none transition-colors hover:border-border-hover focus:border-blue-500"
      >
        <option value={value}>{label}</option>
        {options.filter((option) => option.value !== value).map((option) => (
          <option key={option.value} value={option.value}>{option.label}</option>
        ))}
      </select>
      <ChevronDown className="pointer-events-none absolute right-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
    </label>
  )
}

function ArchiveRow({
  session,
  deleteAvailable,
  deleting,
  restoreAvailable,
  restoring,
  onDelete,
  onRestore,
}: {
  session: ArchiveSession
  deleteAvailable: boolean
  deleting: boolean
  restoreAvailable: boolean
  restoring: boolean
  onDelete: () => void
  onRestore: () => void
}) {
  return (
    <div className="group flex min-h-16 items-center gap-3 border-b border-border/60 px-4 py-3 last:border-b-0">
      <div className="min-w-0 flex-1">
        <p className="truncate text-sm font-semibold leading-5 text-foreground">{session.title}</p>
        <p className="mt-1 text-xs text-muted-foreground">{formatTime(session.lastActivityAt ?? session.createdAt)}</p>
      </div>
      <button
        type="button"
        disabled={!deleteAvailable || deleting}
        title={deleteAvailable ? '永久删除' : '归档插件不可用，暂时不能删除'}
        aria-label={session.title + ' 永久删除'}
        onClick={onDelete}
        className="rounded-lg p-1.5 text-muted-foreground opacity-0 transition-opacity hover:bg-red-500/10 hover:text-red-600 group-hover:opacity-100 disabled:cursor-not-allowed"
      >
        <Trash2 className="size-4" strokeWidth={1.8} />
      </button>
      <Button
        type="button"
        variant="secondary"
        disabled={!restoreAvailable || restoring}
        onClick={onRestore}
        className="h-8 rounded-lg px-3 text-xs font-medium"
      >
        {restoring ? '恢复中…' : '取消归档'}
      </Button>
    </div>
  )
}

function ArchiveGroupCard({
  group,
  sessions,
  deleteAvailable,
  deletingId,
  restoreAvailable,
  restoringId,
  onDelete,
  onRestore,
}: {
  group: ArchiveGroup
  sessions: ArchiveSession[]
  deleteAvailable: boolean
  deletingId: string | null
  restoreAvailable: boolean
  restoringId: string | null
  onDelete: (sessionId: string) => void
  onRestore: (sessionId: string) => void
}) {
  return (
    <section className="mb-6">
      <div className="mb-2 flex items-center gap-2 px-1">
        <Folder className="size-4 text-muted-foreground" strokeWidth={1.8} />
        <h2 className="text-sm font-semibold text-foreground">{group.title}</h2>
        <span className="ml-auto text-xs text-muted-foreground">{sessions.length} 个聊天</span>
        <button type="button" aria-label={group.title + ' 更多操作'} className="rounded-md p-1 text-muted-foreground hover:bg-muted hover:text-foreground">
          <MoreHorizontal className="size-4" />
        </button>
      </div>
      <div className="overflow-hidden rounded-xl border border-border bg-card">
        {sessions.map((session) => (
          <ArchiveRow
            key={session.sessionId}
            session={session}
            deleteAvailable={deleteAvailable}
            deleting={deletingId === session.sessionId}
            restoreAvailable={restoreAvailable}
            restoring={restoringId === session.sessionId}
            onDelete={() => onDelete(session.sessionId)}
            onRestore={() => onRestore(session.sessionId)}
          />
        ))}
      </div>
    </section>
  )
}

export function ArchivesPage() {
  const { toast } = useToast()
  const [snapshot, setSnapshot] = React.useState<ArchivesSnapshot | null>(null)
  const [query, setQuery] = React.useState('')
  const [chatFilter, setChatFilter] = React.useState<ChatFilter>('all')
  const [projectFilter, setProjectFilter] = React.useState('all')
  const [restoringId, setRestoringId] = React.useState<string | null>(null)
  const [deletingId, setDeletingId] = React.useState<string | null>(null)
  const [deletingAll, setDeletingAll] = React.useState(false)
  const [error, setError] = React.useState<string | null>(null)

  const load = React.useCallback(async () => {
    setError(null)
    try {
      setSnapshot(await api.archivesGetSnapshot())
    } catch (cause) {
      setError(String(cause))
    }
  }, [])

  React.useEffect(() => {
    void load()
  }, [load])

  const projectOptions = React.useMemo(
    () => snapshot?.groups
      .filter((group) => group.workspaceId !== null)
      .map((group) => ({ value: group.workspaceId!, label: group.title })) ?? [],
    [snapshot],
  )

  const visibleGroups = React.useMemo(() => {
    if (!snapshot) return []
    return snapshot.groups.map((group) => {
      const filtered = group.sessions.filter((session) => {
        const projectMatch = projectFilter === 'all'
          || (projectFilter === 'none' && group.workspaceId === null)
          || projectFilter === group.workspaceId
        const typeMatch = chatFilter === 'all'
          || (chatFilter === 'project' && group.workspaceId !== null)
          || (chatFilter === 'no-project' && group.workspaceId === null)
        return projectMatch && typeMatch && sessionMatches(session, group, query)
      })
      return { group, sessions: filtered }
    }).filter(({ sessions }) => sessions.length > 0)
  }, [chatFilter, projectFilter, query, snapshot])

  const restore = async (sessionId: string) => {
    setRestoringId(sessionId)
    try {
      await api.archivesRestore(sessionId)
      toast({ kind: 'success', title: '会话已恢复', detail: '它已回到原来的工作区。' })
      await load()
    } catch (cause) {
      toast({ kind: 'error', title: '恢复失败', detail: String(cause) })
    } finally {
      setRestoringId(null)
    }
  }

  const deleteOne = async (session: ArchiveSession) => {
    if (!window.confirm(`确定永久删除「${session.title}」吗？删除后无法恢复。`)) return
    setDeletingId(session.sessionId)
    try {
      await api.archivesDelete(session.sessionId)
      toast({ kind: 'success', title: '会话已删除', detail: '会话日志和归档记录已移除。' })
      await load()
    } catch (cause) {
      toast({ kind: 'error', title: '删除失败', detail: String(cause) })
    } finally {
      setDeletingId(null)
    }
  }

  const deleteAll = async () => {
    if (!snapshot || snapshot.total === 0) return
    if (!window.confirm(`确定永久删除全部 ${snapshot.total} 个归档会话吗？删除后无法恢复。`)) return
    setDeletingAll(true)
    try {
      const result = await api.archivesDeleteAll()
      toast({ kind: 'success', title: '归档会话已全部删除', detail: `已删除 ${result.deletedCount} 个会话。` })
      await load()
    } catch (cause) {
      toast({ kind: 'error', title: '全部删除失败', detail: String(cause) })
    } finally {
      setDeletingAll(false)
    }
  }

  return (
    <div className="min-h-0 flex-1 overflow-y-auto bg-background">
      <div className="mx-auto max-w-[1050px] px-5 pb-10 pt-5">
        <div className="mb-5 flex items-center justify-between gap-4">
          <div>
            <h1 className="text-lg font-semibold text-foreground">已归档的聊天</h1>
            <p className="mt-1 text-xs text-muted-foreground">按项目查看、恢复或删除已归档会话</p>
          </div>
          <Button
            type="button"
            disabled={!snapshot?.deleteAvailable || snapshot.total === 0 || deletingAll}
            title={snapshot?.deleteAvailable ? '永久删除全部归档会话' : '归档插件不可用，暂时不能删除'}
            variant="destructive"
            className="h-8 rounded-lg px-3 text-xs font-medium"
            onClick={() => void deleteAll()}
          >
            <Trash2 className="size-3.5" /> {deletingAll ? '删除中…' : '全部删除'}
          </Button>
        </div>

        <div className="mb-6 grid grid-cols-1 gap-2 sm:grid-cols-[minmax(0,1fr)_160px_180px]">
          <label className="relative block">
            <Search className="pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
            <Input
              aria-label="搜索已归档聊天"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="搜索已归档聊天"
              className="h-8 rounded-lg pl-8 text-xs shadow-none"
            />
          </label>
            <FilterSelect
              icon={SlidersHorizontal}
              label={chatFilter === 'all' ? '全部聊天' : chatFilter === 'project' ? '有项目' : '无项目'}
              value={chatFilter}
              ariaLabel="聊天筛选"
              onChange={(value) => setChatFilter(value as ChatFilter)}
              options={[
                { value: 'all', label: '全部聊天' },
                { value: 'project', label: '有项目' },
                { value: 'no-project', label: '无项目' },
              ]}
            />
            <FilterSelect
              icon={Folder}
              label={projectFilter === 'all'
                ? '所有项目'
                : projectFilter === 'none'
                  ? '无项目'
                  : projectOptions.find((option) => option.value === projectFilter)?.label ?? '所有项目'}
              value={projectFilter}
              ariaLabel="项目筛选"
              onChange={setProjectFilter}
              options={[
                { value: 'all', label: '所有项目' },
                { value: 'none', label: '无项目' },
                ...projectOptions,
              ]}
            />
        </div>

        {snapshot?.status ? (
          <div className="mb-4 rounded-xl border border-amber-500/30 bg-amber-500/5 px-4 py-3 text-xs text-amber-700 dark:text-amber-300">
            {snapshot.status}
          </div>
        ) : null}
        {error ? (
          <div className="rounded-xl border border-red-500/30 bg-red-500/5 px-4 py-4 text-xs text-red-600 dark:text-red-400">
            <p className="font-medium">归档数据加载失败</p>
            <p className="mt-1 break-words">{error}</p>
            <Button variant="outline" size="sm" className="mt-3" onClick={() => void load()}>重新加载</Button>
          </div>
        ) : snapshot === null ? (
          <div className="py-10 text-center text-sm text-muted-foreground">加载归档会话中…</div>
        ) : visibleGroups.length === 0 ? (
          <div className="rounded-xl border border-dashed border-border px-4 py-10 text-center">
            <Archive className="mx-auto size-8 text-muted-foreground" strokeWidth={1.5} />
            <p className="mt-3 text-sm font-medium">没有符合条件的归档聊天</p>
            <p className="mt-1 text-xs text-muted-foreground">归档的会话会按工作区显示在这里。</p>
          </div>
        ) : (
          visibleGroups.map(({ group, sessions }) => (
            <ArchiveGroupCard
              key={group.workspaceId ?? 'no-project'}
              group={group}
              sessions={sessions}
              deleteAvailable={snapshot.deleteAvailable}
              deletingId={deletingId}
              restoreAvailable={snapshot.restoreAvailable}
              restoringId={restoringId}
              onDelete={(sessionId) => {
                const session = sessions.find((item) => item.sessionId === sessionId)
                if (session) void deleteOne(session)
              }}
              onRestore={(sessionId) => void restore(sessionId)}
            />
          ))
        )}
      </div>
    </div>
  )
}
