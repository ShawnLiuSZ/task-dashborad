# GitHub API 与本地看板状态的架构说明

> **核心原则**：本项目的看板执行状态完全存储在本地 SQLite，**绝不写回 GitHub**。GitHub 仅作为任务来源（只读）。

---

## 1. 数据流向总览

```
GitHub (只读)          本地 SQLite (读写)          用户界面 / MCP
─────────────────      ─────────────────────      ─────────────
Search API             tasks 表                    Board 列视图
  ├─ org:xxx           ├─ status (四态/项目态)      ├─ 拖拽移动卡片
  ├─ involves:login    ├─ gh_status (GitHub原文)    ├─ 点击切换状态
  ├─ assignee:login    ├─ ownership                └─ 记录 session
  ├─ author:login      ├─ session_id
  ├─ mentions:login    ├─ handoff
  └─ commenter:login   ├─ updated_at
                       └─ synced_at
```

| 数据类型 | 来源 | 存储位置 | 可否通过 GitHub API 获取 |
|---------|------|---------|------------------------|
| Issue 基础信息 (标题/编号/URL/状态) | GitHub Search/REST API | SQLite `tasks` 表 | ✅ 是 |
| 分配人 / @提及人 / 评论人 | GitHub Search API | SQLite `tasks.assignees` | ✅ 是 |
| GitHub 原始 Issue 状态 | GitHub REST API | SQLite `tasks.gh_state` | ✅ 是 |
| **看板执行状态** (待处理/处理中/已处理/已完成) | **本地决策** | **SQLite `tasks.status`** | ❌ 否 |
| **GitHub Project Status 原文** | GraphQL (可选) | SQLite `tasks.gh_status` | ❌ 否 (仅本项目可读) |
| Session ID / 中断记录 | 本地生成 | SQLite `tasks.session_id` | ❌ 否 |
| 交接任务记录 | 本地生成 | SQLite `tasks.handoff` | ❌ 否 |

---

## 2. 看板状态同步机制

### 2.1 同步时的状态决策逻辑 (sync.rs)

```rust
// 优先级从高到低：
1. gh_state == "closed"     → "done" (已完成)  // GitHub 关闭 = 权威覆盖
2. 命中 Label→Status 映射    → 映射的状态      // 用户自定义 label 映射
3. gh_status (Project Status) → 映射到四态     // GitHub Project 字段
4. 保持既有本地状态           → 不变           // 无新信息时维持手动态
5. 默认                       → "todo"         // 兜底
```

### 2.2 手动同步

- 用户在 UI 点击「立即同步」按钮
- 后台执行 `sync_now()`，拉取 GitHub 最新 Issue 列表
- 根据上述逻辑更新 `tasks.status` 和 `tasks.gh_status`
- **本地手动拖拽的状态不会被自动覆盖**（除非触发规则 1-3）

---

## 3. 获取看板状态的三种方式

### 方式 1：直接查询 SQLite（推荐用于自动化脚本）

```bash
# 数据库路径（macOS）
DB=~/Library/Application\ Support/com.shawnliu.taskboard/taskboard.db

# 查看所有「处理中」任务
sqlite3 "$DB" "SELECT repo, number, title, status, ownership FROM tasks WHERE status = 'doing';"

# 查看某账号的任务分布
sqlite3 "$DB" "SELECT status, COUNT(*) FROM tasks WHERE account_id = 1 GROUP BY status;"
```

### 方式 2：桌面应用 UI

- 打开 TaskBoard.app
- 顶栏切换账号/视图模式
- 看板列即为当前状态分布
- 点击卡片可查看详情、记录 session、写入交接

### 方式 3：MCP Server（供 AI Agent 调用）

```bash
# 前提：已在 Agent 配置中注册 taskboard mcp
# 示例：Claude Code / WorkBuddy

# 列出「处理中」任务
list_my_tasks(status="doing")

# 更新任务状态
update_task_status("owner/repo#123", "done")

# 记录中断 session
record_session("owner/repo#123", "session-abc", "claude-code")

# 写入交接任务
record_handoff("owner/repo#123", "已完成重构，待测试验收...")
```

---

## 4. 与 GitHub Projects v2 的关系

| 维度 | GitHub Projects v2 | 本项目 |
|-----|-------------------|-------|
| 状态存储 | GitHub 云端 | 本地 SQLite |
| 权限需求 | 需 org admin 创建 Project | 仅需 PAT `repo` 读权限 |
| 状态写入 | 可通过 GraphQL mutation | 本地直接写 SQLite |
| 数据归属 | GitHub 组织资产 | 用户本地完全控制 |
| 离线可用 | 否 | ✅ 是 |

> **历史背景**：原 PRD 设想复用 Projects v2，但实测组织权限不允许创建 Project。v0.3 起改为纯本地方案，**完全不依赖 GitHub Projects**。

---

## 5. 常见误区澄清

| 误区 | 事实 |
|-----|------|
| "能不能通过 GitHub API 查看板状态？" | ❌ 不能。状态在本地 SQLite，GitHub 无感知。 |
| "同步会把本地状态推回 GitHub？" | ❌ 不会。同步是**单向拉取**，只读 GitHub，写本地。 |
| "GitHub Projects 里的 Status 字段会自动同步？" | 仅同步时**读取** `gh_status` 作为参考，**不写回**。 |
| "Issue close 了，看板会自动变已完成？" | ✅ 是的。同步时 `gh_state=closed` 触发规则 1，强制 `done`。 |
| "我想在 GitHub 上看到看板进度" | 请使用 MCP Server 或导出本地数据，GitHub 侧无对应字段。 |

---

## 6. 数据库 Schema 关键字段

```sql
CREATE TABLE tasks (
  key           TEXT PRIMARY KEY,     -- "repo#number"
  repo          TEXT NOT NULL,
  number        INTEGER NOT NULL,
  title         TEXT NOT NULL,
  gh_state      TEXT NOT NULL,        -- "open" / "closed" (来自 GitHub)
  status        TEXT NOT NULL,        -- "todo"/"doing"/"processed"/"done" (本地看板态)
  gh_status     TEXT NOT NULL DEFAULT '', -- GitHub Project Status 原文
  ownership     TEXT NOT NULL,        -- "assigned"/"notassignee"/"assigned-others"
  session_id    TEXT,                 -- 中断会话 ID
  handoff       TEXT,                 -- 交接任务记录
  updated_at    TEXT,                 -- GitHub 侧 updated_at
  synced_at     INTEGER NOT NULL,     -- 最后同步时间戳
  account_id    INTEGER NOT NULL      -- 归属账号
);
```

---

## 6. 相关文件

- `app/src-tauri/src/sync.rs` — 同步核心逻辑（状态决策）
- `app/src-tauri/src/github.rs` — GitHub GraphQL 取 Project Status
- `app/src/components/Board.tsx` — 看板列渲染（Project Status 模式）
- `mcp_server/server.py` / `app/src-tauri/src/mcp.rs` — MCP 工具定义
- `README.md` — 快速上手与架构概览
- `PRD.md` — §4.2 状态承载方案