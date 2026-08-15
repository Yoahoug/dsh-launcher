// dsh-launcher · 标题栏拖动 hook 回归
// - 拖动只从非交互区域触发(空白标题栏);
// - 按钮/链接/输入框/显式禁用的 drag region 不触发拖动;
// - 双击空白区域切换最大化/还原;
// - 非主键(右键等)不触发。
import { useRef } from 'react'
import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest'
import { render } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { isDragInteractive, useTitleBarDrag, DOUBLE_CLICK_MS } from '@/hooks/use-titlebar-drag'
import { isTauri } from '@/lib/desktop-api'

const { mockWindow } = vi.hoisted(() => ({
  mockWindow: {
    startDragging: vi.fn().mockResolvedValue(undefined),
    isMaximized: vi.fn().mockResolvedValue(false),
    maximize: vi.fn().mockResolvedValue(undefined),
    unmaximize: vi.fn().mockResolvedValue(undefined),
  },
}))

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => mockWindow,
}))

function Harness() {
  const ref = useRef<HTMLDivElement | null>(null)
  useTitleBarDrag(ref)
  return (
    <div ref={ref} data-testid="titlebar">
      <div className="blank" data-testid="blank" />
      <button type="button" data-testid="btn">
        按钮
      </button>
      <a href="#" data-testid="link">
        链接
      </a>
      <input data-testid="input" />
      <div data-tauri-drag-region="false" data-testid="disabled-region" />
    </div>
  )
}

describe('isDragInteractive', () => {
  it('交互元素与显式禁用的 drag region 不可拖动', () => {
    document.body.innerHTML = `
      <div id="bar">
        <button id="btn"></button>
        <a id="link" href="#"></a>
        <input id="input" />
        <div id="disabled" data-tauri-drag-region="false"></div>
        <div id="plain"></div>
      </div>`
    expect(isDragInteractive(document.getElementById('btn'))).toBe(true)
    expect(isDragInteractive(document.getElementById('link'))).toBe(true)
    expect(isDragInteractive(document.getElementById('input'))).toBe(true)
    expect(isDragInteractive(document.getElementById('disabled'))).toBe(true)
    expect(isDragInteractive(document.getElementById('plain'))).toBe(false)
    expect(isDragInteractive(null)).toBe(false)
  })
})

describe('useTitleBarDrag', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    ;(window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {}
  })

  afterEach(() => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__
  })

  it('空白区域主键按下 → startDragging', async () => {
    expect(isTauri()).toBe(true) // 已注入 __TAURI_INTERNALS__ → hook 生效
    render(<Harness />)
    const blank = document.querySelector('[data-testid="blank"]')!
    await userEvent.pointer({ keys: '[MouseLeft]', target: blank })
    expect(mockWindow.startDragging).toHaveBeenCalledOnce()
  })

  it('按钮/链接/输入框按下不触发拖动', async () => {
    render(<Harness />)
    const btn = document.querySelector('[data-testid="btn"]')!
    const link = document.querySelector('[data-testid="link"]')!
    const input = document.querySelector('[data-testid="input"]')!
    const disabled = document.querySelector('[data-testid="disabled-region"]')!
    await userEvent.pointer({ keys: '[MouseLeft]', target: btn })
    await userEvent.pointer({ keys: '[MouseLeft]', target: link })
    await userEvent.pointer({ keys: '[MouseLeft]', target: input })
    await userEvent.pointer({ keys: '[MouseLeft]', target: disabled })
    expect(mockWindow.startDragging).not.toHaveBeenCalled()
  })

  it('非主键(右键)按下不触发拖动', async () => {
    render(<Harness />)
    const blank = document.querySelector('[data-testid="blank"]')!
    await userEvent.pointer({ keys: '[MouseRight]', target: blank })
    expect(mockWindow.startDragging).not.toHaveBeenCalled()
  })

  it('双击空白区域切换最大化/还原', async () => {
    render(<Harness />)
    const blank = document.querySelector('[data-testid="blank"]')!
    mockWindow.isMaximized.mockResolvedValueOnce(false)
    await userEvent.pointer({ keys: '[MouseLeft]', target: blank })
    await userEvent.pointer({ keys: '[MouseLeft]', target: blank })
    expect(mockWindow.maximize).toHaveBeenCalledOnce()
    expect(mockWindow.unmaximize).not.toHaveBeenCalled()

    // 第二次双击:已最大化 → unmaximize
    mockWindow.isMaximized.mockResolvedValueOnce(true)
    await userEvent.pointer({ keys: '[MouseLeft]', target: blank })
    await userEvent.pointer({ keys: '[MouseLeft]', target: blank })
    expect(mockWindow.unmaximize).toHaveBeenCalledOnce()
  })

  it('超过双击间隔的两次点击各自只触发拖动', async () => {
    render(<Harness />)
    const blank = document.querySelector('[data-testid="blank"]')!
    await userEvent.pointer({ keys: '[MouseLeft]', target: blank })
    await new Promise((r) => setTimeout(r, DOUBLE_CLICK_MS + 50))
    await userEvent.pointer({ keys: '[MouseLeft]', target: blank })
    expect(mockWindow.startDragging).toHaveBeenCalledTimes(2)
    expect(mockWindow.maximize).not.toHaveBeenCalled()
  })
})
