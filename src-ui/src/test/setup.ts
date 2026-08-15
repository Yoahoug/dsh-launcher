import '@testing-library/jest-dom/vitest'
import { afterEach, vi } from 'vitest'
import { cleanup } from '@testing-library/react'
import { __resetDshView } from '@/lib/mock'

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  // 重置 mock 工作区状态(workspace/status/pendingEnter),避免跨测试泄漏
  __resetDshView()
})

// jsdom 缺少 matchMedia:主题 hook 需要
Object.defineProperty(window, 'matchMedia', {
  writable: true,
  value: vi.fn().mockImplementation((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })),
})

// 剪贴板 mock
Object.assign(navigator, {
  clipboard: {
    writeText: vi.fn().mockResolvedValue(undefined),
  },
})
