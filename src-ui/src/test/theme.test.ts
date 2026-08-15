// dsh-launcher · UI 回归:主题应用(theme.ts 纯逻辑)
import { describe, expect, it } from 'vitest'
import { applyTheme } from '@/lib/theme'

describe('applyTheme', () => {
  it('light:移除 dark 类并写入 data-theme', () => {
    applyTheme('light')
    expect(document.documentElement.classList.contains('dark')).toBe(false)
    expect(document.documentElement.getAttribute('data-theme')).toBe('light')
  })

  it('dark:添加 dark 类并写入 data-theme', () => {
    applyTheme('dark')
    expect(document.documentElement.classList.contains('dark')).toBe(true)
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark')
  })

  it('system:移除 data-theme 并按系统匹配', () => {
    document.documentElement.classList.add('dark')
    applyTheme('system')
    expect(document.documentElement.getAttribute('data-theme')).toBeNull()
    // jsdom matchMedia 固定返回 matches=false → 浅色
    expect(document.documentElement.classList.contains('dark')).toBe(false)
  })
})
