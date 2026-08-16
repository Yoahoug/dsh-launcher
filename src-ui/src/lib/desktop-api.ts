// dsh-launcher · DesktopApi:渲染进程唯一 IPC 入口
// 铁律:renderer 绝不直接 fetch 3090 或创建 EventSource;所有数据经 Tauri invoke/event。
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { UnlistenFn } from '@tauri-apps/api/event'
import {
  EVENTS,
  type ActionAccepted,
  type ChatStateSnapshot,
  type ActionName,
  type AppSnapshot,
  type CloneDialogData,
  type CloneRequest,
  type CloneState,
  type DesktopPreferences,
  type DesktopSnapshot,
  type DshViewSnapshot,
  type EnvironmentSnapshot,
  type InstallationSnapshot,
  type LogEntry,
  type LogPage,
  type PageName,
  type PatchWriteResult,
  type PerfMark,
  type PluginsSnapshot,
  type SettingsSnapshot,
  type SkillSummary,
  type SkillsSnapshot,
  type UpdateCheckResult,
  type Workspace,
} from '@/types/schema'

/** 是否运行在 Tauri WebView 内(否则进入浏览器预览 mock)。 */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

export interface DesktopApi {
  getAppSnapshot(): Promise<AppSnapshot>
  runAction(action: ActionName): Promise<ActionAccepted>
  confirmAndRun(action: ActionName | 'stop-and-quit'): Promise<ActionAccepted>
  getLogs(sinceId?: number): Promise<LogPage>
  clearLogs(): Promise<void>
  getSettings(): Promise<SettingsSnapshot>
  saveSettings(patch: Partial<SettingsSnapshot>): Promise<SettingsSnapshot>
  /** 环境检测(带文件缓存;force=true 强制重新探测)。 */
  inspectEnvironment(force?: boolean): Promise<EnvironmentSnapshot>
  getDesktopSnapshot(): Promise<DesktopSnapshot>
  savePreferences(preferences: DesktopPreferences): Promise<DesktopPreferences>
  /** 完成/跳过首次运行引导(skip=true 跳过;提供 repoPath 时一并保存)。 */
  completeFirstRun(skip: boolean, repoPath?: string): Promise<DesktopSnapshot>
  /** 顶部栏隐藏状态(全屏 + DeepSeek 工作区自动隐藏 chrome 时同步给原生侧)。 */
  setTopbarHidden(hidden: boolean): Promise<void>
  /** 光标相对主窗口客户区的位置(逻辑坐标 [x, y];null = 无法获取)。 */
  getCursorPosition(): Promise<[number, number] | null>
  checkForUpdate(): Promise<UpdateCheckResult>
  applyUpdate(): Promise<ActionAccepted>
  openDsh(): Promise<void>
  /** M3:打开内嵌 chat WebView(零权限;服务未就绪时先启动)。 */
  openChat(): Promise<ChatStateSnapshot>
  closeChat(): Promise<void>
  getChatState(): Promise<ChatStateSnapshot>
  // M4.1:主窗口内 DeepSeek 工作区(dsh-content 子 WebView)
  openDshWorkspace(): Promise<DshViewSnapshot>
  backToLauncher(): Promise<DshViewSnapshot>
  retryDshView(): Promise<DshViewSnapshot>
  setWorkspace(workspace: Workspace): Promise<DshViewSnapshot>
  getDshViewState(): Promise<DshViewSnapshot>
  openRepoDirectory(): Promise<void>
  openLogDirectory(): Promise<void>
  pickDirectory(): Promise<string | null>
  quitApp(): Promise<void>
  // M1:Clone 弹窗 + 托管工具链
  openCloneDialog(): Promise<CloneDialogData>
  submitCloneRequest(request: CloneRequest, full: boolean): Promise<ActionAccepted>
  getCloneState(): Promise<CloneState>
  getInstallationSnapshot(): Promise<InstallationSnapshot>
  // M0:性能测量
  perfMark(name: string): Promise<void>
  getPerfMetrics(): Promise<PerfMark[]>
  // M3:chat WebView(事件驱动,无命令)
  onStateChanged(cb: (snapshot: AppSnapshot) => void): Promise<UnlistenFn>
  onLogAppended(cb: (entry: LogEntry) => void): Promise<UnlistenFn>
  onOpenPage(cb: (page: PageName) => void): Promise<UnlistenFn>
  onPreferencesChanged(cb: (prefs: DesktopPreferences) => void): Promise<UnlistenFn>
  onPerfMetrics(cb: (marks: PerfMark[]) => void): Promise<UnlistenFn>
  // M4.1:DeepSeek 工作区状态事件
  onDshViewState(cb: (snapshot: DshViewSnapshot) => void): Promise<UnlistenFn>
  // M5:插件管理子界面
  pluginsGetSnapshot(profile?: string): Promise<PluginsSnapshot>
  pluginsSetEnabled(profile: string, id: string, enabled: boolean): Promise<PatchWriteResult>
  pluginsSaveConfig(
    profile: string,
    id: string,
    config: Record<string, unknown>,
    rawYaml?: string | null,
  ): Promise<PatchWriteResult>
  pluginsResetRow(profile: string, id: string): Promise<PatchWriteResult>
  pluginsValidatePatch(profile: string): Promise<PatchWriteResult>
  dshctlDumpConfig(profile?: string): Promise<string>
  pluginsOpenInExplorer(absDir: string): Promise<void>
  pluginsInstallPackage(profile: string, absDir: string): Promise<ActionAccepted>
  pluginsRemovePackage(profile: string, name: string): Promise<ActionAccepted>
  // M5:技能管理子界面
  skillsGetSnapshot(): Promise<SkillsSnapshot>
  skillsCreate(
    name: string,
    description: string,
    whenToUse?: string | null,
    body?: string,
  ): Promise<SkillSummary>
  skillsUpdate(
    name: string,
    description: string,
    whenToUse?: string | null,
    body?: string,
  ): Promise<SkillSummary>
  skillsDelete(name: string): Promise<void>
  skillsImport(sourcePath: string, name?: string | null): Promise<SkillSummary>
  skillsPreview(sourcePath: string): Promise<string>
  skillsEnableRoot(profile: string, rootPath: string): Promise<PatchWriteResult>
  onSkillsChanged(cb: () => void): Promise<UnlistenFn>
}

