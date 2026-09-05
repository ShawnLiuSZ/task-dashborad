# copilot.instruction.md — TaskBoard（GitHub Copilot 入口）

> **GitHub Copilot 在本仓库工作时加载本文件**。本文件**只承载 Copilot 在代码补全 / 文件生成场景下的额外约束**；项目身份、硬约束、KB 文档规则、Git / 分支 / PR 流程、构建命令等共享规则请直接看 [`../AGENTS.md`](../AGENTS.md)。
>
> 跨 agent 通用的 MCP 触发规则（MCP 工具调用、issue 引用格式、状态枚举）：
> [`../mcp_server/AGENT_INSTRUCTIONS.md`](../mcp_server/AGENT_INSTRUCTIONS.md)
>
> **遇到冲突：AGENTS.md > 本文件**。

---

## 1. 代码风格与生成约定（Copilot 专属）

### 1.1 前端（`app/src/`，React + TS + Vite）

- **TypeScript 严格模式**（`tsconfig.json` 启用 `strict`），所有新文件必须带类型；不要在 `.tsx` 中用 `any`。
- **i18n 双语**：所有用户可见字符串走 `app/src/i18n/locales/{zh-CN,en-US}.json` 的 key，**不要**在组件里硬编码中文 / 英文文案。改文案时两份文件**同时**改，保持 key 集合一致；提交前 `cd app && npm run i18n:check`。
- **占位符保留**：i18n value 里的 `{name}` / `{count}` 等占位符原样保留，不翻译括号内 token。
- **样式**：跟随既有 Tauri / 系统风格，不引入额外的 UI 框架（如无必要不要新增 MUI / Ant Design）。
- **测试**：`app/src/**/*.test.ts(x)` 用 vitest，提交前 `cd app && npm test`。

### 1.2 后端（`app/src-tauri/src/`，Rust + rusqlite）

- **Schema 变更**：所有表 / 字段调整走 `db.rs::init` 的迁移分支，幂等且向后兼容；不要在多处定义 schema。
- **错误处理**：用 `anyhow::Result` / `thiserror` 自定义错误类型，Tauri 命令层统一 `map_err` 为用户可读消息；不要在 Rust 代码里 `unwrap()` 暴露给 UI 的路径。
- **API 调用**：GitHub 仅通过 `gh api` 走只读 Search / REST 端点；GraphQL 仅在确实必要时（如 Projects v2 字段）才引入。
- **MCP 工具签名**：与 `mcp_server/server.py` 的工具契约保持 1:1 兼容（`list_my_tasks` / `get_task_status` / `update_task_status` / `record_session` / `record_handoff` / `clear_session`）；改一处必同步另一处。

### 1.3 通用

- **不引入新依赖**：能不引入就不引入；新增 crate / npm 包前先确认是否已有等价能力。
- **不写散落配置**：MCP 配置统一在 `~/.workbuddy/mcp.json`（应用层）和各 agent 配置点（用户侧），仓库内只保留示例。
- **新功能先 PRD**：跨模块 / 涉及架构调整的功能，先在 `PRD.md` 写一节「决策与权衡」再写代码。

---

## 2. 速查：代码生成场景（Copilot 视角）

仅列与 Copilot 代码生成直接相关的场景；**Git / 分支 / PR / 发版 / KB 文档规则**等见 `../AGENTS.md §4.3 §5 §6`。

| 场景 | 该做 | 不该做 |
|---|---|---|
| 用户要求「改看板状态」 | 调 MCP `update_task_status` | 调 GitHub API 改 issue / label |
| 用户要求「同步任务」 | 触发本地 `sync_now()` | 触发任何 GitHub 写操作 |
| 用户要求「新增 MCP 工具」 | 同步改 `mcp.rs` 与 `server.py` | 只改一处导致契约漂移 |
| 用户要求「加文案」 | 改 `zh-CN.json` + `en-US.json`，跑 `npm run i18n:check` | 只改一份 / 硬编码进组件 |
| 用户要求「记录交接 / 会话」 | 调 MCP `record_handoff` / `record_session` | 写到 issue 评论里 |

---

> 本文件由项目维护者维护。如发现本指令与 `AGENTS.md` / `AGENT_INSTRUCTIONS.md` 不一致，**以 `AGENTS.md` 为准**，并提交 PR 修复。