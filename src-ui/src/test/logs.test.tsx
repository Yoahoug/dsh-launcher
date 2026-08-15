// dsh-launcher · UI 回归:Logs 页筛选/搜索/暂停/清空/复制
import { describe, expect, it, vi } from 'vitest'
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { LogsPage } from '@/components/logs/logs-page'
import { ToastProvider } from '@/components/ui/toast'

function renderLogs() {
  return render(
    <ToastProvider>
      <LogsPage onBack={() => {}} />
    </ToastProvider>,
  )
}

describe('LogsPage', () => {
  it('加载历史日志并按来源/级别筛选', async () => {
    const user = userEvent.setup()
    renderLogs()
    const log = await screen.findByRole('log')
    expect(within(log).getByText(/dsh-launcher 启动/)).toBeInTheDocument()

    // 来源筛选:git
    await user.selectOptions(screen.getByRole('combobox', { name: '日志来源' }), 'git')
    expect(within(log).getByText(/git pull/)).toBeInTheDocument()
    expect(within(log).queryByText(/dsh-launcher 启动/)).not.toBeInTheDocument()
  })

  it('搜索过滤日志文本', async () => {
    const user = userEvent.setup()
    renderLogs()
    const log = await screen.findByRole('log')
    await user.type(screen.getByPlaceholderText('搜索日志…'), 'build')
    expect(within(log).getByText(/pnpm run build/)).toBeInTheDocument()
    expect(within(log).queryByText(/git pull/)).not.toBeInTheDocument()
  })

  it('暂停按钮切换自动滚动状态', async () => {
    const user = userEvent.setup()
    renderLogs()
    await screen.findByRole('log')
    await user.click(screen.getByRole('button', { name: /暂停/ }))
    expect(screen.getByRole('button', { name: /继续/ })).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: /继续/ }))
    expect(screen.getByRole('button', { name: /暂停/ })).toBeInTheDocument()
  })

  it('清空后显示空状态', async () => {
    const user = userEvent.setup()
    renderLogs()
    await screen.findByRole('log')
    await user.click(screen.getByRole('button', { name: /清空/ }))
    expect(await screen.findByText('暂无日志')).toBeInTheDocument()
  })

  it('复制按钮使用剪贴板', async () => {
    const writeSpy = vi.spyOn(navigator.clipboard, 'writeText').mockResolvedValue()
    const user = userEvent.setup()
    renderLogs()
    await screen.findByRole('log')
    await user.click(screen.getByRole('button', { name: /复制/ }))
    expect(writeSpy).toHaveBeenCalled()
  })

  it('错误级别初始筛选(从 Dashboard 跳转)', async () => {
    render(
      <ToastProvider>
        <LogsPage initialLevel="err" onBack={() => {}} />
      </ToastProvider>,
    )
    const log = await screen.findByRole('log')
    const select = screen.getByRole('combobox', { name: '日志级别' }) as HTMLSelectElement
    expect(select.value).toBe('err')
    expect(within(log).queryByText(/dsh-launcher 启动/)).not.toBeInTheDocument()
  })
})
