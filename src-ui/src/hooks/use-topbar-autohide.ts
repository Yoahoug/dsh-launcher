// dsh-launcher · 顶部栏全屏自动隐藏 hook
//
// 需求(UI 优化 1):全屏 + DeepSeek 工作区时,顶部菜单栏默认收起,
// 让 DeepSeek 子 WebView 真正全屏;鼠标悬浮到窗口顶部才重新显示。
// - 全屏检测:Tauri 全屏切换伴随窗口 resize,在初始与每次 resize 时复查 isFullscreen;
// - 光标跟踪:子 WebView 是原生视图,鼠标在其上时主 WebView 收不到 pointer/mouse 事件,
//   因此用命令轮询原生 cursor_position(逻辑坐标),带滞回阈值避免抖动;
// - 原生联动:隐藏状态同步给 Rust(set_topbar_hidden),子 WebView 扩展为全窗;
// - 仅 Tauri 生效;浏览器预览(mock)为 no-op。
import { useEffect, useState } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { api, isTauri } from '@/hooks/use-app'
import type { Workspace } from '@/types/schema'

/** 光标进入该逻辑 Y 阈值内 → 显示顶部栏。 */
export const SHOW_CURSOR_Y = 3
/** 光标离开该逻辑 Y 阈值后 → 隐藏顶部栏。 */
export const HIDE_CURSOR_Y = 72
/** 光标轮询间隔(ms)。 */
export const POLL_MS = 120

/** 纯函数:给定当前隐藏状态与光标逻辑 Y,返回下一隐藏状态(滞回,防抖动)。 */
export function nextHidden(hidden: boolean, cursorY: number): boolean {
  if (cursorY <= SHOW_CURSOR_Y) return false
  if (cursorY >= HIDE_CURSOR_Y) return true
  return hidden
}

export function useTopbarAutohide(workspace: Workspace) {
  const [fullscreen, setFullscreen] = useState(false)
  const [hidden, setHidden] = useState(false)

  // 全屏状态:初始 + 每次窗口 resize(全屏切换会触发 resize)
  useEffect(() => {
    if (!isTauri()) return
    let disposed = false
    const win = getCurrentWindow()
    const check = async () => {
      try {
        const fs = await win.isFullscreen()
        if (!disposed) setFullscreen(fs)
      } catch {
        /* 忽略:保持现状 */
      }
    }
    void check()
    let unlisten: UnlistenFn | null = null
    void win.onResized(() => void check()).then((u) => {
      if (disposed) u()
      else unlisten = u
    })
    return () => {
      disposed = true
      unlisten?.()
    }
  }, [])

  const autoHide = isTauri() && fullscreen && workspace === 'dsh'

  // 光标轮询:全屏 + DeepSeek 工作区时,光标贴顶显示、离开隐藏
  useEffect(() => {
    if (!autoHide) {
      setHidden(false)
      return
    }
    let disposed = false
    let timer = 0
    const tick = async () => {
      if (disposed) return
      try {
        const pos = await api.getCursorPosition()
        if (disposed || !pos) return
        setHidden((prev) => nextHidden(prev, pos[1]))
      } catch {
        /* 获取失败保持现状 */
      }
      if (!disposed) timer = window.setTimeout(tick, POLL_MS)
    }
    timer = window.setTimeout(tick, POLL_MS)
    return () => {
      disposed = true
      window.clearTimeout(timer)
    }
  }, [autoHide])

  // 同步给原生侧:隐藏时 DeepSeek 子 WebView 扩展为全窗(真全屏)
  useEffect(() => {
    if (!isTauri()) return
    void api.setTopbarHidden(autoHide && hidden)
  }, [autoHide, hidden])

  return { fullscreen, hidden: autoHide && hidden }
}
