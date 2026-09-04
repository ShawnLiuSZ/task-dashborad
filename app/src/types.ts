export type StatusKey = "todo" | "doing" | "processed" | "done";
export type Ownership = "assigned" | "notassignee" | "assigned-others";
/** v0.3.16+：视图模式。`single`=仅显示激活账号任务；`all`=显示所有账号任务。 */
export type ViewMode = "single" | "all";

export interface Account {
  id: number;
  label: string;
  login: string;
  org: string;
  /** 是否已配置 PAT（不回显 token 本体）。 */
  hasPat: boolean;
  isDefault: boolean;
  createdAt: number;
}

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
  /** v0.3.16+：归属账号 id（指向 accounts.id）。 */
  accountId: number;
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
  /** 历史字段：保留兼容（v0.3.15 起不再用于任何路径，仅显示）。 */
  ghPath: string;
  login: string;
  org: string;
  lastSyncAt: number;
  dbPath: string;
  /** v0.3.15+：是否已配置 PAT（不回显 token 本体）。 */
  hasPat: boolean;
  /** v0.3.15+：最近一次同步的错误信息，用作 banner 展示。 */
  lastSyncError: string;
  /** v0.3.16+：当前激活账号 id（单账号视图下同步此账号）。 */
  activeAccountId: number;
  /** v0.3.16+：视图模式。 */
  viewMode: ViewMode;
  /** v0.3.16+：所有账号列表（不含 PAT 本体）。 */
  accounts: Account[];
  /** v0.3.17+：GitHub OAuth Device Flow 的 client_id（注册 OAuth App 后填一次）。 */
  oauthClientId: string;
}

/** v0.3.15+：`save_pat` / `test_pat` 命令的返回值。 */
export interface PatStatus {
  login: string;
  hasPat: boolean;
}

/** v0.3.17+：Device Flow 第 1 步返回（申请设备码成功）。 */
export interface DeviceLoginStart {
  deviceCode: string;
  userCode: string;
  verificationUri: string;
  /** 已预填 user_code 的完整 URL；直接打开可免手动输码。 */
  verificationUriComplete: string;
  /** user_code 有效期（秒），通常 900。 */
  expiresIn: number;
  /** 建议轮询间隔（秒），通常 5。 */
  interval: number;
}

/** v0.3.17+：Device Flow 第 2 步单次轮询结果。 */
export interface DeviceLoginPoll {
  /** pending | slow_down | success | error */
  status: "pending" | "slow_down" | "success" | "error";
  /** 成功时填：授权账号的 GitHub login。 */
  login: string;
  /** 成功时填：新建/更新的账号 id。 */
  accountId: number;
  /** 提示或错误信息。 */
  message: string;
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
