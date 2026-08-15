// dsh-launcher · 标题栏拖动 hook
//
// 不依赖父元素上的 data-tauri-drag-region 穿透子节点:在标题栏容器上直接绑定
// pointerdown,满足以下条件才拖动:
//   - 鼠标主键(e.button === 0);
//   - 目标不是交互元素(button/a/input/select/textarea/label/summary、
//     contenteditable、交互 role、data-tauri-drag-region="false");
// 双击空白区域切换最大化/还原;
// preventDefault 抑制兼容 mousedown,避免与 Tauri 原生 drag 脚本双重触发拖动。
// 仅在 Tauri 环境生效;浏览器预览(mock)中为 no-op。
import { useEffect, useRef } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { isTauri } from '@/lib/desktop-api'

const INTERACTIVE_TAGS = new Set(['A', 'BUTTON', 'INPUT', 'SELECT', 'TEXTAREA', 'LABEL', 'SUMMARY'])
const INTERACTIVE_ROLES = new Set(['button', 'link', 'menuitem', 'tab', 'checkbox', 'radio', 'switch', 'option'])
/** 双击判定间隔(ms)。 */
export const DOUBLE_CLICK_MS = 400

/** 目标(或其祖先)是否属于「不可拖动」的交互元素。纯函数,便于单测。 */
export function isDragInteractive(target: Element | null): boolean {
  let el: Element | null = target
  while (el && el !== document.documentElement) {
    if (el.getAttribute('data-tauri-drag-region') === 'false') return true
    if (el instanceof HTMLElement) {
      if (INTERACTIVE_TAGS.has(el.tagName)) return true
      if (el.getAttribute('contenteditable') === 'true') return true
      const role = el.getAttribute('role')
      if (role && INTERACTIVE_ROLES.has(role)) return true
    }
    el = el.parentElement
  }
  return false
}

/**
 * 标题栏拖动 + 双击最大化。
 * @param ref 标题栏容器(header)引用;事件绑定在容器上,子节点是否拖动由目标判定。
 */
export function useTitleBarDrag(ref: React.RefObject<HTMLElement | null>): void {
  const lastDown = useRef(0)
  const enabled = isTauri()

  useEffect(() => {
    const el = ref.current
    if (!el || !enabled) return

    const onPointerDown = (e: PointerEvent) => {
      if (e.button !== 0) return
      if (isDragInteractive(e.target as Element | null)) return
      e.preventDefault() // 抑制兼容 mousedown → Tauri 原生脚本不会重复 startDragging
      const now = Date.now()
      if (now - lastDown.current < DOUBLE_CLICK_MS) {
        lastDown.current = 0
        const win = getCurrentWindow()
        void (async () => {
          if (await win.isMaximized()) {
            void win.unmaximize()
          } else {
            void win.maximize()
          }
        })()
        return
      }
      lastDown.current = now
      void getCurrentWindow().startDragging()
    }

    el.addEventListener('pointerdown', onPointerDown)
    return () => el.removeEventListener('pointerdown', onPointerDown)
  }, [ref, enabled])
}
