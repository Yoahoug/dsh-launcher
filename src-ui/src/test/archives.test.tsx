import { describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { ArchivesPage } from '@/components/archives/archives-page'
import { ToastProvider } from '@/components/ui/toast'
import { mockApi } from '@/lib/mock'
import type { ArchivesSnapshot } from '@/types/schema'

const snapshot: ArchivesSnapshot = {
  groups: [
    {
      workspaceId: 'w1',
      title: 'dsh-launcher',
      path: '/tmp/dsh-launcher',
      sessions: [
        { sessionId: 's1', title: '更新插件技能管理模块', createdAt: 1000, lastActivityAt: 2000 },
        { sessionId: 's2', title: '完成 DeepSeek 工作区集成', createdAt: 3000, lastActivityAt: 3000 },
      ],
    },
    {
      workspaceId: null,
      title: '无项目',
      path: null,
      sessions: [{ sessionId: 's3', title: '确认 web.run 搜索工具', createdAt: 4000, lastActivityAt: 4000 }],
    },
  ],
  total: 3,
  running: true,
  pluginAvailable: true,
  restoreAvailable: true,
  deleteAvailable: true,
  status: null,
}

function renderPage() {
  vi.spyOn(mockApi, 'archivesGetSnapshot').mockResolvedValue(snapshot)
  return render(
    <ToastProvider>
      <ArchivesPage />
    </ToastProvider>,
  )
}

describe('归档会话子界面', () => {
  it('呈现 Codex 风格布局、项目分组和筛选控件', async () => {
    renderPage()
    expect(await screen.findByRole('heading', { name: '已归档的聊天' })).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'dsh-launcher' })).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: '无项目' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '全部删除' })).toBeEnabled()
    expect(screen.getAllByRole('button', { name: /永久删除$/ })).toHaveLength(3)
    expect(screen.getAllByRole('button', { name: '取消归档' })).toHaveLength(3)
  })

  it('支持搜索和项目范围筛选', async () => {
    const user = userEvent.setup()
    renderPage()
    await screen.findByText('更新插件技能管理模块')
    await user.type(screen.getByRole('textbox', { name: '搜索已归档聊天' }), 'web.run')
    expect(screen.getByText('确认 web.run 搜索工具')).toBeInTheDocument()
    expect(screen.queryByText('更新插件技能管理模块')).not.toBeInTheDocument()

    await user.clear(screen.getByRole('textbox', { name: '搜索已归档聊天' }))
    await user.selectOptions(screen.getByRole('combobox', { name: '项目筛选' }), 'w1')
    expect(screen.getByText('更新插件技能管理模块')).toBeInTheDocument()
    expect(screen.queryByText('确认 web.run 搜索工具')).not.toBeInTheDocument()
  })

  it('点击取消归档调用后端恢复接口', async () => {
    const user = userEvent.setup()
    const restore = vi.spyOn(mockApi, 'archivesRestore').mockResolvedValue({ sessionId: 's1', hot: true })
    renderPage()
    await user.click((await screen.findAllByRole('button', { name: '取消归档' }))[0]!)
    expect(restore).toHaveBeenCalledWith('s1')
  })

  it('确认后永久删除单条归档会话', async () => {
    const user = userEvent.setup()
    vi.spyOn(window, 'confirm').mockReturnValue(true)
    const remove = vi.spyOn(mockApi, 'archivesDelete').mockResolvedValue({ deletedCount: 1, hot: true })
    renderPage()
    await user.click((await screen.findAllByRole('button', { name: /永久删除$/ }))[0]!)
    expect(remove).toHaveBeenCalledWith('s1')
  })

  it('确认后永久删除全部归档会话', async () => {
    const user = userEvent.setup()
    vi.spyOn(window, 'confirm').mockReturnValue(true)
    const removeAll = vi.spyOn(mockApi, 'archivesDeleteAll').mockResolvedValue({ deletedCount: 3, hot: true })
    renderPage()
    await user.click(await screen.findByRole('button', { name: '全部删除' }))
    expect(removeAll).toHaveBeenCalledOnce()
  })

  it('插件不可用时删除按钮仍保持悬浮显示规则', async () => {
    vi.spyOn(mockApi, 'archivesGetSnapshot').mockResolvedValueOnce({ ...snapshot, deleteAvailable: false })
    render(
      <ToastProvider>
        <ArchivesPage />
      </ToastProvider>,
    )

    const deleteButton = (await screen.findAllByRole('button', { name: /永久删除$/ }))[0]!
    expect(deleteButton).toBeDisabled()
    expect(deleteButton).toHaveClass('opacity-0', 'group-hover:opacity-100')
    expect(deleteButton).not.toHaveClass('disabled:opacity-40')
  })
})
