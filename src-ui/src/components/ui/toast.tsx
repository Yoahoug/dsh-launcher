import * as React from 'react'
import { CheckCircle2, XCircle, Info, AlertTriangle, X } from 'lucide-react'
import { cn } from '@/lib/utils'

type ToastKind = 'success' | 'error' | 'info' | 'warning'

export interface ToastItem {
  id: number
  kind: ToastKind
  title: string
  detail?: string
}

interface ToastContextValue {
  toast: (t: Omit<ToastItem, 'id'>) => void
}

const ToastContext = React.createContext<ToastContextValue | null>(null)

const ICONS: Record<ToastKind, React.ReactNode> = {
  success: <CheckCircle2 className="size-4 text-[var(--success)]" />,
  error: <XCircle className="size-4 text-[var(--danger)]" />,
  info: <Info className="size-4 text-[var(--primary)]" />,
  warning: <AlertTriangle className="size-4 text-[var(--warning)]" />,
}

/** Toast 容器 + Provider:动作反馈统一出口。 */
export function ToastProvider({ children }: { children: React.ReactNode }) {
  const [toasts, setToasts] = React.useState<ToastItem[]>([])
  const idRef = React.useRef(0)

  const remove = (id: number) => setToasts((t) => t.filter((x) => x.id !== id))

  const toast = React.useCallback((t: Omit<ToastItem, 'id'>) => {
    const id = ++idRef.current
    setToasts((prev) => [...prev.slice(-3), { ...t, id }])
    window.setTimeout(() => remove(id), t.kind === 'error' ? 6000 : 3500)
  }, [])

  return (
    <ToastContext.Provider value={{ toast }}>
      {children}
      <div className="pointer-events-none fixed right-4 top-4 z-50 flex w-80 flex-col gap-2">
        {toasts.map((t) => (
          <div
            key={t.id}
            role="status"
            className={cn(
              'pointer-events-auto flex items-start gap-2.5 rounded-[var(--radius-control)] border border-[var(--border)] bg-[var(--card)] px-3.5 py-3 shadow-lg',
            )}
          >
            <span className="mt-0.5 shrink-0">{ICONS[t.kind]}</span>
            <div className="min-w-0 flex-1">
              <p className="text-[13px] font-medium leading-snug text-[var(--foreground)]">{t.title}</p>
              {t.detail ? (
                <p className="mt-0.5 break-words text-xs leading-snug text-[var(--muted-foreground)]">{t.detail}</p>
              ) : null}
            </div>
            <button
              onClick={() => remove(t.id)}
              className="shrink-0 rounded p-0.5 text-[var(--muted-foreground)] hover:text-[var(--foreground)]"
              aria-label="关闭提示"
            >
              <X className="size-3.5" />
            </button>
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  )
}

export function useToast(): ToastContextValue {
  const ctx = React.useContext(ToastContext)
  if (!ctx) throw new Error('useToast 必须在 ToastProvider 内使用')
  return ctx
}
