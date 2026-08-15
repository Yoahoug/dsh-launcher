// dsh-launcher · DesktopApi:渲染进程唯一 IPC 入口
// 铁律:renderer 绝不直接 fetch 3090 或创建 EventSource;所有数据经 Tauri invoke/event。
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { UnlistenFn } from '@tauri-apps/api/event'
import {
  EVENTS,
  type ActionAccepted,
  type ActionName,
  type AppSnapshot,
  type DesktopPreferences,
  type DesktopSnapshot,
  type EnvironmentSnapshot,
  type LogEntry,
  type LogPage,
  type PageName,
  type SettingsSnapshot,
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
  inspectEnvironment(): Promise<EnvironmentSnapshot>
  getDesktopSnapshot(): Promise<DesktopSnapshot>
  savePreferences(preferences: DesktopPreferences): Promise<DesktopPreferences>
  checkForUpdate(): Promise<AppSnapshot['update']>
  applyUpdate(): Promise<ActionAccepted>
  openDsh(): Promise<void>
  openRepoDirectory(): Promise<void>
  openLogDirectory(): Promise<void>
  pickDirectory(): Promise<string | null>
  quitApp(): Promise<void>
  onStateChanged(cb: (snapshot: AppSnapshot) => void): Promise<UnlistenFn>
  onLogAppended(cb: (entry: LogEntry) => void): Promise<UnlistenFn>
  onOpenPage(cb: (page: PageName) => void): Promise<UnlistenFn>
  onPreferencesChanged(cb: (prefs: DesktopPreferences) => void): Promise<UnlistenFn>
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
  inspectEnvironment: () => invoke('inspect_environment'),
  getDesktopSnapshot: () => invoke('get_desktop_snapshot'),
  savePreferences: (preferences) => invoke('save_preferences', { preferences }),
  checkForUpdate: () => invoke('check_for_update'),
  applyUpdate: () => invoke('apply_update'),
  openDsh: () => invoke('open_dsh'),
  openRepoDirectory: () => invoke('open_repo_directory'),
  openLogDirectory: () => invoke('open_log_directory'),
  pickDirectory: () => invoke('pick_directory'),
  quitApp: () => invoke('quit_app'),
  onStateChanged: (cb) => listen<AppSnapshot>(EVENTS.STATE_CHANGED, (e) => cb(e.payload)),
  onLogAppended: (cb) => listen<LogEntry>(EVENTS.LOG_APPENDED, (e) => cb(e.payload)),
  onOpenPage: (cb) => listen<PageName>(EVENTS.OPEN_PAGE, (e) => cb(e.payload)),
  onPreferencesChanged: (cb) =>
    listen<DesktopPreferences>(EVENTS.PREFERENCES_CHANGED, (e) => cb(e.payload)),
}
