export type StatusKey = "todo" | "doing" | "processed" | "done";
export type Ownership = "assigned" | "notassignee" | "assigned-others";

export interface Task {
  key: string;
  owner: string;
  repo: string;
  number: number;
  title: string;
  url: string;
  ghState: string;
  ownership: Ownership;
  status: StatusKey;
  ghStatus: string;
  assignees: string;
  mentioned: boolean;
  latestCommentUrl: string;
  prNumber: number;
  prUrl: string;
  branch: string;
  sessionId: string | null;
  sessionAgent: string | null;
  sessionAt: number | null;
  candidateDone: boolean;
  handoff: string;
  updatedAt: string | null;
}

export interface SyncResult {
  total: number;
  added: number;
  updated: number;
  candidateDone: number;
  removed: number;
  pruned: number;
  warning: string;
  syncedAt: number;
}

export interface Settings {
  scheduleMinutes: number;
  ghPath: string;
  login: string;
  org: string;
  lastSyncAt: number;
  dbPath: string;
}

export const COLUMNS: { key: StatusKey; label: string; hint: string }[] = [
  { key: "todo", label: "待处理", hint: "已拉入看板，尚未开始" },
  { key: "doing", label: "处理中", hint: "正在处理，可记录中断会话" },
  { key: "processed", label: "已处理", hint: "做完了，待确认收尾" },
  { key: "done", label: "已完成", hint: "确认完成，归档" },
];

export const OWNERSHIP_LABEL: Record<Ownership, string> = {
  assigned: "分配给我",
  notassignee: "无人认领",
  "assigned-others": "分配给他人",
};
