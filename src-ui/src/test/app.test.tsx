// dsh-launcher · UI 回归:主流程(First-run 组件流程 → Dashboard → Logs → Settings)
import { describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import App from '@/App'
import { FirstRunPage } from '@/components/first-run/first-run'
import { ToastProvider } from '@/components/ui/toast'
import { mockApi } from '@/lib/mock'

describe('FirstRunPage 流程', () => {
  it('欢迎 → 选择仓库 → 检测环境 → 完成', async () => {
    const user = userEvent.setup()
    render(
      <ToastProvider>
        <FirstRunPage onDone={() => {}} onOpenSettings={() => {}} />
      </ToastProvider>,
    )
    expect(screen.getByText(/欢迎使用 DSH Launcher/)).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: '开始配置' }))
    expect(await screen.findByText('仓库位置')).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: '检测环境' }))
    expect(await screen.findByText(/可用 ✓/)).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: '完成,进入主界面' }))
    expect(await screen.findByText('设置已保存')).toBeInTheDocument()
  })
})

describe('App 首次运行门控', () => {
  it('first-run 未完成时显示引导页', async () => {
    window.history.replaceState(null, '', '/?first-run=1')
    render(<App />)
    expect(await screen.findByText(/欢迎使用 DSH Launcher/)).toBeInTheDocument()
    window.history.replaceState(null, '', '/')
  })

  it('已完成时直接进入 Dashboard', async () => {
    window.history.replaceState(null, '', '/')
    render(<App />)
    expect(await screen.findByText('DeepSeek Harness')).toBeInTheDocument()
    expect(await screen.findByText('仓库与构建')).toBeInTheDocument()
    expect(await screen.findByText('工具链')).toBeInTheDocument()
    expect(screen.getByRole('radiogroup', { name: '启动方式' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '普通启动' })).toBeInTheDocument()
  })
})

describe('App 页面导航', () => {
  it('首页运行态的打开按钮进入主窗口 DeepSeek 工作区(不弹独立 chat 窗口)', async () => {
    const openChat = vi.spyOn(mockApi, 'openChat')
    const openDsh = vi.spyOn(mockApi, 'openDsh')
    const openWorkspace = vi.spyOn(mockApi, 'openDshWorkspace')
    render(<App />)
    const user = userEvent.setup()
    await user.click(await screen.findByRole('button', { name: '普通启动' }))
    await user.click(await screen.findByRole('button', { name: '打开 dsh' }, { timeout: 2_000 }))
    expect(openWorkspace).toHaveBeenCalledOnce()
    expect(openChat).not.toHaveBeenCalled()
    expect(openDsh).not.toHaveBeenCalled()
  })

  it('日志页可进入并可返回', async () => {
    render(<App />)
    const user = userEvent.setup()
    await screen.findByText('DeepSeek Harness')
    await user.click(screen.getByTitle('日志'))
    expect(await screen.findByRole('log')).toBeInTheDocument()
    // 侧边栏「服务」返回主页
    await user.click(screen.getByRole('tab', { name: '服务' }))
    expect(await screen.findByText('DeepSeek Harness')).toBeInTheDocument()
  })

  it('设置页可进入并保存偏好', async () => {
    render(<App />)
    const user = userEvent.setup()
    await screen.findByText('DeepSeek Harness')
    await user.click(screen.getByTitle('设置'))
    expect(await screen.findByText(/仓库路径/)).toBeInTheDocument()
    // 切换到「外观」分类修改主题
    await user.click(screen.getByRole('tab', { name: '外观' }))
    await user.selectOptions(await screen.findByRole('combobox', { name: '主题' }), 'dark')
    await user.click(screen.getByRole('button', { name: '保存' }))
    expect(await screen.findByText('设置已保存')).toBeInTheDocument()
  })
})
