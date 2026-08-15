import { useCallback, useEffect, useRef, useState } from 'react'
import { desktopApi, isTauri } from '@/lib/desktop-api'
import { mockApi } from '@/lib/mock'
import type { AppSnapshot } from '@/types/schema'

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

/** 执行后端动作(返回值供调用方提示)。 */
export function useAction() {
  return useCallback((action: Parameters<typeof api.runAction>[0]) => api.runAction(action), [])
}

export { api, isTauri }
