// dsh-launcher · UI 回归:技能子界面(已启动/外部发现 + 注入开关 + 去重 + 新建/删除/导入/一键启用)
import { describe, expect, it, vi } from 'vitest'
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { SkillsPage } from '@/components/skills/skills-page'
import { ToastProvider } from '@/components/ui/toast'
import { mockApi } from '@/lib/mock'
import type { SkillsActiveSnapshot, SkillsSnapshot } from '@/types/schema'

const snap: SkillsSnapshot = {
  roots: [
    { key: 'managed', label: '已管理 · $DSH_HOME/skills', path: '/Users/u/.dsh/skills', exists: true, managed: true, enabled: false },
    { key: 'codex', label: 'Codex', path: '/Users/u/.codex/skills', exists: true, managed: false, enabled: false },
    { key: 'claude', label: 'Claude Code', path: '/Users/u/.claude/skills', exists: true, managed: false, enabled: true },
    { key: 'project', label: '项目 · .dsh/skills', path: '/repo/.dsh/skills', exists: false, managed: false, enabled: false },
  ],
  skills: [
    {
      name: 'my-skill', description: '我的技能', whenToUse: null,
      modelInvocable: true, userInvocable: true, source: 'managed',
      dir: '/Users/u/.dsh/skills/my-skill', path: '/Users/u/.dsh/skills/my-skill/SKILL.md',
      sizeBytes: 320, hasScripts: false,
    },
    {
      name: 'codex-helper', description: 'Codex helper', whenToUse: 'when needed',
      modelInvocable: false, userInvocable: true, source: 'codex',
      dir: '/Users/u/.codex/skills/codex-helper', path: '/Users/u/.codex/skills/codex-helper/SKILL.md',
      sizeBytes: 2048, hasScripts: true,
    },
    {
      name: 'tavily-extract', description: 'Extract via Tavily', whenToUse: null,
      modelInvocable: true, userInvocable: true, source: 'claude',
      dir: '/Users/u/.claude/skills/tavily-extract', path: '/Users/u/.claude/skills/tavily-extract/SKILL.md',
      sizeBytes: 1024, hasScripts: false,
    },
  ],
  pluginsInstalled: true,
  skipped: ['bad:目录名非 kebab-case,跳过'],
}

function renderPage(options: {
  snapshot?: SkillsSnapshot
  active?: SkillsActiveSnapshot
  runtime?: { state: 'idle' | 'syncing' | 'installing' | 'building' | 'starting' | 'running' | 'stopping' | 'failed'; mode: 'none' | 'normal' | 'dev'; hmrActive: boolean }
} = {}) {
  vi.spyOn(mockApi, 'getSettings').mockResolvedValue({
    repoPath: '/Users/u/deepseek-harness', port: 3080, host: '127.0.0.1', dshHome: '',
    autostart: false, openBrowser: true, autoUpdateCheck: true, buildArgs: '',
    readyTimeoutMs: 180_000, startTimeoutMs: 180_000, firstRunSkipped: true,
    profileName: 'web', dshPluginsPath: '/x', externalSkillRoots: [], skillManagedRoot: '',
  })
  vi.spyOn(mockApi, 'skillsGetSnapshot').mockResolvedValue(options.snapshot ?? snap)
  if (options.active) vi.spyOn(mockApi, 'skillsGetActive').mockResolvedValue(options.active)
  return render(
    <ToastProvider>
      <SkillsPage runtime={options.runtime} />
    </ToastProvider>,
  )
}

/** 切到「外部发现」子界面。 */
async function goDiscover(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole('button', { name: /外部发现/ }))
}

