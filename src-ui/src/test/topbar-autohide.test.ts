// dsh-launcher · 顶部栏全屏自动隐藏决策(纯函数)
import { describe, expect, it } from 'vitest'
import { HIDE_CURSOR_Y, SHOW_CURSOR_Y, nextHidden } from '@/hooks/use-topbar-autohide'

describe('nextHidden 滞回决策', () => {
  it('光标贴顶(<= SHOW)时始终显示', () => {
    expect(nextHidden(true, 0)).toBe(false)
    expect(nextHidden(true, SHOW_CURSOR_Y)).toBe(false)
  })

  it('光标离开(>= HIDE)后隐藏', () => {
    expect(nextHidden(false, HIDE_CURSOR_Y)).toBe(true)
    expect(nextHidden(false, 500)).toBe(true)
  })

  it('中间区域保持当前状态(防抖动)', () => {
    expect(nextHidden(true, 20)).toBe(true)
    expect(nextHidden(false, 20)).toBe(false)
  })
})
