// dsh-launcher · UI 回归:工具链页面产品语义
//
// 覆盖:当前生效工具链(版本/来源/路径/检测状态) + 可选托管工具链(catalog 次要区域)。
// 1) 系统工具完整但没有托管工具 → 显示系统版本而非「未安装」;
// 2) 托管工具启用 → 显示托管版本与 SHA-256 校验状态;
// 3) 系统版本不兼容 → 明确提示并推荐托管版本;
// 4) 所有来源都不存在 → 才显示「未安装」。
import { describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { EnvPage } from '@/components/env/env-page'
import { ToastProvider } from '@/components/ui/toast'
import { mockApi } from '@/lib/mock'
import type { AppSnapshot, EnvironmentSnapshot, InstallationSnapshot } from '@/types/schema'

const baseSnap: AppSnapshot = {
  version: '0.4.0', state: 'idle', mode: 'none', phase: '', error: null,
  url: null, webPid: null, devPid: null, startedAt: null, readyAt: null,
  hmrActive: false, busy: false, launcherPid: 1,
  repo: { branch: 'main', head: 'abc', behind: 0, ahead: 0, dirty: false, dirtyFiles: 0, syncAt: null, remoteUpToDate: true },
  update: { mode: null, checking: false, available: false, version: null, url: null, size: null, notes: null, message: null, error: null, installing: false, progress: null },
  operation: null,
  disabledActions: [],
}

function renderEnv(env: EnvironmentSnapshot, inst: InstallationSnapshot) {
  vi.spyOn(mockApi, 'inspectEnvironment').mockResolvedValue(env)
  vi.spyOn(mockApi, 'getInstallationSnapshot').mockResolvedValue(inst)
  const onInstallToolchain = vi.fn()
  render(
    <ToastProvider>
      <EnvPage
        snap={baseSnap}
        onInstallNode={vi.fn()}
        onInstallGit={vi.fn()}
        onInstallPnpm={vi.fn()}
        onInstallToolchain={onInstallToolchain}
      />
    </ToastProvider>,
  )
  return { onInstallToolchain }
}

function missingTool(): EnvironmentSnapshot['node'] {
  return { version: null, source: null, path: null, status: 'missing', verified: false, hint: null, managedAvailable: true }
}

function emptyInst(offered: { node: string; git: string | null; pnpm: string }): InstallationSnapshot {
  return { catalogVersion: 1, node: null, git: null, pnpm: null, installedAt: null, offered }
}

describe('工具链页面:当前实际生效为主', () => {
  it('系统工具完整但没有托管工具 → 显示系统版本而非「未安装」', async () => {
    const env: EnvironmentSnapshot = {
      repoPath: '/Users/x/deepseek-harness',
      repoUsable: { ok: true },
      distBuilt: true,
      platform: 'macos',
      node: { version: 'v24.19.0', source: 'system', path: '/usr/local/bin/node', status: 'detected', verified: false, hint: null, managedAvailable: true },
      pnpm: { version: '11.7.0', source: 'system', path: '/usr/local/bin/pnpm', status: 'detected', verified: false, hint: null, managedAvailable: true },
      git: { version: '2.47.0', source: 'system', path: '/usr/bin/git', status: 'detected', verified: false, hint: null, managedAvailable: false },
      warnings: [],
    }
    const inst = emptyInst({ node: 'v24.9.0', git: null, pnpm: '11.7.0' })
    renderEnv(env, inst)

    expect(await screen.findByText('v24.19.0')).toBeInTheDocument()
    expect(screen.getByText('11.7.0')).toBeInTheDocument()
    expect(screen.getByText('2.47.0')).toBeInTheDocument()
    // 系统安装来源 + 自检通过,而不是「未安装」
    expect(screen.getAllByText('系统安装')).toHaveLength(3)
    expect(screen.getAllByText('自检通过')).toHaveLength(3)
    expect(screen.queryByText('未安装')).not.toBeInTheDocument()
    // 系统工具不得显示 SHA-256 已校验(只有托管工具才有该标记)
    expect(screen.queryByText(/SHA-256 已校验/)).not.toBeInTheDocument()
    // 实际路径可见
    expect(screen.getByText('/usr/bin/git')).toBeInTheDocument()
  })

  it('托管工具启用 → 显示托管版本与 SHA-256 校验状态', async () => {
    const env: EnvironmentSnapshot = {
      repoPath: '/Users/x/deepseek-harness',
      repoUsable: { ok: true },
      distBuilt: true,
      platform: 'macos',
      node: { version: 'v24.9.0', source: 'managed', path: '/state/toolchains/node/v24.9.0/bin/node', status: 'detected', verified: true, hint: null, managedAvailable: true },
      pnpm: { version: '11.7.0', source: 'managed', path: '/state/toolchains/pnpm/11.7.0/pnpm', status: 'detected', verified: true, hint: null, managedAvailable: true },
      git: { version: '2.47.0', source: 'system', path: '/usr/bin/git', status: 'detected', verified: false, hint: null, managedAvailable: false },
      warnings: [],
    }
    const inst: InstallationSnapshot = {
      catalogVersion: 1,
      node: { version: 'v24.9.0', path: '/state/toolchains/node/v24.9.0/bin/node', verified: true, source: 'managed' },
      git: null,
      pnpm: { version: '11.7.0', path: '/state/toolchains/pnpm/11.7.0/pnpm', verified: true, source: 'managed' },
      installedAt: Date.now(),
      offered: { node: 'v24.9.0', git: null, pnpm: '11.7.0' },
    }
    renderEnv(env, inst)

    expect(await screen.findByText('v24.9.0')).toBeInTheDocument()
    // 托管来源徽章
    expect(screen.getAllByText('Launcher 托管').length).toBeGreaterThanOrEqual(2)
    // 托管工具显示校验标记(生效行 + 托管行)
    expect(screen.getAllByText(/SHA-256 已校验/).length).toBeGreaterThanOrEqual(2)
    // 托管路径可见
    expect(screen.getByText('/state/toolchains/node/v24.9.0/bin/node')).toBeInTheDocument()
    // 托管行:已安装 + 校验
    expect(screen.getByText('已安装 v11.7.0')).toBeInTheDocument()
  })

  it('系统版本不兼容 → 明确提示并推荐托管版本', async () => {
    const env: EnvironmentSnapshot = {
      repoPath: '/Users/x/deepseek-harness',
      repoUsable: { ok: true },
      distBuilt: true,
      platform: 'macos',
      node: {
        version: 'v23.1.0', source: 'system', path: '/usr/local/bin/node',
        status: 'incompatible', verified: false,
        hint: '系统 Node v23.1.0 不在 dsh 要求范围(^22.19 || >=24);推荐安装托管 Node v24.9.0',
        managedAvailable: true,
      },
      pnpm: { version: '11.7.0', source: 'system', path: '/usr/local/bin/pnpm', status: 'detected', verified: false, hint: null, managedAvailable: true },
      git: { version: '2.47.0', source: 'system', path: '/usr/bin/git', status: 'detected', verified: false, hint: null, managedAvailable: false },
      warnings: ['系统 Node 版本不在 dsh 要求范围(^22.19 || >=24),推荐安装托管 Node'],
    }
    const inst = emptyInst({ node: 'v24.9.0', git: null, pnpm: '11.7.0' })
    renderEnv(env, inst)

    // 明确提示不兼容
    expect(await screen.findByText('版本不兼容')).toBeInTheDocument()
    expect(screen.getAllByText(/不在 dsh 要求范围/).length).toBeGreaterThanOrEqual(1)
    // 推荐托管版本
    expect(screen.getByText(/推荐安装托管 Node v24.9.0/)).toBeInTheDocument()
    // 提供「切换到托管」入口(系统存在 → 切换而非安装)
    expect(screen.getByRole('button', { name: /切换到托管 Node/ })).toBeInTheDocument()
    // 页面状态为异常
    expect(screen.getByText('异常')).toBeInTheDocument()
  })

  it('所有来源都不存在 → 才显示「未安装」', async () => {
    const env: EnvironmentSnapshot = {
      repoPath: '/Users/x/deepseek-harness',
      repoUsable: { ok: true },
      distBuilt: true,
      platform: 'macos',
      node: { ...missingTool(), hint: '未找到 Node;可安装托管 Node v24.9.0(或系统安装 ^22.19 || >=24 的 Node)' },
      pnpm: { ...missingTool(), hint: '未找到 pnpm(系统/托管均无);可安装托管 pnpm 11.7.0' },
      git: { ...missingTool(), hint: '未找到系统 git;请安装 Xcode Command Line Tools 或 Homebrew git' },
      warnings: [
        '未找到 dsh 要求版本(^22.19 || >=24)的 Node;可安装托管 Node',
        '未找到 pnpm(系统/托管均无);「启动/开发模式/更新并构建」需要它,可安装托管 pnpm',
        '未找到 git,「克隆仓库/更新并构建」不可用;macOS/Linux 请安装系统 Git,Windows 可安装托管 MinGit',
      ],
    }
    const inst = emptyInst({ node: 'v24.9.0', git: null, pnpm: '11.7.0' })
    const { onInstallToolchain } = renderEnv(env, inst)

    // 只有全部来源缺失才显示「未安装」(三个主行各一个)
    expect(await screen.findAllByText('未安装')).toHaveLength(3)
    // 缺失推荐
    expect(screen.getByText(/未找到 Node;可安装托管 Node v24.9.0/)).toBeInTheDocument()
    // 托管行给出可安装入口
    expect(screen.getByRole('button', { name: /安装托管 Node/ })).toBeInTheDocument()
    // 主 CTA 改为「安装托管工具链」(不再是「一键安装工具链」)
    const cta = screen.getByRole('button', { name: /安装托管工具链/ })
    expect(cta).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /一键安装工具链/ })).not.toBeInTheDocument()
    cta.click()
    expect(onInstallToolchain).toHaveBeenCalled()
    // catalog 状态位于次要区域
    expect(screen.getByText('catalog v1 · 签名已验证')).toBeInTheDocument()
  })
})