describe('技能管理子界面', () => {
  it('按工具分组展示技能 + 调用策略徽标 + 已启用根状态', async () => {
    const user = userEvent.setup()
    renderPage()
    await goDiscover(user)
    expect(await screen.findByText('已管理')).toBeInTheDocument()
    expect(screen.getByText('Codex')).toBeInTheDocument()
    expect(screen.getByText('Claude Code')).toBeInTheDocument()
    expect(screen.getByText('my-skill')).toBeInTheDocument()
    expect(screen.getByText('codex-helper')).toBeInTheDocument()
    expect(screen.getByText('tavily-extract')).toBeInTheDocument()
    // 调用策略徽标:模型禁用 / 模型可调用
    expect(screen.getByText('模型禁用')).toBeInTheDocument()
    expect(screen.getAllByText('模型可调用').length).toBeGreaterThanOrEqual(2)
    // codex 根未启用 → 一键启用按钮;claude 根已启用 → 已启用徽章
    expect(screen.getAllByRole('button', { name: '一键启用' }).length).toBeGreaterThanOrEqual(1)
    expect(screen.getAllByText('已启用 ✓').length).toBeGreaterThanOrEqual(1)
    // skill-external-roots 已安装提示
    expect(screen.getByText(/skill-external-roots 已安装/)).toBeInTheDocument()
    // 被跳过条目提示
    expect(screen.getByText(/1 个条目被跳过/)).toBeInTheDocument()
  })

  it('新建技能:kebab 校验失败不提交,合法后调用 skillsCreate', async () => {
    const user = userEvent.setup()
    renderPage()
    await goDiscover(user)
    const spy = vi.spyOn(mockApi, 'skillsCreate').mockResolvedValue({
      name: 'new-skill', description: '新技能', whenToUse: null,
      modelInvocable: true, userInvocable: true, source: 'managed',
      dir: '/Users/u/.dsh/skills/new-skill', path: '/Users/u/.dsh/skills/new-skill/SKILL.md',
      sizeBytes: 10, hasScripts: false,
    })
    await user.click(await screen.findByRole('button', { name: '新建技能' }))
    await user.type(screen.getByLabelText(/技能名/), 'Bad Name')
    await user.type(screen.getByLabelText(/描述/), '描述')
    await user.click(screen.getByRole('button', { name: '保存' }))
    expect(spy).not.toHaveBeenCalled()
    // 清空重填合法 kebab 名
    await user.clear(screen.getByLabelText(/技能名/))
    await user.type(screen.getByLabelText(/技能名/), 'new-skill')
    await user.click(screen.getByRole('button', { name: '保存' }))
    expect(spy).toHaveBeenCalledWith('new-skill', '描述', null, '')
  })

  it('删除 managed 技能:确认后调用 skillsDelete', async () => {
    const user = userEvent.setup()
    const confirmSpy = vi.spyOn(window, 'confirm').mockReturnValue(true)
    const spy = vi.spyOn(mockApi, 'skillsDelete').mockResolvedValue(undefined)
    renderPage()
    await goDiscover(user)
    await user.click(await screen.findByLabelText('删除 my-skill'))
    expect(confirmSpy).toHaveBeenCalled()
    expect(spy).toHaveBeenCalledWith('my-skill')
  })

  it('外部技能可导入到 dsh(skillsImport)', async () => {
    const user = userEvent.setup()
    renderPage()
    await goDiscover(user)
    const spy = vi.spyOn(mockApi, 'skillsImport').mockResolvedValue({
      name: 'codex-helper', description: 'Codex helper', whenToUse: null,
      modelInvocable: true, userInvocable: true, source: 'managed',
      dir: '/Users/u/.dsh/skills/codex-helper', path: '/Users/u/.dsh/skills/codex-helper/SKILL.md',
      sizeBytes: 10, hasScripts: true,
    })
    const card = (await screen.findByText('codex-helper')).closest('div.rounded-xl') as HTMLElement | null
    expect(card).not.toBeNull()
    const importBtn = within(card!).getByRole('button', { name: '导入到 dsh' })
    await user.click(importBtn)
    expect(spy).toHaveBeenCalledWith('/Users/u/.codex/skills/codex-helper', 'codex-helper')
  })

  it('一键启用:调用 skillsEnableRoot 并刷新', async () => {
    const user = userEvent.setup()
    const spy = vi.spyOn(mockApi, 'skillsEnableRoot').mockResolvedValue({
      backup: 'cordis.patch.yml.bak-1', ok: true, summary: '已写入 customSkillDirs', validated: true, error: null,
    })
    renderPage()
    await goDiscover(user)
    const enableBtn = (await screen.findAllByRole('button', { name: '一键启用' }))[0]!
    await user.click(enableBtn)
    expect(spy).toHaveBeenCalledWith('web', '/Users/u/.codex/skills')
  })

  it('同名技能继续按名称控制,但标注重复根目录来源', async () => {
    const user = userEvent.setup()
    const duplicate: SkillsSnapshot['skills'][number] = {
      ...snap.skills[2]!,
      source: 'codex',
      dir: '/Users/u/.codex/skills/tavily-extract',
      path: '/Users/u/.codex/skills/tavily-extract/SKILL.md',
    }
    renderPage({ snapshot: { ...snap, skills: [...snap.skills, duplicate] } })
    await goDiscover(user)
    expect(await screen.findAllByText('同名 ×2')).toHaveLength(2)
    expect(screen.getAllByText(/发现根目录:.*\.codex\/skills/).length).toBeGreaterThanOrEqual(1)
    expect(screen.getAllByText(/发现根目录:.*\.claude\/skills/).length).toBeGreaterThanOrEqual(1)
    expect(screen.getAllByText(/同名来源:/).length).toBeGreaterThanOrEqual(2)
  })
})

