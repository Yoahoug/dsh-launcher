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
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Cpu className="size-4 text-[var(--primary)]" />
          运行环境
        </CardTitle>
        <Badge variant={env?.warnings?.length ? 'danger' : 'success'}>{env?.warnings?.length ? '异常' : '正常'}</Badge>
      </CardHeader>
      <CardContent>
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-sm text-[var(--muted-foreground)]">
          <span className="font-mono text-[var(--foreground)]">{nodeLabel}</span>
          <span>·</span>
          <span className="font-mono">{env?.pnpm ? `pnpm ${env.pnpm}` : 'pnpm 缺失'}</span>
          <span>·</span>
          <span className="font-mono">{env?.git ? `git ${env.git}` : 'git 缺失'}</span>
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
