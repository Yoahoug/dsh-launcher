// dsh-launcher · 主题管理:system/light/dark + 系统监听
import type { Theme } from '@/types/schema'

const mq = typeof window !== 'undefined'
  ? window.matchMedia('(prefers-color-scheme: dark)')
  : null

/** 应用主题到 DOM。system 时跟随系统并监听变化。 */
export function applyTheme(theme: Theme): () => void {
  const html = document.documentElement
  const sync = () => {
    if (theme === 'light') {
      html.classList.remove('dark')
      html.setAttribute('data-theme', 'light')
    } else if (theme === 'dark') {
      html.classList.add('dark')
      html.setAttribute('data-theme', 'dark')
    } else {
      html.removeAttribute('data-theme')
      html.classList.toggle('dark', mq?.matches ?? false)
    }
  }
  sync()
  if (theme === 'system' && mq) {
    const h = () => sync()
    mq.addEventListener('change', h)
    return () => mq.removeEventListener('change', h)
  }
  return () => {}
}
