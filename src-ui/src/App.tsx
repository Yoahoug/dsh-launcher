import { useState } from 'react'
import { Dashboard } from '@/components/dashboard/dashboard'
import { TopBar, type ModeTab } from '@/components/dashboard/topbar'
import { api, useAction, useAppSnapshot } from '@/hooks/use-app'
import type { UiActionName } from '@/types/schema'

function App() {
  const snap = useAppSnapshot()
  const runAction = useAction()
  const [mode, setMode] = useState<ModeTab>('normal')

  const handleAction = (a: UiActionName) => {
    if (a === 'open-dsh') {
      void api.openDsh()
      return
    }
    if (a === 'cancel') return
    void runAction(a)
  }

  return (
    <div className="flex h-full flex-col bg-[var(--background)] text-[var(--foreground)]">
      <TopBar
        snap={snap}
        mode={mode}
        onModeChange={setMode}
        onAction={handleAction}
        onOpenDsh={() => void api.openDsh()}
        onOpenLogs={() => void api.openLogDirectory()}
        onOpenRepo={() => void api.openRepoDirectory()}
        onOpenSettings={() => void api.openLogDirectory()}
      />
      <Dashboard snap={snap} onAction={handleAction} onOpenDsh={() => void api.openDsh()} />
    </div>
  )
}

export default App
