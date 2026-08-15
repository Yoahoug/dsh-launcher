// dsh-launcher · 主窗口内 DeepSeek 工作区 UI 回归
// 语义与 Rust dsh_view 对齐:accepted ≠ success;只有真实 ready 才进入工作区;
// 失败/断线保留错误与日志/重试/返回启动器入口;返回启动器后再进入不重置会话。
import { describe, expect, it, vi } from 'vitest'
import { act, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import App from '@/App'
import { mockApi } from '@/lib/mock'

describe('DeepSeek 工作区', () => {
  it('点击标题栏「DeepSeek」进入工作区:先加载状态,就绪后显示(启动器内容隐藏)', async () => {
    window.history.replaceState(null, '', '/')
    render(<App />)
    const user = userEvent.setup()
    await screen.findByText('DeepSeek Harness') // dashboard 已渲染

    await user.click(screen.getByRole('radio', { name: 'DeepSeek' }))

    // 加载状态(服务启动中)
    expect(await screen.findByText('正在启动 DeepSeek 工作区…')).toBeInTheDocument()
    // 启动器侧边栏/主体被隐藏
    expect(screen.queryByRole('tab', { name: '服务' })).not.toBeInTheDocument()

    // 就绪后:原生子 WebView 覆盖区域(占位 div,无 DOM 内容)
    await waitFor(
      async () => {
        expect((await mockApi.getDshViewState()).status).toBe('ready')
      },
      { timeout: 3_000 },
    )
    expect(screen.queryByText('正在启动 DeepSeek 工作区…')).not.toBeInTheDocument()
    expect(screen.queryByRole('tab', { name: '服务' })).not.toBeInTheDocument()
  })

  it('加载期间返回启动器会取消自动进入,服务稍后就绪也不把用户拉回', async () => {
    window.history.replaceState(null, '', '/')
    render(<App />)
    const user = userEvent.setup()
    await screen.findByText('DeepSeek Harness')

    await user.click(screen.getByRole('radio', { name: 'DeepSeek' }))
    expect(await screen.findByText('正在启动 DeepSeek 工作区…')).toBeInTheDocument()
    await user.click(screen.getByRole('radio', { name: '启动器' }))

    expect(await screen.findByRole('tab', { name: '服务' })).toBeInTheDocument()
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1_400))
    })
    expect(screen.getByRole('radio', { name: '启动器' })).toHaveAttribute('aria-checked', 'true')
    expect((await mockApi.getDshViewState()).pendingEnter).toBe(false)
  })

  it('点击「普通启动」accepted 后不提前进入;真实 running + ready 后自动切换', async () => {
    window.history.replaceState(null, '', '/')
    render(<App />)
    const user = userEvent.setup()
    await screen.findByText('DeepSeek Harness')

    await user.click(screen.getByRole('button', { name: '普通启动' }))
    // accepted 后仍是启动器工作区(不提前显示成功)
    expect(screen.queryByText('启动成功')).not.toBeInTheDocument()
    expect(screen.getByText('启动已受理')).toBeInTheDocument()
    expect(screen.queryByText('正在启动 DeepSeek 工作区…')).not.toBeInTheDocument()
    expect(screen.getByRole('tab', { name: '服务' })).toBeInTheDocument()

    // 服务 running + 视图 ready → 自动进入 DeepSeek 工作区
    await waitFor(
      () => {
        expect(screen.getByRole('radio', { name: 'DeepSeek' })).toHaveAttribute('aria-checked', 'true')
      },
      { timeout: 3_000 },
    )
    // 启动器主体隐藏
    expect(screen.queryByRole('tab', { name: '服务' })).not.toBeInTheDocument()
  })

  it('启动失败:保留错误状态与日志/重试/返回入口,不进入空白页面', async () => {
    window.history.replaceState(null, '', '/?dsh-fail=1')
    render(<App />)
    const user = userEvent.setup()
    await screen.findByText('DeepSeek Harness')

    await user.click(screen.getByRole('radio', { name: 'DeepSeek' }))

    expect(await screen.findByText('DeepSeek 工作区不可用')).toBeInTheDocument()
    expect(screen.getByText(/DSH 服务启动失败/)).toBeInTheDocument()
    // 明确的内嵌错误入口
    expect(screen.getByRole('button', { name: '重试' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '查看日志' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '返回启动器' })).toBeInTheDocument()
  })

  it('返回启动器后再进入:会话保持(不重新加载,不出现创建/加载态)', async () => {
    window.history.replaceState(null, '', '/')
    render(<App />)
    const user = userEvent.setup()
    await screen.findByText('DeepSeek Harness')

    // 进入 → 就绪
    await user.click(screen.getByRole('radio', { name: 'DeepSeek' }))
    await waitFor(
      () => {
        expect(screen.getByRole('radio', { name: 'DeepSeek' })).toHaveAttribute('aria-checked', 'true')
      },
      { timeout: 3_000 },
    )

    // 返回启动器
    await user.click(screen.getByRole('radio', { name: '启动器' }))
    expect(await screen.findByRole('tab', { name: '服务' })).toBeInTheDocument()

    // 再次进入:直接就绪(会话保持,无创建/加载中间态)
    await user.click(screen.getByRole('radio', { name: 'DeepSeek' }))
    await waitFor(
      () => {
        expect(screen.queryByText('正在启动 DeepSeek 工作区…')).not.toBeInTheDocument()
      },
      { timeout: 3_000 },
    )
    expect(screen.getByRole('radio', { name: 'DeepSeek' })).toHaveAttribute('aria-checked', 'true')
    expect(screen.queryByText('DeepSeek 工作区不可用')).not.toBeInTheDocument()
    // 启动器内容隐藏
    expect(screen.queryByRole('tab', { name: '服务' })).not.toBeInTheDocument()
  })

  it('连续点击 DeepSeek 幂等:不重复触发启动流程', async () => {
    window.history.replaceState(null, '', '/')
    render(<App />)
    const user = userEvent.setup()
    await screen.findByText('DeepSeek Harness')
    const openWorkspace = vi.spyOn(mockApi, 'openDshWorkspace')

    const seg = screen.getByRole('radio', { name: 'DeepSeek' })
    await user.click(seg)
    await user.click(seg)
    await user.click(seg)
    // 幂等:已就绪后重复点击不再发起
    await waitFor(
      () => {
        expect(screen.getByRole('radio', { name: 'DeepSeek' })).toHaveAttribute('aria-checked', 'true')
      },
      { timeout: 3_000 },
    )
    await user.click(screen.getByRole('radio', { name: 'DeepSeek' }))
    const callsAfterReady = openWorkspace.mock.calls.length
    await user.click(screen.getByRole('radio', { name: 'DeepSeek' }))
    // 就绪后再点不应新增调用(Rust 侧幂等;mock 直接短路)
    expect(openWorkspace.mock.calls.length).toBe(callsAfterReady)
  })
})