describe('技能注入控制(已启动/外部发现两子界面)', () => {
  it('默认展示「已启动」子界面:运行中 dsh 注入清单 + 开关关闭调用 skillsSetInjected', async () => {
    const user = userEvent.setup()
    const spy = vi.spyOn(mockApi, 'skillsSetInjected').mockResolvedValue({
      ok: true, summary: 'win-host 已关闭注入(mock)', enabled: false,
    })
    renderPage()
    // 已启动清单(来自 mock active):tavily-extract 与 win-host
    expect(await screen.findByText('tavily-extract')).toBeInTheDocument()
    expect(screen.getByText('win-host')).toBeInTheDocument()
    expect(screen.getAllByText('系统自带').length).toBeGreaterThanOrEqual(2)
    expect(screen.getAllByText('外部发现').length).toBeGreaterThanOrEqual(2)
    expect(screen.getByText('my-skill')).toBeInTheDocument()
    expect(screen.queryByRole('switch', { name: /my-skill/ })).not.toBeInTheDocument()
    expect(screen.getAllByText('已启动').length).toBeGreaterThanOrEqual(2)
    expect(screen.getByText(/注入控制已启用/)).toBeInTheDocument()
    // 关闭 win-host 注入
    const sw = screen.getByRole('switch', { name: /关闭注入 win-host/ })
    await user.click(sw)
    expect(spy).toHaveBeenCalledWith('win-host', false)
    expect(await screen.findAllByText(/已关闭注入/).then((els) => els.length)).toBeGreaterThanOrEqual(1)
  })

  it('「外部发现」与「已启动」去重:已注入 ✓ / 未注入 徽标 + 每卡注入开关', async () => {
    const user = userEvent.setup()
    const spy = vi.spyOn(mockApi, 'skillsSetInjected').mockResolvedValue({
      ok: true, summary: 'tavily-extract 已关闭注入(mock)', enabled: false,
    })
    renderPage()
    await goDiscover(user)
    // 去重:active 里的 tavily-extract → 已注入;其余 → 未注入
    expect(await screen.findByText('已注入 ✓')).toBeInTheDocument()
    // 系统自带技能不再显示外部注入开关;外部技能未注入时开关必须同步为关闭。
    expect(screen.getAllByText('未注入').length).toBeGreaterThanOrEqual(1)
    // 外部技能卡片带注入开关(默认开),关闭调用 skillsSetInjected
    const sw = screen.getByRole('switch', { name: '注入 tavily-extract' })
    expect(sw).toHaveAttribute('aria-checked', 'true')
    await user.click(sw)
    expect(spy).toHaveBeenCalledWith('tavily-extract', false)
  })

  it('未启用注入控制时「已启动」子界面引导一键启用', async () => {
    const user = userEvent.setup()
    vi.spyOn(mockApi, 'skillsGetActive').mockResolvedValue({
      file: '/Users/u/.dsh/state/skills-active.json',
      writtenAt: null,
      skills: [],
      error: null,
      controlFile: null,
      controlFileExists: false,
    })
    const spy = vi.spyOn(mockApi, 'skillsEnableControl').mockResolvedValue({
      backup: 'cordis.patch.yml.bak-1', ok: true, summary: '已启用注入控制(mock)', validated: true, error: null,
    })
    renderPage()
    expect(await screen.findByText('尚未启用注入控制')).toBeInTheDocument()
    const btn = screen.getByRole('button', { name: /启用注入控制/ })
    await user.click(btn)
    expect(spy).toHaveBeenCalledWith('web')
  })

  it('已启动技能支持预览,并按实际根目录分组后一键关闭', async () => {
    const user = userEvent.setup()
    const active: SkillsActiveSnapshot = {
      file: '/Users/u/.dsh/state/skills-active.json',
      writtenAt: null,
      skills: [{
        name: 'tavily-extract', description: 'Extract', whenToUse: null, source: 'external',
        root: '/Users/u/.claude/skills', path: '/Users/u/.claude/skills/tavily-extract/SKILL.md',
        modelInvocable: true, userInvocable: true,
      }],
      error: null,
      controlFile: '/Users/u/.dsh/skills-control.json',
      controlFileExists: true,
    }
    const snapshot = {
      ...snap,
      roots: snap.roots.map((root) => root.key === 'claude' ? { ...root, path: '/Users/u/.claude/skills' } : root),
    }
    const rootSpy = vi.spyOn(mockApi, 'skillsSetRootInjected').mockResolvedValue({
      ok: true, summary: 'claude 根目录下技能已关闭注入(mock)', enabled: false,
    })
    renderPage({ snapshot, active })
    expect(await screen.findByText('Claude Code')).toBeInTheDocument()
    expect(screen.getByText('根目录:/Users/u/.claude/skills')).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: '关闭Claude Code全部' }))
    expect(rootSpy).toHaveBeenCalledWith('claude', false)
    await user.click(screen.getByRole('button', { name: '预览' }))
    expect(await screen.findAllByText('/Users/u/.claude/skills/tavily-extract/SKILL.md')).not.toHaveLength(0)
  })

  it('普通模式明确提示技能开关不会热重载', async () => {
    renderPage({ runtime: { state: 'running', mode: 'normal', hmrActive: false } })
    expect(await screen.findByText(/当前普通模式:技能开关不会热重载/)).toBeInTheDocument()
    expect(screen.getByText(/普通模式。技能开关已写入控制文件/)).toBeInTheDocument()
  })
})
