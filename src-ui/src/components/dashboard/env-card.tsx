import { Cpu, HardDriveDownload } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { api, isTauri } from '@/hooks/use-app'
import { useEffect, useState } from 'react'
import type { EnvironmentSnapshot } from '@/types/schema'

export function EnvCard({ onInstallNode }: { onInstallNode: () => void }) {
  const [env, setEnv] = useState<EnvironmentSnapshot | null>(null)

  useEffect(() => {
    void api.inspectEnvironment().then(setEnv)
  }, [])

  const nodeLabel = env?.node
    ? env.node.inRange
      ? `Node ${env.node.current}`
      : env.node.usedVersion
        ? `Node ${env.node.usedVersion}(${env.node.usedSource})`
        : `Node ${env.node.current}(不支持)`
    : '检测中…'

  return (
    <Card className="h-full">
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <span className="flex size-8 items-center justify-center rounded-xl bg-[var(--primary)]/10">
            <Cpu className="size-4 text-[var(--primary)]" />
          </span>
          运行环境
        </CardTitle>
        <Badge variant={env?.warnings?.length ? 'danger' : 'success'}>{env?.warnings?.length ? '异常' : '正常'}</Badge>
      </CardHeader>
      <CardContent>
        <div className="space-y-1.5 text-sm text-[var(--muted-foreground)]">
          <p className="font-mono text-[var(--foreground)]">{nodeLabel}</p>
          <p className="truncate font-mono text-xs">{env?.pnpm ? `pnpm ${env.pnpm}` : 'pnpm 缺失'} · {env?.git ? `git ${env.git}` : 'git 缺失'}</p>
        </div>
        {env?.warnings?.length ? (
          <p className="mt-2 text-xs leading-relaxed text-[var(--warning)]">{env.warnings[0]}</p>
        ) : null}
        {env?.node && !env.node.inRange && !env.node.usedVersion ? (
          <Button variant="outline" size="sm" className="mt-3" onClick={onInstallNode}>
            <HardDriveDownload /> 一键安装 Node 24 LTS
          </Button>
        ) : null}
        {!isTauri() && <p className="mt-2 text-[11px] text-[var(--muted-foreground)]">(浏览器预览 mock)</p>}
      </CardContent>
    </Card>
  )
}
