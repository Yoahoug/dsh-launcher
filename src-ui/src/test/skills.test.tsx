// dsh-launcher · UI 回归:技能管理子界面(分组展示 + 新建/删除 + 导入 + 一键启用)
import { describe, expect, it, vi } from 'vitest'
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { SkillsPage } from '@/components/skills/skills-page'
import { ToastProvider } from '@/components/ui/toast'
import { mockApi } from '@/lib/mock'
import type { SkillsSnapshot } from '@/types/schema'

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

function renderPage() {
  vi.spyOn(mockApi, 'getSettings').mockResolvedValue({
    repoPath: '/Users/u/deepseek-harness', port: 3080, host: '127.0.0.1', dshHome: '',
    autostart: false, openBrowser: true, autoUpdateCheck: true, buildArgs: '',
    readyTimeoutMs: 180_000, startTimeoutMs: 180_000, firstRunSkipped: true,
    profileName: 'web', dshPluginsPath: '/x', externalSkillRoots: [], skillManagedRoot: '',
  })
  vi.spyOn(mockApi, 'skillsGetSnapshot').mockResolvedValue(snap)
  return render(
    <ToastProvider>
      <SkillsPage />
    </ToastProvider>,
  )
}

describe('技能管理子界面', () => {
  it('按工具分组展示技能 + 调用策略徽标 + 已启用根状态', async () => {
    renderPage()
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
    const spy = vi.spyOn(mockApi, 'skillsCreate').mockResolvedValue({
      name: 'new-skill', description: '新技能', whenToUse: null,
      modelInvocable: true, userInvocable: true, source: 'managed',
      dir: '/Users/u/.dsh/skills/new-skill', path: '/Users/u/.dsh/skills/new-skill/SKILL.md',
      sizeBytes: 10, hasScripts: false,
    })
    renderPage()
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
    await user.click(await screen.findByLabelText('删除 my-skill'))
    expect(confirmSpy).toHaveBeenCalled()
    expect(spy).toHaveBeenCalledWith('my-skill')
  })

  it('外部技能可导入到 dsh(skillsImport)', async () => {
    const user = userEvent.setup()
    const spy = vi.spyOn(mockApi, 'skillsImport').mockResolvedValue({
      name: 'codex-helper', description: 'Codex helper', whenToUse: null,
      modelInvocable: true, userInvocable: true, source: 'managed',
      dir: '/Users/u/.dsh/skills/codex-helper', path: '/Users/u/.dsh/skills/codex-helper/SKILL.md',
      sizeBytes: 10, hasScripts: true,
    })
    renderPage()
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
    const enableBtn = (await screen.findAllByRole('button', { name: '一键启用' }))[0]!
    await user.click(enableBtn)
    expect(spy).toHaveBeenCalledWith('web', '/Users/u/.codex/skills')
  })
})
