// dsh-launcher · UI 回归:MainAction 状态 → 动作映射 / 设置校验
import { describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MainAction } from '@/components/dashboard/main-action'
import { SettingsPage } from '@/components/settings/settings-page'
import { mockApi } from '@/lib/mock'
import { ToastProvider } from '@/components/ui/toast'
import type { AppSnapshot } from '@/types/schema'

const base: AppSnapshot = {
  version: '0.3.0', state: 'idle', mode: 'none', phase: '', error: null,
  url: null, webPid: null, devPid: null, startedAt: null, readyAt: null,
  hmrActive: false, busy: false, launcherPid: 1,
  repo: { branch: 'main', head: 'abc', behind: 0, ahead: 0, dirty: false, dirtyFiles: 0, syncAt: null, remoteUpToDate: true },
  update: { mode: null, checking: false, available: false, version: null, url: null, size: null, notes: null, message: null, error: null, installing: false, progress: null },
}

function renderMain(snap: AppSnapshot, mode: 'normal' | 'dev' | 'maintenance') {
  const onAction = vi.fn()
  render(<MainAction snap={snap} mode={mode} onAction={onAction} />)
  return onAction
}

describe('MainAction 状态 → 动作映射', () => {
  it('idle + normal → start', async () => {
    const onAction = renderMain(base, 'normal')
    await userEvent.click(screen.getByRole('button'))
    expect(onAction).toHaveBeenCalledWith('start')
  })

  it('idle + dev → dev', async () => {
    const onAction = renderMain(base, 'dev')
    await userEvent.click(screen.getByRole('button'))
    expect(onAction).toHaveBeenCalledWith('dev')
  })

  it('idle + maintenance → update', async () => {
    const onAction = renderMain(base, 'maintenance')
    await userEvent.click(screen.getByRole('button'))
    expect(onAction).toHaveBeenCalledWith('update')
  })

  it('running → open-dsh', async () => {
    const onAction = renderMain({ ...base, state: 'running', mode: 'normal', url: 'http://127.0.0.1:3080/' }, 'normal')
    await userEvent.click(screen.getByRole('button'))
    expect(onAction).toHaveBeenCalledWith('open-dsh')
  })

  it('failed + normal → start(重试)', async () => {
    const onAction = renderMain({ ...base, state: 'failed', error: { summary: 'x', detail: 'y' } }, 'normal')
    await userEvent.click(screen.getByRole('button'))
    expect(onAction).toHaveBeenCalledWith('start')
  })

  it('busy 时禁用', async () => {
    const onAction = renderMain({ ...base, state: 'syncing', busy: true }, 'normal')
    const btn = screen.getByRole('button')
    expect(btn).toBeDisabled()
    await userEvent.click(btn).catch(() => {})
    expect(onAction).not.toHaveBeenCalled()
  })

  it('stopping → cancel(stop)', async () => {
    const onAction = renderMain({ ...base, state: 'stopping', busy: true }, 'normal')
    const btn = screen.getByRole('button')
    expect(btn).toBeDisabled()
    expect(onAction).not.toHaveBeenCalled()
  })
})

describe('设置校验', () => {
  it('端口越界时拒绝保存并提示', async () => {
    vi.spyOn(mockApi, 'saveSettings')
    render(
      <ToastProvider>
        <SettingsPage onBack={() => {}} />
      </ToastProvider>,
    )
    const user = userEvent.setup()
    await screen.findByText(/仓库路径/)
    const portInput = screen.getByDisplayValue('3080') as HTMLInputElement
    await user.clear(portInput)
    await user.type(portInput, '70000')
    await user.click(screen.getByRole('button', { name: '保存' }))
    expect(await screen.findByText('设置无效')).toBeInTheDocument()
    expect(mockApi.saveSettings).not.toHaveBeenCalled()
  })

  it('有效修改可保存成功', async () => {
    render(
      <ToastProvider>
        <SettingsPage onBack={() => {}} />
      </ToastProvider>,
    )
    const user = userEvent.setup()
    await screen.findByText(/仓库路径/)
    const portInput = screen.getByDisplayValue('3080') as HTMLInputElement
    await user.clear(portInput)
    await user.type(portInput, '3081')
    await user.click(screen.getByRole('button', { name: '保存' }))
    expect(await screen.findByText('设置已保存')).toBeInTheDocument()
  })
})
