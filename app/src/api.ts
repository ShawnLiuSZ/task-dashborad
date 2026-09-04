import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { PatStatus, Settings, SyncResult, Task } from "./types";

export const SYNCED_EVENT = "taskboard://synced";

export const api = {
  listTasks: (ownership?: string) =>
    invoke<Task[]>("list_tasks", { ownership: ownership ?? null }),
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
  // v0.3.15+：GitHub PAT 相关命令。
  savePat: (pat: string) => invoke<PatStatus>("save_pat", { pat }),
  testPat: () => invoke<PatStatus>("test_pat"),
  clearPat: () => invoke<PatStatus>("clear_pat"),
};

export function onSynced(cb: (r: SyncResult) => void) {
  return listen<SyncResult>(SYNCED_EVENT, (e) => cb(e.payload));
}
