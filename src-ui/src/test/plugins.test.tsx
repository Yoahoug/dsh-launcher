// dsh-launcher · UI 回归:插件管理子界面(卡片化 + 启停 + 配置表单 + 原始 YAML)
import { describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { PluginsPage } from '@/components/plugins/plugins-page'
import { ToastProvider } from '@/components/ui/toast'
import { mockApi } from '@/lib/mock'
import type { PluginsSnapshot } from '@/types/schema'

const snap: PluginsSnapshot = {
  profiles: [
    { name: 'web', bundles: ['@deepseek-ai/dsh-base'], deps: { '@dsh-plugins/web-search-tavily': 'file:/x/packages/web-search-tavily' }, patchOk: true },
  ],
  rows: [
    {
      id: 'web', module: '@deepseek-ai/dsh-web', layer: 'profile-patch',
      layerLabel: '/Users/u/.dsh/profiles/web/cordis.patch.yml', inUserPatch: true,
      enabled: true, config: { searchProvider: 'tavily', timeoutMs: 3000 },
      configSource: 'dump', rawBlock: '- id: web\n  config:\n    searchProvider: tavily\n',
      editable: true, description: null,
    },
    {
      id: 'session-persistence-jsonl', module: '@deepseek-ai/dsh-session-persistence-jsonl',
      layer: 'bundle', layerLabel: '@deepseek-ai/dsh-base', inUserPatch: false,
      enabled: true, config: null, configSource: 'raw-yaml',
      rawBlock: "- id: session-persistence-jsonl\n  config:\n    root: !!js dshHomePath('sessions')\n",
      editable: true, description: null,
    },
    {
      id: 'web-search-deepseek', module: '@deepseek-ai/dsh-web-search-deepseek',
      layer: 'bundle', layerLabel: '@deepseek-ai/dsh-base', inUserPatch: true,
      enabled: false, config: { apiKeyEnv: 'DEEPSEEK_API_KEY' }, configSource: 'dump',
      rawBlock: '- id: web-search-deepseek\n  disabled: true\n',
      editable: true, description: null,
    },
  ],
  packages: [
    {
      dir: 'web-search-tavily', absDir: '/x/packages/web-search-tavily',
      name: '@dsh-plugins/web-search-tavily', version: '0.1.0',
      description: 'Tavily search', isBundle: false, patchFile: null, installedIn: ['web'],
    },
    {
      dir: 'vision-bridge', absDir: '/x/packages/vision-bridge',
      name: '@dsh-plugins/vision-bridge', version: '0.1.0',
      description: 'Vision bridge', isBundle: false, patchFile: null, installedIn: [],
    },
  ],
  profile: 'web',
  dumpError: null,
}

function renderPage() {
  vi.spyOn(mockApi, 'getSettings').mockResolvedValue({
    repoPath: '/Users/u/deepseek-harness', port: 3080, host: '127.0.0.1', dshHome: '',
    autostart: false, openBrowser: true, autoUpdateCheck: true, buildArgs: '',
    readyTimeoutMs: 180_000, startTimeoutMs: 180_000, firstRunSkipped: true,
    profileName: 'web', dshPluginsPath: '/x', externalSkillRoots: [], skillManagedRoot: '',
  })
  vi.spyOn(mockApi, 'pluginsGetSnapshot').mockResolvedValue(snap)
  return render(
    <ToastProvider>
      <PluginsPage />
    </ToastProvider>,
  )
}

describe('插件管理子界面', () => {
  it('列出全部行 + 来源徽标 + 分组计数 + 停用徽章', async () => {
    renderPage()
    // "web" 同时出现在 profile 选择器 option 与卡片名,用 findAllByText
    expect((await screen.findAllByText('web')).length).toBeGreaterThanOrEqual(1)
    expect(screen.getByText('session-persistence-jsonl')).toBeInTheDocument()
    expect(screen.getByText('web-search-deepseek')).toBeInTheDocument()
    // 来源徽标
    expect(screen.getAllByText('用户补丁').length).toBeGreaterThanOrEqual(1)
    expect(screen.getAllByText('组合包').length).toBeGreaterThanOrEqual(2)
    // !!js 行徽章
    expect(screen.getByText('!!js 原始 YAML')).toBeInTheDocument()
    // 已停用徽章(分组标签 + 行徽章)
    expect(screen.getAllByText('已停用').length).toBeGreaterThanOrEqual(2)
    // 分组标签与计数(计数在嵌套 span,用 textContent 断言)
    const allTab = screen.getByRole('button', { name: /全部/ })
    expect(allTab.textContent).toContain('3')
    const disabledTab = screen.getByRole('button', { name: /已停用/ })
    expect(disabledTab.textContent).toContain('1')
    // dsh-plugins 面板
    expect(screen.getByText('dsh-plugins 仓库')).toBeInTheDocument()
    expect(screen.getByText('@dsh-plugins/web-search-tavily')).toBeInTheDocument()
    expect(screen.getByText('已安装到 web')).toBeInTheDocument()
  })

  it('启用开关调用 pluginsSetEnabled', async () => {
    const user = userEvent.setup()
    const spy = vi.spyOn(mockApi, 'pluginsSetEnabled').mockResolvedValue({
      backup: null, ok: true, summary: 'web 已停用(写 profile patch 覆盖)', validated: true, error: null,
    })
    renderPage()
    const switchEl = await screen.findByRole('switch', { name: '启用 web' })
    await user.click(switchEl)
    expect(spy).toHaveBeenCalledWith('web', 'web', false)
  })

  it('展开卡片:表单编辑 + 保存调用 pluginsSaveConfig(整行全量 config)', async () => {
    const user = userEvent.setup()
    const spy = vi.spyOn(mockApi, 'pluginsSaveConfig').mockResolvedValue({
      backup: 'cordis.patch.yml.bak-1', ok: true, summary: 'web 的 config 已固化整行(非深合并)', validated: true, error: null,
    })
    renderPage()
    await user.click(await screen.findByRole('button', { name: '展开 web' }))
    const input = screen.getByLabelText('searchProvider')
    await user.clear(input)
    await user.type(input, 'deepseek-official')
    await user.click(screen.getByRole('button', { name: '保存配置' }))
    expect(spy).toHaveBeenCalledWith('web', 'web', {
      searchProvider: 'deepseek-official',
      timeoutMs: 3000,
    })
  })

  it('raw-yaml 行锁定原始 YAML 编辑(不出现表单字段)', async () => {
    const user = userEvent.setup()
    renderPage()
    await user.click(await screen.findByRole('button', { name: '展开 session-persistence-jsonl' }))
    // 原始 YAML 编辑框存在,表单字段不存在
    expect(screen.getByLabelText('session-persistence-jsonl 原始 YAML')).toBeInTheDocument()
    expect(screen.queryByLabelText('root')).not.toBeInTheDocument()
    // 提示锁定原因
    expect(screen.getByText(/锁定为原始 YAML 模式,禁止表单化/)).toBeInTheDocument()
  })

  it('dsh-plugins 面板:未安装包显示「安装到 web」,点击调用 pluginsInstallPackage', async () => {
    const user = userEvent.setup()
    const spy = vi.spyOn(mockApi, 'pluginsInstallPackage').mockResolvedValue({ ok: true })
    renderPage()
    const installBtn = await screen.findByRole('button', { name: '安装到 web' })
    await user.click(installBtn)
    expect(spy).toHaveBeenCalledWith('web', '/x/packages/vision-bridge')
  })
})