/** Tauri 实现:command 名与 src-tauri/src/commands.rs 对齐。 */
export const desktopApi: DesktopApi = {
  getAppSnapshot: () => invoke('get_app_snapshot'),
  runAction: (action) => invoke('run_action', { action }),
  confirmAndRun: (action) => invoke('confirm_and_run', { action }),
  getLogs: (sinceId = 0) => invoke('get_logs', { sinceId }),
  clearLogs: () => invoke('clear_logs'),
  getSettings: () => invoke('get_settings'),
  saveSettings: (patch) => invoke('save_settings', { patch }),
  inspectEnvironment: (force) => invoke('inspect_environment', { force }),
  getDesktopSnapshot: () => invoke('get_desktop_snapshot'),
  savePreferences: (preferences) => invoke('save_preferences', { preferences }),
  completeFirstRun: (skip, repoPath) => invoke('complete_first_run', { skip, repoPath }),
  setTopbarHidden: (hidden) => invoke('set_topbar_hidden', { hidden }),
  getCursorPosition: () => invoke('get_cursor_position'),
  checkForUpdate: () => invoke('check_for_update'),
  applyUpdate: () => invoke('apply_update'),
  openDsh: () => invoke('open_dsh'),
  openChat: () => invoke('open_chat'),
  closeChat: () => invoke('close_chat'),
  getChatState: () => invoke('get_chat_state'),
  openDshWorkspace: () => invoke('open_dsh_workspace'),
  backToLauncher: () => invoke('back_to_launcher'),
  retryDshView: () => invoke('retry_dsh_view'),
  setWorkspace: (workspace) => invoke('set_workspace', { workspace }),
  getDshViewState: () => invoke('get_dsh_view_state'),
  openRepoDirectory: () => invoke('open_repo_directory'),
  openLogDirectory: () => invoke('open_log_directory'),
  pickDirectory: () => invoke('pick_directory'),
  quitApp: () => invoke('quit_app'),
  openCloneDialog: () => invoke('open_clone_dialog'),
  submitCloneRequest: (request, full) => invoke('submit_clone_request', { request, full }),
  getCloneState: () => invoke('get_clone_state'),
  getInstallationSnapshot: () => invoke('get_installation_snapshot'),
  perfMark: (name) => invoke('perf_mark', { name }),
  getPerfMetrics: () => invoke('get_perf_metrics'),
  onStateChanged: (cb) => listen<AppSnapshot>(EVENTS.STATE_CHANGED, (e) => cb(e.payload)),
  onLogAppended: (cb) => listen<LogEntry>(EVENTS.LOG_APPENDED, (e) => cb(e.payload)),
  onOpenPage: (cb) => listen<PageName>(EVENTS.OPEN_PAGE, (e) => cb(e.payload)),
  onPreferencesChanged: (cb) =>
    listen<DesktopPreferences>(EVENTS.PREFERENCES_CHANGED, (e) => cb(e.payload)),
  onPerfMetrics: (cb) => listen<PerfMark[]>(EVENTS.PERF_METRICS, (e) => cb(e.payload)),
  onDshViewState: (cb) => listen<DshViewSnapshot>(EVENTS.DSH_VIEW_STATE, (e) => cb(e.payload)),
  // M5:插件管理子界面
  pluginsGetSnapshot: (profile) => invoke('plugins_get_snapshot', { profile }),
  pluginsSetEnabled: (profile, id, enabled) =>
    invoke('plugins_set_enabled', { profile, id, enabled }),
  pluginsSaveConfig: (profile, id, config, rawYaml) =>
    invoke('plugins_save_config', { profile, id, config, rawYaml }),
  pluginsResetRow: (profile, id) => invoke('plugins_reset_row', { profile, id }),
  pluginsValidatePatch: (profile) => invoke('plugins_validate_patch', { profile }),
  dshctlDumpConfig: (profile) => invoke('dshctl_dump_config', { profile }),
  pluginsOpenInExplorer: (absDir) => invoke('plugins_open_in_explorer', { absDir }),
  pluginsInstallPackage: (profile, absDir) =>
    invoke('plugins_install_package', { profile, absDir }),
  pluginsRemovePackage: (profile, name) => invoke('plugins_remove_package', { profile, name }),
  // M5:技能管理子界面
  skillsGetSnapshot: () => invoke('skills_get_snapshot'),
  skillsCreate: (name, description, whenToUse, body) =>
    invoke('skills_create', { name, description, whenToUse, body }),
  skillsUpdate: (name, description, whenToUse, body) =>
    invoke('skills_update', { name, description, whenToUse, body }),
  skillsDelete: (name) => invoke('skills_delete', { name }),
  skillsImport: (sourcePath, name) => invoke('skills_import', { sourcePath, name }),
  skillsPreview: (sourcePath) => invoke('skills_preview', { sourcePath }),
  skillsEnableRoot: (profile, rootPath) => invoke('skills_enable_root', { profile, rootPath }),
  onSkillsChanged: (cb) => listen(EVENTS.SKILLS_CHANGED, () => cb()),
}
