import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  Account,
  CheckUpdate,
  DeviceLoginPoll,
  DeviceLoginStart,
  PatStatus,
  Settings,
  SyncResult,
  Task,
  ViewMode,
} from "./types";

export const SYNCED_EVENT = "taskboard://synced";

export const api = {
  listTasks: (ownership?: string, accountId?: number | null) =>
    invoke<Task[]>("list_tasks", {
      ownership: ownership ?? null,
      accountId: accountId ?? null,
    }),
  syncNow: () => invoke<SyncResult>("sync_now"),
  updateStatus: (key: string, status: string) =>
    invoke<void>("update_task_status", { key, status }),
  recordSession: (key: string, sessionId: string, agent?: string) =>
    invoke<void>("record_session", { key, sessionId, agent: agent ?? null }),
  clearSession: (key: string) => invoke<void>("clear_session", { key }),
  recordHandoff: (key: string, text: string) =>
    invoke<void>("record_handoff", { key, text }),
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (scheduleMinutes: number, ghPath: string) =>
    invoke<Settings>("save_settings", { scheduleMinutes, ghPath }),
  openInBrowser: (url: string) => invoke<void>("open_in_browser", { url }),
  // v0.3.15+：GitHub PAT 相关命令（保留兼容，新版用账号管理）。
  savePat: (pat: string) => invoke<PatStatus>("save_pat", { pat }),
  testPat: () => invoke<PatStatus>("test_pat"),
  clearPat: () => invoke<PatStatus>("clear_pat"),
  // v0.3.16+：多账号管理。
  listAccounts: () => invoke<Account[]>("list_accounts"),
  addAccount: (label: string, login: string, org: string, pat: string) =>
    invoke<Account>("add_account", { label, login, org, pat }),
  updateAccount: (
    id: number,
    label: string | null,
    login: string | null,
    org: string | null,
    pat: string | null,
  ) => invoke<Account>("update_account", { id, label, login, org, pat }),
  deleteAccount: (id: number) => invoke<void>("delete_account", { id }),
  testAccountPat: (id: number) => invoke<PatStatus>("test_account_pat", { id }),
  setDefaultAccount: (id: number) =>
    invoke<void>("set_default_account", { id }),
  setActiveAccount: (id: number) =>
    invoke<void>("set_active_account", { id }),
  setViewMode: (mode: ViewMode) => invoke<void>("set_view_mode", { mode }),
  // v0.3.17+：GitHub OAuth Device Flow 登录（token 不回流前端，成功即建账号）。
  saveOauthClientId: (clientId: string) =>
    invoke<void>("save_oauth_client_id", { clientId }),
  deviceLoginStart: (clientId: string) =>
    invoke<DeviceLoginStart>("device_login_start", { clientId }),
  deviceLoginPoll: (clientId: string, deviceCode: string, org: string, label: string) =>
    invoke<DeviceLoginPoll>("device_login_poll", { clientId, deviceCode, org, label }),
  // v0.3.19+：关于页面 —— 当前版本 + 检查更新。
  getAppVersion: () => invoke<string>("get_app_version"),
  checkLatestRelease: () =>
    invoke<CheckUpdate>("check_latest_release"),
};

export function onSynced(cb: (r: SyncResult) => void) {
  return listen<SyncResult>(SYNCED_EVENT, (e) => cb(e.payload));
}
