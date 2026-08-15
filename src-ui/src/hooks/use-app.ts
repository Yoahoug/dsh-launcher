import { useCallback, useEffect, useRef, useState } from 'react'
import { desktopApi, isTauri } from '@/lib/desktop-api'
import { mockApi } from '@/lib/mock'
import type {
  AppSnapshot,
  DesktopPreferences,
  DesktopSnapshot,
  DshViewSnapshot,
  PageName,
} from '@/types/schema'

const api = isTauri() ? desktopApi : mockApi

/** 快照数据源:首次拉取 + 订阅 state-changed 事件。 */
export function useAppSnapshot(): AppSnapshot | null {
  const [snap, setSnap] = useState<AppSnapshot | null>(null)
  const mounted = useRef(true)

  useEffect(() => {
    mounted.current = true
    void api.getAppSnapshot().then((s) => {
      if (mounted.current) setSnap(s)
    })
    const unsub = api.onStateChanged((s) => {
      if (mounted.current) setSnap(s)
    })
    return () => {
      mounted.current = false
      void unsub.then((fn) => fn())
    }
  }, [])

  return snap
}

/** 桌面信息:偏好 + 首次运行状态(订阅偏好变更)。 */
export function useDesktopSnapshot(): DesktopSnapshot | null {
  const [desktop, setDesktop] = useState<DesktopSnapshot | null>(null)
  const mounted = useRef(true)

  const refresh = useCallback(() => {
    void api.getDesktopSnapshot().then((d) => {
      if (mounted.current) setDesktop(d)
    })
  }, [])

  useEffect(() => {
    mounted.current = true
    refresh()
    const unsubs = [
      api.onPreferencesChanged((prefs: DesktopPreferences) => {
        setDesktop((d) => (d ? { ...d, preferences: prefs } : d))
      }),
      api.onStateChanged(() => refresh()),
    ]
    return () => {
      mounted.current = false
      void Promise.all(unsubs).then((fns) => fns.forEach((fn) => fn()))
    }
  }, [refresh])

  return desktop
}

/** 页面路由:托盘 app://open-page 事件驱动 + 本地 setState。 */
export function usePage(initial: PageName = 'dashboard'): [PageName, (p: PageName) => void] {
  const [page, setPage] = useState<PageName>(initial)

  useEffect(() => {
    const unsub = api.onOpenPage((p) => setPage(p))
    return () => {
      void unsub.then((fn) => fn())
    }
  }, [])

  return [page, setPage]
}

/** DeepSeek 工作区/子 WebView 状态:首次拉取 + 订阅 dsh-view-state 事件。 */
export function useDshViewState(): DshViewSnapshot | null {
  const [dsh, setDsh] = useState<DshViewSnapshot | null>(null)
  const mounted = useRef(true)

  useEffect(() => {
    mounted.current = true
    void api.getDshViewState().then((s) => {
      if (mounted.current) setDsh(s)
    })
    const unsub = api.onDshViewState((s) => {
      if (mounted.current) setDsh(s)
    })
    return () => {
      mounted.current = false
      void unsub.then((fn) => fn())
    }
  }, [])

  return dsh
}

/** 执行后端动作(返回值供调用方提示)。 */
export function useAction() {
  return useCallback((action: Parameters<typeof api.runAction>[0]) => api.runAction(action), [])
}

export { api, isTauri }
